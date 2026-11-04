//! Synthetic pop-out checks. No Gmail requests or personal settings writes.
use super::Reader;
use crate::{Command, MailApp, reader_check};
use anyhow::{Result, ensure};
use gpui::{AnyWindowHandle, AsyncApp, Context, Entity, Keystroke, WindowHandle, px, size};
use serde_json::json;
use std::{rc::Rc, time::Duration};
use wry::{WebView, WebViewExtWindows};

async fn ready(reader: &Entity<Reader>, cx: &mut AsyncApp) -> Result<Rc<WebView>> {
    for _ in 0..60 {
        cx.background_executor().timer(Duration::from_millis(100)).await;
        let browser = reader.update(cx, |r, _| (r.loaded && r.page_ready).then(|| r.browser()).flatten());
        if let Some(browser) = browser {
            let mut visible = false.into();
            unsafe { browser.controller().IsVisible(&mut visible)?; }
            if visible.as_bool() { return Ok(browser); }
        }
    }
    anyhow::bail!("Reader did not finish loading visibly")
}

async fn detached(cx: &mut AsyncApp) -> Result<WindowHandle<Reader>> {
    for _ in 0..40 {
        cx.background_executor().timer(Duration::from_millis(100)).await;
        if let Some(window) = cx.update(|cx| cx.windows().into_iter().find_map(|w| w.downcast::<Reader>())) {
            return Ok(window);
        }
    }
    anyhow::bail!("Pop-out button did not open a reader window")
}

