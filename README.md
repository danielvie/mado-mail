# Mado Mail

A Windows and macOS Gmail triage app built with Rust and GPUI. The repository root is a single Rust crate; application code lives in `src/`.

## Requirements

- Current stable Rust. The macOS port was verified with Rust 1.99.1; Rust 1.92 cannot compile the pinned GPUI dependencies.
- Windows: Visual Studio C++ build tools with a Windows SDK, and Microsoft Edge WebView2 Runtime.
- macOS: Xcode Command Line Tools and Python 3 for native checks. WKWebView ships with macOS. GPUI compiles its Metal shaders at runtime, so a separate build-time Metal toolchain is not required.
- Task, optional. Cargo commands work directly.

## Run and check

```text
task run           # Build and launch with Gmail
task demo          # Synthetic Inbox, no Gmail access or personal settings writes
task build         # target/release/mado-mail on macOS, mado-mail.exe on Windows
task install       # Build release and copy the native executable to ~/.local/bin
task reader-check  # macOS only: isolated offline native reader/security checks
task check         # Check all Rust targets with locked dependencies
task test          # Model, settings, OAuth, MIME, and synthetic HTTP tests
task clean         # Remove Rust build artifacts
task --list        # List tasks
```

Without Task, run `cargo run --release --locked`. Cargo fetches the required Rust dependencies.

On macOS, Command-R refreshes, Command-F focuses the filter, Command-L opens Rules, Command-Shift-B toggles staging review, and Command-Enter applies pending work outside text inputs. Standard Command editing shortcuts work in text inputs. Option-I toggles images in the reader; Escape returns focus to the Inbox.

The macOS reader blocks remote images by default. Explicitly allowing images can disclose opens and IP addresses. Windows retains its existing default of allowing HTTPS images. macOS cancels links rather than opening an external browser; Windows still opens user-initiated links externally.

## Gmail setup

1. Enable Gmail API in your Google Cloud project and create a Desktop app OAuth client.
2. Save the downloaded JSON at `~/.mado/mado-mail/client_secret.json` on macOS or `%USERPROFILE%\.mado\mado-mail\client_secret.json` on Windows.
3. Add your account as a test user if the consent app is in Testing.
4. Run the app and choose Connect Gmail. It requests `gmail.modify` permission.

Refresh tokens use macOS Keychain or Windows Credential Manager, never plaintext settings. Settings, sender rules, and submission recovery files live in `~/.mado/mado-mail` on macOS or `%USERPROFILE%\.mado\mado-mail` on Windows. Removing the old project does not remove personal settings or credentials. Existing exported settings can still be imported through Rules.

## Verification limits

Apple Silicon macOS checks cover compilation, Rust tests, native font availability/glyph metrics, isolated run/demo startup, and native reader behavior. `task reader-check` uses a synthetic-only custom-scheme image, not a live HTTPS server. Physical keyboard traversal, visual alignment, native window controls, real Gmail/Keychain authorization, Intel macOS, and Windows runtime regression checks still require operator verification. No signed app bundle or notarized installer is provided.

For a supplied app-window screenshot, `swift scripts/check-macos-text.swift /path/to/window.png` checks that Rules and Refresh text is visible using macOS Vision. It does not capture the screen or inspect other windows.

## Documentation

- [Application guide](docs/application.md): controls, settings, recovery, and checks.
- [Project overview](docs/project-overview.md): workflows, architecture, and limitations.
- [Code context](CONTEXT.md): module responsibilities and triage terminology.
- [macOS support decision](docs/adr/0003-macos-support.md): native reader policy, platform differences, and runtime shaders.
- [GPUI snapshot decision](docs/adr/0002-gpui-pre.md): pinned packages, native startup, and update requirements.
- [GPUI migration results](docs/research/gpui-pre-migration.md): compatibility changes, native check repairs, and verification limits.
- [App icon](docs/app-icon.md): Windows icon assets.

Design, research, and `docs/history/` documents retain migration history, including paths and names from earlier snapshots. Matched speed and total-memory comparisons against the former Electron app have not been performed.
