# Spec: Split reader and slim staging tray

Implement approved option 1 from `docs/design/mado-mail-reader-staging-proposals.excalidraw`, frame `1. Split reader + slim tray`.

- Keep normal reading embedded beside the Inbox. A persistent top-toolbar Show reader / Hide reader control stays in one position. Hiding preserves the message, scroll, and preferred width; activating an individual message reveals it. Sender groups expand instead of reading. Checkboxes and actions do not open messages.
- Replace the right staging column with a roughly 48px bottom line. Pending count, destination counts including saved spaces, Review / Collapse, and Apply N remain available with details collapsed.
- Review reveals a shallow, scrollable bottom tray that reduces the list/reader height. Rows show sender, count, destination, outcome, and Put back. Reader visibility and review expansion are independent.
- Preserve labels, filtering, action scopes, drag staging, spaces, unread/selection independence, Auto-apply, preview preservation, and write/recovery safety. Collapsed staging must still expose failed/unknown counts and reconciliation. Put back cancels pending decisions; it never reverses Gmail writes.
- Keep native WebView2 bounds, clipping, and focus correct during resize, hiding, and collapse. Preserve color hot reload.
- Existing settings remain readable. New review expansion defaults to collapsed independently of the old sidebar visibility. Keep legacy pane values for compatibility, but the approved normal reader layout is beside the list.

## Implementation and verification
First integrate the completed `migration/gpui-pre` branch changes without discarding existing work. Keep `GPUI_DISABLE_DIRECT_COMPOSITION=1`. Use minimal compiler checks and focused tests during implementation, then run the complete unit/build/native checks at the end. Synthetic checks must not read or change personal mail/settings.

Code belongs in `src/main.rs`, a staging UI module, `src/settings.rs`, and the existing native check modules/scripts. Update `docs/application.md`. No new dependency is planned beyond the separately approved GPUI migration.
