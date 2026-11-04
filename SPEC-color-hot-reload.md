# Spec: Color hot reload

## Objective
Apply saved color edits in `settings.json` without restarting Mado Mail. This includes `themes.<name>.header_background`. Hot reload covers colors only.

## Behavior and success criteria
- Detect saved changes within about one second, including editor saves that replace the file.
- Reload color values for the currently selected theme without switching themes.
- Refresh the Inbox, header, text fields, and open reader controls. Email HTML keeps its sender's styling.
- Keep the last valid colors if the file is missing, temporarily incomplete, invalid, or lacks the selected theme. Show a nonblocking reload error without replacing mail-operation or recovery status. Clear it after a valid save.
- Never enable Auto-apply, change rules or spaces, move panes, alter selection or staging, or perform Gmail requests as part of color reload.
- Reloading must not write to the configuration file. Later in-app saves must retain successfully reloaded colors.
- Demo and automated checks must not watch personal settings.

## Stack and commands
Rust, the repository's GPUI version, and existing Serde settings validation. No new dependency is planned.

```text
cargo test --locked --offline
cargo build --locked --offline
powershell.exe -NoProfile -File scripts/check-colors.ps1 -DebugBuild
```

## Project structure and code style
Keep configuration parsing and reload tests in `src/settings.rs`; application lifetime and palette refresh belong in `src/main.rs`. Continue using `Palette::apply` from `src/theme.rs`. Update `docs/application.md` with the supported editing workflow.

Match the existing style and palette update pattern:

```rust
self.settings.palette().apply(cx);
cx.notify();
```

## Testing strategy
Use temporary configuration files to cover valid edits, missing color defaults, invalid JSON/colors, missing selected themes, file replacement, and recovery after a valid save. Verify that non-color preferences and mail state do not change and that reloaded colors survive an in-app save. Check the live update with synthetic mail and an isolated temporary configuration, never personal mail or settings.

## Boundaries
- Always validate colors, preserve the last valid palette, and keep file reads off the UI thread.
- Ask first before adding a dependency or extending reload to non-color settings.
- Never weaken existing safety checks or read/write personal settings in synthetic checks.

## Out of scope
Reloading the selected theme name, rules, Auto-apply, pane layout, spaces, or credentials. Recoloring email HTML. Adding a settings editor.

## Open questions
None. The proposed reload interval is about one second.
