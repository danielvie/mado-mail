# Plan: Split reader and slim staging tray

Approved design: `SPEC-split-reader-tray.md` and proposal frame 1.

1. Integrate `migration/gpui-pre` before layout edits. Preserve dirty/untracked work in `target/pre-gpui-pre-integration`, apply the checked migration patch, and retain both completed plans under feature-specific names. Verify with a compiler check only.
2. Deliver the core layout slice: side-by-side list/reader, fixed-position labeled top reader toggle, message activation reveals the reader, bottom staging summary and shallow review tray. Keep Apply and reconciliation visible when collapsed. Persist review state separately from legacy sidebar visibility.
3. Preserve saved-space editing, destination drop targets, compact sender/outcome rows, and native reader clipping/focus. Adjust existing synthetic assumptions and add targeted tray checks without weakening safety assertions.
4. Update documentation, review the final changes, then run the complete tests/build and synthetic native checks once at the end. Fix failures with focused checks before the final complete run.

## Risks
- Native child windows must shrink/reposition with the reader, never cover the bottom tray.
- Unknown outcomes cannot be cancelled or resubmitted without reconciliation. Collapsed details must not hide them.
- Existing saved pane preferences and color hot reload must survive migration and layout changes.
- Compact review overflow must leave Apply reachable and retain saved-space controls.

Previous completed work is preserved in `tasks/color-hot-reload-*.md` and `tasks/gpui-pre-*.md`.
