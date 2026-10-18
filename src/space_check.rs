//! Offline native check, using only synthetic mail and process-local input.
use super::*;

pub(super) async fn check_spaces(window: AnyWindowHandle, this: &gpui::WeakEntity<MailApp>, cx: &mut gpui::AsyncApp) -> Result<()> {
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        window.activate_window();
        app.triage.marks.clear();
        app.triage.emails.truncate(3);
        app.triage.expanded.insert("studio.updates@example.test".into());
        app.sidebar_width = 420.;
        app.settings.auto_apply = true;
        app.command(Command::NewSpace, window, cx);
        ensure!(app.space_draft.is_some() && app.editing.is_none() && app.settings.spaces.is_empty(), "New space did not create an inline draft");
        app.command(Command::SaveSpace, window, cx);
        ensure!(app.settings.spaces.is_empty(), "An empty draft was saved");
        app.space_name.update(cx, |input, cx| input.set("Work", cx));
        app.command(Command::SpacePicker, window, cx);
        app.space_query.update(cx, |input, cx| input.replace_text_in_range(None, "prj", window, cx));
        cx.notify(); Ok(())
    }))??;
    cx.background_executor().timer(Duration::from_millis(300)).await;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx); })?;
    this.update(cx, |app, _| {
        ensure!(app.space_draft.as_ref().unwrap().labels.contains("projects"), "Enter failed to select the fuzzy name match");
        Ok(())
    })??;
    this.update(cx, |app, cx| app.space_query.update(cx, |input, cx| input.set("fnc", cx)))?;
    cx.background_executor().timer(Duration::from_millis(200)).await;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx); })?;
    this.update(cx, |app, _| {
        ensure!(app.space_draft.as_ref().unwrap().labels.len() == 2, "Second search replaced the first selected label"); Ok(())
    })??;
    capture("space-inline-picker.png", cx).await?;
    window.update(cx, |_, window, cx| { window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx); })?;
    let original = window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        ensure!(app.space_draft.as_ref().is_some_and(|draft| !draft.picker && draft.labels.len() == 2), "Esc discarded the draft or failed to close the picker");
        app.command(Command::SaveSpace, window, cx);
        ensure!(app.settings.spaces.len() == 1 && app.space_draft.is_none(), "Save failed");
        app.command(Command::NewSpace, window, cx);
        app.command(Command::CancelSpace, window, cx);
        ensure!(app.space_draft.is_none() && app.settings.spaces.len() == 1, "Cancel removed a saved space");
        app.triage.selection = ["demo-0".into(), "demo-1".into()].into_iter().collect();
        cx.notify();
        Ok(app.triage.emails.iter().map(|email| (email.id.clone(), email.label_ids.clone())).collect::<Vec<_>>())
    }))??;
    cx.background_executor().timer(Duration::from_millis(300)).await;
    capture("space-before-drop.png", cx).await?;
    window.update(cx, |_, window, _| window.activate_window())?;
    cx.background_executor().timer(Duration::from_millis(200)).await;
    window.update(cx, |root, window, cx| {
        let entity = root.downcast::<MailApp>().unwrap(); let app = entity.read(cx);
        let row = app.row_bounds.get("m:demo-0").ok_or_else(|| anyhow::anyhow!("Selected child row is not laid out"))?;
        let from = point(row.left() + px(160.), row.top() + px(18.));
        mouse(window, WM_MOUSEMOVE, 0, from)
    })??;
    // GPUI hover hitboxes settle on a rendered frame before the press.
    cx.background_executor().timer(Duration::from_millis(200)).await;
    window.update(cx, |root, window, cx| {
        let entity = root.downcast::<MailApp>().unwrap(); let app = entity.read(cx);
        let row = app.row_bounds["m:demo-0"];
        mouse(window, WM_LBUTTONDOWN, 1, point(row.left() + px(160.), row.top() + px(18.)))
    })??;
    cx.background_executor().timer(Duration::from_millis(150)).await;
    window.update(cx, |root, window, cx| {
        let entity = root.downcast::<MailApp>().unwrap(); let app = entity.read(cx);
        let row = app.row_bounds["m:demo-0"];
        mouse(window, WM_MOUSEMOVE, 1, point(row.left() + px(180.), row.top() + px(18.)))
    })??;
    cx.background_executor().timer(Duration::from_millis(300)).await;
    window.update(cx, |root, window, cx| {
        let entity = root.downcast::<MailApp>().unwrap(); let app = entity.read(cx);
        ensure!(app.mail_drag.as_ref().is_some_and(|drag| drag.ids.len() == 2), "Drag did not start: payload {:?}, selection {:?}, can_stage {}, hover {:?}, mouse {:?}, row {:?}", app.mail_drag.as_ref().map(|drag| &drag.ids), app.triage.selection, app.can_stage(), app.hovered_row, window.mouse_position(), app.row_bounds["m:demo-0"]); Ok(())
    })??;
    window.update(cx, |root, window, cx| {
        let entity = root.downcast::<MailApp>().unwrap(); let app = entity.read(cx);
        let to = point(app.bin_bounds.left() + px(90.), app.bin_bounds.top() + px(244.));
        mouse(window, WM_MOUSEMOVE, 1, to)?;
        mouse(window, WM_LBUTTONUP, 0, to)
    })??;
    cx.background_executor().timer(Duration::from_millis(400)).await;
    this.update(cx, |app, _| {
        ensure!(app.triage.marks.len() == 2 && app.triage.marks.values().all(|mark| mark.action == Action::Space(1)), "Drag did not stage the selected batch in Work: {:?}", app.triage.work());
        ensure!(app.triage.selection.is_empty(), "Drop did not clear moved checkmarks");
        ensure!(app.triage.emails.iter().map(|email| (email.id.clone(), email.label_ids.clone())).collect::<Vec<_>>() == original, "Space drop wrote immediately or changed labels"); Ok(())
    })??;
    capture("space-pending.png", cx).await?;
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        app.command(Command::EditSpace(1), window, cx);
        ensure!(app.space_draft.is_none(), "Pending labels were editable");
        ensure!(!app.can_remove_space(1), "Pending space was removable");
        let pending = app.triage.work();
        app.command(Command::RemoveSpace(1), window, cx);
        ensure!(app.settings.spaces.len() == 1 && app.triage.work() == pending, "Removal discarded a space with pending mail");
        let removed = app.labels.entries.remove("finance").unwrap();
        app.command(Command::Apply, window, cx);
        ensure!(app.triage.marks.len() == 2 && app.triage.emails.len() == 3, "Missing label failed to block Apply");
        app.labels.entries.insert(removed.id.clone(), removed);
        app.command(Command::PutBack(vec!["demo-1".into()]), window, cx);
        ensure!(app.triage.marks.len() == 1 && app.triage.emails.len() == 3, "Put back changed mail");
        app.command(Command::Stage(vec!["demo-1".into()], Action::Space(1)), window, cx);
        app.command(Command::Apply, window, cx);
        ensure!(app.triage.marks.is_empty() && app.triage.emails.len() == 1 && app.triage.emails[0].id == "demo-2", "Apply did not remove only the staged emails");
        ensure!(app.settings.spaces.len() == 1, "Apply removed the reusable space");
        ensure!(app.demo && app.session.is_none(), "Check reached Gmail");
        window.resize(gpui::size(px(1100.), px(600.)));
        app.command(Command::EditSpace(1), window, cx);
        Ok(())
    }))??;
    cx.background_executor().timer(Duration::from_millis(300)).await;
    window.update(cx, |root, window, cx| {
        let entity = root.downcast::<MailApp>().unwrap(); let app = entity.read(cx);
        ensure!(app.space_draft.is_some() && app.space_query.read(cx).focus_handle.is_focused(window), "Editing did not focus the inline finder");
        ensure!(app.bin_bounds.right() <= window.viewport_size().width && app.bin_bounds.size.height > px(100.), "Small-window staging overflowed");
        Ok(())
    })??;
    capture("space-small-window.png", cx).await?;
    Ok(())
}

