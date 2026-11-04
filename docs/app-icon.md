# App icon

The app uses Inbox pocket, concept 3 from `build/icon-concepts-round-2.html`. A tilted teal envelope sits in a blue inbox pocket on a transparent background. The large envelope identifies mail; the pocket signals inbox organization. Colored edges keep the artwork visible on light backgrounds without a dark tile. The artwork is static, not a live mail indicator.

- `build/icon.svg` is the editable source.
- `build/icon.ico` is the checked-in Windows icon with 16, 24, 32, 48, 64, 128, and 256 pixel representations.

`build.rs` embeds the icon through `build/app.rc` for the executable, taskbar, and native windows. A regular Rust build needs no icon generator or separate runtime icon file.

Build with `task build`; the executable is `target/release/mado-mail.exe`.

The current comparison gallery remains in `build/icon-concepts-round-2.html`. The earlier icon study and selection verdict remain on the Git branch `prototype/mail-icon-study`.
