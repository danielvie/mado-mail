# Handoff: finish macOS support

## Resumed state

The user resumed from this checkpoint. The local macOS port now passes compilation, 67 Rust tests, release build/native reader checks, and isolated run/demo startup on Apple Silicon. Documentation is updated in README.md, GOAL.md, CONTEXT.md, docs/application.md, docs/project-overview.md, and docs/adr/0003-macos-support.md. The specification now explicitly preserves Windows' enabled-by-default remote images.

Independent source review found no blocking security or compatibility issue. Its Command-Shift-B finding was reproduced, corrected, and covered by native-key assertions; a stale body-focus callback was also guarded. Physical keyboard/control usability, live HTTPS image behavior, real Gmail authorization, Intel macOS, and Windows runtime checks remain unverified. See README.md and ADR 0003 for the current behavior and limits. Changes remain uncommitted.

A subsequent user screenshot exposed missing text despite the earlier smoke passes. The macOS GPUI platform had `font-kit` disabled and was using NoopTextSystem. Cargo now explicitly enables the native font backend. The reader check now rejects an empty font backend and asserts a positive system-font glyph advance. With real fonts enabled, the check also exposed native focus transfer occurring before synchronous key dispatch; focus now returns to GPUI after dispatch. A direct OS window capture was unavailable in this session, so the font/reader checks are not visual certification. An optional supplied-screenshot OCR check is documented in README.md.

The remainder of this document records the earlier stopped checkpoint, not the latest verification state.

## User intent and stop state

The original request was to fix `task run`. Reproduction failed on Rust 1.92 inside gpui-pre-util with unstable `slice_as_array`. The user ran `rustup update`, then explicitly requested macOS support and asked to use subagents. The host now has Rust 1.99.1 on Apple Silicon macOS.

The user subsequently requested that all agents stop and this handoff be saved. Do not automatically restart background work. Continue implementation only in the next session as requested.

- Workflow `a8bd778f-b91f-47fd-bef7-05734f3ecb2b` is stopped.
- Two read-only scouts completed. Implementation worker `42c3be05-8f7f-4730-b01c-273fa5d0e15d` was stopped during its final verification command.
- The planned independent reviewer never ran.
- No commits were made. Preserve the uncommitted implementation and untracked source/check files.
- A process inspection after stopping found no remaining Cargo build/run, rustc, component-download, or Mado Mail process matching this work.

## Scope and authoritative artifacts

Read `SPEC-macos-support.md` first. It records local macOS support, existing Windows behavior, reader privacy, Keychain, keyboard access, native checks, and acceptance criteria. No Linux port, UI redesign, signed app packaging, notarization, or distribution work was authorized.

Inspect the actual working-tree diff and new files rather than rebuilding from this summary. `README.md`, `GOAL.md`, and existing ADRs still describe Windows-only support. Documentation work was not completed.

## Current implementation locations

Tracked modifications:

- `Cargo.toml`, `Cargo.lock`, `build.rs`: target-specific Windows dependencies/build resources; macOS native crates and runtime shader feature.
- `Taskfile.yml`: locked Cargo commands, platform-specific install commands, and a new macOS `reader-check` task. Existing run/test/clean/default tasks remain.
- `src/main.rs`: macOS native titlebar, system font, Command shortcuts, Windows-only native checks, and macOS reader-check entry point.
- `src/input.rs`: macOS Command and Option text-editing shortcuts alongside existing bindings.
- `src/settings.rs`: platform home-variable selection, path validation, and a regression check.
- `src/gmail.rs`: platform-neutral credential-store errors. Existing keyring dependency already selects Keychain on macOS; no plaintext fallback was added.
- `src/message.rs`: macOS custom document origin and origin-policy tests.
- `src/reader.rs`: platform-gated backend setup, WK policy integration, loading/focus/lifecycle changes, and synthetic check integration.

New untracked implementation files that must not be lost:

- `src/reader_macos.rs`: WKContentRuleList compilation/application, default-blocked image policy, native keyboard monitor, navigation/popup/permission restrictions, and policy regression test.
- `src/macos_reader_check.rs`: offline native reader checks, including the latest unfinished synthetic-native-key assertions.
- `scripts/check-macos-reader.py`: isolated temporary-HOME reader-check runner with a timeout and process-group cleanup.
- `scripts/check-macos-startup.py`: temporary-HOME smoke launcher that checks only its own on-screen windows.
- `SPEC-macos-support.md`: scope and acceptance criteria.

This handoff is also new. The tracked diff at stop covered 10 files, approximately 252 insertions and 61 deletions; that count excludes all untracked files.

## Decisions made during implementation

### Native reader privacy

Wry 0.57.0 does not expose WebView2-style general HTTPS subresource interception on WKWebView. A CSP-only relaxation was rejected by the supervisor. The implementation must keep native WKContentRuleList protection in addition to the existing sanitizer and CSP, with no unguarded fallback if rule compilation fails.

Direct use of crates already transitive through Wry was approved, not arbitrary new dependencies. See the macOS target section of `Cargo.toml` for pinned objc2, WebKit, AppKit, Foundation, and block2 crates.

- macOS remote images start blocked. Existing Windows behavior starts allowed and must not be changed incidentally. The original spec assumption about the old default was incorrect; reconcile that explicitly in documentation/spec.
- macOS non-document navigation is denied instead of automatically opening an external browser, since the Wry callback lacks trusted user-initiation metadata. This behavior difference was accepted and needs documentation.
- Offline opt-in uses a raster image at `mado://localhost/check-image` only in explicit synthetic modes. Production CSP/native rules must never acquire that fixture allowance. Unit tests cover the production/synthetic separation.
- The offline custom-scheme fixture is not evidence of real HTTPS request behavior, redirect handling, or exact WebView2 parity. Native-rule URL matching, resource types, credential/internal-origin rejection, and failure behavior still need independent security review.

