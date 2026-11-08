# Code context

## Entry points and modules

- `src/main.rs` starts the GPUI application and coordinates Inbox state, commands, panes, rules, settings, and background Gmail work.
- `src/triage.rs` owns sender grouping, filtering, selection, pending decisions, rule matching, and per-message write outcomes.
- `src/staging_ui.rs` renders the persistent bottom staging line and shallow review tray, including destination drop targets, counts, outcomes, and Put back controls.
- `src/gmail.rs` owns OAuth with loopback callback and PKCE, credential storage, Inbox retrieval, MIME decoding, Gmail writes, and reconciliation.
- `src/settings.rs` validates and saves preferences, separate sender rules, imports, and submission journals.
- `src/reader.rs` hosts the restricted Wry reader using WebView2 on Windows or WKWebView on macOS. `reader_macos.rs` adds native WK resource rules and keyboard routing. `message.rs` sanitizes content and defines resource/link policies. See `docs/adr/0003-macos-support.md` for platform differences.
- `src/input.rs` implements text input. `theme.rs` and `icons.rs` provide GPUI presentation assets.
- `src/labels.rs` and `label_ui.rs` provide label metadata, chips, filtering, and assignment controls.
- `src/spaces.rs` and `space_ui.rs` provide saved label destinations and staging controls.
- `src/*_check.rs` contain synthetic native UI checks.

## Triage terminology

- A message is one Gmail message, not a conversation.
- A sender group represents messages matching the current filters from one sender.
- Selection identifies command targets. Opening the reader does not select or mark a message read.
- A mark is an unsubmitted Archive, Trash, or space decision. Put back cancels it.
- A sender rule is a saved case-insensitive From substring and Archive or Trash action. The first matching rule wins; Run query stages matches.
- A space is a saved destination with user labels. Apply adds those labels and removes INBOX.
- An unknown write outcome requires Gmail reconciliation before cancellation or resubmission.

## Persistence and safety

Personal files live in `%USERPROFILE%\.mado\mado-mail` on Windows or `~/.mado/mado-mail` on macOS: `settings.json`, `rules.json`, `client_secret.json`, `gpui.lock`, and `gpui-submission.json`. Refresh tokens live in Windows Credential Manager or macOS Keychain.

Settings include the theme, Auto-apply, reader size/visibility/placement, independent `stagingReviewOpen`, and spaces. Legacy staging width/visibility values remain readable but no longer create a right sidebar. Active-theme colors reload while the Inbox is open; other settings do not. Selection, reader contents, and ordinary unsubmitted marks are session state. Interrupted submissions survive in the journal and must be reconciled. Invalid settings are reported rather than overwritten.

Archive removes INBOX; Trash is not permanent deletion. Both preserve unread status. Read/unread and direct label assignment/removal write immediately; only confirmed results update local labels. Auto-apply defaults to off. Space decisions always require Apply.

## Build and checks

Use `task run`, `task demo`, `task build`, `task test`, and `task clean`. Run native UI checks directly with the PowerShell scripts after building; see `docs/application.md` for commands. The repository root is a single Rust crate named `mado-mail`. Native check scripts live in `scripts/`; embedded UI icons live in `assets/icons/`, and synthetic mail lives in `fixtures/`. The runtime and build require no Node tooling. `build/icon.ico` is embedded by the Rust build.

See `docs/application.md` for detailed controls, settings migration, recovery, and verification limits.