pub fn start(inbox: AnyWindowHandle, cx: &mut Context<MailApp>) {
    cx.spawn(async move |this, cx| {
        let result: Result<()> = async {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let source = inbox.update(cx, |root, window, cx| {
                root.downcast::<MailApp>().unwrap().update(cx, |app, cx| app.ensure_reader(window, cx))
            })?;
            source.update(cx, |r, cx| {
                ensure!(!r.can_detach(), "Empty reader can detach");
                r.detach(cx);
                Ok(())
            })?;
            ensure!(cx.update(|cx| cx.windows().len()) == 1, "Empty reader opened a window");
            let mail_before = this.update(cx, |app, _| (
                app.triage.work(), app.triage.selection.clone(),
                app.triage.emails.iter().map(|e| (e.id.clone(), e.label_ids.clone())).collect::<Vec<_>>(),
            ))?;
            inbox.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
                let id = app.triage.emails[0].id.clone();
                app.command(Command::Open(id), window, cx);
            }))?;
            let source_browser = ready(&source, cx).await?;
            // Distinguish the first HTML body from the common demo fixture, then block images.
            source.update(cx, |r, cx| {
                let mut doc = r.document.write().unwrap();
                doc.html = doc.html.replace("Your membership has been renewed", "Pinned first message");
                drop(doc);
                r.toggle_images(cx);
            });
            ready(&source, cx).await?;
            source_browser.evaluate_script("window.scrollTo(0,80)")?;
            let before = reader_check::probe(&source_browser).await?;
            let snapshot = source.update(cx, |r, _| (
                r.email.id.clone(), r.email.subject.clone(), r.labels.clone(), r.document.read().unwrap().html.clone(),
            ));
            let preferences = this.update(cx, |app, _| app.settings.clone())?;
            // Tab-focusable SVG button uses the same command for click, Enter, and Space.
            inbox.update(cx, |_, window, cx| {
                window.activate_window();
                source.read(cx).detach_focus.clone().focus(window, cx);
                window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
            })?;
            let popout = detached(cx).await?;
            let popout_entity = popout.entity(cx)?;
            let popout_browser = ready(&popout_entity, cx).await?;
            let copy = reader_check::probe(&popout_browser).await?;
            ensure!(copy["heading"] == "Pinned first message" && copy["remote"] == 0 && copy["embedded"].as_u64().unwrap_or_default() > 0,
                "Detached reader did not preserve HTML, embedded images, and blocked remote images: {copy}");
            popout.read_with(cx, |r, _| {
                ensure!(!r.detachable && r.email.id == snapshot.0 && r.email.subject == snapshot.1
                    && r.labels == snapshot.2 && r.document.read().unwrap().html == snapshot.3,
                    "Detached reader lost message metadata, labels, or body");
                Ok(())
            })??;
            ensure!(reader_check::probe(&source_browser).await?["scroll"] == before["scroll"], "Pop-out changed inline scroll");
            this.update(cx, |app, _| {
                ensure!(app.settings == preferences && app.active_message.as_ref() == Some(&snapshot.0), "Pop-out changed Inbox layout or active message");
                Ok(())
            })??;
            popout.update(cx, |r, _, cx| r.toggle_images(cx))?;
            ready(&popout_entity, cx).await?;
            ensure!(reader_check::probe(&popout_browser).await?["remote"].as_u64().unwrap_or_default() > 0,
                "Detached image control did not work");
            source.update(cx, |r, _| { ensure!(!r.document.read().unwrap().remote, "Detached image toggle changed the inline reader"); Ok(()) })?;
            // Browsing another message and switching to Rules must not replace or hide the pop-out.
            inbox.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
                let id = app.triage.emails[1].id.clone();
                app.command(Command::Open(id), window, cx);
                app.command(Command::Rules, window, cx);
            }))?;
            ready(&popout_entity, cx).await?;
            popout.read_with(cx, |r, _| {
                ensure!(r.email.id == snapshot.0 && r.document.read().unwrap().html == snapshot.3, "Pop-out followed Inbox selection"); Ok(())
            })??;
            popout.update(cx, |_, window, _| window.resize(size(px(640.), px(650.))))?;
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let resized = reader_check::probe(&popout_browser).await?;
            ensure!(resized["width"].as_u64() < copy["width"].as_u64(), "Detached browser did not resize");
            popout_browser.evaluate_script("window.scrollTo(0,document.body.scrollHeight)")?;
            ensure!(reader_check::probe(&popout_browser).await?["scroll"].as_f64().unwrap_or_default() > 0., "Detached body cannot scroll");
            drop(popout_browser);
            drop(popout_entity);
            popout.update(cx, |_, window, _| window.remove_window())?;
            cx.background_executor().timer(Duration::from_millis(300)).await;
            ensure!(cx.update(|cx| cx.windows().len()) == 1, "Closing detached reader closed the Inbox or retained a window");
            // A second pop-out pins the new message; Space activates the same SVG in the below layout.
            inbox.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
                window.activate_window();
                app.command(Command::Rules, window, cx);
                app.command(Command::ToggleReaderPosition, window, cx);
            }))?;
            ready(&source, cx).await?;
            inbox.update(cx, |_, window, cx| {
                source.read(cx).detach_focus.clone().focus(window, cx);
                window.dispatch_keystroke(Keystroke::parse("space").unwrap(), cx);
            })?;
            let second = detached(cx).await?;
            let second_entity = second.entity(cx)?;
            let second_browser = ready(&second_entity, cx).await?;
            second.read_with(cx, |r, _| { ensure!(r.email.id != snapshot.0, "Second pop-out reused the first message"); Ok(()) })??;
            this.update(cx, |app, _| {
                ensure!(app.triage.work() == mail_before.0 && app.triage.selection == mail_before.1
                    && app.triage.emails.iter().map(|e| (e.id.clone(), e.label_ids.clone())).collect::<Vec<_>>() == mail_before.2,
                    "Pop-out changed mail, unread status, checks, or staging"); Ok(())
            })??;
            drop(source_browser);
            drop(source);
            inbox.update(cx, |_, window, _| window.remove_window())?;
            cx.background_executor().timer(Duration::from_millis(300)).await;
            ensure!(cx.update(|cx| cx.windows().len()) == 1, "Closing Inbox closed the pinned reader");
            ready(&second_entity, cx).await?;
            ensure!(reader_check::probe(&second_browser).await?["ready"] == "complete", "Pinned reader stopped after Inbox closed");
            Ok(())
        }.await;
        let report = match result {
            Ok(()) => json!({"ok":true,"pinned_message":true,"independent_image_policy":true,"svg_keyboard":true,
                "both_layouts":true,"resize_scroll":true,"independent_window_lifetime":true,"mail_unchanged":true,"gmail_access":false}),
            Err(error) => json!({"ok":false,"error":format!("{error:#}")}),
        };
        let _ = std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/target/detach-check.json"), report.to_string());
        let _ = cx.update(|cx| cx.quit());
    }).detach();
}