Pinned source evidence is available under `~/.cargo/registry/src/index.crates.io-*/`, particularly Wry 0.57.0 and objc2-web-kit 0.3.2. The completed scout reports are retention-managed session artifacts, not necessary to resume; use repository code and pinned source as durable evidence.

### Runtime shaders and Xcode blocker

Initial compilation hit a missing Xcode Metal toolchain. The approved command `xcodebuild -downloadComponent MetalToolchain` failed with exit 70 because IDESimulatorFoundation could not load `/Library/Developer/PrivateFrameworks/DVTDownloads.framework`.

No sudo, license acceptance, xcode-select change, or runFirstLaunch was authorized or attempted. Do not repair Xcode system configuration as part of this task.

Instead, the supervisor approved GPUI's existing macOS-only `gpui-pre-platform/runtime_shaders` feature, forwarded through its native backend. This avoids the build-time `metal` invocation and compiles shaders at runtime. Builds and startup subsequently succeeded on this host. Document the startup compilation tradeoff without unsupported performance claims. No separate Metal-toolchain download should be described as required if runtime shaders remain the chosen solution.

## Verification evidence and freshness

All evidence below is from the stopped worker and persisted local logs, not a final parent acceptance or independent review. Files in `target/` are ignored build artifacts and can disappear after cleaning.

| Check | Observed result | Evidence and freshness |
| --- | --- | --- |
| `task check` | Passed with warnings at an earlier source revision | `target/task-check.log`. Not rerun after subsequent reader/focus/native-check edits. |
| `task test` | 66 passed, 0 failed, 0 ignored | `target/task-test.log`. Also predates later edits. |
| `task build` | Release build succeeded | `target/task-build.log`. Later smoke tests rebuilt successfully, but the final edits remain unverified. |
| Native reader check | Earlier revision produced `passed: true` | `target/macos-reader-check.json`. Covers sanitized body, embedded image, blocked/allowed/reblocked fixture, independent native rule blocking, navigation cancellation, resizing, focus return, Command editing, detached independence, and stale completion. Predates latest synthetic-native-key assertions. |
| `python3 scripts/check-macos-startup.py run` | Passed | Worker transcript recorded an on-screen window with temporary HOME and no Gmail connection. `target/startup-run.log` contains startup/build output. |
| `python3 scripts/check-macos-startup.py demo` | Passed | Same isolated window check; see `target/startup-demo.log`. |
| `task --list`, install dry run | Task list and macOS install command rendered correctly | Worker transcript. No real install was attested. |
| Final `task reader-check` | Interrupted, not passed | `target/task-reader-check.log` ends during compilation; worker tool result says `Command aborted`. |

The startup logs end with SIGTERM/exit 143 because the smoke harness intentionally terminates its own process group after observing a live window. These terminations were expected, not startup failures.

Immediately before stopping, the worker enabled additional AppKit features in `Cargo.toml` and edited `src/macos_reader_check.rs` to send synthetic NSEvents for Option-I, Escape, and Command-F through the native keyboard monitor. Those edits are newer than the saved passing JSON. Rebuild and rerun; do not cite the old JSON as proof of the latest check.

Earlier checks/builds warned about platform-specific unused imports/variables and helper dead code. Some warnings have already been addressed since those logs, so inspect current warnings before changing anything.

No Windows runtime validation, real Gmail sign-in, live Keychain authorization round trip, physical-keyboard traversal, visual alignment inspection, or native control usability was attested. Testing must not connect a real Gmail account or overwrite personal settings. Existing credential tests should be inspected for isolated test identities and cleanup before rerunning.

## Next actions

1. Inspect `git status`, the diff, and all new files. Preserve the stopped partial implementation.
2. Run `task check`, `task test`, and `task reader-check` against the latest source. The reader task builds release first and its Python runner isolates HOME. Fix only demonstrated failures.
3. Recheck startup with the temporary-HOME smoke script for both run and demo. Prefer `uv` when invoking Python in new commands, per project instructions; the current Taskfile uses `python3`.
4. Obtain the independent review that never ran, especially native rule policy, scheme/fixture trust boundaries, callback lifetime, keyboard routing, detached teardown, target cfg, and Windows regressions. Delegation was previously requested, but respect the user's explicit stop until the next session resumes work.
5. Finish README and an ADR matching `docs/adr/` conventions. Record expanded platform scope, runtime shaders, macOS blocked-by-default images versus Windows behavior, denied external-link navigation, exact check commands, and verification limits. Reconcile `SPEC-macos-support.md` and current platform statements without rewriting unrelated history.
6. Verify physical keyboard/native controls and visual behavior manually if available. Keep HTTPS/privacy and Windows limitations explicit rather than claiming parity from the offline fixture.

## Suggested skills

- `incremental-implementation`: finish in thin, verified slices.
- `source-driven-development`: verify pinned Wry, GPUI, WebKit, and AppKit APIs.
- `frontend-ui-engineering`: native focus, keyboard access, and window behavior.
- `documentation-and-adrs`: document the platform expansion and security/runtime tradeoffs.
- `pi-subagents`: if the resumed session is authorized to delegate the unfinished review or implementation.
- `clear-explanations` and `unslop`: concise, accurate reporting without overstating verification.
