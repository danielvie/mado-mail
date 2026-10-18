# App icon

The app uses the supplied `gemini-svg.svg` artwork. Stacked envelopes sit above a blue filter bar and a checkmark badge on a dark rounded square. The artwork is static, not a live mail indicator.

- `build/icon.svg` is the editable source.
- `build/icon.ico` is the checked-in Windows icon with 16, 24, 32, 48, 64, 128, and 256 pixel representations.

`build.rs` embeds the icon through `build/app.rc` for the executable, taskbar, and native windows. A regular Rust build needs no icon generator or separate runtime icon file.

Build with `task build`; the executable is `target/release/mado-mail.exe`.

The original comparison gallery and selection verdict remain on the Git branch `prototype/mail-icon-study`.
