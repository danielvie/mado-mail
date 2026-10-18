//! Native label controls with synthetic messages only; no Gmail or personal settings.
use super::*;
use crate::{LabelMode, Outcome};

pub(super) async fn check_labels(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    check_hover_assignment(window, this, cx).await?;
    check_row_labels(window, this, cx).await?;
    check_floating_pickers(window, this, cx).await?;
    let original = window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, _| {
        window.activate_window();
        for email in &mut app.triage.emails { email.label_ids.push("INBOX".into()); }
        let original = app.triage.emails.iter().map(|email| (email.id.clone(), email.label_ids.clone())).collect::<Vec<_>>();
        app.triage.selection.insert("demo-0".into());
        app.table_focus.focus(window);
        original
    }))?;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    // With no active chips, the icon-only label control is immediately before the table.
    window.update(cx, |_, window, cx| {
        window.focus_prev();
        window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
    })?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        ensure!(app.label_picker == Some(LabelMode::Filter), "Label icon did not open the floating picker");
        app.label_query.update(cx, |input, cx| input.replace_text_in_range(None, "prj", window, cx));
        Ok(())
    }))??;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx); })?;
    this.update(cx, |app, _| {
        ensure!(app.triage.label_filter.contains("projects") && app.triage.selection.is_empty(), "Fuzzy filter failed or retained hidden checkmarks");
        ensure!(app.triage.rows("")[0].ids.len() == 2, "Filter did not narrow the sender group");
        ensure!(app.triage.emails.iter().map(|email| (email.id.clone(), email.label_ids.clone())).collect::<Vec<_>>() == original, "Filtering modified mail");
        Ok(())
    })??;
    capture("label-filter-picker.png", cx).await?;
    this.update(cx, |app, cx| app.label_query.update(cx, |input, cx| input.set("nws", cx)))?;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx); })?;
    this.update(cx, |app, _| {
        ensure!(app.triage.label_filter == ["projects".into(), "news".into()].into_iter().collect(), "Picker replaced rather than added a label");
        ensure!(app.triage.rows("").iter().filter(|row| !row.header).map(|row| row.ids.len()).sum::<usize>() == 14, "Picker labels did not use OR");
        Ok(())
    })??;
    capture("label-filter-or.png", cx).await?;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx); })?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| app.command(Command::RemoveLabelFilter("news".into()), window, cx)))?;
    // Text and labels intersect; clearing text keeps the selected label.
    this.update(cx, |app, cx| app.filter.update(cx, |input, cx| input.set("Updated schedule", cx)))?;
    cx.background_executor().timer(Duration::from_millis(200)).await;
    this.update(cx, |app, cx| {
        ensure!(app.triage.rows(&app.filter.read(cx).content)[0].ids == vec!["demo-25"], "Text and label filters did not intersect");
        app.filter.update(cx, |input, cx| input.set("", cx)); Ok(())
    })??;
    cx.background_executor().timer(Duration::from_millis(200)).await;
    // The remove badge is a real tab stop and becomes visible on keyboard focus.
    window.update(cx, |root, window, cx| {
        root.downcast::<MailApp>().unwrap().read(cx).table_focus.focus(window);
        window.focus_prev();
    })?;
    cx.background_executor().timer(Duration::from_millis(200)).await;
    capture("label-filter-remove.png", cx).await?;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx); })?;
    let pending = window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        ensure!(app.triage.label_filter.is_empty(), "Keyboard remove badge did not clear the filter");
        ensure!(app.triage.emails.iter().map(|email| (email.id.clone(), email.label_ids.clone())).collect::<Vec<_>>() == original, "Removing a filter removed Gmail labels");
        app.change_label_filter("projects".into(), cx);
        let rows = app.triage.rows("");
        app.triage.select(&rows, &rows[0].key);
        app.command(Command::TopLabelPicker(LabelMode::Assign), window, cx);
        app.label_query.update(cx, |input, cx| input.set("nws", cx));
        Ok(app.triage.work())
    }))??;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx); })?;
    this.update(cx, |app, _| {
        ensure!(app.triage.selection == ["demo-24".into(), "demo-25".into()].into_iter().collect(), "Assignment changed selection");
        ensure!(app.triage.work() == pending, "Immediate label assignment changed staging");
        for (email, (id, labels)) in app.triage.emails.iter().zip(&original) {
            let mut expected = labels.clone();
            if id == "demo-24" || id == "demo-25" { expected.push("news".into()); }
            ensure!(email.label_ids == expected, "Label assignment changed the wrong mail, INBOX, or unread: {id}");
        }
        Ok(())
    })??;
    capture("label-assignment.png", cx).await?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        app.command(Command::CloseLabelPicker, window, cx);
        app.triage.selection = ["demo-25".into()].into_iter().collect();
        app.command(Command::TopLabelPicker(LabelMode::Assign), window, cx);
        app.command(Command::PickTopLabel("hidden".into()), window, cx);
        ensure!(app.triage.emails.iter().find(|email| email.id == "demo-25").unwrap().label_ids.contains(&"hidden".into()), "Single-message label assignment failed");
        ensure!(!app.triage.emails.iter().find(|email| email.id == "demo-24").unwrap().label_ids.contains(&"hidden".into()), "Single-message assignment affected another member");
        let before = app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>();
        app.command(Command::PickTopLabel("INBOX".into()), window, cx);
        app.labels.unavailable = true;
        app.command(Command::PickTopLabel("news".into()), window, cx);
        ensure!(app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>() == before, "Unavailable or system label was assigned");
        app.labels.unavailable = false;
        // Emulate a mixed network response without touching credentials or making requests.
        app.finish_label_change("projects", vec![("demo-0".into(), Outcome::Confirmed), ("demo-1".into(), Outcome::Failed("rejected".into())), ("demo-2".into(), Outcome::Unknown("timeout".into()))], false, cx);
        ensure!(app.triage.emails.iter().find(|email| email.id == "demo-0").unwrap().label_ids.contains(&"projects".into()), "Confirmed label was not updated");
        for id in ["demo-1", "demo-2"] {
            ensure!(!app.triage.emails.iter().find(|email| email.id == id).unwrap().label_ids.contains(&"projects".into()), "Unconfirmed label appeared as success");
        }
        ensure!(app.failed && app.status.contains("1 failed, 1 unknown") && app.triage.work() == pending, "Partial failures were not distinguished");
        app.command(Command::CloseLabelPicker, window, cx);
        app.failed = false;
        app.status = "Synthetic label checks. No Gmail access.".into();
        window.resize(gpui::size(px(1100.), px(600.)));
        app.command(Command::TopLabelPicker(LabelMode::Filter), window, cx);
        Ok(())
    }))??;
    cx.background_executor().timer(Duration::from_millis(350)).await;
    this.update(cx, |app, _| {
        ensure!(app.table_bounds.size.height >= px(100.), "Label picker consumed the small-window Inbox");
        ensure!(app.demo && app.session.is_none(), "Check reached Gmail"); Ok(())
    })??;
    capture("label-small-window.png", cx).await?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        app.command(Command::CloseLabelPicker, window, cx);
        app.command(Command::Open("demo-25".into()), window, cx);
    }))?;
    let mut browser = None;
    for _ in 0..50 {
        cx.background_executor().timer(Duration::from_millis(100)).await;
        browser = this.update(cx, |app, cx| app.reader.as_ref().and_then(|reader| reader.read(cx).browser()))?;
        if let Some(ref browser) = browser { if visible(browser)? { break; } }
    }
    let browser = browser.ok_or_else(|| anyhow::anyhow!("Synthetic reader did not load"))?;
    for below in [false, true] {
        window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
            if app.reader_below() != below { app.command(Command::ToggleReaderPosition, window, cx); }
        }))?;
        cx.background_executor().timer(Duration::from_millis(250)).await;
        ensure!(visible(&browser)?, "Reader was hidden before opening picker");
        let bounds = this.update(cx, |app, _| (app.table_bounds, app.workspace_bounds, app.bin_bounds))?;
        window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| app.command(Command::TopLabelPicker(LabelMode::Assign), window, cx)))?;
        cx.background_executor().timer(Duration::from_millis(250)).await;
        ensure!(!visible(&browser)?, "Native reader would paint over the floating picker");
        this.update(cx, |app, _| {
            ensure!((app.table_bounds, app.workspace_bounds, app.bin_bounds) == bounds, "Floating picker moved a reader layout"); Ok(())
        })??;
        window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx); })?;
        cx.background_executor().timer(Duration::from_millis(250)).await;
        ensure!(visible(&browser)?, "Closing picker failed to restore native reader");
    }
    check_quick_labels(window, this, cx).await?;
    Ok(())
}