pub(super) async fn check_space_removal(window: AnyWindowHandle, cx: &mut gpui::AsyncApp) -> Result<()> {
    window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
        app.triage.marks.clear();
        app.command(Command::NewSpace, window, cx);
        app.space_name.update(cx, |input, cx| input.set("Work", cx));
        app.command(Command::SpaceLabel("projects".into()), window, cx);
        app.command(Command::SaveSpace, window, cx);
        ensure!(app.settings.spaces.len() == 1, "Synthetic space failed to save");
        app.command(Command::EditSpace(1), window, cx);
        app.command(Command::RemoveSpace(1), window, cx);
        ensure!(app.settings.spaces.len() == 1 && app.space_draft.is_some(), "Removal ran during a space edit");
        app.command(Command::CancelSpace, window, cx);
        let space = app.settings.spaces[0].clone();
        let id = app.triage.emails[0].id.clone();
        app.triage.stage_space(&[id.clone()], &space);
        ensure!(!app.can_remove_space(1), "Pending space was removable");
        let pending = app.triage.work();
        app.command(Command::RemoveSpace(1), window, cx);
        ensure!(app.settings.spaces.len() == 1 && app.triage.work() == pending, "Removal discarded pending mail");
        app.triage.marks.get_mut(&id).unwrap().outcome = Some(crate::triage::Outcome::Unknown("Synthetic unknown".into()));
        ensure!(!app.can_remove_space(1), "Unknown submission allowed removal");
        app.command(Command::RemoveSpace(1), window, cx);
        ensure!(app.settings.spaces.len() == 1, "Unknown submission lost its space");
        app.triage.marks.clear();
        let mut next = app.settings.clone();
        let mut other = next.spaces[0].clone(); other.id = 2; other.name = "Other".into();
        next.spaces.push(other.clone());
        ensure!(app.save_settings(next, cx), "Second synthetic space failed to save");
        let mail = app.triage.emails.iter().map(|email| (email.id.clone(), email.label_ids.clone())).collect::<Vec<_>>();
        let marks = app.triage.work(); let selection = app.triage.selection.clone();
        let preview = app.active_message.clone(); let rules = app.settings.rules.clone();
        app.busy = true;
        ensure!(!app.can_remove_space(1), "Removal enabled during a write");
        app.command(Command::RemoveSpace(1), window, cx);
        app.busy = false; app.blocked = true;
        app.command(Command::RemoveSpace(1), window, cx);
        app.blocked = false;
        ensure!(app.settings.spaces.len() == 2, "Blocked removal changed saved spaces");
        app.labels.unavailable = true;
        ensure!(app.can_remove_space(1), "Unavailable Gmail labels blocked local space removal");
        app.command(Command::RemoveSpace(999), window, cx);
        ensure!(app.settings.spaces.len() == 2, "Missing space removed another space");
        app.command(Command::RemoveSpace(1), window, cx);
        ensure!(app.settings.spaces == vec![other] && !app.can_remove_space(1), "Removal changed another custom space");
        ensure!(app.triage.emails.iter().map(|email| (email.id.clone(), email.label_ids.clone())).collect::<Vec<_>>() == mail
            && app.triage.work() == marks && app.triage.selection == selection && app.active_message == preview
            && app.settings.rules == rules && app.demo && app.session.is_none(), "Space removal changed mail, preview, staging, rules, or reached Gmail");
        app.labels.unavailable = false;
        app.command(Command::RemoveSpace(2), window, cx);
        ensure!(app.settings.spaces.is_empty(), "Last custom space was not removed");
        Ok(())
    }))??;
    cx.background_executor().timer(Duration::from_millis(200)).await;
    capture("space-removed.png", cx).await?;
    Ok(())
}

async fn capture(name: &str, cx: &mut gpui::AsyncApp) -> Result<()> {
    let output = cx.background_executor().spawn(async move {
        std::process::Command::new("powershell.exe").args([
            "-NoProfile", "-File", concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"),
            "-Demo", "-ProcessId", &std::process::id().to_string(),
        ]).output()
    }).await?;
    ensure!(output.status.success(), "Space capture failed: {}", String::from_utf8_lossy(&output.stderr));
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/target/triage-window.png"), std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/target")).join(name))?;
    Ok(())
}
