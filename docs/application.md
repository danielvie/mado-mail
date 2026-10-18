# Mado Mail GPUI application

Windows Gmail triage app built with Rust and GPUI. The repository root is a single Rust crate named `mado-mail`, with application code in `src/`.

## Run

```text
task run           # Build and launch with Gmail
task demo          # Synthetic Inbox; no credentials, network, or settings writes
task test          # Model, settings, OAuth, MIME, and synthetic HTTP tests
task build         # target/release/mado-mail.exe
task install       # Build release and copy mado-mail.exe to ~/.local/bin
task clean         # Rust build artifacts only
```

Requires stable Rust, Visual Studio C++ build tools with a Windows SDK, and Microsoft Edge WebView2 Runtime. Without Task, use `cargo run --release`.

Windows builds embed `build/icon.ico` for the executable, taskbar, and native windows. No separate icon file is needed at runtime.

## Native UI checks

Run the scripts directly after building:

```text
task build
powershell.exe -NoProfile -File scripts/check-triage.ps1
powershell.exe -NoProfile -File scripts/check-reader.ps1
powershell.exe -NoProfile -File scripts/check-window.ps1 -Demo
```

For focused triage checks, run `cargo build`, then invoke `scripts/check-triage.ps1` with `-DebugBuild` and one switch: `-ScrollbarOnly`, `-DetachOnly`, `-MiddleOnly`, `-SelectedActionsOnly`, `-SpacesOnly`, `-LabelsOnly`, `-ReaderVisibilityOnly`, or `-LabelRemovalOnly`.

## Gmail connection

1. Enable Gmail API in your Google Cloud project and create a Desktop app OAuth client.
2. Save its downloaded JSON at `%USERPROFILE%\.mado\mado-mail\client_secret.json`.
3. Add your account as a test user if the Google consent app is in Testing.
4. Run the app and choose Connect Gmail.

The app now requests `gmail.modify` for Archive, Trash, and read/unread actions. This requires fresh sign-in even if an earlier read-only build was authorized. Refresh tokens live in Windows Credential Manager under `mado-mail/gmail.modify`, scoped to the OAuth client. The old `mado-mail-poc/gmail.readonly` entry and Electron `token.json` are untouched. Access tokens remain in memory. Transient network errors retain authorization; revoked grants require reconnection. Google Testing-mode grants can expire after seven days.

To forget authorization, close the app and remove only its `mado-mail/gmail.modify` Generic Credential in Windows Credential Manager.

## Triage

The top toolbar contains the message filter, Run query, Rules, Refresh, checked-message actions, Auto-apply, and Theme. Label filtering has its own row below the status message. Sender groups are ordered by count, then address. Click a group to expand it and expose individual messages. Filtering searches From, Subject, and snippet. When the filter contains text, an SVG X appears inside its right edge. Click it, or focus it with Tab and press Enter or Space, to clear the filter and return input focus. It disappears when empty; Ctrl+Z restores the cleared text.

