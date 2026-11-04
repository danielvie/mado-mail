# ADR 0002: Pin the GPUI snapshot packages

Mado Mail uses `gpui-pre` and `gpui-pre-platform`, both pinned to `0.3.11`. The dependencies are aliased as `gpui` and `gpui_platform`. This updates the GPUI implementation required by ADR 0001; it does not replace GPUI or change the application's triage scope.

These packages are published through Longbridge's GPUI Kit release infrastructure rather than an official Zed prerelease channel. Snapshot updates can change APIs even when the recorded upstream GPUI version is unchanged. Keep the core and platform versions aligned, commit Cargo.lock, and rerun native checks before updating either package.

Native initialization now uses `gpui_platform::application()`. Set `GPUI_DISABLE_DIRECT_COMPOSITION=1` before constructing the platform. The snapshot's Windows renderer still supports this DirectX HWND path, which is required to keep the Wry/WebView2 child reader visible. Creating the WebView must also remain outside GPUI App and Entity borrows because Wry can pump Windows messages during startup.

The migration preserves settings and recovery-journal formats, Gmail behavior, WebView restrictions, and the custom text input. Successful builds and synthetic checks do not establish real IME, screen-reader, real-account or performance behavior. Those retain the verification limits in the application guide.

Sources: [package manifest](https://docs.rs/crate/gpui-pre/0.3.11/source/Cargo.toml), [platform initialization](https://docs.rs/crate/gpui-pre-platform/0.3.11/source/src/gpui_platform.rs), [Windows platform](https://docs.rs/crate/gpui-pre-windows/0.3.11/source/src/platform.rs), and [snapshot release workflow](https://github.com/longbridge/gpui-kit/blob/main/.github/workflows/release-gpui.yml).
