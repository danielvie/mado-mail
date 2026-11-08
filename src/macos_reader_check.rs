//! Explicit offline WKWebView check; never loads settings, credentials, or Gmail.
use crate::reader::Reader;
use anyhow::{Context as _, Result, ensure};
use futures::channel::oneshot;
use gpui::{Context, Window};
use serde_json::{Value, json};
use std::{rc::Rc, time::Duration};
use wry::{WebView, WebViewExtMacOS};

pub fn fail(error: &str) -> ! {
    let output = json!({"passed":false,"error":error});
    let _ = std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/target/macos-reader-check.json"), output.to_string());
    eprintln!("{output}");
    std::process::exit(1);
}

async fn evaluate(webview: &WebView, script: &str) -> Result<Value> {
    let (send, receive) = oneshot::channel();
    let send = std::sync::Mutex::new(Some(send));
    webview.evaluate_script_with_callback(script, move |result| {
        if let Some(send) = send.lock().unwrap().take() { let _ = send.send(result); }
    })?;
    let result = receive.await?;
    Ok(serde_json::from_str(&result)?)
}

async fn probe(webview: &WebView) -> Result<Value> {
    evaluate(webview, r#"({ready:document.readyState,heading:document.querySelector('h1')?.textContent,
        active:document.querySelectorAll('script,iframe,object,form,input,meta[http-equiv],base').length,
        embedded:document.images[0]?.naturalWidth,remote:document.images[1]?.naturalWidth,
        width:innerWidth,height:innerHeight,url:location.href})"#).await
}

fn send_key(webview: &WebView, characters: &str, flags: objc2_app_kit::NSEventModifierFlags, code: u16) -> Result<()> {
    use objc2_app_kit::{NSApplication, NSEvent, NSEventType};
    let text = objc2_foundation::NSString::from_str(characters);
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown, objc2_foundation::NSPoint::new(0., 0.), flags, 0., webview.ns_window().windowNumber(), None, &text, &text, false, code,
    ).context("Could not create synthetic reader key")?;
    NSApplication::sharedApplication(objc2::MainThreadMarker::new().unwrap()).sendEvent(&event);
    Ok(())
}

fn focused(webview: &WebView) -> bool {
    let native = webview.webview();
    let mut responder = webview.ns_window().firstResponder();
    while let Some(current) = responder {
        if objc2::rc::Retained::as_ptr(&current).cast::<()>() == objc2::rc::Retained::as_ptr(&native).cast::<()>() { return true; }
        responder = unsafe { current.nextResponder() };
    }
    false
}

