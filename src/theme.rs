use gpui::{App, Global, Hsla, rgb};
use serde::{Deserialize, Serialize};

/// Custom palettes inherit omitted colors from Frost. GPUI validates hexadecimal color strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Palette {
    pub canvas: Hsla,
    pub panel: Hsla,
    pub panel2: Hsla,
    pub line: Hsla,
    pub ink: Hsla,
    pub ink_dim: Hsla,
    pub accent: Hsla,
    pub danger: Hsla,
    pub on_accent: Hsla,
    pub header_background: Hsla,
}
impl Default for Palette {
    fn default() -> Self {
        Self {
            canvas: rgb(0xf6f8fa).into(),
            panel: rgb(0xffffff).into(),
            panel2: rgb(0xeef1f5).into(),
            line: rgb(0xdfe4ea).into(),
            ink: rgb(0x16191d).into(),
            ink_dim: rgb(0x626b76).into(),
            accent: rgb(0x4f46e5).into(),
            danger: rgb(0xd92d20).into(),
            on_accent: rgb(0xffffff).into(),
            header_background: rgb(0x1b2230).into(),
        }
    }
}
impl Global for Palette {}
impl Palette {
    /// Dark application chrome, with the active palette's accent and danger hues.
    pub fn header(self) -> Self {
        Self {
            canvas: self.header_background,
            panel: rgb(0x171e2a).into(),
            panel2: rgb(0x252e3d).into(),
            line: rgb(0x374357).into(),
            ink: rgb(0xedf1f7).into(),
            ink_dim: rgb(0xaab6c7).into(),
            accent: Hsla { l: self.accent.l.max(0.82), ..self.accent },
            danger: Hsla { l: self.danger.l.max(0.76), ..self.danger },
            on_accent: rgb(0x171e2a).into(),
            ..self
        }
    }
    pub fn current(cx: &App) -> Self {
        *cx.global::<Self>()
    }
    pub fn apply(self, cx: &mut App) {
        cx.set_global(self);
        // Refresh existing readers as well as the Inbox and all input entities.
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contrast(a: Hsla, b: Hsla) -> f32 {
        fn luminance(color: Hsla) -> f32 {
            let color: gpui::Rgba = color.into();
            let linear = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
            0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    #[test]
    fn header_is_dark_with_readable_text_and_controls() {
        let p = Palette::default().header();
        assert!(p.panel.l < p.canvas.l && p.canvas.l < p.panel2.l);
        for background in [p.canvas, p.panel, p.panel2, p.line] {
            for foreground in [p.ink, p.ink_dim, p.accent, p.danger] {
                let ratio = contrast(foreground, background);
                assert!(ratio >= 4.5, "Header contrast {ratio:.2} is below 4.5:1");
            }
        }
        assert!(contrast(p.on_accent, p.accent) >= 4.5);
    }
    #[test]
    fn header_preserves_custom_accent_hues_without_changing_the_palette() {
        let custom = Palette {
            accent: rgb(0x087f5b).into(),
            danger: rgb(0x992244).into(),
            ..Palette::default()
        };
        let header = custom.header();
        assert_eq!(header.accent.h, custom.accent.h);
        assert_eq!(header.accent.s, custom.accent.s);
        assert_eq!(header.danger.h, custom.danger.h);
        assert!(contrast(header.accent, header.panel2) >= 4.5);
        assert_eq!(custom.panel, Palette::default().panel);
        assert_eq!(serde_json::to_value(custom).unwrap().as_object().unwrap().len(), 10);
    }
    #[test]
    fn header_background_defaults_and_round_trips_without_changing_other_colors() {
        let inherited: Palette = serde_json::from_str("{}").unwrap();
        assert_eq!(inherited.header().canvas, rgb(0x1b2230).into());
        let custom: Palette = serde_json::from_str(r##"{"header_background":"#233044"}"##).unwrap();
        assert_eq!(custom.header().canvas, rgb(0x233044).into());
        assert_eq!(custom.canvas, inherited.canvas);
        assert_eq!(custom.header().panel2, inherited.header().panel2);
        assert_eq!(serde_json::from_slice::<Palette>(&serde_json::to_vec(&custom).unwrap()).unwrap(), custom);
    }
    #[test]
    fn custom_palette_inherits_frost_and_rejects_invalid_colors() {
        let p: Palette = serde_json::from_str(r##"{"accent":"#087f5b"}"##).unwrap();
        assert_eq!(p.accent, rgb(0x087f5b).into());
        assert_eq!(p.canvas, Palette::default().canvas);
        for json in [
            r##"{"accent":"red"}"##,
            r##"{"canvas":"#zz0000"}"##,
            r##"{"header_background":"invalid"}"##,
            r##"{"unknown":"#ffffff"}"##,
        ] {
            assert!(serde_json::from_str::<Palette>(json).is_err());
        }
        assert_eq!(
            serde_json::from_slice::<Palette>(&serde_json::to_vec(&p).unwrap()).unwrap(),
            p
        );
    }
}