async fn check_quick_labels(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    let baseline = window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        if app.reader_visible { app.command(Command::ToggleReader, window, cx); }
        app.triage = crate::triage::demo();
        for email in &mut app.triage.emails { email.label_ids.push("INBOX".into()); }
        app.settings.label_usage.clear();
        app.finish_label_change("projects", vec![("demo-12".into(), Outcome::Confirmed), ("demo-13".into(), Outcome::Confirmed)], false, cx);
        ensure!(app.settings.label_usage["projects"].count == 1, "Batch size inflated quick-label frequency");
        let now = crate::labels::usage_time();
        for id in ["news", "studies", "finance", "hidden"] {
            app.settings.label_usage.insert(id.into(), crate::labels::Usage { count: 1, last_used: now - 3600 });
        }
        app.triage.selection = ["demo-14".into(), "demo-15".into()].into_iter().collect();
        app.command(Command::TopLabelPicker(LabelMode::Assign), window, cx);
        // Quick labels are independent of the fuzzy query.
        app.label_query.update(cx, |input, cx| input.set("nws", cx));
        Ok((app.triage.work(), app.triage.selection.clone(), app.active_message.clone(), app.settings.label_usage.clone()))
    }))??;
    cx.background_executor().timer(Duration::from_millis(300)).await;
    capture("label-quick-before.png", cx).await?;
    let click = |window: &mut gpui::Window, pos| -> Result<()> {
        mouse(window, WM_MOUSEMOVE, 0, pos)?;
        mouse(window, WM_LBUTTONDOWN, 1, pos)?;
        mouse(window, WM_LBUTTONUP, 0, pos)
    };
    let pos = this.update(cx, |app, _| point(app.label_picker_bounds.left() + px(32.), app.label_picker_bounds.top() + px(84.)))?;
    window.update(cx, |_, window, _| click(window, pos))??;
    cx.background_executor().timer(Duration::from_millis(200)).await;
    this.update(cx, |app, _| {
        ensure!(app.settings.label_usage["projects"].count == 2, "Clicking a quick label did not record one use");
        for id in ["demo-14", "demo-15"] {
            let email = app.triage.emails.iter().find(|email| email.id == id).unwrap();
            ensure!(email.label_ids.contains(&"projects".into()) && email.label_ids.contains(&"INBOX".into()), "Quick label missed targets or removed INBOX");
        }
        ensure!(app.triage.work() == baseline.0 && app.triage.selection == baseline.1 && app.active_message == baseline.2, "Quick label changed staging, selection, or preview");
        for id in ["news", "studies", "finance", "hidden"] {
            ensure!(app.settings.label_usage[id] == baseline.3[id], "Quick label changed another label's score");
        }
        Ok(())
    })??;
    // Already-applied shortcuts are disabled, not toggles or extra frequency events.
    window.update(cx, |_, window, _| click(window, pos))??;
    capture("label-quick-after.png", cx).await?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        let history = app.settings.label_usage.clone();
        app.command(Command::PickTopLabel("projects".into()), window, cx);
        app.finish_label_change("projects", vec![("demo-16".into(), Outcome::Failed("rejected".into())), ("demo-17".into(), Outcome::Unknown("timeout".into()))], false, cx);
        app.finish_label_change("projects", vec![("demo-14".into(), Outcome::Confirmed)], true, cx);
        app.command(Command::CloseLabelPicker, window, cx);
        app.command(Command::TopLabelPicker(LabelMode::Filter), window, cx);
        app.command(Command::PickTopLabel("projects".into()), window, cx);
        ensure!(app.settings.label_usage == history, "No-op, failed, unknown, removed, or filtered label changed frequency");
        ensure!(app.demo && app.session.is_none(), "Quick-label check reached Gmail");
        app.command(Command::CloseLabelPicker, window, cx);
        Ok(())
    }))??;
    Ok(())
}

