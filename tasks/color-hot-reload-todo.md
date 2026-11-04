# Tasks: Color hot reload

- [x] Read only the active theme's validated colors from a bounded settings file.
  - Verify defaults, invalid/missing files and themes, atomic replacement, unchanged preferences, and persistence with `cargo test --locked --offline settings::tests`.
  - Files: `src/settings.rs`.
- [x] Connect background polling, last-valid palette updates, and a separate nonblocking error message.
  - Verify `cargo test --locked --offline` and `cargo build --locked --offline`.
  - Files: `src/main.rs`.
  - Depends on the loader.
- [x] Verify the running app with temporary settings and synthetic mail, then update the guide.
  - Verify valid edits, invalid edits, file replacement, recovery, unchanged mail/preferences, retained colors on save, and task cancellation.
  - Files: `src/color_check.rs`, `src/main.rs`, `scripts/check-colors.ps1`, `docs/application.md`.
  - Depends on background polling.

## Completion checkpoint

- [x] Full unit tests and build pass.
- [x] Isolated native color check passes.
- [x] Documentation and final diff reviewed; unrelated work preserved.

Verification: 61 unit tests passed with `cargo test --locked --offline`; `cargo build --locked --offline` passed; `scripts/check-colors.ps1 -DebugBuild` passed. `git diff --check` passed. The native report is `target/color-check.json`.
