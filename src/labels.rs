use crate::theme::Palette;
use gpui::{Hsla, IntoElement, Rgba, div, prelude::*, px};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize)]
pub struct Label {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, rename = "messageListVisibility")]
    pub visibility: String,
    #[serde(default)]
    pub color: Option<LabelColor>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct LabelColor {
    #[serde(rename = "textColor")]
    pub text: String,
    #[serde(rename = "backgroundColor")]
    pub background: String,
}
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub entries: BTreeMap<String, Label>,
    pub unavailable: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Usage {
    pub count: u64,
    pub last_used: u64,
}
impl Usage {
    pub fn score(&self, now: u64) -> f64 {
        // Zoxide's recency weights: https://github.com/ajeetdsouza/zoxide/wiki/Algorithm#frecency
        self.count as f64 * match now.saturating_sub(self.last_used) {
            0..3600 => 4.,
            3600..86400 => 2.,
            86400..604800 => 0.5,
            _ => 0.25,
        }
    }
}
pub fn usage_time() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
pub fn quick_labels<'a>(catalog: &'a Catalog, usage: &BTreeMap<String, Usage>, now: u64) -> Vec<(&'a Label, f64)> {
    if catalog.unavailable { return Vec::new(); }
    let mut ranked: Vec<_> = usage.iter().filter(|(_, used)| used.count > 0).filter_map(|(id, used)| {
        catalog.entries.get(id).filter(|label| label.kind == "user").map(|label| (label, used.score(now)))
    }).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| (&a.0.name, &a.0.id).cmp(&(&b.0.name, &b.0.id))));
    ranked.truncate(6);
    ranked
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub id: Option<String>,
    pub text: String,
    pub colors: Option<(Hsla, Hsla)>,
}

pub fn colors(color: &LabelColor) -> Option<(Hsla, Hsla)> {
    let parse = |s: &str| {
        (s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| Rgba::try_from(s).ok()).flatten()
    };
    let text = parse(&color.text)?;
    let background = parse(&color.background)?;
    Some((text.into(), background.into()))
}

pub fn summarize<'a>(members: impl Iterator<Item = &'a [String]>, catalog: &Catalog, hide: bool) -> Vec<Badge> {
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut total = 0;
    for labels in members {
        total += 1;
        for id in labels.iter().map(String::as_str).collect::<BTreeSet<_>>() {
            *counts.entry(id).or_default() += 1;
        }
    }
    let mut badges = Vec::new();
    let mut unknown = false;
    for (id, count) in counts {
        match catalog.entries.get(id) {
            Some(label) if label.kind == "user" && (!hide || label.visibility != "hide") => {
                let text = if count < total { format!("{} {count}/{total}", label.name) } else { label.name.clone() };
                badges.push((label.name.clone(), id, Badge { id: Some(id.into()), text, colors: label.color.as_ref().and_then(colors) }));
            }
            Some(_) => {}
            // Gmail system IDs are uppercase. Unknown IDs are still reported unless known system IDs.
            None if matches!(id, "INBOX" | "UNREAD" | "STARRED" | "IMPORTANT" | "SENT" | "DRAFT" | "DRAFTS" | "SPAM" | "TRASH" | "CHAT") || id.starts_with("CATEGORY_") => {}
            None => unknown = true,
        }
    }
    badges.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    let mut result: Vec<_> = badges.into_iter().map(|(_, _, badge)| badge).collect();
    if unknown { result.push(Badge { id: None, text: "Label unavailable".into(), colors: None }); }
    result
}

pub fn full_text(badges: &[Badge]) -> String {
    badges.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n")
}

pub fn visible_count(count: usize, budget: f32) -> usize {
    count.min(if budget >= 180. { 2 } else if budget >= 90. { 1 } else { 0 })
}

pub fn chip(badge: &Badge, width: f32, p: Palette) -> impl IntoElement {
    let (ink, background) = badge.colors.unwrap_or((p.ink, p.panel2));
    // ponytail: approximate intrinsic width; GPUI truncates to the shared budget. Use shaped metrics if this wastes space.
    div().w(px(width.min(badge.text.chars().count() as f32 * 7. + 10.))).min_w_0().px_1().py(px(1.)).rounded_sm()
        .bg(background).text_color(ink).text_xs().flex()
        .child(div().flex_1().min_w_0().truncate().child(badge.text.clone()))
}