async fn check_hover_assignment(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    let cases: &[(&str, &[&str], &[&str], bool)] = &[
        ("m:demo-24", &[], &["demo-24"], false),
        ("m:demo-24", &["demo-0"], &["demo-24"], true),
        ("m:demo-24", &["demo-0", "demo-24", "demo-25"], &["demo-0", "demo-24", "demo-25"], false),
        ("b:ada.chen@example.test", &["demo-0", "demo-24"], &["demo-24", "demo-25", "demo-26"], true),
    ];
    for &(key, checked, targets, keyboard) in cases {
        let before = window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
            app.triage = crate::triage::demo();
            for email in &mut app.triage.emails { email.label_ids.push("INBOX".into()); }
            app.triage.selection = checked.iter().map(|id| id.to_string()).collect();
            app.cursor = app.triage.rows("").iter().position(|row| row.key == key).unwrap();
            app.hovered_row = Some(key.into());
            app.table_focus.focus(window); window.activate_window(); cx.notify();
            (app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>(), app.triage.work(), app.triage.selection.clone())
        }))?;
        cx.background_executor().timer(Duration::from_millis(400)).await;
        capture("row-hover-actions.png", cx).await?;
        if keyboard {
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let app = view.read(cx);
                let row = app.triage.rows("").into_iter().find(|row| row.key == key).unwrap();
                let badges = app.row_labels(&row);
                let budget = ((f32::from(app.table_bounds.size.width) - 90.) * 0.4).clamp(34., 220.);
                let chips = badges.iter().take(crate::labels::visible_count(badges.len(), budget)).filter(|badge| badge.id.is_some()).count();
                app.label_focus[key].focus(window);
                for _ in 0..chips * 2 + 1 { window.focus_next(); }
                window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
            })?;
        } else {
            window.update(cx, |root, window, cx| {
                let view = root.downcast::<MailApp>().unwrap(); let app = view.read(cx);
                let pos = app.label_row_anchors[key].center();
                window.activate_window(); drag(window, pos, pos, pos)
            })??;
        }
        cx.background_executor().timer(Duration::from_millis(400)).await;
        let selection = this.update(cx, |app, cx| {
            ensure!(app.label_picker == Some(LabelMode::Assign), "Hover label icon did not open assignment picker");
            let (source, ids) = app.label_row_targets.as_ref().ok_or_else(|| anyhow::anyhow!("Row picker lost its targets"))?;
            ensure!(source == key && ids.iter().map(String::as_str).collect::<std::collections::BTreeSet<_>>() == targets.iter().copied().collect(), "Hover assignment scope is wrong");
            ensure!(app.triage.selection == before.2 && app.active_message.is_none() && !app.reader_visible, "Opening label picker changed checks or opened mail");
            ensure!(app.label_picker_bounds.top() >= app.label_row_anchors[key].bottom(), "Picker did not anchor under hover icon");
            if checked.len() == 3 { app.triage.selection = ["demo-20".into()].into_iter().collect(); }
            app.label_query.update(cx, |input, cx| input.set("hddn", cx));
            Ok(app.triage.selection.clone())
        })??;
        cx.background_executor().timer(Duration::from_millis(250)).await;
        window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx); })?;
        this.update(cx, |app, _| {
            for (email, labels) in app.triage.emails.iter().zip(&before.0) {
                let mut expected = labels.clone();
                if targets.contains(&email.id.as_str()) { expected.push("hidden".into()); }
                ensure!(email.label_ids == expected, "Hover label assignment changed the wrong message or INBOX/unread: {}", email.id);
            }
            ensure!(app.triage.selection == selection && app.triage.work() == before.1, "Hover assignment changed checks or staging");
            Ok(())
        })??;
        capture("row-label-assignment.png", cx).await?;
        window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
            app.command(Command::CloseLabelPicker, window, cx);
            ensure!(app.label_row_targets.is_none(), "Closing row picker retained its targets");
            // Reusing the toolbar picker must not reuse the last row's targets.
            app.triage.selection = ["demo-1".into()].into_iter().collect();
            let before = app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>();
            app.command(Command::TopLabelPicker(LabelMode::Assign), window, cx);
            app.command(Command::PickTopLabel("finance".into()), window, cx);
            for (email, mut expected) in app.triage.emails.iter().zip(before) {
                if email.id == "demo-1" { expected.push("finance".into()); }
                ensure!(email.label_ids == expected, "Toolbar assignment reused hover targets");
            }
            app.command(Command::CloseLabelPicker, window, cx);
            Ok(())
        }))??;
    }
    this.update(cx, |app, cx| { app.triage = crate::triage::demo(); app.hovered_row = None; app.cursor = 0; cx.notify(); })?;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    Ok(())
}

