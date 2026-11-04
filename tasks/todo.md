# Tasks: Split reader and slim staging tray

- [x] Integrate the approved GPUI migration without losing existing changes.
  - Snapshot: `target/pre-gpui-pre-integration`. Checked migration patch applied cleanly except completed default plan files, which were archived separately.
  - `cargo check --locked --offline` passed on the combined code.
- [x] Implement the embedded split reader and persistent top Show / Hide control.
  - Individual message activation reveals the reader; group and checkbox semantics remain independent.
- [x] Replace right staging with a compact persistent summary and shallow review tray.
  - Apply and failed/unknown outcome discovery remain available when collapsed.
  - Preserve spaces, drop staging, Put back, and reconciliation safety.
- [x] Persist review state, update focused native/unit checks, and verify native reader resizing/focus.
- [x] Update docs and review all changed files, preserving unrelated work.
- [x] Run complete tests/build and synthetic native checks at the end.

## Final verification

- `cargo test --locked --offline -q`: 64 tests passed.
- `cargo check --all-targets --locked --offline -q`: passed.
- `cargo build --release --locked --offline`: passed. Executable: `target/release/mado-mail.exe`.
- All 12 sequential release native groups passed: reader, full triage, scrollbar, detach, middle selection, selected actions, spaces, space removal, labels, reader visibility, label removal, and color hot reload.
- Reports/logs: `target/final-tray-native/results.json` and per-group JSON/log files in that directory.
- Final release previews: `target/split-reader-collapsed.png`, `target/split-reader-expanded.png`, `target/split-reader-hidden.png`.
- Read-only production safety review found no substantiated defects. Manual captures verified native reader clipping above the tray and list-width restoration when hidden.
- `git diff --check` passed. Original icon files, icon documentation, and authoritative Excalidraw matched the pre-integration snapshot byte-for-byte.
- Main implementation paths: `src/main.rs`, new `src/staging_ui.rs`, `src/settings.rs`, `src/space_ui.rs`, `src/triage_check.rs`, `src/space_check.rs`, `docs/application.md`, and `CONTEXT.md`, in addition to the approved migration files.
- Tests used synthetic mail and temporary settings only. No real-account Gmail writes were exercised. The currently running installed app was not replaced or closed.

During implementation use only minimal compiler/syntax checks and focused tests. No complete suite before the final verification stage.
