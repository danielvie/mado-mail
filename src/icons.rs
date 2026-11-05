use gpui::{AssetSource, Hsla, SharedString, div, prelude::*, px, svg};
use std::borrow::Cow;

// Embed assets so installed builds do not depend on the source checkout or current directory.
const ICONS: [(&str, &[u8]); 17] = [
    ("icons/window-maximize.svg", include_bytes!("../assets/icons/window-maximize.svg")),
    ("icons/window-restore.svg", include_bytes!("../assets/icons/window-restore.svg")),
    ("icons/label.svg", include_bytes!("../assets/icons/label.svg")),
    (
        "icons/x.svg",
        include_bytes!("../assets/icons/x.svg"),
    ),
    (
        "icons/check.svg",
        include_bytes!("../assets/icons/check.svg"),
    ),
    (
        "icons/minus.svg",
        include_bytes!("../assets/icons/minus.svg"),
    ),
    (
        "icons/chevron-right.svg",
        include_bytes!("../assets/icons/chevron-right.svg"),
    ),
    (
        "icons/chevron-down.svg",
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
    (
        "icons/mark-read.svg",
        include_bytes!("../assets/icons/mark-read.svg"),
    ),
    (
        "icons/mark-unread.svg",
        include_bytes!("../assets/icons/mark-unread.svg"),
    ),
    ("icons/archive.svg", include_bytes!("../assets/icons/archive.svg")),
    (
        "icons/trash.svg",
        include_bytes!("../assets/icons/trash.svg"),
    ),
    (
        "icons/pop-out.svg",
        include_bytes!("../assets/icons/pop-out.svg"),
    ),
    (
        "icons/reader.svg",
        include_bytes!("../assets/icons/reader.svg"),
    ),
    (
        "icons/reader-below.svg",
        include_bytes!("../assets/icons/reader-below.svg"),
    ),
    (
        "icons/reader-beside.svg",
        include_bytes!("../assets/icons/reader-beside.svg"),
    ),
    (
        "icons/sidebar-right.svg",
        include_bytes!("../assets/icons/sidebar-right.svg"),
    ),
];
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| (*name).into())
            .collect())
    }
}
pub fn icon(path: &'static str, color: Hsla) -> impl IntoElement {
    svg().path(path).size(px(16.)).text_color(color)
}

pub fn checkbox(checked: bool, mixed: bool, p: crate::theme::Palette) -> impl IntoElement {
    div()
        .size(px(18.))
        .flex_shrink_0()
        .rounded_sm()
        .border_1()
        .border_color(if checked || mixed {
            p.accent
        } else {
            p.ink_dim
        })
        .bg(if checked || mixed { p.accent } else { p.panel })
        .flex()
        .items_center()
        .justify_center()
        .when(checked || mixed, |d| {
            d.child(icon(
                if mixed {
                    "icons/minus.svg"
                } else {
                    "icons/check.svg"
                },
                p.on_accent,
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icons_are_embedded_and_unknown_paths_are_not_loaded() {
        for path in Assets.list("icons/").unwrap() {
            assert!(Assets.load(&path).unwrap().is_some());
        }
        assert_eq!(Assets.list("icons/").unwrap().len(), 17);
        assert!(Assets.load("../settings.json").unwrap().is_none());
    }
}