- Left-click a message row to display it when the reader is visible. If the reader is hidden, row clicks and Enter leave it hidden and preserve its previous message. Reading leaves checkmarks and Gmail unread status unchanged. Clicking a sender group expands or collapses it without replacing the reader.
- The SVG pop-out button beside the reader's subject opens the loaded message in a separate window. It stays on that message while you browse others; the inline reader remains available. Each click opens a new window. The button is disabled until a message has loaded. Tab reaches it, and Enter or Space activates it. Pop-out windows can be resized, scrolled, and closed independently without changing checkmarks, staging, or Gmail unread status.
- Left-click a checkbox to select or clear that row's messages. Group checkboxes mark or clear their messages; a mixed group click marks all.
- Middle-click an unchecked message row to select all undecided messages matching the current filter, including off-screen messages and collapsed groups. Middle-click a checked row to clear all selection, even if other rows are unchecked. A partially checked sender group counts as unchecked. This also works over a row's checkbox or action controls without activating those controls. Middle-click does not open messages, expand groups, change unread status, or stage mail. There are no Ctrl, right-click, or range selection gestures.
- The displayed message has an accent row; its sender and subject stay unchanged. Bulk targets have green checkboxes and a green gutter. Both states can coexist; keyboard focus has a separate outline.
- Unread messages have a small dot and bold sender/subject text. Read messages use regular text. Sender groups use the unread style when any represented message is unread. Filtering and staging update that group indicator; opening a preview does not mark messages read.
- Hover or keyboard focus reveals icons for Add label, Read/Unread, Archive, Trash, and `...` at the row end. Archive uses a tray-and-down-arrow SVG; its tooltip identifies the action. There is no fixed action column. Add label, Read/Unread, Archive, and Trash on a fully checked row act on all checked messages, including other senders and off-screen messages. On an unchecked or partially checked group, they act only on that row's messages. Tooltips show the target count and scope. Toolbar Archive N and Trash N still act on the checked set.
- Read/Unread, `+`, and Archive share one connected control. Hovering either icon highlights only its section; hovering `+` highlights all three. The `+` marks the same row or checked batch read, then stages Archive. With Auto-apply on, it archives after read confirmation. Already-read targets still archive. If any read request fails or has an unknown outcome, nothing is archived; confirmed read changes remain, and the status explains how to retry. The separate Read/Unread and Archive buttons keep their existing behavior.
- Read/unread actions write immediately, regardless of Auto-apply. They remove or add only UNREAD, keep messages in the Inbox, and leave checkmarks, staging, and the preview unchanged. The clicked row determines the operation for the entire target batch: an unread row marks them all read; a read row marks them all unread. A sender group counts as unread if any represented message is unread. The icon, dot, and bold text update only after confirmation. Failed or uncertain requests leave local labels unchanged; Refresh checks Gmail read status.
- `...`, right-click, and Shift+F10 open the same context menu. Its Archive and Trash entries use the same selected-message scope and show the target count. Sender rule, Open message, expand/collapse, and the selection toggle still refer to the clicked row. Arrows navigate, Enter acts, and Escape or clicking outside closes the menu without changing the reader or checkmarks.
- Sender rule opens a prefilled editor. Confirmation saves the rule and acts on current undecided messages matching its From pattern. Editing an existing rule from Rules only changes the rule; it does not stage messages.
- Drag from a fully checked row to move the checked batch into a staging bin. Drag from an unchecked or partially checked row moves only that row's messages. The badge shows the message and sender counts. Checkbox and action controls never start mail drags.
- Drop outside a bin or press Escape to cancel. Hidden staging stays hidden until you open it. With Auto-apply on, Archive and Trash drop hints warn that release writes immediately; space drops always create pending marks. Stale message IDs are discarded, and blocked or unknown writes disable drops.
- Archive, Trash, and saved spaces stay separate in staging. Hover a sender for a peek; click to pin it. Put back and Empty all cancel unsubmitted marks.
- Drag either pane's left divider to resize it. When the reader is below the list, drag its top divider to change its height. Reader width, reader height, and staging width are saved independently across launches. Smaller windows clamp the displayed widths without overwriting the preferred sizes. Rows always use two lines: sender and date above labels and subject. Resizing changes available text space, not row height.
- The layout SVG at the bottom-right switches the reader between beside and below the list. Its icon and tooltip show the next position. Staging stays on the right at full height. Placement is saved across launches; switching keeps the current message and does not open a hidden reader. The page and sidebar icons beside it independently show or hide reader and staging, in that order. Visibility and widths are saved when changed and restored on the next launch. They stay reachable when panes are hidden, with a pending count beside them. Hiding the reader preserves its message and scroll within the session; a visible reader starts empty after restart. Hiding staging closes its sender peek but keeps pending decisions.
- The email list has a right-edge scrollbar whenever its rows overflow, in either reader layout. Drag the thumb or click the track to page; wheel and keyboard scrolling keep it in sync. Its gutter keeps it clear of row actions. It disappears when all filtered rows fit.
- With only Archive and Trash, drag their divider to adjust heights for the session. Adding spaces switches to a scrollable vertical stack; Apply remains pinned below it.
- Apply submits staged decisions. Trash is not permanent deletion. Archive and Trash requests do not add or remove the UNREAD label.
- Auto-apply defaults to off. Enabling it asks for confirmation; subsequent row actions submit immediately. Existing staged marks still require Apply. Run query always stages rules rather than submitting them.
- Show a hidden reader with its toggle or Alt+R. The explicit Open message menu action can also show it; ordinary row clicks and Enter cannot. Rules hides the native reader until you return to Inbox. A staged or removed message keeps its preview with an explicit status.
- Frost is the only built-in theme. Theme cycles through Frost and any custom palettes defined in settings. The Inbox, reader controls, and text fields share the active palette.

