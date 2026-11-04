# GPUI snapshot migration results

## Scope

The `migration/gpui-pre` worktree migrates committed baseline `48ac218` from `gpui 0.2.2` to `gpui-pre 0.3.11` and `gpui-pre-platform 0.3.11`. Uncommitted work in the primary checkout is excluded.

Both packages are exact-pinned and aliased as `gpui` and `gpui_platform`. Native startup uses `gpui_platform::application()`. The DirectComposition opt-out remains set before initialization so the native Wry/WebView2 reader is visible. See [ADR 0002](../adr/0002-gpui-pre.md) for provenance and update policy.

## Compatibility changes

- Pass App context to focus, blur and tab traversal. Clone handles obtained through App reads before borrowing App mutably.
- Adapt app and strong-entity async updates to direct return values. Preserve fallible weak-entity and window-handle updates.
- Supply left alignment and no width constraint when painting the horizontally scrolled custom input line.
- Borrow the uniform-list scroll handle and use the current scroll-event names.
- Accept the window ID in the window-close callback.

The Gmail, message sanitization, triage and persistence implementations are unchanged. A new unit test covers UTF-16 offsets inside surrogate pairs, empty strings and oversized offsets.

## Native check repairs

Native testing exposed stale harness assumptions, which were corrected without changing production mail-action behavior or removing existing assertions:

- Mouse events now target the HWND belonging to the supplied GPUI Window, not whichever window happens to be active. The helper validates Win32, current-process ownership and `--triage-check` mode. Input remains process-local; it does not move the desktop pointer or use global input injection.
- Selected-row action checks allow contextual hover hitboxes to render before pressing the button.
- Keyboard traversal counts both the filter and remove controls on each visible label chip.
- The row-local menu check explicitly deselects its group before Archive. Fully checked rows intentionally target the checked batch, which the separate selected-actions check covers. The full suite also verifies menu deselection preserves unrelated checkmarks and the preview.

## Verified

The installed Windows Rust 1.95 toolchain completed:

```text
cargo check --all-targets --locked
cargo test --locked
cargo build --release --locked
task --list
```

All-target checking and the release build passed without warnings. All 56 unit tests passed. The release build includes the Windows resources and native renderer build.

All eleven native check groups passed sequentially on the final source:

| Group | Invocation |
| --- | --- |
| Reader | `powershell.exe -NoProfile -File scripts/check-reader.ps1` |
| Full triage | `powershell.exe -NoProfile -File scripts/check-triage.ps1` |
| Scrollbar | Same triage script with `-ScrollbarOnly` |
| Detached readers | Same triage script with `-DetachOnly` |
| Middle-click selection | Same triage script with `-MiddleOnly` |
| Selected-row actions | Same triage script with `-SelectedActionsOnly` |
| Spaces | Same triage script with `-SpacesOnly` |
| Space removal | Same triage script with `-SpaceRemovalOnly` |
| Labels | Same triage script with `-LabelsOnly` |
| Reader visibility | Same triage script with `-ReaderVisibilityOnly` |
| Label removal | Same triage script with `-LabelRemovalOnly` |

The reader check verifies child visibility in parent-window captures, image controls, script and host-object restrictions, stale-completion rejection, resizing and scrolling. Full triage covers keyboard input, synthetic IME ranges, menus, batch scope, drag cancellation and reader focus/visibility across layouts. The detached-reader check covers independent content and controls, closing each window, and keeping a detached reader alive after the Inbox closes.

Run the native groups sequentially so they do not compete for focus. Final local reports and synthetic captures were retained under `target/migration-native/`, including `summary.json`. They are build artifacts, not tracked files. No credentials were loaded and no real Gmail operations were performed.

## Limits

Synthetic IME range checks do not establish real Windows IME composition or screen-reader behavior. Real-account writes and persisted personal settings still require operator verification. No matched performance or total-memory comparison was performed, and no performance improvement is claimed.
