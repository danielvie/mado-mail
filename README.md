# Mado Mail

A Windows Gmail triage app built with Rust and GPUI. The repository root is a single Rust crate; application code lives in `src/`.

## Requirements

- Stable Rust.
- Visual Studio C++ build tools with a Windows SDK.
- Microsoft Edge WebView2 Runtime for the restricted HTML reader.
- Task, optional. Cargo commands work directly.

## Run and check

```text
task run           # Build and launch with Gmail
task demo          # Synthetic Inbox, no Gmail access or personal settings writes
task build         # Release executable in target/release/mado-mail.exe
task install       # Build release and copy mado-mail.exe to ~/.local/bin
task check         # Check all Rust targets with locked dependencies
task test          # Model, settings, OAuth, MIME, and synthetic HTTP tests
task clean         # Remove Rust build artifacts
task --list        # List tasks
```

Without Task, run `cargo run --release`. Cargo fetches the required Rust dependencies.

## Gmail setup

1. Enable Gmail API in your Google Cloud project and create a Desktop app OAuth client.
2. Save the downloaded JSON at `%USERPROFILE%\.mado\mado-mail\client_secret.json`.
3. Add your account as a test user if the consent app is in Testing.
4. Run the app and choose Connect Gmail. It requests `gmail.modify` permission.

Refresh tokens use Windows Credential Manager. Settings, sender rules, and submission recovery files live in `%USERPROFILE%\.mado\mado-mail`. Removing the old project does not remove personal settings or credentials. Existing exported settings can still be imported through Rules.

## Documentation

- [Application guide](docs/application.md): controls, settings, recovery, and checks.
- [Project overview](docs/project-overview.md): workflows, architecture, and limitations.
- [Code context](CONTEXT.md): module responsibilities and triage terminology.
- [GPUI snapshot decision](docs/adr/0002-gpui-pre.md): pinned packages, native startup, and update requirements.
- [GPUI migration results](docs/research/gpui-pre-migration.md): compatibility changes, native check repairs, and verification limits.
- [App icon](docs/app-icon.md): Windows icon assets.

Design, research, and `docs/history/` documents retain migration history, including paths and names from earlier snapshots. Matched speed and total-memory comparisons against the former Electron app have not been performed.