Tab reaches checkboxes, label details, and contextual actions. A row's controls remain visible while its menu is open. Menus stay inside the viewport and follow their target row rather than the pointer.

### Label filtering and immediate assignment

The label icon below the status message opens a floating fuzzy-search panel anchored beneath the icon. Add label uses the same floating panel beneath its toolbar button or the row's label icon. Opening either picker leaves the Inbox and staging layout unchanged. Pick one or more user labels. Messages match any selected label, together with the text filter. Filtering happens before sender grouping, so group counts, checkboxes, middle-click selection, and drags refer only to matching messages. Changing label filters clears checkmarks. The filters last for the session and do not affect staged mail or sender rules.

Selected filter labels appear as chips beside the icon. Multiple labels use OR, shown as "Any of". Hover reveals an X; keyboard focus also reveals it. X removes only the filter, never a Gmail label. The picker supports Up/Down and Enter. Esc, Tab, Close, or an outside click dismisses it without activating the controls underneath. Panels stay within the window when resized. The native reader body is temporarily hidden so it cannot paint over the floating panel; its pane size and message stay unchanged, and the body returns when the panel closes.

Click the name on a visible label chip on a message or sender group to add that label to the filters. It does not open, expand, drag, or assign labels to the message. Clicking an already active label keeps it active and preserves checkmarks. Row chips also support Tab, Enter, and Space. The text filter remains in effect.

Check a message, sender group, or several messages, then use **Add label** in the top toolbar. You can also use the label icon in a row's hover controls, without changing checkmarks. A checked row targets the checked batch; an unchecked or partially checked row targets only its represented messages. Row pickers keep those targets until closed, even if selection changes; the toolbar picker uses the current checked set. Choosing a result writes immediately to the targets, regardless of Auto-apply. It adds only that label, keeps INBOX, preserves existing labels and unread status, and does not create a staging decision. A group includes only its currently represented members. The picker shows how many target messages already have each label and skips those messages when adding it.

