# GPUI snapshot migration plan

## Scope

Migrate committed baseline `48ac218` from `gpui 0.2.2` to the exact `gpui-pre 0.3.11` package family in the `migration/gpui-pre` worktree. Preserve triage behavior, persistence formats, security restrictions, layout, and the DirectComposition workaround. Do not copy unrelated uncommitted header or icon changes from the primary checkout.

## Decisions

- Alias `gpui-pre` as `gpui` to retain imports and add the matching `gpui-pre-platform` alias for native initialization.
- Preserve `GPUI_DISABLE_DIRECT_COMPOSITION=1` before platform construction. The published Windows backend still supports it.
- Adapt call sites rather than add compatibility wrappers or refactor components.
- Review async updates individually. Strong app/entity updates now return values directly, but weak entities and window handles can still fail after closure.
- Keep the existing Taskfile and document the pinned snapshot and validation limits.

## Tasks and ownership

1. Supervisor: dependencies, lockfile, application startup and compatibility changes in `src/main.rs`.
2. Runtime worker: only `src/input.rs`, `src/reader.rs`, `src/label_ui.rs`, `src/space_ui.rs`.
3. Check worker: only `src/detach_check.rs`, `src/reader_check.rs`, `src/triage_check.rs`, `src/label_check.rs`, `src/space_check.rs`.
4. Supervisor: integrate, inspect all changes, add targeted regression coverage, and run checks.
5. Supervisor: document the dependency decision and actual results, then commit the migration on its worktree branch.

Tasks 1-3 run concurrently after package aliases are established. Workers must not edit each other's files, run GUI checks, or commit the shared worktree. Native checks run sequentially after compilation to avoid window-focus interference.

## Acceptance and verification

- `cargo check --all-targets --locked` succeeds without migration warnings.
- `cargo test --locked` passes, including color/settings serialization and text-input ranges.
- `cargo build --release --locked` succeeds, including Windows resources and shaders.
- Native synthetic reader, triage, detached-window and focused check modes pass, or each environmental limitation is recorded with evidence.
- No authentication or real Gmail operations are used for verification.
- Business logic and persistence formats remain unchanged.
- Worktree changes contain only migration, related tests, and documentation.

## Source evidence

Published sources are also available in the local Cargo registry for exact-version inspection.

- https://docs.rs/crate/gpui-pre/0.3.11/source/Cargo.toml
- https://docs.rs/crate/gpui-pre-platform/0.3.11/source/src/gpui_platform.rs
- https://docs.rs/crate/gpui-pre/0.3.11/source/src/window.rs
- https://docs.rs/crate/gpui-pre/0.3.11/source/src/app/async_context.rs
- https://docs.rs/crate/gpui-pre/0.3.11/source/src/text_system/line.rs
- https://docs.rs/crate/gpui-pre-windows/0.3.11/source/src/platform.rs

## Risks

The native child reader may expose rendering, focus, reentrancy or DPI regressions despite a successful compile. Native IME processing has changed and real IME/screen-reader behavior requires operator verification. Performance gains are not assumed.
