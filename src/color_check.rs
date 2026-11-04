//! Synthetic color reload check. Uses temporary settings, never Gmail or personal files.
use crate::{Command, MailApp, Settings, settings, theme::Palette};
use anyhow::{Result, ensure};
use gpui::{AnyWindowHandle, App, AsyncApp, Context, WeakEntity};
use serde_json::{Value, json};
use std::{fs, time::Duration};

fn unchanged_state(app: &MailApp, cx: &App) -> Value {
    let mut preferences = serde_json::to_value(&app.settings).unwrap();
    preferences.as_object_mut().unwrap().remove("themes");
    json!({
        "preferences": preferences,
        "mail": app.triage.emails.iter().map(|email| (
            &email.id, &email.from, &email.subject, &email.date, &email.snippet, &email.label_ids,
        )).collect::<Vec<_>>(),
        "selection": app.triage.selection,
        "marks": app.triage.work(),
        "expanded": app.triage.expanded,
        "active_message": app.active_message,
        "filter": app.filter.read(cx).content.as_ref(),
        "layout": [app.reader_width, app.reader_height, app.sidebar_width],
        "visibility": [app.reader_visible, app.sidebar_visible],
        "status": app.status,
        "flags": [app.failed, app.blocked, app.busy, app.demo, app.session.is_some()],
    })
}

async fn wait_for_colors(this: &WeakEntity<MailApp>, cx: &mut AsyncApp, expected: Palette, error: Option<&str>) -> Result<()> {
    for _ in 0..50 {
        cx.background_executor().timer(Duration::from_millis(100)).await;
        if this.update(cx, |app, cx| {
            app.settings.palette() == expected && Palette::current(cx) == expected
                && app.filter.read(cx).palette == Some(expected.header())
                && match error {
                    Some(fragment) => app.color_reload_error.as_ref().is_some_and(|error| error.contains(fragment)),
                    None => app.color_reload_error.is_none(),
                }
        })? { return Ok(()); }
    }
    anyhow::bail!("Color reload did not reach the expected palette/error state: {error:?}")
}