pub fn start(webview: Rc<WebView>, window: &Window, cx: &mut Context<Reader>) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::Relaxed) { return; }
    let host = window.window_handle();
    cx.spawn(async move |this, cx| {
        let result: Result<Value> = async {
            let text_system = cx.update(|cx| cx.text_system().clone());
            ensure!(!text_system.all_font_names().is_empty(), "macOS text backend has no fonts; enable gpui_platform/font-kit");
            let font = text_system.resolve_font(&gpui::font(crate::SYSTEM_FONT));
            let glyph_width = text_system.advance(font, gpui::px(12.), 'M')?.width;
            ensure!(glyph_width > gpui::px(0.), "System font has no visible glyph advance");
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let blocked = probe(&webview).await?;
            ensure!(blocked["ready"] == "complete" && blocked["heading"] == "Your membership has been renewed", "Body not loaded: {blocked}");
            ensure!(blocked["active"] == 0 && blocked["embedded"] == 760 && blocked["remote"] == 0, "Sanitizer/default image policy failed: {blocked}");
            webview.focus()?;
            send_key(&webview, "i", objc2_app_kit::NSEventModifierFlags::Option, 34)?;
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let allowed = probe(&webview).await?;
            ensure!(allowed["remote"] == 760, "Offline image opt-in failed: {allowed}");
            // Keep the opt-in CSP, but independently switch the native rules to block.
            this.update(cx, |reader, _| reader.policy.as_ref().unwrap().apply(&webview, false))?;
            webview.load_url(allowed["url"].as_str().unwrap())?;
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let native_guard = probe(&webview).await?;
            ensure!(native_guard["remote"] == 0 && native_guard["embedded"] == 760, "Native rules did not independently block fixture image: {native_guard}");
            this.update(cx, |reader, _| reader.policy.as_ref().unwrap().apply(&webview, true))?;
            webview.load_url(allowed["url"].as_str().unwrap())?;
            cx.background_executor().timer(Duration::from_millis(400)).await;
            ensure!(probe(&webview).await?["remote"] == 760, "Restored native opt-in rules failed");
            webview.load_url("https://example.invalid/escape")?;
            cx.background_executor().timer(Duration::from_millis(300)).await;
            let navigation = probe(&webview).await?;
            ensure!(navigation["url"] == allowed["url"], "Navigation escaped reader: {navigation}");
            host.update(cx, |root, window, cx| root.downcast::<crate::MailApp>().unwrap().update(cx, |app, cx| app.command(crate::Command::ToggleReader, window, cx)))?;
            cx.background_executor().timer(Duration::from_millis(100)).await;
            ensure!(webview.webview().isHidden(), "Native view remained visible");
            host.update(cx, |root, window, cx| root.downcast::<crate::MailApp>().unwrap().update(cx, |app, cx| app.command(crate::Command::ToggleReader, window, cx)))?;
            cx.background_executor().timer(Duration::from_millis(100)).await;
            ensure!(!webview.webview().isHidden(), "Native view did not reappear");
            host.update(cx, |root, window, cx| {
                window.resize(gpui::size(gpui::px(1300.), gpui::px(720.)));
                root.downcast::<crate::MailApp>().unwrap().update(cx, |app, cx| { app.reader_width = 600.; cx.notify(); });
            })?;
            cx.background_executor().timer(Duration::from_millis(300)).await;
            let resized = probe(&webview).await?;
            ensure!(resized["width"].as_f64().unwrap_or_default() > 100. && resized["width"] != allowed["width"], "Resize not reflected: {resized}");
            webview.focus()?;
            ensure!(focused(&webview), "WK view did not take native focus");
            send_key(&webview, "\u{1b}", objc2_app_kit::NSEventModifierFlags::empty(), 53)?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            let escape_focus = host.update(cx, |root, window, cx| {
                let app = root.downcast::<crate::MailApp>().unwrap().read(cx);
                (app.table_focus.is_focused(window), app.reader.as_ref().unwrap().read(cx).body_focus.is_focused(window))
            })?;
            ensure!(!focused(&webview), "Escape retained native reader focus; GPUI table/body focus: {escape_focus:?}");
            webview.focus()?;
            send_key(&webview, "f", objc2_app_kit::NSEventModifierFlags::Command, 3)?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            ensure!(!focused(&webview), "Command-F retained native reader focus");
            ensure!(host.update(cx, |root, window, cx| root.downcast::<crate::MailApp>().unwrap().read(cx).filter.read(cx).focus_handle.is_focused(window))?, "Command-F did not focus Inbox filter");
            let staging = host.update(cx, |root, _, cx| root.downcast::<crate::MailApp>().unwrap().read(cx).sidebar_visible)?;
            webview.focus()?;
            ensure!(focused(&webview), "WK view did not take focus before staging shortcut");
            send_key(&webview, "B", objc2_app_kit::NSEventModifierFlags::Command | objc2_app_kit::NSEventModifierFlags::Shift, 11)?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            ensure!(host.update(cx, |root, _, cx| root.downcast::<crate::MailApp>().unwrap().read(cx).sidebar_visible)? != staging, "Command-Shift-B did not toggle staging from reader focus");
            webview.focus()?;
            ensure!(focused(&webview), "WK view did not take focus before restoring staging");
            send_key(&webview, "B", objc2_app_kit::NSEventModifierFlags::Command | objc2_app_kit::NSEventModifierFlags::Shift, 11)?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            ensure!(host.update(cx, |root, _, cx| root.downcast::<crate::MailApp>().unwrap().read(cx).sidebar_visible)? == staging, "Command-Shift-B did not restore staging");
            let filter = host.update(cx, |root, window, cx| {
                let filter = root.downcast::<crate::MailApp>().unwrap().read(cx).filter.clone();
                filter.update(cx, |input, cx| input.set("Command editing", cx));
                filter.read(cx).focus_handle.clone().focus(window, cx);
                for key in ["cmd-a", "backspace", "cmd-z"] { window.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx); }
                filter
            })?;
            ensure!(filter.read_with(cx, |input, _| input.content.as_str() == "Command editing"), "Command select/delete/undo failed");
            filter.update(cx, |input, cx| input.set("", cx));
            this.update(cx, |reader, cx| reader.toggle_images(cx))?;
            cx.background_executor().timer(Duration::from_millis(300)).await;
            let reblocked = probe(&webview).await?;
            ensure!(reblocked["remote"] == 0, "Blocking again failed: {reblocked}");
            this.update(cx, |reader, cx| reader.detach(cx))?;
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let popout = cx.update(|cx| cx.windows().into_iter().find_map(|window| window.downcast::<Reader>())).context("Detached reader did not open")?;
            let copy = popout.entity(cx)?;
            let copy_browser = copy.update(cx, |reader, _| reader.webview.clone()).context("Detached browser did not mount")?;
            ensure!(probe(&copy_browser).await?["remote"] == 0, "Detached policy did not preserve snapshot");
            popout.update(cx, |reader, _, cx| reader.toggle_images(cx))?;
            cx.background_executor().timer(Duration::from_millis(400)).await;
            ensure!(probe(&copy_browser).await?["remote"] == 760 && probe(&webview).await?["remote"] == 0, "Detached policy is not independent");
            drop(copy_browser);
            drop(copy);
            popout.update(cx, |_, window, _| window.remove_window())?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            ensure!(cx.update(|cx| cx.windows().len()) == 1, "Closing pop-out closed Inbox or leaked window");
            ensure!(host.update(cx, |root, _, _| root.downcast::<crate::MailApp>().is_ok())?, "Inbox no longer exists");
            this.update(cx, |reader, cx| reader.check_stale_completion(cx))??;
            Ok(json!({"blocked":blocked,"allowed":allowed,"native_guard":native_guard,"navigation":navigation,"resized":resized,"reblocked":reblocked,
                "native_font_glyph_width":f32::from(glyph_width),"focus_return":true,"synthetic_native_keys":true,"command_editing":true,"detached_independent":true,"stale_completion":true,
                "limits":["Physical keyboard traversal and native controls require manual verification.", "Offline image fixture uses a synthetic-only custom scheme, not a real HTTPS server.","Visual alignment and native keyboard traversal require manual verification."]}))
        }.await;
        let output = match &result { Ok(value) => json!({"passed":true,"checks":value}), Err(error) => json!({"passed":false,"error":format!("{error:#}")}) };
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/macos-reader-check.json");
        let written = std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap());
        eprintln!("{output}");
        if result.is_err() || written.is_err() { std::process::exit(1); }
        cx.update(|cx| cx.quit());
    }).detach();
}
