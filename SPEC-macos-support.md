# Spec: macOS support

## Objective

Make `task run` launch Mado Mail on macOS while preserving Windows support and the existing triage workflow. This expands the Windows-only platform scope in GOAL.md and ADR 0001; it does not authorize a UI redesign.

Assumptions:
- Support local macOS development and execution first. Signed app bundles, notarization, and distribution installers are outside this change.
- Keep the existing Rust, GPUI, Wry, and keyring dependencies. Use WKWebView on macOS and retain WebView2 on Windows.
- Keep the existing settings format and `.mado/mado-mail` directory layout under the platform's home directory. Do not copy credentials between operating systems.

## Stack and structure

- Rust 2024, gpui-pre and gpui-pre-platform 0.3.11, Wry 0.57.0, keyring 4.2.0.
- Cargo.toml and Cargo.lock own dependencies. Windows-only dependencies must not compile on macOS.
- src/main.rs owns application startup, window controls, and application shortcuts.
- src/reader.rs owns the native HTML reader. Isolate platform-specific behavior only where necessary.
- src/input.rs owns text editing and its shortcuts.
- src/settings.rs owns settings paths and persistence.
- Existing Windows-native check modules and PowerShell scripts remain Windows-only. macOS needs its own runnable synthetic reader check rather than silently skipping security verification.
- Taskfile.yml and README.md describe supported local commands and platform requirements.

## Behavior

- Launch both Gmail mode and synthetic demo mode on macOS.
- Resolve settings and OAuth client configuration under `~/.mado/mado-mail` on macOS; preserve Windows paths.
- Store refresh tokens in macOS Keychain through keyring. Do not fall back to plaintext storage.
- Render sanitized messages in WKWebView, retaining navigation restrictions and blocked downloads and popups. macOS remote images are blocked by default with explicit per-message opt-in; preserve Windows' existing enabled-by-default behavior. The explicit image toggle must continue to work.
- Supplement sanitization and CSP with native WKContentRuleList filtering. Fail closed if policy preparation fails. Production policy must never allow the synthetic custom-scheme image fixture.
- Cancel external navigation on macOS because Wry's callback lacks trusted user-initiation metadata; preserve Windows' existing external-link handling.
- Enable GPUI runtime shaders only on macOS to avoid requiring a separate build-time Metal toolchain. Do not alter host Xcode configuration.
- Keep the webview aligned with its GPUI reader pane during resizing, hiding, and detaching. Closing a detached reader must not close the Inbox.
- Preserve keyboard access to the reader, image toggle, and return to the Inbox. Provide macOS Command-key application and text-editing shortcuts without removing Windows shortcuts.
- Use working macOS window controls and an available system font without changing the triage layout.
- Make `task install` select the native executable and installation command on each supported platform.

## Commands

- Launch Gmail mode: `task run`
- Launch synthetic mail: `task demo`
- Build release: `task build`
- Check all targets: `task check`
- Run Rust tests: `task test`
- Remove artifacts: `task clean`
- List tasks: `task --list`
- macOS synthetic reader check: `task reader-check`
- Isolated startup smoke checks: `uv run --no-project python scripts/check-macos-startup.py run` and `uv run --no-project python scripts/check-macos-startup.py demo`.

## Code style and boundaries

Match existing compact Rust style and inline tests. Use target configuration for platform differences, for example:

```rust
#[cfg(target_os = "windows")]
use wry::WebViewExtWindows;
```

- Always preserve sanitization, settings recovery, and explicit Apply semantics.
- Always verify platform API assumptions against the pinned dependency documentation or source.
- Ask before adding a dependency not already required transitively, changing settings formats, or expanding to Linux.
- Never weaken reader restrictions to make the port compile.
- Never access a real Gmail account or change personal settings during automated verification. Use demo mode and temporary test data.

## Testing and success criteria

1. `task check` and `task test` pass on the available Apple Silicon macOS host with the updated stable Rust toolchain.
2. `task build` succeeds and `task run` opens a usable window without a platform startup error. Verify demo behavior separately without Gmail access.
3. A runnable synthetic check verifies WKWebView body loading, remote-image blocking and opt-in, and navigation restrictions. Record anything that requires manual verification rather than claiming it passed.
4. Verify pane visibility, resize alignment, keyboard focus, and detached-reader lifecycle on macOS where automation is available; document manual checks otherwise.
5. Windows-specific dependencies, startup behavior, and checks remain behind Windows target configuration. Report Windows runtime verification as unavailable unless a Windows host is used.
6. README.md records macOS prerequisites, settings and credential locations, commands, and remaining verification limits.

## Open questions

None for the scoped local macOS port. Intel macOS runtime validation and signed distribution are not acceptance requirements for this change.
