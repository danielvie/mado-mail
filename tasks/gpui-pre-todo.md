# GPUI snapshot migration tasks

## 1. Dependencies and application compatibility

- [x] Pin matching core/platform snapshot packages and regenerate Cargo.lock.
- [x] Adapt startup, focus, lifecycle and scrolling in main.rs without changing behavior.
- [x] Verify with integrated cargo check.

## 2. Runtime components, parallel worker

- [x] Adapt input, reader, label and space UI APIs.
- [x] Preserve WebView security, child-window lifecycle and input behavior.
- [x] Verify changes against published 0.3.11 source and compiler diagnostics.

## 3. Native checks, parallel worker

- [x] Adapt check APIs and async Result handling without weakening assertions.
- [x] Preserve synthetic-only execution and window-lifetime failures.
- [x] Verify native checks compile with the migrated application.

## 4. Integration checkpoint

- [x] Inspect worker diffs and address remaining compiler diagnostics.
- [x] All-target check and 56 unit tests pass.
- [x] Release build passes.
- [x] All eleven native check groups pass sequentially after harness repairs.

## 5. Delivery

- [x] Document package provenance, startup requirements and verified results.
- [x] Review final diff for unrelated changes and record remaining manual checks.
- [x] Commit migration on its worktree branch without merging into main.
