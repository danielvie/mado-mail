# Plan: Color hot reload

Implement `SPEC-color-hot-reload.md` without adding dependencies.

1. Add a bounded, read-only loader for the active theme's palette. Test defaults, validation, replaced files, and persistence.
2. Run that loader on GPUI's background executor about once a second. Retain the task in `MailApp` so closing the app cancels it. Apply results only if the selected theme and settings-save revision still match the read. The revision also rejects reads that predate an in-app save with unchanged colors. Show reload failures separately from mail/recovery status.
3. Add an isolated synthetic native check and document editing colors while the app runs.

## Checks and risks

- Run focused Rust tests after the loader, then the full tests and debug build after integration.
- The loader returns only `Palette`, never preferences or rules. Do not call the settings-save path during reload.
- Refresh only changed palettes; reread the path so atomic editor saves work.
- Rules-only imports must clone live settings after the file picker returns, so they cannot restore colors captured before a reload.
- Use temporary settings in native checks. Coordinate with the migration session before opening native windows.
- No shared-file work can run in parallel with the migration integration. Current migration work is in a separate worktree.

Tasks are tracked in `tasks/todo.md`.