async fn check_floating_pickers(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    this.update(cx, |app, cx| { app.triage.selection = ["demo-24".into()].into_iter().collect(); cx.notify(); })?;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    let baseline = this.update(cx, |app, _| (app.table_bounds, app.workspace_bounds, app.bin_bounds, app.row_bounds.clone(), app.triage.selection.clone(), app.triage.work()))?;
    for mode in [LabelMode::Filter, LabelMode::Assign] {
        window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| app.command(Command::TopLabelPicker(mode), window, cx)))?;
        cx.background_executor().timer(Duration::from_millis(250)).await;
        window.update(cx, |root, window, cx| {
            let entity = root.downcast::<MailApp>().unwrap(); let app = entity.read(cx);
            ensure!((app.table_bounds, app.workspace_bounds, app.bin_bounds, app.row_bounds.clone()) == (baseline.0, baseline.1, baseline.2, baseline.3.clone()), "Opening a picker moved the Inbox or staging");
            let panel = app.label_picker_bounds;
            let anchor = app.label_picker_anchors[mode as usize];
            ensure!(panel.size.width > px(300.) && panel.top() >= anchor.bottom(), "Picker did not anchor below its own button");
            ensure!(panel.right() <= window.viewport_size().width && panel.bottom() <= window.viewport_size().height, "Picker overflowed the viewport");
            Ok(())
        })??;
        this.update(cx, |app, cx| app.label_query.update(cx, |input, cx| input.set("prj", cx)))?;
        cx.background_executor().timer(Duration::from_millis(150)).await;
        this.update(cx, |app, _| {
            ensure!((app.table_bounds, app.bin_bounds) == (baseline.0, baseline.2), "Changing fuzzy results resized the workspace"); Ok(())
        })??;
        if mode == LabelMode::Filter {
            window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("tab").unwrap(), cx); })?;
        } else {
            // Outside clicks dismiss the panel without activating an underlying message.
            window.update(cx, |_, window, _| window.activate_window())?;
            cx.background_executor().timer(Duration::from_millis(200)).await;
            window.update(cx, |_, window, _| mouse(window, WM_MOUSEMOVE, 0, point(px(24.), window.viewport_size().height - px(48.))))??;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            window.update(cx, |_, window, _| {
                let pos = point(px(24.), window.viewport_size().height - px(48.));
                mouse(window, WM_LBUTTONDOWN, 1, pos)?; mouse(window, WM_LBUTTONUP, 0, pos)
            })??;
        }
        cx.background_executor().timer(Duration::from_millis(250)).await;
        this.update(cx, |app, _| {
            ensure!(app.label_picker.is_none(), "Tab or outside click did not dismiss the picker");
            ensure!((app.table_bounds, app.bin_bounds) == (baseline.0, baseline.2), "Closing a picker moved the workspace");
            ensure!(app.triage.selection == baseline.4 && app.triage.work() == baseline.5 && app.active_message.is_none(), "Dismissal activated underlying mail"); Ok(())
        })??;
    }
    this.update(cx, |app, cx| { app.triage.selection.clear(); cx.notify(); })?;
    Ok(())
}

