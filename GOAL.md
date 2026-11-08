# Goal

Create a personal-use version of Mado Mail for Windows and macOS that preserves its Inbox triage workflow while running faster and using less memory than the current Electron application.

## Mental model

Gmail supplies the Inbox messages. The app groups messages by sender and lets the user filter, select, preview, and stage Archive or Delete decisions. Pending marks are temporary; sender rules and preferences persist locally. Apply changes Gmail. Undoing an unsubmitted mark does not undo a Gmail operation.

The initial version keeps the current production layout and triage-only scope. Archive and Delete preserve unread status. Delete moves messages to Trash, not permanent deletion.

## Success criteria

- Matched release-build measurements on the same Windows machine demonstrate faster triage workflows and lower total application memory use than the Electron baseline.
- Sender grouping, filtering, three-button selection, staging, previews, sender rules, and preferences retain their agreed behavior.
- Read-only message previews display HTML formatting and embedded images. Windows retains HTTPS remote images enabled by default; macOS requires per-message opt-in. Blocking cannot undo requests already sent.
- Every action is keyboard-accessible, and text inputs support the platform's standard editing and clipboard shortcuts.
- Archive and Delete preserve unread status, and failed or uncertain writes do not appear as confirmed successes.
- Existing sender rules and preferences migrate without changing rule order or actions. Authentication uses fresh sign-in rather than an assumed token migration.
- Saved authorization survives restarts in Windows Credential Manager or macOS Keychain. Access refreshes automatically; transient network failures do not discard saved sign-in.
- The release application launches and completes the triage workflow on Windows and macOS. See [the macOS scope and verification criteria](SPEC-macos-support.md); synthetic checks do not establish real-account behavior.

## Boundaries

### Non-goals

- Linux support, signed app bundles, notarization, or distribution installers.
- Public distribution or onboarding other users.
- Compose, reply, forward, attachment browsing/downloads, or a full mail client.
- Offline mode or a synchronized local mailbox.
- Redesigning the current production layout during the initial rewrite.

### Constraints

- Windows and macOS deployment for personal use.
- Rust and GPUI are mandatory; see [ADR 0001](docs/adr/0001-rust-gpui.md). [ADR 0003](docs/adr/0003-macos-support.md) expands its original Windows-only scope.
- Preserve user rules and preferences before removing the old application.
- Keep credentials out of settings exports and logs.

### Accepted limitations

- The initial Inbox fetch remains capped at 400 messages. Larger-Inbox support is outside the agreed scope.