pub fn demo() -> Catalog {
    let labels: Vec<Label> = serde_json::from_str(r##"[
        {"id":"projects","name":"Projects","type":"user","color":{"textColor":"#ffffff","backgroundColor":"#076239"}},
        {"id":"studies","name":"ESTUDOS/UW University","type":"user","color":{"textColor":"#41236d","backgroundColor":"#e4d7f5"}},
        {"id":"news","name":"Newsletters","type":"user"},
        {"id":"finance","name":"Finance","type":"user"},
        {"id":"hidden","name":"Hidden label","type":"user","messageListVisibility":"hide"},
        {"id":"INBOX","name":"Inbox","type":"system"}
    ]"##).unwrap();
    Catalog { entries: labels.into_iter().map(|l| (l.id.clone(), l)).collect(), unavailable: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quick_label_scores_use_frequency_and_recency_with_stable_ties() {
        let now = 1_000_000;
        let used = Usage { count: 4, last_used: now };
        for (age, score) in [(0, 16.), (3599, 16.), (3600, 8.), (86399, 8.), (86400, 2.), (604799, 2.), (604800, 1.)] {
            assert_eq!(used.score(now + age), score);
        }
        assert_eq!(used.score(now - 1), 16.);
        let mut catalog = demo();
        let mut usage = BTreeMap::from([
            ("projects".into(), Usage { count: 10, last_used: now - 604800 }),
            ("news".into(), Usage { count: 1, last_used: now }),
            ("finance".into(), Usage { count: 1, last_used: now }),
            ("hidden".into(), Usage { count: 1, last_used: now - 3600 }),
            ("missing".into(), Usage { count: 100, last_used: now }),
            ("INBOX".into(), Usage { count: 100, last_used: now }),
            ("studies".into(), Usage::default()),
        ]);
        let ranked = quick_labels(&catalog, &usage, now);
        assert_eq!(ranked.iter().map(|(label, score)| (label.id.as_str(), *score)).collect::<Vec<_>>(),
            vec![("finance", 4.), ("news", 4.), ("projects", 2.5), ("hidden", 2.)]);
        for i in 0..10 {
            let mut label = catalog.entries["news"].clone();
            label.id = format!("extra-{i}");
            usage.insert(label.id.clone(), used.clone());
            catalog.entries.insert(label.id.clone(), label);
        }
        assert_eq!(quick_labels(&catalog, &usage, now).len(), 6);
        assert!(quick_labels(&catalog, &BTreeMap::new(), now).is_empty());
        catalog.unavailable = true;
        assert!(quick_labels(&catalog, &usage, now).is_empty());
    }
    #[test]
    fn counts_are_per_message_hide_system_and_hidden_and_keep_unknown() {
        let catalog = demo();
        let messages: Vec<Vec<String>> = [vec!["projects", "projects", "INBOX", "hidden"], vec!["projects", "studies"], vec!["missing"]]
            .into_iter().map(|v| v.into_iter().map(str::to_owned).collect()).collect();
        let badges = summarize(messages.iter().map(Vec::as_slice), &catalog, true);
        assert_eq!(badges.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), vec!["ESTUDOS/UW University 1/3", "Projects 2/3", "Label unavailable"]);
        assert_eq!(badges.iter().map(|b| b.id.as_deref()).collect::<Vec<_>>(), vec![Some("studies"), Some("projects"), None]);
        let single = summarize(std::iter::once(messages[0].as_slice()), &catalog, false);
        assert_eq!(single.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), vec!["Hidden label", "Projects"]);
        assert!(summarize(std::iter::once([].as_slice()), &catalog, true).is_empty());
    }
    #[test]
    fn colors_preserve_gmail_values_even_with_low_contrast() {
        for background in ["#076239", "#16a765", "#ffffff"] {
            let expected = Some((Rgba::try_from("#ffffff").unwrap().into(), Rgba::try_from(background).unwrap().into()));
            assert_eq!(colors(&LabelColor { text: "#ffffff".into(), background: background.into() }), expected);
        }
        for text in ["red", "#zzzzzz", "#fff"] {
            assert!(colors(&LabelColor { text: text.into(), background: "#ffffff".into() }).is_none());
        }
        assert_eq!(visible_count(5, 200.), 2);
        assert_eq!(visible_count(5, 100.), 1);
        assert_eq!(visible_count(5, 60.), 0);
        assert_eq!(visible_count(0, 200.), 0);
    }
}