pub(super) async fn check_removal(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    let baseline = window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        window.activate_window();
        for email in &mut app.triage.emails { email.label_ids.push("INBOX".into()); }
        app.triage.add_label(&["demo-0".into()], "finance");
        app.triage.selection = ["demo-0", "demo-24", "demo-25", "demo-26"].into_iter().map(str::to_owned).collect();
        cx.notify();
        (app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>(), app.triage.selection.clone(), app.triage.work())
    }))?;
    cx.background_executor().timer(Duration::from_millis(300)).await;
    let layout = this.update(cx, |app, _| (app.table_bounds, app.bin_bounds))?;
    // The name and remove badge are separate tab stops. Finance is the second chip.
    window.update(cx, |root, window, cx| {
        root.downcast::<MailApp>().unwrap().read(cx).label_focus["b:ada.chen@example.test"].focus(window);
        for _ in 0..4 { window.focus_next(); }
        window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
    })?;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    this.update(cx, |app, _| {
        let removal = app.label_removal.as_ref().ok_or_else(|| anyhow::anyhow!("Group X did not open confirmation"))?;
        ensure!(removal.label == "finance" && removal.ids == vec!["demo-24", "demo-25"], "Group removal includes unrelated checks or members without the label");
        ensure!(app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>() == baseline.0, "Group removal wrote before confirmation");
        ensure!((app.table_bounds, app.bin_bounds) == layout, "Removal confirmation moved the layout");
        Ok(())
    })??;
    capture("label-remove-confirmation.png", cx).await?;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx); })?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        ensure!(app.label_removal.is_none() && app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>() == baseline.0, "Cancel changed labels");
        app.command(Command::RemoveRowLabel("b:ada.chen@example.test".into(), "finance".into()), window, cx);
        Ok(())
    }))??;
    cx.background_executor().timer(Duration::from_millis(200)).await;
    window.update(cx, |_, window, cx| {
        window.dispatch_keystroke(Keystroke::parse("tab").unwrap(), cx);
        window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
    })?;
    this.update(cx, |app, _| {
        ensure!(app.label_removal.is_none(), "Keyboard confirmation did not run");
        for (email, old) in app.triage.emails.iter().zip(&baseline.0) {
            let expected: Vec<_> = old.iter().filter(|label| !(matches!(email.id.as_str(), "demo-24" | "demo-25") && label.as_str() == "finance")).cloned().collect();
            ensure!(email.label_ids == expected, "Group removal changed wrong labels for {}", email.id);
        }
        ensure!(app.triage.selection == baseline.1 && app.triage.work() == baseline.2, "Removal changed unrelated checkmarks or staging");
        Ok(())
    })??;
    cx.background_executor().timer(Duration::from_millis(250)).await;
    window.update(cx, |root, window, cx| {
        root.downcast::<MailApp>().unwrap().read(cx).label_focus["m:demo-24"].focus(window);
        window.focus_next(); window.focus_next();
    })?;
    capture("label-remove-badge.png", cx).await?;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx); })?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        ensure!(app.label_removal.is_none() && !app.triage.emails.iter().find(|email| email.id == "demo-24").unwrap().label_ids.contains(&"studies".into()), "Single-message X did not remove immediately");
        ensure!(app.triage.emails.iter().find(|email| email.id == "demo-25").unwrap().label_ids.contains(&"studies".into()), "Single-message removal affected another checked member");
        ensure!(app.active_message.is_none() && !app.reader_visible && app.mail_drag.is_none(), "Remove badge opened or dragged a message");
        let before = app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>();
        app.command(Command::RemoveRowLabel("m:demo-25".into(), "INBOX".into()), window, cx);
        app.labels.unavailable = true;
        app.command(Command::RemoveRowLabel("m:demo-25".into(), "studies".into()), window, cx);
        app.labels.unavailable = false;
        ensure!(app.triage.emails.iter().map(|email| email.label_ids.clone()).collect::<Vec<_>>() == before, "System or unavailable labels were removed");
        app.triage.add_label(&["demo-24".into()], "studies");
        app.change_label_filter("projects".into(), cx);
        app.command(Command::RemoveRowLabel("b:ada.chen@example.test".into(), "studies".into()), window, cx);
        ensure!(app.label_removal.as_ref().unwrap().ids == vec!["demo-24", "demo-25"], "Filtered group removal included hidden members");
        app.command(Command::RemoveLabelFilter("projects".into()), window, cx);
        ensure!(app.label_removal.is_none(), "Changing filter retained a stale confirmation");
        app.command(Command::RemoveRowLabel("b:ada.chen@example.test".into(), "projects".into()), window, cx);
        app.triage.stage(&["demo-24".into()], Action::Archive);
        app.command(Command::ConfirmLabelRemoval, window, cx);
        ensure!(app.failed && app.triage.emails.iter().find(|email| email.id == "demo-25").unwrap().label_ids.contains(&"projects".into()), "Stale targets were removed without a new confirmation");
        app.triage.unmark(&["demo-24".into()]);
        app.change_label_filter("studies".into(), cx);
        app.triage.selection = ["demo-24", "demo-25", "demo-26"].into_iter().map(str::to_owned).collect();
        app.finish_label_change("studies", vec![("demo-24".into(), Outcome::Confirmed), ("demo-25".into(), Outcome::Failed("denied".into())), ("demo-26".into(), Outcome::Unknown("timeout".into()))], true, cx);
        ensure!(!app.triage.selection.contains("demo-24") && app.triage.selection.len() == 2, "Removal retained an invisible checked target or cleared unrelated checks");
        for id in ["demo-25", "demo-26"] { ensure!(app.triage.emails.iter().find(|email| email.id == id).unwrap().label_ids.contains(&"studies".into()), "Unconfirmed removal was shown as success"); }
        ensure!(app.failed && app.status.contains("1 failed, 1 unknown") && app.triage.work() == baseline.2, "Removal lost failure status or staging");
        ensure!(app.demo && app.session.is_none(), "Removal check reached Gmail");
        Ok(())
    }))??;
    Ok(())
}