Quick labels above the search field show up to six previously used labels, highest score first. Clicking one performs the same immediate Add label action, independent of the search text. Each action with at least one confirmed addition increases that label's use count once, regardless of batch size. Filtering, removal, already-applied labels, and wholly failed or unknown writes do not count. History starts empty and persists as `labelUsage` in settings. Scores use [zoxide's frequency and recency weights](https://github.com/ajeetdsouza/zoxide/wiki/Algorithm#frecency): use count times 4 within an hour, times 2 within a day, divided by 2 within a week, otherwise divided by 4. Equal scores sort by label name, then ID. This uses zoxide's recency weights, not its database-wide aging algorithm. Deleted and system labels are omitted; an unavailable label catalog hides shortcuts until refreshed. Labels already present on every target remain visible but disabled.

Only confirmed writes update local labels. Failed messages stay unchanged; unknown results require Refresh before retrying. No automatic retry is sent. Unlike a space's Apply action, this operation never archives mail.

`powershell.exe -NoProfile -File scripts/check-triage.ps1 -DebugBuild -LabelsOnly` verifies the native controls with synthetic mail, including item-label clicks, OR filters, filtered groups, hover assignment scope and anchoring, keyboard removal, immediate assignment, partial results, floating placement without layout shifts, dismissal, both reader layouts, and a small window. It never contacts Gmail.

### Removing message labels

Message-label chips show an X on hover or keyboard focus. On an individual message, X removes only that user label immediately. On a sender group, X opens a floating confirmation naming the label and the number of displayed members that have it. Cancel is focused first; Tab switches between Cancel and Remove label. Esc or clicking outside cancels.

Removal is always row-local, even when unrelated messages are checked. Filtered-out group members are excluded. The confirmation freezes its target IDs; changed filters dismiss it, and stale targets require a new confirmation. INBOX, unread, other labels, and staging remain unchanged. Only confirmed removals update local chips. If removing a label makes a message disappear from an active filter, its checkmark is cleared so later actions cannot target it invisibly.

This X changes Gmail labels. The X on a filter chip still only clears a filter. Neither operation opens or drags a message. `powershell.exe -NoProfile -File scripts/check-triage.ps1 -DebugBuild -LabelRemovalOnly` checks both removal paths, cancellation, target scope, and partial failures with synthetic mail.

### Spaces

Click **+ New space** in the staging header. A blank bin appears there, with no modal. Enter a name, then press its label icon to open the inline fuzzy finder. Type part of a label name, use Up/Down to choose a result, and Enter or a click to toggle it. Matching is case-insensitive and accepts skipped characters, such as `prj` for Projects. Selected labels remain chips across searches; click a chip to remove it. Esc closes the finder without discarding the draft.

Save space requires a name and at least one available user label. Cancel discards the draft. Drag selected mail to the saved space, or use its Move selected button. These moves never auto-apply. Apply adds every configured label and removes only INBOX in one modify request per message. Existing labels and unread status remain unchanged. Confirmed messages leave staging; the space remains reusable.

The label icon edits an empty saved space. Each custom space has a Remove space button in its header. It removes only that saved space from local settings; Gmail labels, messages, checkmarks, and other spaces stay unchanged. Archive and Trash cannot be removed. Pending mail locks both label editing and removal until you Apply or Put back. Removal is also disabled during edits, mail dragging, writes, or unresolved submissions. Missing labels or an unavailable catalog block space submissions, not rechecks. Recovery keeps the exact submitted label IDs even if settings later change. Import is blocked while space edits or space decisions are outstanding. Spaces persist in settings; drafts and ordinary unsubmitted marks do not persist. The first version supports up to 100 spaces and 100 labels per space, using existing Gmail labels only.

`powershell.exe -NoProfile -File scripts/check-triage.ps1 -DebugBuild -SpacesOnly` runs the synthetic native workflow and writes screenshots under `target/`. Use `-SpaceRemovalOnly` instead for a targeted removal check, including pending-mail and edit locks, preserving other spaces, and unchanged mail. Neither check contacts Gmail or reads personal settings.

### Keyboard

| Keys | Action |
| --- | --- |
| Tab / Shift+Tab | Move between controls |
| Up / Down, Home / End | Navigate the focused triage table |
| Space / Enter on a checkbox | Toggle that checkbox only |
| Enter | Load one message if reader is visible, or expand/collapse a sender |
| Right / Left | Expand/collapse sender |
| Shift+F10 | Open the focused row's menu |
| Up / Down, Enter, Escape in menu | Navigate, act, close |
| Ctrl+F | Focus filter |
| Ctrl+R | Connect or refresh |
| Ctrl+L | Switch Inbox/Rules |
| Ctrl+Shift+B | Hide/show the staging sidebar |
| Alt+R | Hide/show the reader |
| Left / Right on either width divider | Grow/shrink that pane by 16px and save its width |
| Up / Down on reader's top divider | Grow/shrink reader height by 16px and save it |
| Up / Down on bin divider | Shrink/grow the Archive section by 16px |
| Ctrl+Enter | Apply, outside text inputs |
| Escape | End resizing, close rule editor or sender peek, or return from reader body to list |
| Alt+I | Toggle remote images in the reader |

Text fields support Unicode, IME composition, mouse selection, Home/End, word navigation/deletion, Shift selection, Ctrl+A/C/X/V, Shift+Insert, and Ctrl+Z/Y. Undo keeps the last 100 text snapshots. Native checks cover basic typing, filtering, undo, and IME range conversion. Real IME behavior, clipboard workflows, and screen-reader support still need operator validation.

## Gmail label display

Existing user labels appear before the subject. Sender groups show the union of labels across their represented messages, with counts such as `Projects 2/3` when not every message has a label. Filtering and staging change that denominator. At most two badges fit within a shared width budget, with `+N` for overflow; narrow rows show fewer. Keyboard focus reveals full names and counts; message labels have no hover tooltips. The reader header shows the full message label list.

System labels are omitted. Gmail's `messageListVisibility: hide` is respected in rows; hidden user labels remain available in the reader details. Chips preserve Gmail's text and background colors exactly, without contrast-based replacement. Neutral theme colors are used only when Gmail colors are missing or invalid. Message-row chip names add label filters when clicked; their X removes the label. Assignments happen through Add label in the toolbar, the hover label icon, or spaces in staging.

Message label IDs come from the existing metadata requests. A separate read fetches the label catalog once per Inbox refresh, then gets color details for distinct used user labels. Names can display before colors arrive. The cache belongs to the session, and cached metadata survives transient lookup failures. Missing IDs show `Label unavailable`. Label failures do not disable Archive or Trash. Direct label assignment, removal, and space assignment require available label metadata. Cached filters still work during catalog failures. No additional authorization scope is required.

## Rules and settings migration

In Rules, choose Import settings and select an existing settings export or standalone rules JSON file. The former Electron app is no longer included; importing an existing export does not require it.

Import accepts a combined settings export or a standalone rules array. A rules-only import keeps the current preferences; a combined import also restores custom themes and Auto-apply. Both preserve rule order, IDs, and actions. Old built-in theme choices become Frost unless a custom palette with that name is explicitly defined. Check the reported Auto-apply state after importing. Existing rules and preferences are backed up before replacement. Import replaces the rule list rather than merging it. Export settings still produces a combined, credential-free backup.

Rules use case-insensitive substring matching against From; the first match wins. Editing does not reorder them. Run query refreshes the capped Inbox and stages matches without overwriting existing manual marks.

Configuration lives in `%USERPROFILE%\.mado\mado-mail`, equivalent to `~/.mado/mado-mail`:

- `settings.json` contains preferences, custom themes, `paneWidths`, such as `{"reader":480,"staging":320}` in logical pixels, and `paneVisibility`, such as `{"reader":true,"staging":false}`. `readerPosition` is `beside` or `below`; `readerHeight` is the preferred bottom-reader height in logical pixels. Older configurations keep the previous defaults until you change them: reader hidden, staging shown, reader beside the list, bottom-reader height 360. Rules and temporary overlays do not change the saved visibility.
- `settings.json` also stores `spaces`, each with a numeric `id`, a `name`, and `label_ids`. Older settings default to no spaces. Exports include spaces.
- `rules.json` contains an ordered JSON array of sender rules. Each rule has `id`, `from`, and `action`; actions are `archive` or `trash`.
- `client_secret.json`, `gpui.lock`, and `gpui-submission.json` also live here. Electron uses `token.json`; GPUI tokens remain in Windows Credential Manager.

First launch creates missing files. Existing combined settings are migrated without changing rule order or actions. An existing `rules.json` always wins over embedded rules, including when it is an empty array. Preferences are backed up before embedded rules are removed or missing Frost colors are added. A local `gpui-settings.json` is used only if `settings.json` is absent.

When consolidating old folders, close both apps first. Keep the current `settings.json` and `rules.json`; move active credentials and the recovery journal from `mado_mail`. Preserve conflicting legacy files under `mado-mail/legacy-backup` rather than overwriting current settings. Never discard a pending submission journal. `_mado_mail` is not an application data path.

Writes use temporary files and atomic replacement for each file. Importing both files is not a single transaction; a partial-save error blocks further changes until restart reloads the saved state. Invalid configuration is reported rather than overwritten. Only one real GPUI application instance can use the config at a time. Close the app before editing these files. Demo instances are isolated and do not load or save personal rules.

### Custom themes

Close the app before editing settings, then restart. All nine default Frost colors are written under `themes.frost`, and you can edit them directly. Other named palettes may omit colors; omitted values inherit the compiled Frost defaults. Set `theme` to a palette name or choose it with the Theme button after restarting. Keep your existing preferences and `autoApply` value; sender rules belong in `rules.json`.

Example theme fields to add to the existing JSON:

```json
{
  "theme": "ocean",
  "themes": {
    "ocean": {
      "accent": "#087f5b",
      "panel": "#e6f7ef"
    }
  }
}
```

The full settings file also contains `version` and `autoApply`. Available palette colors are `canvas`, `panel`, `panel2`, `line`, `ink`, `ink_dim`, `accent`, `danger`, and `on_accent`. Colors use hexadecimal strings such as `#087f5b`. The `frost` entry is editable and appears only once in the theme chooser. Switching themes updates readers that are already open; email HTML keeps its sender's styling. Native titlebars follow Windows appearance settings.

Selection boxes show empty, checked, and mixed states. Their marks and the expand/collapse chevrons use custom SVGs embedded from `assets/icons/`; installed builds do not need those source files alongside the executable.

## Write outcomes and recovery

Confirmed Archive and Trash writes remove their messages from the local Inbox. Read/unread actions keep them in the Inbox and update only confirmed messages' local labels. Failed items remain staged and can be retried with Apply or cancelled with Put back.

A timeout, connection loss, HTTP 408, or server failure is an unknown outcome, not success. Unknown items cannot be cancelled or resubmitted until Recheck Gmail reads their current labels. Rechecking uses each message ID, never absence from the 400-message Inbox. A message already in the requested state is confirmed; a message still outside that state remains staged for an explicit retry. Unavailable messages remain unknown. POST requests are not automatically replayed after uncertain failures. A rejected HTTP 401 can refresh authorization and retry.

Before submission, `gpui-submission.json` records IDs and actions. On restart, interrupted work appears as unknown and requires a recheck. Pending decisions that have never been submitted remain temporary and are not saved. A recovery-journal save failure blocks further writes. Do not delete the journal to bypass recovery.

Writes currently run one message at a time for per-message outcomes. Large submissions may take time, and reader jobs can wait behind them. Batching is deferred until its partial-failure behavior has equivalent coverage.

## Reader and privacy

The normal reader is an embedded pane in the main window. Its pop-out button copies the loaded, sanitized HTML, message details, labels, and current remote-image permission into a dedicated window without fetching the message from Gmail again. The new window starts at the top; the inline reader keeps its scroll position. Image controls then operate independently in each window. Detached readers remain open if you close the Inbox; the app exits after its last window closes. `cargo run --release -- --reader-demo` opens a standalone synthetic reader for diagnostics only. GPUI owns the controls; Wry hosts WebView2 for the sanitized HTML body. Embedded raster images display directly. HTTPS remote images load by default, including CSS background images. Block images disables remote loading for the current message; Allow images enables it again. Opening another message restores the default of allowing images. Remote images can disclose opens and IP addresses; blocking cannot undo requests already sent. Scripts and other disallowed content remain blocked regardless of image settings. Synthetic demos serve their remote-image fixture locally rather than making external requests.

Ammonia, Content Security Policy, and the native request guard block scripts, forms, frames, file URLs, downloads, dropped files, remote stylesheets/fonts, and disallowed requests. WebView2 uses an InPrivate profile, with scripts, host objects, web messaging, autofill, browser context menus, and browser-specific shortcuts disabled. User-initiated HTTP/HTTPS/mailto links open externally. The WebView never receives Gmail authorization tokens.

Supported embedded formats are PNG/JPEG/GIF/WebP/BMP. Inline SVG, CID CSS backgrounds, attachment browsing/downloads, and exact Gmail rendering parity are not supported. Limits remain 24 MiB JSON response, 4 MiB decoded body/image part, 12 MiB total body plus embedded image bytes, and 32 embedded images.

Native child visibility follows pane visibility, Rules, resizing, and overlapping overlays. List-local menus leave the body visible in the beside layout. In the below layout, row menus and label details temporarily hide the native body so it cannot cover them. Mail dragging temporarily hides the native child so it cannot intercept the pointer while crossing to staging. The same reader and scroll position return after drop or cancellation. Closing a pane returns focus to the list; Tab can leave the browser body. WebView creation runs outside GPUI state borrows because Wry pumps Windows messages while starting.

Startup preserves `GPUI_DISABLE_DIRECT_COMPOSITION=1` before platform initialization. GPUI 0.2.2's default composition layer otherwise covers the WebView2 child. This uses the supported DirectX HWND rendering path, not software rendering.

## Verification limits

Automated Gmail checks use local synthetic HTTP servers. Native triage and reader checks use synthetic mail only. No real messages are archived or trashed by the checks. Captures target only the selected app window, never the desktop. Reports and images live under `target/`.

Real-account write behavior, account restart/refresh, and migrated personal settings need operator verification. Matched speed and total-memory comparisons against Electron have not been performed; implementation does not establish the performance goal. Include WebView2 child processes and the required GPUI renderer setting in those measurements.
