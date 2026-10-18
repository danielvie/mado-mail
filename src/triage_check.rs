//! Synthetic native checks. Never restores credentials, contacts Gmail, or saves preferences.
use crate::{Action, Command, MailApp, triage};
use anyhow::{Result, ensure};
use gpui::{AnyWindowHandle, Context, EntityInputHandler, Keystroke, Pixels, Point, point, px};
use serde_json::json;
use std::time::Duration;
use wry::{WebView, WebViewExtWindows};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{GetActiveWindow, GetFocus},
        WindowsAndMessaging::{
            GetWindowThreadProcessId, PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
            WM_RBUTTONDOWN, WM_RBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
        },
    },
};

fn mouse(window: &gpui::Window, message: u32, button: usize, position: Point<Pixels>) -> Result<()> {
    unsafe {
        let hwnd = GetActiveWindow(); let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        ensure!(pid == std::process::id(), "Refusing input outside the synthetic check window");
        let x = (f32::from(position.x) * window.scale_factor()).round() as i16 as u16;
        let y = (f32::from(position.y) * window.scale_factor()).round() as i16 as u16;
        PostMessageW(Some(hwnd), message, WPARAM(button), LPARAM((u32::from(x) | (u32::from(y) << 16)) as isize))?;
    }
    Ok(())
}

fn drag(
    window: &gpui::Window,
    from: Point<Pixels>,
    to: Point<Pixels>,
    release: Point<Pixels>,
) -> Result<()> {
    // Post only to this synthetic process's active window. Never move the desktop pointer or send global input.
    unsafe {
        let hwnd = GetActiveWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        ensure!(
            pid == std::process::id(),
            "Refusing mouse input outside the synthetic check window"
        );
        for (message, button, position) in [
            (WM_MOUSEMOVE, 0, from),
            (WM_LBUTTONDOWN, 1, from),
            (WM_MOUSEMOVE, 1, to),
            (WM_LBUTTONUP, 0, release),
        ] {
            let x = (f32::from(position.x) * window.scale_factor()).round() as i16 as u16;
            let y = (f32::from(position.y) * window.scale_factor()).round() as i16 as u16;
            PostMessageW(
                Some(hwnd),
                message,
                WPARAM(button),
                LPARAM((u32::from(x) | (u32::from(y) << 16)) as isize),
            )?;
        }
    }
    Ok(())
}

fn click_row(this: &MailApp, window: &gpui::Window, index: usize, checkbox: bool) -> Result<()> {
    let height = 68.;
    let pos = point(this.table_bounds.left() + px(if checkbox { 22. } else { 160. }),
        this.table_bounds.top() + px(index as f32 * height + height / 2.));
    ensure!(pos.y < this.table_bounds.bottom(), "Check row is outside the visible table");
    drag(window, pos, pos, pos)
}
fn check_two_line_rows(this: &MailApp) -> Result<()> {
    let rows = this.triage.rows("");
    let first = this.row_bounds.get(&rows[0].key).ok_or_else(|| anyhow::anyhow!("First row not measured"))?;
    let second = this.row_bounds.get(&rows[1].key).ok_or_else(|| anyhow::anyhow!("Second row not measured"))?;
    let spacing = f32::from(second.top() - first.top());
    ensure!((spacing - 68.).abs() < 1., "Rows switched away from the two-line layout: spacing {spacing}");
    Ok(())
}
fn visible(webview: &WebView) -> Result<bool> {
    unsafe {
        let mut visible = false.into();
        webview.controller().IsVisible(&mut visible)?;
        Ok(visible.as_bool())
    }
}

async fn check_middle_selection(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        window.activate_window();
        app.triage.expanded.insert("studio.updates@example.test".into());
        cx.notify();
    }))?;
    let baseline = this.update(cx, |app, _| (app.triage.work(), app.triage.expanded.clone(),
        app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>()))?;
    for (key, seed, target, expected) in [
        ("m:demo-0", "empty", "body", 32),
        ("m:demo-0", "clicked", "body", 0),
        ("b:studio.updates@example.test", "clicked", "body", 32),
        ("b:studio.updates@example.test", "all", "checkbox", 0),
        ("m:demo-0", "empty", "trash", 32),
        ("m:demo-0", "all", "body", 0),
        ("b:ada.chen@example.test", "filtered", "body", 1),
        ("b:ada.chen@example.test", "clicked", "checkbox", 0),
    ] {
        window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
            if seed == "filtered" {
                app.filter.update(cx, |input, cx| input.set("Review notes", cx));
                app.command(Command::Open("demo-24".into()), window, cx);
            }
            let rows = app.triage.rows(&app.filter.read(cx).content);
            let row = rows.iter().find(|row| row.key == key).unwrap();
            app.triage.selection = match seed {
                "clicked" => std::iter::once(row.ids[0].clone()).collect(),
                "all" => rows.iter().flat_map(|row| row.ids.iter().cloned()).collect(),
                _ => Default::default(),
            };
            app.hovered_row = Some(key.into());
            app.cursor = rows.iter().position(|row| row.key == key).unwrap();
            window.activate_window(); cx.notify();
        }))?;
        // Wait for the filtered list and the toolbar's new selection count to finish layout.
        for _ in 0..20 {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let laid_out = this.update(cx, |app, _| app.row_bounds.get(key).is_some_and(|bounds|
                (f32::from(bounds.top() - app.table_bounds.top()) - app.cursor as f32 * 68.).abs() < 2.))?;
            if laid_out { break; }
        }
        let preview = this.update(cx, |app, _| (app.active_message.clone(), app.reader_visible))?;
        window.update(cx, |root, window, cx| {
            let view = root.downcast::<MailApp>().unwrap(); let app = view.read(cx);
            let row = app.row_bounds[key];
            ensure!(row.top() >= app.table_bounds.top() && row.bottom() <= app.table_bounds.bottom(), "Middle-click target is off-screen");
            let x = match target { "checkbox" => row.left() + px(22.), "trash" => row.right() - px(52.), _ => row.left() + px(160.) };
            let pos = point(x, row.top() + px(18.));
            mouse(window, WM_MOUSEMOVE, 0, pos)?;
            mouse(window, WM_MBUTTONDOWN, 0x10, pos)?;
            mouse(window, WM_MBUTTONUP, 0, pos)
        })??;
        for _ in 0..20 {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if this.update(cx, |app, _| app.triage.selection.len() == expected)? { break; }
        }
        this.update(cx, |app, _| {
            ensure!(app.triage.selection.len() == expected, "Middle-click failed for {seed}/{target}: expected {expected}, got {}", app.triage.selection.len());
            ensure!((app.active_message.clone(), app.reader_visible) == preview && app.row_menu.is_none() && app.mail_drag.is_none(), "Middle-click opened a message, menu, or drag");
            ensure!(app.triage.work() == baseline.0 && app.triage.expanded == baseline.1
                && app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>() == baseline.2,
                "Middle-click changed staging, unread status, or expansion");
            if seed == "filtered" { ensure!(app.triage.selection == std::iter::once("demo-24".to_string()).collect(), "Filtered selection included other messages"); }
            Ok(())
        })??;
    }
    Ok(())
}

