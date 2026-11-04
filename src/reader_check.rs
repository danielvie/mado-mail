// This check runs only with --reader-check, which bypasses Gmail and uses synthetic content.
use crate::{reader::Reader, theme::Palette};
use anyhow::{Context as _, Result, ensure};
use futures::channel::oneshot;
use gpui::{Context, Window, px, size};
use serde_json::{Value, json};
use std::{rc::Rc, time::Duration};
use webview2_com::{
    CapturePreviewCompletedHandler,
    Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
};
use windows::Win32::{
    System::Com::{STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET},
    UI::Shell::SHCreateMemStream,
};
use wry::{WebView, WebViewExtWindows};

pub(crate) async fn probe(webview: &WebView) -> Result<Value> {
    let (send, receive) = oneshot::channel();
    let send = std::sync::Mutex::new(Some(send));
    // Host-injected diagnostics still work with page JavaScript disabled. Never evaluate email-provided code.
    webview.evaluate_script_with_callback(
        r#"JSON.stringify({
        ready:document.readyState,
        heading:document.querySelector('h1')?.textContent,
        active:document.querySelectorAll('script,iframe,object,form,meta[http-equiv],base').length,
        embedded:document.images[0]?.naturalWidth,
        remote:document.images[1]?.naturalWidth,
        width:innerWidth,height:innerHeight,scroll:scrollY
    })"#,
        move |result| {
            if let Some(send) = send.lock().unwrap().take() {
                let _ = send.send(result);
            }
        },
    )?;
    let encoded: String = serde_json::from_str(&receive.await?)?;
    Ok(serde_json::from_str(&encoded)?)
}

async fn capture(webview: &WebView) -> Result<()> {
    unsafe {
        let stream = SHCreateMemStream(None).context("Could not create browser capture stream")?;
        let (send, receive) = oneshot::channel();
        webview.webview().CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &CapturePreviewCompletedHandler::create(Box::new(move |result| {
                let _ = send.send(result);
                Ok(())
            })),
        )?;
        receive.await??;
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME)?;
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let mut bytes = vec![0; stat.cbSize as usize];
        let mut read = 0;
        stream
            .Read(
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                Some(&mut read),
            )
            .ok()?;
        ensure!(read as usize == bytes.len(), "Incomplete browser capture");
        std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/target/reader-body.png"),
            bytes,
        )?;
    }
    Ok(())
}

async fn capture_parent(cx: &mut gpui::AsyncApp, panel: &'static str) -> Result<()> {
    let result = cx
        .background_executor()
        .spawn(async move {
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-File",
                    concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"),
                    "-ReaderDemo",
                    "-ProcessId",
                    &std::process::id().to_string(),
                    "-VerifyBody",
                    "-ExpectedPanel",
                    panel,
                ])
                .output()
        })
        .await?;
    ensure!(
        result.status.success(),
        "Reader visibility/theme check failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}

pub fn start(webview: Rc<WebView>, window: &Window, cx: &mut Context<Reader>) {
    let window = window.window_handle();
    cx.spawn(async move |this, cx| {
        let result = async {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let allowed = probe(&webview).await?;
            ensure!(
                allowed["ready"] == "complete"
                    && allowed["heading"] == "Your membership has been renewed",
                "HTML did not load: {allowed}"
            );
            ensure!(
                allowed["active"] == 0 && allowed["embedded"] == 760 && allowed["remote"] == 760,
                "Unsafe content or default-allow image policy failed: {allowed}"
            );
            ensure!(
                allowed["width"].as_u64().unwrap_or_default() > 100
                    && allowed["height"].as_u64().unwrap_or_default() > 100,
                "Browser layout has no usable area"
            );
            unsafe {
                let mut enabled = true.into();
                webview
                    .webview()
                    .Settings()?
                    .IsScriptEnabled(&mut enabled)?;
                ensure!(!enabled.as_bool(), "Page JavaScript is enabled");
                webview
                    .webview()
                    .Settings()?
                    .AreHostObjectsAllowed(&mut enabled)?;
                ensure!(!enabled.as_bool(), "Host objects are enabled");
                webview.controller().IsVisible(&mut enabled)?;
                ensure!(enabled.as_bool(), "Browser body remained hidden after loading");
            }
            capture(&webview).await?;
            capture_parent(cx, "ffffff").await?;
            // A palette change must update an already open reader without changing the email document.
            let custom: Palette = serde_json::from_str(r##"{"panel":"#e6f7ef","accent":"#087f5b"}"##)?;
            cx.update(|cx| custom.apply(cx));
            cx.background_executor().timer(Duration::from_millis(250)).await;
            capture_parent(cx, "e6f7ef").await?;
            cx.update(|cx| Palette::default().apply(cx));
            cx.background_executor().timer(Duration::from_millis(250)).await;
            capture_parent(cx, "ffffff").await?;
            this.update(cx, |this, cx| this.toggle_images(cx))?;
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let blocked = probe(&webview).await?;
            ensure!(
                blocked["remote"] == 0 && blocked["embedded"] == 760,
                "Block images failed: {blocked}"
            );
            this.update(cx, |this, cx| this.toggle_images(cx))?;
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let reenabled = probe(&webview).await?;
            ensure!(reenabled["remote"] == 760, "Allow images failed: {reenabled}");
            this.update(cx, |this, cx| this.toggle_images(cx))?;
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let reblocked = probe(&webview).await?;
            ensure!(
                reblocked["remote"] == 0 && reblocked["embedded"] == 760,
                "Reblocking images failed: {reblocked}"
            );
            this.update(cx, |this, cx| this.check_stale_completion(cx))??;
            window.update(cx, |_, window, _| window.resize(size(px(640.), px(650.))))?;
            cx.background_executor().timer(Duration::from_secs(1)).await;
            webview.evaluate_script("window.scrollTo(0,document.body.scrollHeight)")?;
            let resized = probe(&webview).await?;
            ensure!(resized["width"].as_u64() < allowed["width"].as_u64() && resized["height"].as_u64().unwrap_or_default() > 100,
                "Child browser did not resize: {resized}");
            ensure!(resized["scroll"].as_f64().unwrap_or_default() > 0.0, "Message does not scroll: {resized}");
            ensure!(resized["remote"] == 760, "A newly opened message did not restore default image loading: {resized}");
            anyhow::Ok(json!({"ok":true,"default_allowed":allowed,"blocked":blocked,"reenabled":reenabled,"reblocked":reblocked,"resized":resized,"stale_completion_rejected":true,"parent_body_visible":true,"frost_header":true,"live_custom_theme":true}))
        }
        .await;
        let report = match result {
            Ok(report) => report,
            Err(error) => json!({"ok":false,"error":format!("{error:#}")}),
        };
        let _ = std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/target/reader-check.json"),
            report.to_string(),
        );
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}
