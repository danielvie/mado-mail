# Mado Mail project overview

Mado Mail is a Windows and macOS Gmail Inbox-triage application built with Rust and GPUI. It groups mail by sender, supports batch decisions, and provides a restricted HTML reader. The repository root is a single Rust crate named `mado-mail`.

## User workflow

1. Connect Gmail using a Desktop OAuth client and `gmail.modify` consent.
2. Load up to 400 Inbox messages, grouped by sender and ordered by count.
3. Filter locally by sender, subject, snippet, or selected user labels.
4. Expand sender groups and preview messages without changing their unread state.
5. Select messages with checkboxes or middle-click select-all/clear-all.
6. Stage Archive, Trash, or space decisions using row actions, toolbar controls, or drag-and-drop.
7. Apply pending decisions and reconcile uncertain results before retrying.

The reader can sit beside or below the list, be hidden, or open pinned messages in separate windows. Pane sizes, visibility, and placement persist across launches.

Read/unread and direct user-label assignment/removal write immediately. Auto-apply requires confirmation and makes subsequent Archive/Trash actions immediate. Space decisions remain staged until Apply.

## Domain model

| Concept | Meaning | Lifetime |
| --- | --- | --- |
| Message | One Gmail message represented in the workspace | Refreshed from Gmail |
| Sender group | Messages from one sender matching current filters | Derived from current Inbox state |
| Selection | Messages targeted by a command | Session state |
| Mark | Pending Archive, Trash, or space decision | Until cancelled or submitted |
| Sender rule | From substring plus Archive or Trash action | Saved until edited or removed |
| Rule query | Refresh and stage first-match sender rules | One operation, no automatic submission |
| Space | Named destination with existing Gmail user labels | Saved independently of pending mail |
| Submission journal | Exact submitted work and recovery state | Until outcomes are reconciled |

Rules use case-insensitive substring matching; the first match wins. Run query does not overwrite manual marks. Archive and Trash preserve unread status. Space submission adds its labels and removes only INBOX.

## Architecture

- `src/main.rs` coordinates the native UI, application state, commands, and background jobs.
- `triage.rs` implements grouping, selection, rules, staging, and write outcomes.
- `gmail.rs` implements OAuth, native credential storage, Gmail requests, MIME decoding, and reconciliation.
- `settings.rs` implements settings, separate rules, import/export, backups, and the submission journal.
- `reader.rs` hosts WebView2 on Windows or WKWebView on macOS through Wry. `reader_macos.rs` adds native resource rules and keyboard routing. `message.rs` sanitizes HTML and restricts requests and external links.
- `labels.rs`, `label_ui.rs`, `spaces.rs`, and `space_ui.rs` implement label controls and saved destinations.
- `input.rs`, `theme.rs`, and `icons.rs` provide text input and presentation.

GPUI owns the interface and reader controls; the platform WebView renders sanitized email bodies. It never receives Gmail authorization tokens. No separate backend or application database is used.

## Persistence and recovery

Configuration lives in `%USERPROFILE%\.mado\mado-mail` on Windows or `~/.mado/mado-mail` on macOS. Preferences are in `settings.json`; ordered sender rules are in `rules.json`. Refresh tokens use Windows Credential Manager or macOS Keychain. Existing exported settings or rules can be imported through Rules.

Ordinary unsubmitted marks and selection are temporary. Submitted work is recorded in `gpui-submission.json` before Gmail writes. Confirmed outcomes update the local Inbox; failed work remains retryable. Timeouts and interrupted submissions remain unknown until Gmail labels are rechecked by message ID. Absence from the capped Inbox is not evidence of success.

## Limitations

- Windows requires Rust build tools and WebView2. macOS requires current stable Rust and Xcode Command Line Tools; WKWebView ships with macOS. See [ADR 0003](adr/0003-macos-support.md) for platform differences and verification limits.
- Inbox retrieval is capped at 400 messages; search is local, not full-body or Gmail query search.
- No compose, send, reply, forward, attachment browser, or conversation view.
- HTML rendering has sanitization and size limits; exact Gmail rendering parity is not supported.
- Remote HTTPS images load by default on Windows and require opt-in on macOS. They can disclose opens and IP addresses. Blocking images cannot undo previous requests.
- macOS reader links are cancelled rather than opened externally.
- Real-account writes, migrated personal settings, real IME behavior, and screen-reader support need operator verification.
- Speed and total-memory improvements against the former Electron application have not been measured.

## Repository map

```text
Cargo.toml         Single application crate; builds mado-mail or mado-mail.exe
src/               Rust modules, inline tests, and native UI checks
assets/icons/      Embedded SVG UI icons
fixtures/          Synthetic mail and reader assets
build/             Windows resource definition, icon, and editable SVG source
build.rs           Embeds Windows resources during the Cargo build
scripts/           Windows PowerShell checks and macOS Python checks
licenses/          Bundled dependency license notices
docs/              Application guide and design/research documentation
docs/history/      Historical migration handoff
Taskfile.yml       Run, synthetic demo, build, test, and clean
```

See `docs/application.md` for detailed behavior and checks.