async fn check_selected_actions(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    for (action, checked, auto) in [
        ("read", true, false), ("unread", true, false),
        ("archive", true, false), ("trash", true, false),
        ("menu-archive", true, false), ("menu-trash", true, false),
        ("archive", true, true), ("trash", true, true),
        ("read-archive", true, false), ("read-archive", true, true),
        ("read-archive", false, false), ("read-archive", false, true),
        ("read-archive-read", true, false), ("read-archive-read", true, true),
        ("read", false, false), ("unread", false, false),
        ("archive", false, false), ("trash", false, false),
        ("menu-archive", false, false), ("menu-trash", false, false),
    ] {
        let reading = matches!(action, "read" | "unread");
        let read_archive = action.starts_with("read-archive");
        let target_action = if action.ends_with("trash") { Action::Trash } else { Action::Archive };
        let expected: Vec<String> = if checked { vec!["demo-0", "demo-24", "demo-25"] } else { vec!["demo-24"] }.into_iter().map(str::to_string).collect();
        let before = window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
            app.triage = triage::demo(); app.settings.auto_apply = auto; app.row_menu = None;
            app.triage.selection = ["demo-0", "demo-25"].into_iter().map(str::to_string).collect();
            if checked { app.triage.selection.insert("demo-24".into()); }
            if action == "unread" { app.triage.set_read(&["demo-24".into()], true); }
            if action == "read-archive-read" { app.triage.set_read(&expected, true); }
            app.command(Command::Open("demo-24".into()), window, cx);
            app.cursor = 4; app.hovered_row = Some("m:demo-24".into());
            window.activate_window(); app.table_focus.focus(window); cx.notify();
            (app.triage.emails.iter().map(|e| (e.id.clone(), e.label_ids.clone())).collect::<Vec<_>>(), app.triage.selection.clone(), app.triage.work())
        }))?;
        // WebView2 creation pumps native messages; do not click while the initial reader is mounting.
        for _ in 0..50 {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let browser = this.update(cx, |app, cx| app.reader.as_ref().and_then(|reader| reader.read(cx).browser()))?;
            if let Some(browser) = browser { if visible(&browser)? { break; } }
        }
        cx.background_executor().timer(Duration::from_millis(250)).await;
        if action.starts_with("menu-") {
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
                app.open_row_menu("m:demo-24", window, cx);
            }))?;
            cx.background_executor().timer(Duration::from_millis(400)).await;
            window.update(cx, |_, window, cx| {
                window.dispatch_keystroke(Keystroke::parse("home").unwrap(), cx);
                if target_action == Action::Trash { window.dispatch_keystroke(Keystroke::parse("down").unwrap(), cx); }
                window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
            })?;
        } else {
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let app = view.read(cx);
                let bounds = app.row_bounds["m:demo-24"];
                let pos = point(bounds.right() - px(if reading { 144. } else if read_archive { 114. } else if action == "archive" { 84. } else { 52. }), bounds.top() + px(18.));
                window.activate_window();
                drag(window, pos, pos, pos)
            })??;
        }
        for _ in 0..20 {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let done = this.update(cx, |app, _| expected.iter().all(|id| {
                if reading { app.triage.emails.iter().any(|e| &e.id == id && e.label_ids.contains(&"UNREAD".into()) == (action == "unread")) }
                else if auto { !app.triage.emails.iter().any(|e| &e.id == id) }
                else { app.triage.marks.get(id).is_some_and(|mark| mark.action == target_action) }
            }))?;
            if done { break; }
        }
        this.update(cx, |app, _| {
            ensure!(!app.failed && app.active_message.as_deref() == Some("demo-24") && app.reader_visible, "{action} changed preview or failed");
            for (id, labels) in &before.0 {
                let email = app.triage.emails.iter().find(|e| &e.id == id);
                if expected.contains(id) && !reading && auto {
                    ensure!(email.is_none(), "{action} did not apply to selected message {id}");
                } else {
                    let email = email.ok_or_else(|| anyhow::anyhow!("{action} removed unexpected message {id}"))?;
                    let mut expected_labels = labels.clone();
                    if (reading || read_archive) && expected.contains(id) {
                        expected_labels.retain(|label| label != "UNREAD");
                        if action == "unread" { expected_labels.push("UNREAD".into()); }
                    }
                    ensure!(email.label_ids == expected_labels, "{action}, checked={checked}: wrong read state for {id}");
                }
            }
            if reading || auto { ensure!(app.triage.work() == before.2, "{action} changed existing staging"); }
            else {
                ensure!(app.triage.marks.len() == before.2.len() + expected.len(), "{action} staged the wrong number of messages");
                ensure!(expected.iter().all(|id| app.triage.marks.get(id).is_some_and(|mark| mark.action == target_action)), "{action} missed selected messages");
            }
            let selection = if reading { before.1.clone() } else { before.1.iter().filter(|id| !expected.contains(id)).cloned().collect() };
            ensure!(app.triage.selection == selection, "{action} changed unrelated checkmarks");
            Ok(())
        })??;
    }
    // Feed mixed write outcomes through the live completion path. No Gmail requests.
    for (auto, confirmed) in [(false, false), (true, false), (false, true), (true, true)] {
        this.update(cx, |app, cx| {
            app.triage = triage::demo(); app.settings.auto_apply = auto;
            let pending = app.triage.work();
            let ids: Vec<String> = ["demo-0", "demo-24", "demo-25"].into_iter().map(str::to_string).collect();
            app.triage.set_read(&ids, false);
            app.triage.selection = ids.iter().cloned().collect();
            app.busy = true;
            app.finish_read(vec![
                (ids[0].clone(), Ok(Some(crate::Outcome::Confirmed))),
                (ids[1].clone(), Ok(Some(if confirmed { crate::Outcome::Confirmed } else { crate::Outcome::Failed("Synthetic rejection".into()) }))),
                (ids[2].clone(), Ok(Some(if confirmed { crate::Outcome::Confirmed } else { crate::Outcome::Unknown("Synthetic timeout".into()) }))),
            ], true, ids.clone(), cx);
            if confirmed {
                ensure!(!app.failed && !app.busy && app.triage.selection.is_empty(), "Confirmed reads did not continue to Archive");
                for id in &ids {
                    let email = app.triage.emails.iter().find(|e| &e.id == id);
                    if auto { ensure!(email.is_none(), "Confirmed target was not archived"); }
                    else {
                        ensure!(!email.unwrap().label_ids.contains(&"UNREAD".into()), "Staged Archive is still unread");
                        ensure!(app.triage.marks[id].action == Action::Archive, "Confirmed target was not staged");
                    }
                }
                ensure!(pending.iter().all(|w| app.triage.work().contains(w)), "Combined action changed unrelated staging");
            } else {
                ensure!(app.failed && !app.busy && app.triage.work() == pending && app.status.contains("Nothing archived"), "Read failure continued to Archive, auto={auto}");
                ensure!(app.triage.selection == ids.iter().cloned().collect(), "Read failure changed checkmarks");
                for (i, id) in ids.iter().enumerate() {
                    let email = app.triage.emails.iter().find(|e| &e.id == id).unwrap();
                    ensure!(email.label_ids.contains(&"UNREAD".into()) == (i != 0), "Unconfirmed read state changed for {id}");
                }
            }
            Ok(())
        })??;
    }
    Ok(())
}

#[path = "space_check.rs"]
mod space_check;
#[path = "label_check.rs"]
mod label_check;

async fn check_reader_visibility(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    let baseline = this.update(cx, |app, _| (app.settings.clone(), app.triage.work(), app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>()))?;
    window.update(cx, |root, window, cx| {
        window.activate_window();
        click_row(root.downcast::<MailApp>().unwrap().read(cx), window, 4, false)
    })??;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    this.update(cx, |app, _| {
        ensure!(!app.reader_visible && app.reader.is_none() && app.active_message.is_none() && app.settings == baseline.0, "Clicking a message opened the hidden reader or changed its preference"); Ok(())
    })??;
    window.update(cx, |root, window, cx| {
        root.downcast::<MailApp>().unwrap().update(cx, |app, _| { app.cursor = 5; app.table_focus.focus(window); });
        window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
    })?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        ensure!(!app.reader_visible && app.reader.is_none(), "Enter opened the hidden reader");
        app.command(Command::ToggleReader, window, cx);
        let row = app.triage.rows("").into_iter().find(|row| row.key == "m:demo-24").unwrap();
        app.activate_row(&row, window, cx);
        ensure!(app.reader_visible && app.active_message.as_deref() == Some("demo-24"), "Visible reader did not load the activated message");
        Ok(())
    }))??;
    let mut browser = None;
    for _ in 0..50 {
        cx.background_executor().timer(Duration::from_millis(100)).await;
        browser = this.update(cx, |app, cx| app.reader.as_ref().and_then(|reader| reader.read(cx).browser()))?;
        if let Some(ref browser) = browser { if visible(browser)? { break; } }
    }
    let browser = browser.ok_or_else(|| anyhow::anyhow!("Synthetic reader did not load"))?;
    ensure!(visible(&browser)?, "Reader did not become visible after explicit toggle");
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        app.command(Command::ToggleReader, window, cx);
        let row = app.triage.rows("").into_iter().find(|row| row.key == "m:demo-25").unwrap();
        app.activate_row(&row, window, cx);
        ensure!(!app.reader_visible && !app.settings.pane_visibility.reader && app.active_message.as_deref() == Some("demo-24"), "Row activation reopened or replaced a hidden preview");
        ensure!(app.triage.work() == baseline.1 && app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>() == baseline.2 && app.triage.selection.is_empty(), "Row activation changed mail or checkmarks");
        Ok(())
    }))??;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    ensure!(!visible(&browser)?, "Native reader stayed visible after hiding");
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        app.open_row_menu("m:demo-25", window, cx);
        app.menu_action(3, window, cx);
        ensure!(app.reader_visible && app.active_message.as_deref() == Some("demo-25"), "Explicit Open message stopped working");
        Ok(())
    }))??;
    Ok(())
}