pub fn start(window: AnyWindowHandle, cx: &mut Context<MailApp>) {
    cx.spawn(async move |this, cx| {
        let result: Result<()> = async {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("settings.json");
            window.update(cx, |root, window, cx| root.downcast::<MailApp>().unwrap().update(cx, |app, cx| {
                ensure!(app.demo && app._color_reload_task.is_none(), "Demo unexpectedly watches personal settings");
                app.command(Command::ToggleReader, window, cx);
                app.triage.selection.insert("demo-0".into());
                app.status = "Synthetic operation status must remain visible.".into();
                app.failed = true;
                app.settings_path = path.clone();
                app.watch_colors(cx);
                Ok(())
            }))??;
            let before = this.update(cx, |app, cx| unchanged_state(app, cx))?;
            let baseline = this.update(cx, |app, _| app.settings.clone())?;
            let first = Palette {
                header_background: gpui::rgb(0x233044).into(),
                panel: gpui::rgb(0xdce8ed).into(),
                ..baseline.palette()
            };
            let second = Palette { header_background: gpui::rgb(0x344455).into(), accent: gpui::rgb(0x087f5b).into(), ..first };
            let mut edited = baseline.clone();
            edited.theme = "other".into();
            edited.themes.insert("other".into(), Palette::default());
            edited.themes.insert(baseline.theme.clone(), first);
            edited.auto_apply = true;
            edited.pane_visibility.reader = false;
            edited.pane_widths.reader = 800;
            edited.rules = settings::decode_rules(br#"[{"id":"ignored","from":"example","action":"trash"}]"#)?;
            edited.spaces.push(crate::spaces::Space { id: 1, name: "Ignored".into(), label_ids: vec!["projects".into()] });
            edited.save(&path)?;
            let bytes = fs::read(&path)?;
            wait_for_colors(&this, cx, first, None).await?;
            ensure!(fs::read(&path)? == bytes, "Reload rewrote settings");
            this.update(cx, |app, cx| {
                ensure!(unchanged_state(app, cx) == before, "Reload changed non-color state");
                let stale_revision = app.settings_revision;
                ensure!(app.save_settings(app.settings.clone(), cx), "Could not simulate a concurrent in-app save");
                app.reloaded_colors("other", app.settings_revision, Ok(Palette::default()), cx);
                app.reloaded_colors(&baseline.theme, stale_revision, Ok(Palette::default()), cx);
                ensure!(app.settings.palette() == first && Palette::current(cx) == first, "Stale read replaced newer colors");
                Ok(())
            })??;
            for (invalid, fragment) in [
                ("{", "Invalid settings JSON"),
                (r#"{"themes":{"frost":{"header_background":"invalid"}}}"#, "Invalid colors"),
                ("{}", "Selected theme"),
            ] {
                fs::write(&path, invalid)?;
                wait_for_colors(&this, cx, first, Some(fragment)).await?;
                ensure!(fs::read(&path)? == invalid.as_bytes(), "Invalid settings were overwritten");
                this.update(cx, |app, cx| {
                    ensure!(unchanged_state(app, cx) == before, "Reload error changed mail/recovery state");
                    Ok(())
                })??;
            }
            fs::remove_file(&path)?;
            wait_for_colors(&this, cx, first, Some("os error")).await?;
            edited.themes.insert(baseline.theme.clone(), second);
            settings::atomic_write(&path, &serde_json::to_vec(&edited)?)?;
            wait_for_colors(&this, cx, second, None).await?;
            ensure!(this.update(cx, |app, cx| unchanged_state(app, cx))? == before, "Recovery changed non-color state");
            this.update(cx, |app, cx| {
                let rules_only = Settings::decode_import(b"[]", &app.settings)?;
                ensure!(rules_only.themes == app.settings.themes, "Rules-only import lost reloaded colors");
                ensure!(app.save_settings(rules_only, cx), "In-app save rejected reloaded colors");
                app.settings.save_config(&path, &baseline)?;
                ensure!(Settings::load(&path)? == app.settings, "Later save lost reloaded colors or changed preferences");
                Ok(())
            })??;
            let pid = std::process::id().to_string();
            let capture = cx.background_executor().spawn(async move {
                std::process::Command::new("powershell.exe").args([
                    "-NoProfile", "-File", concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/check-window.ps1"),
                    "-Demo", "-ProcessId", &pid,
                ]).output()
            }).await?;
            ensure!(capture.status.success(), "Color check capture failed: {}", String::from_utf8_lossy(&capture.stderr));
            fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/target/triage-window.png"), concat!(env!("CARGO_MANIFEST_DIR"), "/target/color-reload.png"))?;
            this.update(cx, |app, _| { drop(app._color_reload_task.take()); })?;
            settings::atomic_write(&path, &serde_json::to_vec(&baseline)?)?;
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            this.update(cx, |app, cx| {
                ensure!(app.settings.palette() == second && Palette::current(cx) == second, "Dropped task kept polling");
                ensure!(app.color_reload_error.is_none() && unchanged_state(app, cx) == before, "Final state changed unexpectedly");
                Ok(())
            })??;
            Ok(())
        }.await;
        let report = match result {
            Ok(()) => json!({"ok":true,"colors_only":true,"last_valid_colors":true,"invalid_json_and_colors":true,"missing_file_and_theme":true,"atomic_replace":true,"stale_reads":true,"recovery":true,"mail_status_preserved":true,"save_preserves_colors":true,"task_cancellation":true,"personal_settings_access":false,"gmail_access":false}),
            Err(error) => json!({"ok":false,"error":format!("{error:#}")}),
        };
        let _ = fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/target/color-check.json"), report.to_string());
        let _ = cx.update(|cx| cx.quit());
    }).detach();
}