async fn check_row_labels(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    let before = this.update(cx, |app, _| (app.triage.emails.iter().map(|e| e.label_ids.clone()).collect::<Vec<_>>(), app.triage.work(), app.triage.expanded.clone()))?;
    let key = "b:ada.chen@example.test";
    for (chip, expected) in [(0, vec!["studies"]), (0, vec!["studies"]), (1, vec!["studies", "finance"])] {
        window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
            window.activate_window();
            app.triage.selection.insert("demo-24".into());
            cx.notify();
        }))?;
        for _ in 0..20 {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if this.update(cx, |app, _| {
                let rows = app.triage.rows("");
                let index = rows.iter().position(|row| row.key == key).unwrap();
                app.row_bounds.get(key).is_some_and(|b| (f32::from(b.top() - app.table_bounds.top()) - index as f32 * 68.).abs() < 2.)
            })? { break; }
        }
        let already_active = this.update(cx, |app, _| app.triage.label_filter.contains(if chip == 0 { "studies" } else { "finance" }))?;
        window.update(cx, |root, window, cx| {
            let view = root.downcast::<MailApp>().unwrap(); let app = view.read(cx);
            let bounds = app.row_bounds[key];
            let row = app.triage.rows("").into_iter().find(|row| row.key == key).unwrap();
            let badges = app.row_labels(&row);
            let budget = ((f32::from(bounds.size.width) - 90.) * 0.4).clamp(34., 220.);
            let shown = crate::labels::visible_count(badges.len(), budget);
            let remaining = badges.len() - shown;
            let width = (budget - if remaining > 0 { 34. } else { 0. } - shown as f32 * 6.).max(0.) / shown.max(1) as f32;
            let preceding: f32 = badges.iter().take(chip).map(|badge| width.min(badge.text.chars().count() as f32 * 7. + 30.) + 6.).sum();
            let pos = point(bounds.left() + px(94. + preceding), bounds.top() + px(48.));
            drag(window, pos, pos, pos)
        })??;
        for _ in 0..20 {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if this.update(cx, |app, _| app.triage.label_filter == expected.iter().map(|s| s.to_string()).collect())? { break; }
        }
        this.update(cx, |app, _| {
            ensure!(app.triage.label_filter == expected.iter().map(|s| s.to_string()).collect(), "Message label click did not add its exact label ID");
            ensure!(if already_active { app.triage.selection.contains("demo-24") } else { app.triage.selection.is_empty() }, "Label click toggled an existing filter or kept hidden checks");
            ensure!(app.active_message.is_none() && !app.reader_visible && app.mail_drag.is_none() && app.triage.expanded == before.2, "Label click opened, expanded, or dragged mail");
            Ok(())
        })??;
    }
    this.update(cx, |app, _| {
        ensure!(app.triage.rows("").iter().filter(|row| !row.header).map(|row| row.ids.len()).sum::<usize>() == 6, "Row-label filters are not OR");
        ensure!(app.triage.work() == before.1 && app.triage.emails.iter().map(|e| e.label_ids.clone()).collect::<Vec<_>>() == before.0, "Row filtering changed mail or staging");
        Ok(())
    })??;
    capture("label-row-filters.png", cx).await?;
    // Individual chips are keyboard controls; activating an already active one does not remove it.
    window.update(cx, |root, window, cx| {
        let view = root.downcast::<MailApp>().unwrap();
        view.read(cx).label_focus[key].focus(window);
        window.focus_next();
        window.dispatch_keystroke(Keystroke::parse("space").unwrap(), cx);
    })?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        ensure!(app.triage.label_filter.len() == 2 && app.active_message.is_none(), "Keyboard label activation removed a filter or opened mail");
        app.command(Command::RemoveLabelFilter("studies".into()), window, cx);
        app.command(Command::RemoveLabelFilter("finance".into()), window, cx);
        Ok(())
    }))??;
    Ok(())
}

async fn capture(name: &str, cx: &mut gpui::AsyncApp) -> Result<()> {
    let output = cx.background_executor().spawn(async move {
        std::process::Command::new("powershell.exe").args([
            "-NoProfile", "-File", concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"),
            "-Demo", "-ProcessId", &std::process::id().to_string(),
        ]).output()
    }).await?;
    ensure!(output.status.success(), "Label capture failed: {}", String::from_utf8_lossy(&output.stderr));
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/target/triage-window.png"), std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/target")).join(name))?;
    Ok(())
}