pub fn start(window: AnyWindowHandle, cx: &mut Context<MailApp>) {
    cx.spawn(async move |this,cx| {
        let scrollbar_only = std::env::args().any(|arg| arg == "--scrollbar-only");
        let middle_only = std::env::args().any(|arg| arg == "--middle-only");
        let selected_actions_only = std::env::args().any(|arg| arg == "--selected-actions-only");
        let spaces_only = std::env::args().any(|arg| arg == "--spaces-only" || arg == "--space-removal-only");
        let space_removal_only = std::env::args().any(|arg| arg == "--space-removal-only");
        let labels_only = std::env::args().any(|arg| arg == "--labels-only");
        let label_removal_only = std::env::args().any(|arg| arg == "--label-removal-only");
        let reader_visibility_only = std::env::args().any(|arg| arg == "--reader-visibility-only");
        let result:Result<()> = async {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            if reader_visibility_only { return check_reader_visibility(window, &this, cx).await; }
            if label_removal_only { return label_check::check_removal(window, &this, cx).await; }
            if labels_only { return label_check::check_labels(window, &this, cx).await; }
            if space_removal_only { return space_check::check_space_removal(window, cx).await; }
            if spaces_only { return space_check::check_spaces(window, &this, cx).await; }
            if middle_only { return check_middle_selection(window, &this, cx).await; }
            if selected_actions_only { return check_selected_actions(window, &this, cx).await; }
            let original_size = window.update(cx, |_, window, _| window.viewport_size())?;
            if !scrollbar_only {
            this.update(cx, |this, _| check_two_line_rows(this))??;
            let from = window.update(cx, |root, window, cx| {
                let this = root.downcast::<MailApp>().unwrap();
                let this = this.read(cx);
                ensure!(this.bin_bounds.size.height > px(200.), "Staging bins have no measured layout");
                Ok(point(window.viewport_size().width - px(this.pane_width(window) + crate::DIVIDER / 2.), this.bin_bounds.top() + px(40.)))
            })??;
            window.update(cx, |_, window, _| {
                let to = point(from.x - px(80.), from.y);
                drag(window, from, to, point(px(-10.), px(-10.)))
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| {
                ensure!((this.sidebar_width - 400.).abs() < 1. && this.resize_drag.is_none(), "Sidebar width drag or outside release failed");
                ensure!(this.triage.selection.is_empty(), "Resize changed mail selection"); Ok(())
            })??;
            window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("right").unwrap(), cx); })?;
            this.update(cx, |this, _| { ensure!((this.sidebar_width - 384.).abs() < 1., "Sidebar keyboard resize failed"); Ok(()) })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let from = this.update(cx, |this, _| point(this.bin_bounds.left() + px(100.), this.bin_bounds.top() + px(this.bin_height() * 0.5 + crate::DIVIDER / 2.)))?;
            window.update(cx, |_, window, _| {
                let to = point(from.x, from.y + px(60.));
                drag(window, from, to, to)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let fraction = this.update(cx, |this, _| {
                ensure!(this.archive_fraction > 0.5 && this.resize_drag.is_none(), "Archive/Delete divider drag failed"); Ok(this.archive_fraction)
            })??;
            window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("up").unwrap(), cx); })?;
            this.update(cx, |this, _| { ensure!(this.archive_fraction < fraction, "Bin keyboard resize failed"); Ok(()) })??;
            let saved = this.update(cx, |this, _| {
                this.peek = Some(("digest@example.test".into(), Action::Archive));
                (this.sidebar_width, this.archive_fraction, this.triage.work())
            })?;
            window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("ctrl-shift-b").unwrap(), cx); })?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| { ensure!(!this.sidebar_visible && !this.settings.pane_visibility.staging && this.peek.is_none() && this.hovered.is_none(), "Collapse did not save visibility or left the sender peek visible"); Ok(()) })??;
            window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("ctrl-shift-b").unwrap(), cx); })?;
            this.update(cx, |this, _| {
                ensure!(this.sidebar_visible && this.settings.pane_visibility.staging && this.sidebar_width == saved.0 && this.archive_fraction == saved.1 && this.triage.work() == saved.2, "Reopening changed pane dimensions or pending decisions"); Ok(())
            })??;
            window.update(cx, |_, window, _| window.resize(gpui::size(px(1100.), px(600.))))?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap();
                ensure!(view.read(cx).pane_width(window) + crate::DIVIDER <= f32::from(window.viewport_size().width) - 420., "Resized sidebar covered the Inbox");
                check_two_line_rows(view.read(cx))
            })??;
            window.update(cx, |_, window, _| window.resize(original_size))?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |root, window, cx| {
                root.downcast::<MailApp>().unwrap().update(cx, |this, cx| {
                    check_two_line_rows(this).expect("Restoring the window width changed row height");
                    this.sidebar_width = 320.; this.archive_fraction = 0.5;
                    this.table_focus.focus(window); cx.notify();
                });
            })?;
            // Body keyboard selection shortcuts are gone. Keyboard selection uses checkbox controls.
            window.update(cx,|_,window,cx| {
                for key in ["space", "shift-space", "ctrl-space"] { window.dispatch_keystroke(Keystroke::parse(key).unwrap(),cx); }
            })?;
            this.update(cx,|this,_| {ensure!(this.demo && this.session.is_none() && this.triage.selection.is_empty(),"Old body selection gestures still run"); Ok(())})??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            for (index, expected) in [(3, 3), (4, 2), (3, 3), (3, 0), (0, 12)] {
                window.update(cx, |root, window, cx| click_row(root.downcast::<MailApp>().unwrap().read(cx), window, index, true))??;
                cx.background_executor().timer(Duration::from_millis(150)).await;
                this.update(cx, |this, _| {
                    ensure!(this.triage.selection.len() == expected, "Checkbox selection failed: expected {expected}, got {}", this.triage.selection.len());
                    ensure!(!this.reader_visible && this.active_message.is_none(), "Checkbox opened the reader"); Ok(())
                })??;
            }
            // Checkbox controls remain keyboard-accessible, without restoring table-wide shortcuts.
            for (key, expected) in [("space", 0), ("enter", 12)] {
                window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx))?;
                this.update(cx, |this, _| {
                    ensure!(this.triage.selection.len() == expected && !this.reader_visible, "Checkbox keyboard activation failed"); Ok(())
                })??;
            }
            // Show the reader explicitly; a row click then loads mail without changing checkmarks.
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| app.command(Command::ToggleReader, window, cx)))?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |root, window, cx| click_row(root.downcast::<MailApp>().unwrap().read(cx), window, 4, false))??;
            let mut browser = None;
            for _ in 0..50 {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                browser = this.update(cx, |this, cx| this.reader.as_ref().and_then(|r| r.read(cx).browser()))?;
                if let Some(ref webview) = browser { if visible(webview)? { break; } }
            }
            let browser = browser.ok_or_else(|| anyhow::anyhow!("Embedded reader never mounted"))?;
            ensure!(visible(&browser)?, "Embedded browser stayed hidden");
            let before = crate::reader_check::probe(&browser).await?;
            ensure!(before["heading"] == "Your membership has been renewed", "Embedded body failed to load: {before}");
            this.update(cx, |this, cx| {
                ensure!(this.reader_visible && this.settings.pane_visibility.reader && this.triage.selection.len() == 12 && this.active_message.is_some(), "Reading changed checkmarks or did not save reader visibility");
                ensure!(cx.windows().len() == 1, "Reading opened a detached window");
                let id = this.active_message.as_ref().unwrap();
                ensure!(this.triage.rows("").iter().any(|row| row.ids.len() == 1 && &row.ids[0] == id && row.unread),
                    "Opening an unread message changed its read status");
                Ok(())
            })??;
            // Hover first so contextual controls render before the click.
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let read_position = window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                let pos = point(this.table_bounds.right() - px(144.), this.table_bounds.top() + px(4. * 68. + 18.));
                mouse(window, WM_MOUSEMOVE, 0, pos)?; anyhow::Ok(pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            // An unchecked row's read button writes only that row, without changing checks/staging.
            let read_before = this.update(cx, |this, _| (this.triage.selection.clone(), this.triage.work(), this.active_message.clone(), this.triage.emails.len()))?;
            window.update(cx, |_, window, _| drag(window, read_position, read_position, read_position))??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| {
                ensure!(!this.failed && this.triage.rows("").iter().any(|row| row.ids.len() == 1 && row.ids.first() == read_before.2.as_ref() && !row.unread), "Read button did not clear unread status: {}", this.status);
                ensure!(this.triage.selection == read_before.0 && this.triage.work() == read_before.1 && this.active_message == read_before.2 && this.triage.emails.len() == read_before.3,
                    "Read button changed selection, staging, preview, or Inbox membership");
                ensure!(this.triage.rows("").iter().any(|row| row.unread), "Read button affected other rows");
                Ok(())
            })??;
            // The same icon button switches to Mark as unread after confirmation.
            window.update(cx, |_, window, _| drag(window, read_position, read_position, read_position))??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| {
                ensure!(!this.failed && this.triage.rows("").iter().any(|row| row.ids.len() == 1 && row.ids.first() == read_before.2.as_ref() && row.unread), "Unread icon did not restore unread status: {}", this.status);
                ensure!(this.triage.selection == read_before.0 && this.triage.work() == read_before.1 && this.active_message == read_before.2 && this.triage.emails.len() == read_before.3,
                    "Unread icon changed selection, staging, preview, or Inbox membership");
                Ok(())
            })??;
            // Tab from the row through its checkbox and labels to the read/unread icon.
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                let row = this.triage.rows("").into_iter().find(|row| row.ids.len() == 1 && row.ids.first() == read_before.2.as_ref()).unwrap();
                this.row_focus[&row.key].focus(window);
                let badges = this.row_labels(&row);
                let budget = ((f32::from(this.table_bounds.size.width) - 90.) * 0.4).clamp(34., 220.);
                let chips = badges.iter().take(crate::labels::visible_count(badges.len(), budget)).filter(|badge| badge.id.is_some()).count();
                for _ in 0..4 + chips { window.focus_next(); }
            })?;
            // Keyboard activation toggles the focused icon without activating the row.
            for (key, unread) in [("enter", false), ("space", true)] {
                window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx))?;
                cx.background_executor().timer(Duration::from_millis(150)).await;
                this.update(cx, |this, _| {
                    ensure!(this.triage.rows("").iter().any(|row| row.ids.len() == 1 && row.ids.first() == read_before.2.as_ref() && row.unread == unread)
                        && this.triage.selection == read_before.0 && this.active_message == read_before.2, "Read icon keyboard toggle failed"); Ok(())
                })??;
            }
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().read(cx).table_focus.focus(window))?;
            // Clicking the displayed row's checkbox keeps the same message loaded.
            let active = this.update(cx, |this, _| this.active_message.clone())?;
            window.update(cx, |root, window, cx| click_row(root.downcast::<MailApp>().unwrap().read(cx), window, 4, true))??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| { ensure!(this.active_message == active && this.triage.selection.len() == 13, "Displayed and checked states are not independent"); Ok(()) })??;
            let capture = cx.background_executor().spawn(async move {
                std::process::Command::new("powershell.exe").args([
                    "-NoProfile", "-File", concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"),
                    "-Demo", "-ProcessId", &std::process::id().to_string(),
                ]).output()
            }).await?;
            ensure!(capture.status.success(), "Inline reader capture failed: {}", String::from_utf8_lossy(&capture.stderr));
            std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/target/triage-window.png"), concat!(env!("CARGO_MANIFEST_DIR"), "/target/classification-layout.png"))?;
            // Uncheck the displayed message: actions on an unchecked row stay row-local.
            this.update(cx, |this, cx| { this.triage.selection.remove(active.as_ref().unwrap()); cx.notify(); })?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                let pos = point(this.table_bounds.right() - px(84.), this.table_bounds.top() + px(4. * 68. + 18.));
                drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, cx| {
                let id = active.as_ref().unwrap();
                ensure!(this.active_message == active && this.triage.marks.len() == 19 && this.triage.marks.contains_key(id)
                    && this.triage.selection.len() == 12 && this.triage.marks[id].action == Action::Archive, "Row action scope mismatch: marks={}, checked={}, active={:?}, target={:?}", this.triage.marks.len(), this.triage.selection.len(), this.active_message, this.triage.marks.get(id));
                this.triage.unmark(std::slice::from_ref(id)); cx.notify(); Ok(())
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                let pos = point(this.table_bounds.right() - px(52.), this.table_bounds.top() + px(4. * 68. + 18.));
                drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, cx| {
                let id = active.as_ref().unwrap();
                ensure!(this.active_message == active && this.triage.marks.len() == 19
                    && this.triage.marks.get(id).is_some_and(|m| m.action == Action::Trash)
                    && this.triage.selection.len() == 12, "Trash icon did not preserve row-local action scope");
                this.triage.unmark(std::slice::from_ref(id)); cx.notify(); Ok(())
            })??;
            browser.evaluate_script("window.scrollTo(0,document.body.scrollHeight)")?;
            let scrolled = crate::reader_check::probe(&browser).await?;
            ensure!(scrolled["scroll"].as_f64().unwrap_or_default() > 0., "Embedded body does not scroll");
            // Context menu sources agree; an unchecked group's actions stay row-local.
            browser.focus()?;
            let menu_row = this.update(cx, |this, _| this.triage.rows("")[3].clone())?;
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                let bounds = this.row_bounds[&menu_row.key];
                let pos = point(bounds.left() + px(160.), bounds.top() + px(18.));
                mouse(window, WM_MOUSEMOVE, 0, pos)?;
                mouse(window, WM_RBUTTONDOWN, 2, pos)?;
                mouse(window, WM_RBUTTONUP, 0, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| {
                let menu = this.row_menu.as_ref().ok_or_else(|| anyhow::anyhow!("Right-click did not open menu"))?;
                let position = crate::menu_position(menu.anchor, original_size);
                ensure!((f32::from(this.menu_bounds.top() - position.y)).abs() < 2., "Menu did not follow its row after layout changed: menu={:?}, anchor={:?}, expected={:?}", this.menu_bounds, menu.anchor, position);
                ensure!(menu.row.key == menu_row.key && menu.row.ids == menu_row.ids && this.active_message == active
                    && this.triage.selection.len() == 12, "Right-click retargeted the reader or selection"); Ok(())
            })??;
            ensure!(visible(&browser)?, "List-local menu unnecessarily hid the reader");
            let capture = cx.background_executor().spawn(async move {
                std::process::Command::new("powershell.exe").args(["-NoProfile", "-File", concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"), "-Demo", "-ProcessId", &std::process::id().to_string()]).output()
            }).await?;
            ensure!(capture.status.success(), "Menu capture failed");
            std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/target/triage-window.png"), concat!(env!("CARGO_MANIFEST_DIR"), "/target/classification-menu.png"))?;
            window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx))?;
            cx.background_executor().timer(Duration::from_millis(100)).await;
            window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("shift-f10").unwrap(), cx))?;
            this.update(cx, |this, _| { ensure!(this.row_menu.as_ref().is_some_and(|m| m.row.key == menu_row.key), "Keyboard menu targets another row"); Ok(()) })??;
            window.update(cx, |_, window, cx| {
                for key in ["down", "down", "enter"] { window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx); }
            })?;
            this.update(cx, |this, _| { ensure!(this.editing == Some((None, Action::Archive)) && this.settings.rules.is_empty() && this.triage.marks.len() == 18, "Sender editor applied before confirmation"); Ok(()) })??;
            cx.background_executor().timer(Duration::from_millis(100)).await;
            for _ in 0..5 { window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("tab").unwrap(), cx))?; }
            window.update(cx, |root, window, cx| {
                ensure!(root.downcast::<MailApp>().unwrap().read(cx).rule_input.read(cx).focus_handle.is_focused(window), "Sender-rule modal focus escaped into labels or reader"); Ok(())
            })??;
            window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx))?;
            cx.background_executor().timer(Duration::from_millis(100)).await;
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                let bounds = this.row_bounds[&menu_row.key];
                let pos = point(bounds.right() - px(22.), bounds.top() + px(18.));
                drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(100)).await;
            this.update(cx, |this, _| { ensure!(this.row_menu.as_ref().is_some_and(|m| m.row.key == menu_row.key), "Overflow menu targets another row"); Ok(()) })??;
            window.update(cx, |_, window, cx| {
                for key in ["end", "enter"] { window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx); }
            })?;
            this.update(cx, |this, _| { ensure!(this.triage.selection.len() == 15 && this.active_message == active, "Menu selection read or lost other checkmarks"); Ok(()) })??;
            window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("shift-f10").unwrap(), cx))?;
            window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx))?;
            this.update(cx, |this, cx| {
                ensure!(this.triage.marks.len() == 21 && this.triage.selection.len() == 12 && this.settings.rules.is_empty(), "Menu archive was not row-local");
                this.triage.unmark(&menu_row.ids); cx.notify(); Ok(())
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            // Sender rule confirmation is the only context action that creates a rule.
            for auto in [false, true] {
                window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| {
                    this.triage = triage::demo(); this.settings.rules.clear(); this.settings.auto_apply = auto;
                    this.open_row_menu(&menu_row.key, window, cx); this.menu_action(2, window, cx);
                    this.command(Command::SaveRule, window, cx);
                    ensure!(this.settings.rules.len() == 1 && this.editing.is_none(), "Sender rule confirmation did not save");
                    ensure!(if auto { this.triage.emails.len() == 47 && this.triage.marks.len() == 18 } else { this.triage.marks.len() == 21 }, "Sender rule ignored auto-apply");
                    this.triage = triage::demo(); this.settings.rules.clear(); this.settings.auto_apply = false; cx.notify(); Ok(())
                }))??;
                cx.background_executor().timer(Duration::from_millis(100)).await;
            }
            // Stage through real GPUI drag/drop, including a path crossing the native reader.
            for case in ["archive", "checked-trash", "partial-group", "outside", "escape", "blocked", "unknown", "stale", "auto"] {
                this.update(cx, |this, cx| {
                    this.triage = triage::demo(); this.busy = false; this.blocked = false; this.row_menu = None;
                    this.settings.auto_apply = case == "auto";
                    if matches!(case, "checked-trash" | "partial-group") {
                        let rows = this.triage.rows("");
                        this.triage.selection.extend(rows[0].ids.clone());
                        this.triage.selection.insert(rows[4].ids[0].clone());
                    }
                    cx.notify();
                })?;
                cx.background_executor().timer(Duration::from_millis(120)).await;
                let row = this.update(cx, |this, _| this.triage.rows("")[match case { "checked-trash" | "stale" => 0, "partial-group" => 3, _ => 4 }].clone())?;
                let expected_ids = this.update(cx, |this, _| this.triage.action_ids(&row))?;
                let from = this.update(cx, |this, _| { let b = this.row_bounds[&row.key]; point(b.left()+px(160.), b.top()+px(18.)) })?;
                window.update(cx, |_, window, _| {
                    mouse(window, WM_MOUSEMOVE, 0, from)?; mouse(window, WM_LBUTTONDOWN, 1, from)?;
                    mouse(window, WM_MOUSEMOVE, 1, point(from.x + px(12.), from.y))
                })??;
                cx.background_executor().timer(Duration::from_millis(120)).await;
                this.update(cx, |this, _| {
                    ensure!(this.mail_drag.as_ref().is_some_and(|d| d.ids == expected_ids), "Drag payload mismatch for {case}: expected={expected_ids:?}, actual={:?}, from={from:?}, bounds={:?}", this.mail_drag.as_ref().map(|d| &d.ids), this.row_bounds.get(&row.key));
                    ensure!(this.active_message == active, "Drag opened another message"); Ok(())
                })??;
                ensure!(!visible(&browser)?, "Native reader intercepts drag for {case}");
                this.update(cx, |this, cx| {
                    if case == "blocked" { this.blocked = true; }
                    if case == "unknown" { this.triage.marks.values_mut().next().unwrap().outcome = Some(crate::Outcome::Unknown("synthetic".into())); }
                    if case == "stale" {
                        this.triage.emails.retain(|e| e.id != expected_ids[0]);
                        this.triage.stage(&expected_ids[1..2], Action::Archive);
                    }
                    cx.notify();
                })?;
                if case == "escape" { window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx))?; }
                let baseline = this.update(cx, |this, _| (this.triage.marks.len(), this.triage.emails.len(), this.triage.undecided_ids(&expected_ids)))?;
                let action = if case == "checked-trash" { Action::Trash } else { Action::Archive };
                window.update(cx, |root, window, cx| {
                    let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                    let middle = point(this.table_bounds.right() + px(this.widths(window).0 / 2.), from.y);
                    let target = if case == "outside" { point(px(-10.),px(-10.)) } else {
                        point(this.bin_bounds.left()+px(100.), this.bin_bounds.top()+px(if action == Action::Archive { 40. } else { this.bin_height()*this.archive_fraction + crate::DIVIDER + 40. }))
                    };
                    mouse(window, WM_MOUSEMOVE, 1, middle)?; mouse(window, WM_MOUSEMOVE, 1, target)?; mouse(window, WM_LBUTTONUP, 0, target)
                })??;
                cx.background_executor().timer(Duration::from_millis(150)).await;
                this.update(cx, |this, _| {
                    let cancel = matches!(case, "outside" | "escape" | "blocked" | "unknown");
                    ensure!(this.mail_drag.is_none() && this.active_message == active && this.settings.rules.is_empty(), "Drag cleanup/read/rule failure for {case}");
                    if cancel { ensure!(this.triage.marks.len() == baseline.0 && this.triage.emails.len() == baseline.1, "Cancelled drop changed messages for {case}"); }
                    else if case == "auto" { ensure!(this.triage.marks.len() == baseline.0 && this.triage.emails.len() == baseline.1 - baseline.2.len(), "Drop ignored auto-apply"); }
                    else { ensure!(this.triage.marks.len() == baseline.0 + baseline.2.len() && baseline.2.iter().all(|id| this.triage.marks[id].action == action), "Drop target/scope incorrect for {case}"); }
                    Ok(())
                })??;
            }
            this.update(cx, |this, cx| {
                this.triage = triage::demo(); this.settings.auto_apply = false; this.blocked = false; this.busy = false;
                this.labels.unavailable = true;
                assert!(this.can_stage(), "Label failure blocked triage");
                this.labels.unavailable = false; cx.notify();
            })?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            ensure!(visible(&browser)?, "Reader did not return after drag");
            let after_interactions = crate::reader_check::probe(&browser).await?;
            ensure!(after_interactions["scroll"] == scrolled["scroll"], "Menus or drags lost reader scroll");
            // Native focus must return to the host when a pane is hidden.
            browser.focus()?;
            window.update(cx, |_, window, _| {
                let pos = point(window.viewport_size().width - px(62.), window.viewport_size().height - px(17.));
                drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            ensure!(!visible(&browser)?, "Reader dock icon left its native child visible");
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap();
                ensure!(view.read(cx).table_focus.is_focused(window), "Hide did not return logical focus to the list");
                ensure!(!view.read(cx).settings.pane_visibility.reader, "Reader dock toggle did not save hidden state");
                unsafe { ensure!(GetFocus() == GetActiveWindow(), "Native focus remained in the hidden child"); }
                let pos = point(window.viewport_size().width - px(62.), window.viewport_size().height - px(17.));
                drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let reopened = crate::reader_check::probe(&browser).await?;
            ensure!(visible(&browser)? && reopened["scroll"] == scrolled["scroll"], "Reopening lost scroll or native visibility");
            // Both right-dock buttons operate independently, including while the reader is open.
            for shown in [false, true] {
                window.update(cx, |_, window, _| {
                    let pos = point(window.viewport_size().width - px(26.), window.viewport_size().height - px(17.));
                    drag(window, pos, pos, pos)
                })??;
                cx.background_executor().timer(Duration::from_millis(150)).await;
                this.update(cx, |this, _| {
                    ensure!(this.sidebar_visible == shown && this.settings.pane_visibility.staging == shown
                        && this.reader_visible && this.settings.pane_visibility.reader && this.triage.marks.len() == 18,
                        "Staging dock icon changed reader visibility or pending work"); Ok(())
                })??;
                ensure!(visible(&browser)?, "Staging toggle hid the reader");
            }
            // Rules hide the native child. A completed load while hidden must not reveal it.
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| {
                let visibility = this.settings.pane_visibility.clone();
                this.command(Command::Rules, window, cx);
                assert_eq!(this.settings.pane_visibility, visibility, "Rules changed the saved dock visibility");
                this.reader.as_ref().unwrap().update(cx, |r, cx| r.demo(cx));
            }))?;
            cx.background_executor().timer(Duration::from_millis(700)).await;
            ensure!(!visible(&browser)?, "Message load exposed the reader over Rules");
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| this.command(Command::Rules, window, cx)))?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            ensure!(visible(&browser)?, "Returning to Inbox did not restore reader");
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                let (r, s) = this.widths(window);
                let from = point(window.viewport_size().width - px(r + s + 1.5 * crate::DIVIDER), this.bin_bounds.top() + px(40.));
                let to = point(from.x - px(40.), from.y);
                drag(window, from, to, to)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| { ensure!(this.reader_width == 520. && this.resize_drag.is_none(), "Reader divider drag failed"); Ok(()) })??;
            window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("left").unwrap(), cx))?;
            this.update(cx, |this, _| {
                ensure!(this.reader_width == 536. && this.settings.pane_widths.reader == 536, "Reader resize was not saved"); Ok(())
            })??;
            window.update(cx, |_, window, _| window.resize(gpui::size(px(1100.), px(600.))))?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            let smaller = crate::reader_check::probe(&browser).await?;
            ensure!(smaller["width"].as_u64() < before["width"].as_u64(), "Embedded browser did not resize");
            window.update(cx, |root, window, cx| {
                let this = root.downcast::<MailApp>().unwrap(); let this = this.read(cx);
                let (r, s) = this.widths(window);
                ensure!(r+s+2.*crate::DIVIDER <= f32::from(window.viewport_size().width)-420.+0.01, "Panes overflow the list");
                ensure!(this.settings.pane_widths.reader == 536, "Clamping overwrote saved preferred width"); Ok(())
            })??;
            window.update(cx, |_, window, _| window.resize(original_size))?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            browser.evaluate_script("window.scrollTo(0,80)")?;
            let layout_scroll = crate::reader_check::probe(&browser).await?;
            let layout_before = this.update(cx, |this, _| (this.triage.work(), this.triage.selection.clone(), this.active_message.clone(), this.bin_bounds, this.settings.pane_widths.clone()))?;
            // The new SVG button changes only reader placement, leaving staging on the right.
            window.update(cx, |_, window, _| {
                let pos = point(window.viewport_size().width - px(98.), window.viewport_size().height - px(17.));
                drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                let body = browser.bounds()?.position.to_logical::<f64>(window.scale_factor().into());
                ensure!(this.reader_below() && this.reader_visible && this.sidebar_visible, "Layout SVG did not place the reader below");
                ensure!(this.bin_bounds == layout_before.3 && this.table_bounds.right() < this.bin_bounds.left(), "Bottom layout moved staging");
                ensure!((body.x - f64::from(f32::from(this.table_bounds.left()))).abs() < 2.
                    && body.y > f64::from(f32::from(this.table_bounds.bottom())), "Native reader is not below the list");
                ensure!(this.triage.work() == layout_before.0 && this.triage.selection == layout_before.1
                    && this.active_message == layout_before.2 && this.settings.pane_widths == layout_before.4, "Changing placement changed mail or widths");
                check_two_line_rows(this)?;
                let from = point(this.table_bounds.left() + px(200.), this.table_bounds.bottom() + px(crate::DIVIDER / 2.));
                let to = point(from.x, from.y - px(40.));
                drag(window, from, to, to)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| { ensure!(this.reader_height == 400. && this.settings.reader_height == 400 && this.resize_drag.is_none(), "Bottom reader drag was not saved"); Ok(()) })??;
            window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("up").unwrap(), cx))?;
            this.update(cx, |this, _| { ensure!(this.settings.reader_height == 416, "Bottom reader keyboard resize failed"); Ok(()) })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let bottom = crate::reader_check::probe(&browser).await?;
            ensure!(visible(&browser)? && bottom["scroll"] == layout_scroll["scroll"], "Moving the reader lost its document scroll");
            let capture = cx.background_executor().spawn(async move {
                std::process::Command::new("powershell.exe").args([
                    "-NoProfile", "-File", concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"),
                    "-Demo", "-ProcessId", &std::process::id().to_string(),
                ]).output()
            }).await?;
            ensure!(capture.status.success(), "Bottom reader capture failed");
            std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/target/triage-window.png"), concat!(env!("CARGO_MANIFEST_DIR"), "/target/reader-below-layout.png"))?;
            // List overlays can extend into the bottom reader; its native child must not cover them.
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| {
                let key = this.triage.rows("")[0].key.clone();
                this.open_row_menu(&key, window, cx);
            }))?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            ensure!(!visible(&browser)?, "Bottom reader covered the row menu");
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| this.close_row_menu(window, cx)))?;
            window.update(cx, |_, window, _| window.resize(gpui::size(px(1100.), px(600.))))?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            this.update(cx, |this, _| {
                ensure!(this.reader_height() < 416. && this.settings.reader_height == 416
                    && this.table_bounds.size.height >= px(120.), "Bottom reader clamping lost preferred height or list space: shown={}, saved={}, table={:?}, workspace={:?}", this.reader_height(), this.settings.reader_height, this.table_bounds, this.workspace_bounds); Ok(())
            })??;
            window.update(cx, |_, window, _| window.resize(original_size))?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            window.update(cx, |_, window, _| {
                let pos = point(window.viewport_size().width - px(98.), window.viewport_size().height - px(17.));
                drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            this.update(cx, |this, _| { ensure!(!this.reader_below() && this.reader_width == 536. && this.settings.reader_height == 416, "Switching back lost independent reader dimensions"); Ok(()) })??;
            // Enter and Space activate the SVG button too, without opening a hidden reader.
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| this.command(Command::ToggleReader, window, cx)))?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |_, window, _| {
                let pos = point(window.viewport_size().width - px(98.), window.viewport_size().height - px(17.));
                drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |_, window, _| {
                window.blur();
                for _ in 0..3 { window.focus_prev(); }
            })?;
            for (key, below) in [("enter", false), ("space", true)] {
                window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx))?;
                this.update(cx, |this, _| { ensure!(this.reader_below() == below && !this.reader_visible && this.sidebar_visible, "Keyboard layout switch changed visibility or failed"); Ok(()) })??;
            }
            ensure!(!visible(&browser)?, "Layout switch reopened hidden reader");
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| {
                this.command(Command::ToggleReaderPosition, window, cx);
                this.triage.selection.clear();
                this.table_focus.focus(window); cx.notify();
            }))?;
            window.update(cx,|_,window,cx| {window.dispatch_keystroke(Keystroke::parse("ctrl-f").unwrap(),cx);})?;
            cx.background_executor().timer(Duration::from_millis(100)).await;
            for key in ["r","e","v","i","e","w"] {
                window.update(cx,|_,window,cx| {window.dispatch_keystroke(Keystroke::parse(key).unwrap(),cx);})?;
            }
            this.update(cx,|this,cx| {
                ensure!(this.filter.read(cx).content.as_ref()=="review","Native text input failed");
                ensure!(this.triage.rows(&this.filter.read(cx).content).len()==1,"Live filtering failed");Ok(())
            })??;
            window.update(cx,|_,window,cx| {
                for key in ["ctrl-a","backspace","ctrl-z"] {window.dispatch_keystroke(Keystroke::parse(key).unwrap(),cx);}
            })?;
            this.update(cx,|this,cx| {ensure!(this.filter.read(cx).content.as_ref()=="review","Input undo failed"); Ok(())})??;
            // Clear supports mouse and keyboard, including whitespace, Unicode, and scrolled text.
            let filter_before = this.update(cx, |this, _| (this.triage.rows("").len(), this.triage.work(), this.active_message.clone()))?;
            for (value, key) in [
                ("review", None), ("   ", Some("enter")),
                ("Long filter with Unicode: café 中文 and enough text to scroll horizontally", Some("space")),
            ] {
                window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| {
                    this.filter.update(cx, |input, cx| input.set(value, cx));
                    this.filter.read(cx).focus_handle.focus(window);
                }))?;
                cx.background_executor().timer(Duration::from_millis(150)).await;
                window.update(cx, |_, window, cx| {
                    if let Some(key) = key {
                        window.focus_next();
                        window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                    } else {
                        let pos = point(px(188.), px(24.));
                        drag(window, pos, pos, pos)?;
                    }
                    anyhow::Ok(())
                })??;
                cx.background_executor().timer(Duration::from_millis(150)).await;
                window.update(cx, |root, window, cx| {
                    let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                    ensure!(this.filter.read(cx).content.is_empty() && this.filter_value.is_empty(), "Filter X did not clear the text");
                    ensure!(this.filter.read(cx).focus_handle.is_focused(window), "Filter X did not return input focus");
                    ensure!(this.triage.rows("").len() == filter_before.0 && this.triage.work() == filter_before.1 && this.active_message == filter_before.2,
                        "Clearing the filter changed mail, staging, or preview");
                    window.dispatch_keystroke(Keystroke::parse("ctrl-z").unwrap(), cx); Ok(())
                })??;
                this.update(cx, |this, cx| { ensure!(this.filter.read(cx).content.as_ref() == value, "Clearing the filter could not be undone"); Ok(()) })??;
            }
            // An empty field has no clear control at its right edge; clicking there still permits typing.
            this.update(cx, |this, cx| this.filter.update(cx, |input, cx| input.set("", cx)))?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                this.filter.read(cx).focus_handle.focus(window);
                window.focus_next();
                ensure!(!this.filter.read(cx).focus_handle.contains_focused(window, cx), "Empty filter retained a clear-button tab stop");
                let pos = point(px(188.), px(24.)); drag(window, pos, pos, pos)
            })??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |_, window, cx| window.dispatch_keystroke(Keystroke::parse("x").unwrap(), cx))?;
            this.update(cx, |this, cx| { ensure!(this.filter.read(cx).content.as_ref() == "x", "Empty filter retained the clear button"); Ok(()) })??;
            window.update(cx,|root,window,cx| {
                let view=root.downcast::<MailApp>().unwrap();
                view.update(cx,|this,cx| {
                    this.filter.update(cx,|input,cx| {
                        input.set("ab",cx);
                        input.replace_and_mark_text_in_range(Some(1..1),"\u{1f680}é",Some(2..3),window,cx);
                        let range=input.selected_text_range(false,window,cx).unwrap();
                        ensure!(range.range==(3..4),"IME selection offset was not relative to inserted text");
                        ensure!(input.content.as_ref()=="a\u{1f680}éb","IME insertion failed");
                        input.unmark_text(window,cx);input.set("",cx);Ok(())
                    })?;
                    let from=this.triage.emails[0].from.clone();
                    this.save_sender_rule(from,Action::Archive,cx);
                    ensure!(this.settings.rules.len()==1 && this.triage.marks.len()==30,"Sender rule did not save and stage matches");
                    this.command(Command::Rules,window,cx);
                    this.command(Command::EditRule(0),window,cx);
                    this.rule_input.update(cx,|input,cx|input.set("studio",cx));
                    this.command(Command::SaveRule,window,cx);
                    ensure!(this.settings.rules[0].from=="studio","Rule editor failed");
                    this.command(Command::DropRule(0),window,cx);
                    this.command(Command::Rules,window,cx);
                    this.command(Command::Apply,window,cx);
                    ensure!(this.triage.marks.is_empty() && this.triage.emails.len()==20,"Demo apply failed");
                    this.triage=triage::demo();this.command(Command::Empty,window,cx);
                    ensure!(this.triage.marks.is_empty() && this.triage.emails.len()==50,"Put back all failed");
                    this.triage=triage::demo();this.status="Native synthetic checks passed.".into();cx.notify();Ok(())
                })
            })??;
            window.update(cx,|root,window,cx| {
                root.downcast::<MailApp>().unwrap().update(cx,|this,cx| {
                    let from=this.triage.emails[0].from.clone();
                    this.save_sender_rule(from,Action::Archive,cx);
                    this.command(Command::Rules,window,cx);
                    this.command(Command::EditRule(0),window,cx);
                });
            })?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            for _ in 0..5 {
                window.update(cx,|_,window,cx| {window.dispatch_keystroke(Keystroke::parse("tab").unwrap(),cx);})?;
            }
            window.update(cx,|root,window,cx| {
                let view=root.downcast::<MailApp>().unwrap();
                ensure!(view.read(cx).rule_input.read(cx).focus_handle.is_focused(window),"Rule editor tab focus escaped the modal");
                window.dispatch_keystroke(Keystroke::parse("ctrl-enter").unwrap(),cx);
                ensure!(view.read(cx).triage.marks.len()==30,"Apply shortcut ran behind the editor");
                Ok(())
            })??;
            window.update(cx,|_,window,cx| {window.dispatch_keystroke(Keystroke::parse("escape").unwrap(),cx);})?;
            this.update(cx,|this,_| {ensure!(this.editing.is_none(),"Escape did not close the rule editor");Ok(())})??;
            }
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| {
                if this.rules_view { this.command(Command::Rules, window, cx); }
                this.triage = triage::demo();
                this.triage.expanded = this.triage.emails.iter().map(|e| triage::sender_address(&e.from)).collect();
                this.command(Command::Open(this.triage.emails[0].id.clone()), window, cx);
                this.scroll.scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
                cx.notify();
            }))?;
            let mut browser = None;
            for _ in 0..50 {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                browser = this.update(cx, |this, cx| this.reader.as_ref().and_then(|r| r.read(cx).browser()))?;
                if let Some(ref webview) = browser { if visible(webview)? { break; } }
            }
            let browser = browser.ok_or_else(|| anyhow::anyhow!("Scrollbar check reader never mounted"))?;
            let scroll_before = this.update(cx, |this, _| (this.triage.work(), this.triage.selection.clone(), this.active_message.clone()))?;
            for below in [false, true] {
                window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |this, cx| {
                    if this.reader_below() != below { this.command(Command::ToggleReaderPosition, window, cx); }
                    this.scroll.scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
                    cx.notify();
                }))?;
                for _ in 0..20 {
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                    let ready = window.update(cx, |root, window, cx| {
                        let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                        let (reader, staging) = this.widths(window);
                        let width = f32::from(window.viewport_size().width) - reader - staging - crate::SCROLLBAR_WIDTH
                            - ((this.reader_visible && !below) as u8 + this.sidebar_visible as u8) as f32 * crate::DIVIDER;
                        this.inbox_scrollbar.get().is_some_and(|bar| bar.top.abs() < 1.)
                            && (f32::from(this.table_bounds.size.width) - width).abs() < 1.
                    })?;
                    if ready { break; }
                }
                window.update(cx, |root, window, cx| {
                    let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                    let bar = this.inbox_scrollbar.get().ok_or_else(|| anyhow::anyhow!("Overflowing Inbox has no scrollbar"))?;
                    ensure!(bar.track.left() == this.table_bounds.right() && bar.track.size.height == this.table_bounds.size.height
                        && bar.top.abs() < 1., "Scrollbar is not aligned with the list or starts stale: below={below}, track={:?}, table={:?}, thumb={}", bar.track, this.table_bounds, bar.top);
                    let from = point(bar.track.left() + px(6.), bar.track.top() + px(bar.height / 2.));
                    let to = point(from.x + px(50.), bar.track.bottom() + px(50.));
                    drag(window, from, to, to)
                })??;
                cx.background_executor().timer(Duration::from_millis(400)).await;
                window.update(cx, |root, window, cx| {
                    let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                    let bar = this.inbox_scrollbar.get().unwrap();
                    let offset = -f32::from(this.scroll.0.borrow().base_handle.offset().y);
                    ensure!((offset - bar.max_offset).abs() < 1. && this.inbox_scroll_drag.is_none(), "Scrollbar drag did not reach the bottom or release outside");
                    ensure!((bar.top + bar.height - f32::from(bar.track.size.height)).abs() < 1., "Thumb did not follow dragged content: top={}, height={}, track={:?}", bar.top, bar.height, bar.track);
                    let pos = point(bar.track.left() + px(6.), bar.track.top() + px(2.));
                    drag(window, pos, pos, pos)
                })??;
                cx.background_executor().timer(Duration::from_millis(400)).await;
                window.update(cx, |root, window, cx| {
                    let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                    let bar = this.inbox_scrollbar.get().unwrap();
                    let offset = -f32::from(this.scroll.0.borrow().base_handle.offset().y);
                    ensure!((offset - (bar.max_offset - f32::from(bar.track.size.height)).max(0.)).abs() < 1., "Scrollbar track click did not page up");
                    this.table_focus.focus(window);
                    window.dispatch_keystroke(Keystroke::parse("home").unwrap(), cx);
                    Ok(())
                })??;
                cx.background_executor().timer(Duration::from_millis(400)).await;
                window.update(cx, |root, window, cx| {
                    let view = root.downcast::<MailApp>().unwrap(); let this = view.read(cx);
                    ensure!(this.inbox_scrollbar.get().unwrap().top.abs() < 1., "Thumb did not follow keyboard scrolling");
                    let position = this.inbox_scrollbar.get().unwrap().track.center();
                    unsafe {
                        let hwnd = GetActiveWindow(); let mut pid = 0;
                        GetWindowThreadProcessId(hwnd, Some(&mut pid));
                        ensure!(pid == std::process::id(), "Refusing wheel input outside synthetic check window");
                        let mut screen = windows::Win32::Foundation::POINT {
                            x: (f32::from(position.x) * window.scale_factor()).round() as i32,
                            y: (f32::from(position.y) * window.scale_factor()).round() as i32,
                        };
                        ensure!(windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut screen).as_bool(), "Could not map wheel position");
                        PostMessageW(Some(hwnd), windows::Win32::UI::WindowsAndMessaging::WM_MOUSEWHEEL,
                            WPARAM(((-120i16 as u16 as u32) << 16) as usize),
                            LPARAM((u32::from(screen.x as i16 as u16) | (u32::from(screen.y as i16 as u16) << 16)) as isize))?;
                    }
                    Ok(())
                })??;
                cx.background_executor().timer(Duration::from_millis(400)).await;
                this.update(cx, |this, _| {
                    let bar = this.inbox_scrollbar.get().unwrap();
                    let offset = -f32::from(this.scroll.0.borrow().base_handle.offset().y);
                    ensure!(offset > 0. && bar.top > 0., "Wheel over scrollbar did not update list and thumb");
                    ensure!(this.triage.work() == scroll_before.0 && this.triage.selection == scroll_before.1
                        && this.active_message == scroll_before.2 && this.mail_drag.is_none(), "Scrollbar changed mail or started a mail drag"); Ok(())
                })??;
                ensure!(visible(&browser)?, "Scrollbar drag left native reader hidden");
            }
            let capture = cx.background_executor().spawn(async move {
                std::process::Command::new("powershell.exe").args([
                    "-NoProfile", "-File", concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"),
                    "-Demo", "-ProcessId", &std::process::id().to_string(),
                ]).output()
            }).await?;
            ensure!(capture.status.success(), "Inbox scrollbar capture failed");
            std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/target/triage-window.png"), concat!(env!("CARGO_MANIFEST_DIR"), "/target/inbox-scrollbar.png"))?;
            window.update(cx, |_, window, _| window.resize(gpui::size(px(1100.), px(600.))))?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            this.update(cx, |this, _| {
                let bar = this.inbox_scrollbar.get().unwrap();
                ensure!(bar.track.left() == this.table_bounds.right() && bar.track.size.height == this.table_bounds.size.height
                    && bar.top + bar.height <= f32::from(bar.track.size.height) + 1., "Scrollbar did not resize with the list"); Ok(())
            })??;
            for filter in ["Review notes", "no-synthetic-message-matches-this"] {
                this.update(cx, |this, cx| this.filter.update(cx, |input, cx| input.set(filter, cx)))?;
                cx.background_executor().timer(Duration::from_millis(400)).await;
                this.update(cx, |this, _| { ensure!(this.inbox_scrollbar.get().is_none(), "Non-overflowing or empty list retained a scrollbar"); Ok(()) })??;
            }
            Ok(())
        }.await;
        if result.is_err() {
            let _ = cx.background_executor().spawn(async move {
                std::process::Command::new("powershell.exe").args([
                    "-NoProfile", "-File", concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"),
                    "-Demo", "-ProcessId", &std::process::id().to_string(),
                ]).output()
            }).await;
        }
        let mut report=match result {Ok(())=>json!({"ok":true,"checkbox_selection":true,"expanded_group_checkbox":true,"inline_reader":true,"reading_preserves_checks":true,"preview_preserves_unread":true,"mark_read_row_local":true,"mark_unread_row_local":true,"read_icons_keyboard":true,"reader_hide_show":true,"reader_dock_icon":true,"independent_dock_icons":true,"keyboard_checkbox":true,"reader_focus_return":true,"row_action_scope":true,"trash_icon":true,"reader_scroll_preserved":true,"reader_resize":{"beside":true,"below":true,"svg_keyboard":true,"height_saved":true,"staging_stays_right":true},"preferred_width_preserved":true,"pane_visibility_saved":true,"row_menus":true,"two_line_rows_at_all_widths":true,"confirmed_sender_rules":true,"drag_scopes_and_cancel":true,"drag_across_reader":true,"drop_auto_apply":true,"labels_do_not_block_triage":true,"hidden_load_stays_hidden":true,"filter_input":true,"clear_filter":true,"clear_filter_keyboard":true,"clear_filter_undo":true,"undo":true,"ime_ranges":true,"rules":true,"staging":true,"modal_focus":true,"sidebar_resize":true,"bin_resize":true,"collapse_preserves_marks":true,"gmail_access":false}),Err(e)=>json!({"ok":false,"error":format!("{e:#}")})};
        if (scrollbar_only || middle_only || selected_actions_only || spaces_only || labels_only || reader_visibility_only || label_removal_only) && report["ok"] == true { report = json!({"ok":true,"gmail_access":false}); }
        if middle_only && report["ok"] == true {
            report["middle_selection"] = json!({"clicked_row_state":true,"filter_scope":true,"off_screen_messages":true,"partial_groups":true,"row_controls":true,"mail_unchanged":true});
        }
        if selected_actions_only && report["ok"] == true {
            report["selected_actions"] = json!({"read_unread":true,"read_archive":true,"read_archive_failures":true,"archive_trash":true,"context_menu":true,"unchecked_row_local":true,"auto_apply":true,"preview_preserved":true});
        }
        if spaces_only && report["ok"] == true {
            report["spaces"] = if space_removal_only {
                json!({"remove_saved_space":true,"pending_and_unknown_lock":true,"edit_and_write_lock":true,"unavailable_labels":true,"other_spaces_preserved":true,"mail_unchanged":true})
            } else {
                json!({"inline_create_cancel":true,"fuzzy_keyboard_multiselect":true,"drag_checked_batch":true,"ignores_auto_apply":true,"label_edit_lock":true,"missing_label_blocks_apply":true,"put_back":true,"explicit_apply":true})
            };
        }
        if labels_only && report["ok"] == true {
            report["labels"] = json!({"hover_assignment":true,"hover_target_scope":true,"hover_anchor":true,"icon_filter_bar":true,"row_chip_filters":true,"or_labels":true,"idempotent_row_chips":true,"fuzzy_filter":true,"text_intersection":true,"group_scope":true,"remove_filter_keyboard":true,"immediate_assignment":true,"inbox_unread_preserved":true,"partial_results":true,"small_window":true,"floating_picker":true,"no_layout_shift":true,"outside_dismissal":true,"reader_overlay_restore":true});
        }
        if reader_visibility_only && report["ok"] == true {
            report["reader_visibility"] = json!({"hidden_click":true,"hidden_enter":true,"visible_activation":true,"hidden_preview_preserved":true,"explicit_open":true});
        }
        if label_removal_only && report["ok"] == true {
            report["label_removal"] = json!({"single_immediate":true,"group_confirmation":true,"row_local_scope":true,"keyboard_badges":true,"cancel":true,"filtered_group_scope":true,"stale_targets":true,"partial_results":true,"hidden_checks_cleared":true});
        }
        if !middle_only && !selected_actions_only && !spaces_only && !labels_only && !reader_visibility_only && !label_removal_only && report["ok"] == true {
            report["inbox_scrollbar"] = json!({"drag":true,"track":true,"wheel_keyboard":true,"resize_filter":true,"both_layouts":true});
        }
        let _=std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"),"/target/triage-check.json"),report.to_string());
        let _=cx.update(|cx|cx.quit());
    }).detach();
}
