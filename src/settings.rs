use crate::{
    theme::Palette,
    triage::{Rule, Work},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LEGACY_THEMES: [&str; 10] = [
    "slate", "carbon", "ink", "moss", "plum", "paper", "bone", "frost", "linen", "swiss",
];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PaneWidths {
    pub reader: u32,
    pub staging: u32,
}
impl Default for PaneWidths {
    fn default() -> Self {
        Self { reader: 480, staging: 320 }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PaneVisibility {
    pub reader: bool,
    pub staging: bool,
}
impl Default for PaneVisibility {
    fn default() -> Self {
        Self { reader: false, staging: true }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReaderPosition {
    #[default]
    Beside,
    Below,
}
fn default_reader_height() -> u32 { 360 }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spaces: Vec<crate::spaces::Space>,
    #[serde(default, rename = "labelUsage", skip_serializing_if = "BTreeMap::is_empty")]
    pub label_usage: BTreeMap<String, crate::labels::Usage>,
    #[serde(rename = "autoApply")]
    pub auto_apply: bool,
    pub theme: String,
    #[serde(default)]
    pub themes: BTreeMap<String, Palette>,
    #[serde(default, rename = "paneWidths")]
    pub pane_widths: PaneWidths,
    #[serde(default, rename = "paneVisibility")]
    pub pane_visibility: PaneVisibility,
    #[serde(default, rename = "stagingReviewOpen")]
    pub staging_review_open: bool,
    #[serde(default, rename = "readerPosition")]
    pub reader_position: ReaderPosition,
    #[serde(default = "default_reader_height", rename = "readerHeight")]
    pub reader_height: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            rules: Vec::new(),
            spaces: Vec::new(),
            label_usage: BTreeMap::new(),
            auto_apply: false,
            theme: "frost".into(),
            themes: BTreeMap::from([("frost".into(), Palette::default())]),
            pane_widths: PaneWidths::default(),
            pane_visibility: PaneVisibility::default(),
            staging_review_open: false,
            reader_position: ReaderPosition::default(),
            reader_height: default_reader_height(),
        }
    }
}
impl Settings {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= 1024 * 1024, "Settings exceed 1 MiB");
        let mut s: Self = serde_json::from_slice(bytes).context("Invalid settings export")?;
        ensure!(s.version == 1, "Unsupported settings version");
        ensure!(
            s.themes.keys().all(|name| !name.trim().is_empty()
                && name.len() <= 64
                && name == name.trim()
                && !name.chars().any(char::is_control)),
            "Theme names must be nonempty and at most 64 bytes"
        );
        s.themes.entry("frost".into()).or_default();
        // Old built-in choices become Frost; explicitly defined custom themes retain their names.
        if !s.themes.contains_key(&s.theme) && LEGACY_THEMES.contains(&s.theme.as_str()) {
            s.theme = "frost".into();
        }
        ensure!(
            s.theme == "frost" || s.themes.contains_key(&s.theme),
            "Selected theme has no definition in 'themes'"
        );
        ensure!(
            (320..=8192).contains(&s.pane_widths.reader)
                && (260..=8192).contains(&s.pane_widths.staging),
            "Pane widths must be 320..8192 for reader and 260..8192 for staging"
        );
        ensure!((240..=8192).contains(&s.reader_height), "Reader height must be 240..8192");
        validate_rules(&s.rules)?;
        crate::spaces::validate(&s.spaces)?;
        ensure!(s.label_usage.len() <= 10000, "Too many quick-label history entries");
        for (id, usage) in &s.label_usage {
            crate::spaces::validate_labels(std::slice::from_ref(id))?;
            ensure!(usage.count > 0, "Quick-label use count must be positive");
        }
        Ok(s)
    }
    pub fn load(path: &Path) -> Result<Self> {
        match read_bounded(path) {
            Ok(bytes) => Self::decode(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let normalized = Self::decode(&serde_json::to_vec(self)?)?;
        atomic_write(path, &serde_json::to_vec_pretty(&normalized)?)
    }
    pub fn load_or_migrate(path: &Path, legacy: &Path) -> Result<Self> {
        let (mut settings, migrate) = match read_bounded(path) {
            Ok(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                let settings = Self::decode(&bytes)?;
                let palette = serde_json::to_value(Palette::default())?;
                let missing_colors = palette.as_object().unwrap().keys()
                    .any(|key| value["themes"]["frost"].get(key).is_none());
                (settings, value.get("rules").is_some() || missing_colors)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Self::load(legacy)?, true),
            Err(e) => return Err(e.into()),
        };
        let rules_file = rules_path(path);
        match read_bounded(&rules_file) {
            Ok(bytes) => settings.rules = decode_rules(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                save_rules(&rules_file, &settings.rules)?
            }
            Err(e) => return Err(e.into()),
        }
        if migrate {
            backup(path)?;
            settings.save_preferences(path)?;
        }
        Ok(settings)
    }
    fn save_preferences(&self, path: &Path) -> Result<()> {
        let mut preferences = self.clone();
        preferences.rules.clear();
        preferences.save(path)
    }
    pub fn save_config(&self, path: &Path, previous: &Self) -> Result<()> {
        // ponytail: files are atomic individually, not as a pair; block after partial errors. Use a journal if imports need all-or-nothing commits.
        let normalized = Self::decode(&serde_json::to_vec(self)?)?;
        if normalized.rules != previous.rules || !rules_path(path).exists() {
            save_rules(&rules_path(path), &normalized.rules)?;
        }
        if normalized.version != previous.version
            || normalized.auto_apply != previous.auto_apply
            || normalized.theme != previous.theme
            || normalized.themes != previous.themes
            || normalized.pane_widths != previous.pane_widths
            || normalized.pane_visibility != previous.pane_visibility
            || normalized.staging_review_open != previous.staging_review_open
            || normalized.reader_position != previous.reader_position
            || normalized.reader_height != previous.reader_height
            || normalized.spaces != previous.spaces
            || normalized.label_usage != previous.label_usage
            || !path.exists()
        {
            normalized.save_preferences(path)?;
        }
        Ok(())
    }
    pub fn decode_import(bytes: &[u8], current: &Self) -> Result<Self> {
        ensure!(bytes.len() <= 1024 * 1024, "Import exceeds 1 MiB");
        if serde_json::from_slice::<serde_json::Value>(bytes)?.is_array() {
            let mut next = current.clone();
            next.rules = decode_rules(bytes)?;
            Ok(next)
        } else {
            Self::decode(bytes)
        }
    }
    pub fn palette(&self) -> Palette {
        self.themes.get(&self.theme).copied().unwrap_or_default()
    }
    pub fn theme_names(&self) -> Vec<&str> {
        std::iter::once("frost")
            .chain(self.themes.keys().map(String::as_str).filter(|name| *name != "frost"))
            .collect()
    }
}
/// Reload only color values; external edits must not enable mail actions or move panes.
pub fn load_colors(path: &Path, theme: &str) -> Result<Palette> {
    let value: serde_json::Value = serde_json::from_slice(&read_bounded(path)?)
        .context("Invalid settings JSON")?;
    let colors = value.get("themes").and_then(|themes| themes.get(theme))
        .with_context(|| format!("Selected theme '{theme}' is missing from settings"))?;
    serde_json::from_value(colors.clone())
        .with_context(|| format!("Invalid colors for theme '{theme}'"))
}

pub fn rules_path(settings_path: &Path) -> PathBuf {
    settings_path.with_file_name("rules.json")
}
fn validate_rules(rules: &[Rule]) -> Result<()> {
    ensure!(rules.len() <= 5000, "Too many sender rules");
    let mut ids = BTreeSet::new();
    for r in rules {
        ensure!(!matches!(r.action, crate::triage::Action::Space(_)), "Sender rules only support Archive and Trash");
        ensure!(
            !r.id.trim().is_empty() && r.id.len() <= 256 && ids.insert(&r.id),
            "Empty or duplicate rule ID"
        );
        ensure!(
            !r.from.trim().is_empty()
                && r.from.len() <= 4096
                && !r.from.contains(['\r', '\n', '\0']),
            "Invalid sender rule"
        );
    }
    Ok(())
}
pub fn decode_rules(bytes: &[u8]) -> Result<Vec<Rule>> {
    ensure!(bytes.len() <= 1024 * 1024, "Rules exceed 1 MiB");
    let rules: Vec<Rule> = serde_json::from_slice(bytes)
        .context("Invalid rules.json; expected an ordered array of sender rules")?;
    validate_rules(&rules)?;
    Ok(rules)
}
pub fn save_rules(path: &Path, rules: &[Rule]) -> Result<()> {
    validate_rules(rules)?;
    let bytes = serde_json::to_vec_pretty(rules)?;
    ensure!(bytes.len() <= 1024 * 1024, "Rules exceed 1 MiB");
    atomic_write(path, &bytes)
}
pub fn backup(path: &Path) -> Result<()> {
    if path.try_exists()? {
        let copy = path.with_extension(format!(
            "backup-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::copy(path, copy).context("Could not back up existing configuration")?;
    }
    Ok(())
}
#[cfg(target_os = "windows")]
const HOME_VARIABLE: &str = "USERPROFILE";
#[cfg(target_os = "macos")]
const HOME_VARIABLE: &str = "HOME";

fn directory_in(home: Option<std::ffi::OsString>) -> Result<PathBuf> {
    let home = home.filter(|home| !home.is_empty()).context("Home directory is not set")?;
    let home = PathBuf::from(home);
    ensure!(home.is_absolute(), "Home directory must be absolute");
    Ok(home.join(".mado/mado-mail"))
}
pub fn directory() -> Result<PathBuf> {
    directory_in(std::env::var_os(HOME_VARIABLE))
}
#[cfg(test)]
#[test]
fn home_directory_policy() {
    assert!(directory_in(None).is_err());
    assert!(directory_in(Some("".into())).is_err());
    assert!(directory_in(Some("relative".into())).is_err());
    let root = std::env::temp_dir();
    assert_eq!(directory_in(Some(root.clone().into())).unwrap(), root.join(".mado/mado-mail"));
    #[cfg(target_os = "macos")]
    assert_eq!(HOME_VARIABLE, "HOME");
    #[cfg(target_os = "windows")]
    assert_eq!(HOME_VARIABLE, "USERPROFILE");
}

pub fn config_path() -> Result<PathBuf> {
    Ok(directory()?.join("settings.json"))
}
pub fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Settings file exceeds 1 MiB",
        ));
    }
    Ok(bytes)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Missing settings directory")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|e| e.error)
        .context("Could not replace saved settings")?;
    Ok(())
}
pub fn save_journal(path: &Path, work: &[Work]) -> Result<()> {
    let bytes = serde_json::to_vec(work)?;
    ensure!(bytes.len() <= 1024 * 1024, "Submission journal exceeds 1 MiB. Apply fewer messages at a time.");
    atomic_write(path, &bytes)
}
pub fn load_journal(path: &Path) -> Result<Vec<Work>> {
    let bytes = match read_bounded(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        bytes.len() <= 1024 * 1024,
        "Submission journal exceeds 1 MiB"
    );
    let work: Vec<Work> = serde_json::from_slice(&bytes)
        .context("Invalid submission journal; preserve it before recovery")?;
    let mut ids = BTreeSet::new();
    ensure!(
        work.iter()
            .all(|w| !w.id.is_empty() && w.id.len() <= 256 && ids.insert(&w.id)),
        "Invalid submission IDs"
    );
    for w in &work {
        if let crate::triage::Action::Space(id) = w.action {
            ensure!(id > 0, "Invalid submission space ID");
            crate::spaces::validate_labels(&w.label_ids)?;
        } else {
            ensure!(w.label_ids.is_empty(), "Unexpected submission labels");
        }
    }
    Ok(work)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_reload_ignores_non_color_edits_and_survives_a_later_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let previous = Settings::default();
        previous.save_config(&path, &previous).unwrap();
        let rules = fs::read(rules_path(&path)).unwrap();
        let edited = br##"{"theme":"other","autoApply":true,"rules":[{"id":"x","from":"x","action":"trash"}],"paneWidths":{"reader":800,"staging":500},"themes":{"frost":{"header_background":"#233044","accent":"#087f5b"},"other":{"accent":"invalid"}}}"##;
        fs::write(&path, edited).unwrap();
        let colors = load_colors(&path, &previous.theme).unwrap();
        assert_eq!(colors.header().canvas, gpui::rgb(0x233044).into());
        assert_eq!(colors.accent, gpui::rgb(0x087f5b).into());
        assert_eq!(colors.canvas, Palette::default().canvas);
        assert_eq!(fs::read(&path).unwrap(), edited);
        assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
        let mut next = previous.clone();
        next.themes.insert(next.theme.clone(), colors);
        next.save_config(&path, &previous).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), next);
        assert!(!next.auto_apply && next.rules.is_empty());
        assert_eq!(next.pane_widths, previous.pane_widths);
        assert_eq!(next.theme, previous.theme);
        assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
    }
    #[test]
    fn color_reload_rejects_bad_files_and_recovers_without_rewriting_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(load_colors(&path, "frost").is_err());
        for invalid in [
            "{", "{}", r#"{"themes":{"other":{}}}"#,
            r#"{"themes":{"frost":null}}"#,
            r#"{"themes":{"frost":{"header_background":"invalid"}}}"#,
            r##"{"themes":{"frost":{"unknown":"#233044"}}}"##,
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(load_colors(&path, "frost").is_err());
            assert_eq!(fs::read(&path).unwrap(), invalid.as_bytes());
        }
        fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
        assert!(load_colors(&path, "frost").is_err());
        fs::write(&path, br#"{"themes":{"frost":{}}}"#).unwrap();
        assert_eq!(load_colors(&path, "frost").unwrap(), Palette::default());
    }
    #[test]
    fn color_reload_reopens_atomically_replaced_files_even_with_same_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for color in ["#233044", "#344455", "#233044"] {
            let json = format!(r#"{{"themes":{{"frost":{{"header_background":"{color}"}}}}}}"#);
            atomic_write(&path, json.as_bytes()).unwrap();
            let expected: Palette = serde_json::from_str(&format!(r#"{{"header_background":"{color}"}}"#)).unwrap();
            assert_eq!(load_colors(&path, "frost").unwrap(), expected);
        }
    }
    #[test]
    fn quick_label_history_defaults_validates_and_persists_without_changing_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let old = Settings::decode(br#"{"version":1,"autoApply":false,"theme":"frost"}"#).unwrap();
        assert!(old.label_usage.is_empty());
        old.save_config(&path, &old).unwrap();
        let rules = fs::read(rules_path(&path)).unwrap();
        let mut next = old.clone();
        next.label_usage.insert("projects".into(), crate::labels::Usage { count: 3, last_used: 1_000_000 });
        next.save_config(&path, &old).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), next);
        assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
        let saved = fs::read(&path).unwrap();
        for (id, count) in [("INBOX", 1), ("", 1), ("projects", 0)] {
            let mut invalid = old.clone();
            invalid.label_usage.insert(id.into(), crate::labels::Usage { count, last_used: 0 });
            assert!(invalid.save_config(&path, &next).is_err());
            assert_eq!(fs::read(&path).unwrap(), saved);
        }
    }
    #[test]
    fn spaces_persist_without_changing_rules_and_journals_keep_label_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let old = Settings::decode(br#"{"version":1,"autoApply":true,"theme":"frost"}"#).unwrap();
        assert!(old.spaces.is_empty());
        old.save_config(&path, &old).unwrap();
        let rules = fs::read(rules_path(&path)).unwrap();
        let mut next = old.clone();
        next.spaces.push(crate::spaces::Space { id: 1, name: "Work".into(), label_ids: vec!["projects".into(), "finance".into()] });
        next.save_config(&path, &old).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), next);
        assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
        let saved = fs::read(&path).unwrap();
        let mut invalid = next.clone();
        invalid.spaces[0].label_ids.push("INBOX".into());
        assert!(invalid.save_config(&path, &next).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        invalid = next.clone(); invalid.spaces.push(next.spaces[0].clone());
        assert!(Settings::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
        assert!(decode_rules(br#"[{"id":"x","from":"x","action":{"space":1}}]"#).is_err());
        let journal = dir.path().join("submission.json");
        let work = vec![Work { id: "message".into(), action: crate::triage::Action::Space(1), label_ids: next.spaces[0].label_ids.clone() }];
        save_journal(&journal, &work).unwrap();
        assert_eq!(load_journal(&journal).unwrap(), work);
        let oversized = vec![work[0].clone(); 20000];
        assert!(save_journal(&journal, &oversized).is_err());
        assert_eq!(load_journal(&journal).unwrap(), work);
        fs::write(&journal, br#"[{"id":"message","action":"archive"}]"#).unwrap();
        assert!(load_journal(&journal).unwrap()[0].label_ids.is_empty());
        fs::write(&journal, br#"[{"id":"message","action":{"space":1}}]"#).unwrap();
        assert!(load_journal(&journal).is_err());
        let mut removed = next.clone();
        removed.spaces.clear();
        removed.save_config(&path, &next).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), old);
        assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
    }
    #[test]
    fn reader_layout_defaults_validates_and_persists_without_changing_other_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut previous = Settings::decode(br#"{"version":1,"autoApply":false,"theme":"frost"}"#).unwrap();
        assert_eq!(previous.reader_position, ReaderPosition::Beside);
        assert_eq!(previous.reader_height, 360);
        previous.save_config(&path, &previous).unwrap();
        let rules = fs::read(rules_path(&path)).unwrap();
        for (position, height) in [(ReaderPosition::Below, 360), (ReaderPosition::Below, 416), (ReaderPosition::Beside, 416)] {
            let mut next = previous.clone();
            next.reader_position = position;
            next.reader_height = height;
            next.save_config(&path, &previous).unwrap();
            assert_eq!(Settings::load(&path).unwrap(), next);
            assert_eq!(next.pane_widths, previous.pane_widths);
            assert_eq!(next.pane_visibility, previous.pane_visibility);
            assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
            previous = next;
        }
        let saved = fs::read(&path).unwrap();
        for height in [0, 239, 8193] {
            let mut next = previous.clone();
            next.reader_height = height;
            assert!(next.save_config(&path, &previous).is_err());
            assert_eq!(fs::read(&path).unwrap(), saved);
        }
        assert!(Settings::decode(br#"{"version":1,"autoApply":false,"theme":"frost","readerPosition":"left"}"#).is_err());
    }
    #[test]
    fn staging_review_defaults_round_trips_and_rejects_nonbooleans_without_rewriting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let legacy = dir.path().join("gpui-settings.json");
        let original = br##"{"version":1,"autoApply":true,"theme":"custom","themes":{"custom":{"accent":"#087f5b"}},"paneWidths":{"reader":610,"staging":390},"paneVisibility":{"reader":true,"staging":true},"readerPosition":"below","readerHeight":416,"spaces":[{"id":1,"name":"Work","label_ids":["projects"]}],"labelUsage":{"projects":{"count":3,"lastUsed":1000000}},"rules":[{"id":"b","from":"B","action":"trash"},{"id":"a","from":"A","action":"archive"}]}"##;
        fs::write(&legacy, original).unwrap();
        let baseline = Settings::load_or_migrate(&path, &legacy).unwrap();
        assert!(baseline.pane_visibility.staging);
        assert!(!baseline.staging_review_open);
        assert!(!Settings::default().staging_review_open);
        let rules = fs::read(rules_path(&path)).unwrap();
        let mut previous = baseline.clone();
        for open in [true, false] {
            let mut next = previous.clone();
            next.staging_review_open = open;
            next.save_config(&path, &previous).unwrap();
            let restored = Settings::load_or_migrate(&path, &legacy).unwrap();
            assert_eq!(restored, next);
            let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved["stagingReviewOpen"], serde_json::json!(open));
            let mut unchanged = restored.clone();
            unchanged.staging_review_open = false;
            assert_eq!(unchanged, baseline);
            assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
            previous = restored;
        }
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for invalid in [serde_json::json!("true"), serde_json::json!(1), serde_json::Value::Null] {
            let mut edited = saved.clone();
            edited["stagingReviewOpen"] = invalid;
            let bytes = serde_json::to_vec(&edited).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(Settings::load_or_migrate(&path, &legacy).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
        }
        assert_eq!(fs::read(&legacy).unwrap(), original);
    }
    #[test]
    fn pane_visibility_restores_all_states_without_changing_widths_or_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut previous = Settings::decode(br#"{"version":1,"autoApply":false,"theme":"frost","paneWidths":{"reader":610,"staging":390}}"#).unwrap();
        assert_eq!(previous.pane_visibility, PaneVisibility::default());
        previous.save_config(&path, &previous).unwrap();
        let rules = fs::read(rules_path(&path)).unwrap();
        for (reader, staging) in [(true, false), (true, true), (false, true), (false, false)] {
            let mut next = previous.clone();
            next.pane_visibility = PaneVisibility { reader, staging };
            next.save_config(&path, &previous).unwrap();
            let restored = Settings::load_or_migrate(&path, &dir.path().join("gpui-settings.json")).unwrap();
            assert_eq!(restored.pane_visibility, next.pane_visibility);
            assert_eq!(restored.pane_widths, previous.pane_widths);
            assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
            previous = restored;
        }
        assert!(Settings::decode(br#"{"version":1,"autoApply":false,"theme":"frost","paneVisibility":{"reader":"yes"}}"#).is_err());
    }
    #[test]
    fn pane_widths_migrate_validate_and_persist_without_changing_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let old = Settings::decode(br#"{"version":1,"autoApply":false,"theme":"frost"}"#).unwrap();
        assert_eq!(old.pane_widths, PaneWidths::default());
        old.save_config(&path, &old).unwrap();
        let rules = fs::read(rules_path(&path)).unwrap();
        let mut next = old.clone();
        next.pane_widths = PaneWidths { reader: 610, staging: 390 };
        next.save_config(&path, &old).unwrap();
        assert_eq!(Settings::load(&path).unwrap().pane_widths, next.pane_widths);
        assert_eq!(fs::read(rules_path(&path)).unwrap(), rules);
        let saved = fs::read(&path).unwrap();
        for widths in [(0, 320), (319, 320), (480, 259), (9000, 320)] {
            next.pane_widths = PaneWidths { reader: widths.0, staging: widths.1 };
            assert!(next.save_config(&path, &old).is_err());
            assert_eq!(fs::read(&path).unwrap(), saved);
        }
    }
    #[test]
    fn frost_is_the_only_builtin_and_custom_themes_round_trip() {
        assert_eq!(Settings::default().theme_names(), vec!["frost"]);
        let custom = Settings::decode(br##"{"version":1,"rules":[],"autoApply":false,"theme":"ocean","themes":{"ocean":{"accent":"#087f5b","panel":"#e6f7ef","header_background":"#233044"}}}"##).unwrap();
        assert_eq!(custom.theme_names(), vec!["frost", "ocean"]);
        assert_eq!(custom.palette().accent, gpui::rgb(0x087f5b).into());
        assert_eq!(custom.palette().canvas, Palette::default().canvas);
        assert_eq!(custom.palette().header().canvas, gpui::rgb(0x233044).into());
        assert_eq!(
            Settings::decode(&serde_json::to_vec(&custom).unwrap()).unwrap(),
            custom
        );
        for bad in [
            br##"{"version":1,"rules":[],"autoApply":false,"theme":"missing"}"##.as_slice(),
            br##"{"version":1,"rules":[],"autoApply":false,"theme":"frost","themes":{"":{}}}"##,
            br##"{"version":1,"rules":[],"autoApply":false,"theme":"frost","themes":{"custom":{"accent":"invalid"}}}"##,
        ] { assert!(Settings::decode(bad).is_err()); }
    }
    #[test]
    fn frost_is_editable_and_missing_colors_are_saved_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = br##"{"version":1,"autoApply":true,"theme":"frost","themes":{"frost":{"panel2":"#ddeeff"}},"paneWidths":{"reader":610,"staging":390}}"##;
        let mut original = serde_json::to_value(Settings::decode(original).unwrap()).unwrap();
        original["themes"]["frost"].as_object_mut().unwrap().remove("header_background");
        let original = serde_json::to_vec(&original).unwrap();
        fs::write(&path, &original).unwrap();
        let settings = Settings::load_or_migrate(&path, &dir.path().join("gpui-settings.json")).unwrap();
        assert_eq!(settings.palette().panel2, gpui::rgb(0xddeeff).into());
        assert_eq!(settings.palette().canvas, Palette::default().canvas);
        assert_eq!(settings.theme_names(), vec!["frost"]);
        assert!(settings.auto_apply);
        assert_eq!(settings.pane_widths.reader, 610);
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["themes"]["frost"].as_object().unwrap().len(), 10);
        assert_eq!(settings.palette().header().canvas, gpui::rgb(0x1b2230).into());
        assert_eq!(saved["themes"]["frost"]["header_background"], "#1b2230ff");
        let before = fs::read(&path).unwrap();
        let count = fs::read_dir(dir.path()).unwrap().count();
        Settings::load_or_migrate(&path, &dir.path().join("gpui-settings.json")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), count);
        assert!(fs::read_dir(dir.path()).unwrap().any(|entry| {
            let entry = entry.unwrap();
            entry.file_name().to_string_lossy().starts_with("settings.backup-")
                && fs::read(entry.path()).unwrap() == original
        }));
        let mut custom = settings.clone();
        custom.themes.get_mut("frost").unwrap().header_background = gpui::rgb(0x233044).into();
        custom.save_config(&path, &settings).unwrap();
        assert_eq!(Settings::load_or_migrate(&path, &dir.path().join("gpui-settings.json")).unwrap(), custom);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), count);
        assert_eq!(config_path().unwrap(), directory().unwrap().join("settings.json"));
        assert!(directory().unwrap().ends_with(".mado/mado-mail"));
    }
    #[test]
    fn migration_keeps_legacy_data_and_does_not_replace_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("mado_mail/gpui-settings.json");
        let config = dir.path().join("mado-mail/settings.json");
        let original = br#"{"version":1,"rules":[{"id":"b","from":"B","action":"trash"},{"id":"a","from":"A","action":"archive"}],"autoApply":true,"theme":"ink"}"#;
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, original).unwrap();
        let migrated = Settings::load_or_migrate(&config, &legacy).unwrap();
        assert_eq!(migrated.theme, "frost");
        assert!(migrated.auto_apply);
        assert_eq!(
            migrated
                .rules
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert_eq!(fs::read(&legacy).unwrap(), original);
        assert_eq!(
            Settings::load_or_migrate(&config, &legacy).unwrap(),
            migrated
        );
        let preferences: serde_json::Value =
            serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert!(preferences.get("rules").is_none());
        assert_eq!(
            decode_rules(&fs::read(rules_path(&config)).unwrap()).unwrap(),
            migrated.rules
        );
        Settings::default().save_config(&config, &migrated).unwrap();
        assert!(
            Settings::load_or_migrate(&config, &legacy)
                .unwrap()
                .rules
                .is_empty()
        );
        fs::write(&config, b"broken").unwrap();
        assert!(Settings::load_or_migrate(&config, &legacy).is_err());
        assert_eq!(fs::read(&config).unwrap(), b"broken");
        let fresh = dir.path().join("fresh/settings.json");
        assert_eq!(
            Settings::load_or_migrate(&fresh, &dir.path().join("absent"))
                .unwrap()
                .theme,
            "frost"
        );
        assert!(fresh.exists());
    }
    #[test]
    fn separate_rules_are_authoritative_and_standalone_import_keeps_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("settings.json");
        let legacy = dir.path().join("legacy.json");
        let original = Settings::decode(br#"{"version":1,"rules":[{"id":"old","from":"old","action":"archive"}],"autoApply":true,"theme":"frost"}"#).unwrap();
        original.save(&config).unwrap();
        let rules = decode_rules(br#"[{"id":"b","from":"second","action":"trash"},{"id":"a","from":"first","action":"archive"}]"#).unwrap();
        save_rules(&rules_path(&config), &rules).unwrap();
        let loaded = Settings::load_or_migrate(&config, &legacy).unwrap();
        assert_eq!(loaded.rules, rules);
        assert!(loaded.auto_apply);
        let import = Settings::decode_import(br#"[]"#, &loaded).unwrap();
        assert!(import.auto_apply && import.rules.is_empty());
        assert_eq!(import.theme, loaded.theme);
        import.save_config(&config, &loaded).unwrap();
        assert!(
            Settings::load_or_migrate(&config, &legacy)
                .unwrap()
                .rules
                .is_empty()
        );
        let before = fs::read(&config).unwrap();
        fs::write(rules_path(&config), b"broken").unwrap();
        assert!(Settings::load_or_migrate(&config, &legacy).is_err());
        assert_eq!(fs::read(&config).unwrap(), before);
        assert_eq!(fs::read(rules_path(&config)).unwrap(), b"broken");
        assert!(decode_rules(br#"[{"id":"x","from":"sender","action":"delete"}]"#).is_err());
        assert!(decode_rules(br#"[{"id":"x","from":"sender","action":"trash"},{"id":"x","from":"other","action":"archive"}]"#).is_err());
    }
    #[test]
    fn settings_round_trip_preserves_order_and_rejects_invalid_imports() {
        let s=Settings::decode(br#"{"version":1,"rules":[{"id":"b","from":"@example.test","action":"trash"},{"id":"a","from":"Ada","action":"archive"}],"autoApply":true,"theme":"paper"}"#).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        let mut updated = s.clone();
        updated.themes.insert("custom".into(), Palette::default());
        updated.theme = "custom".into();
        updated.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), updated);
        let before = fs::read(&path).unwrap();
        updated.rules[0].from = "".into();
        assert!(updated.save(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(
            Settings::decode(br#"{"version":2,"rules":[],"autoApply":false,"theme":"ink"}"#)
                .is_err()
        );
        assert!(
            Settings::decode(
                br#"{"version":1,"rules":[],"autoApply":false,"theme":"ink","token":"secret"}"#
            )
            .is_err()
        );
        let journal = dir.path().join("writes.json");
        let work = vec![Work {
            id: "synthetic".into(),
            action: crate::triage::Action::Trash,
            label_ids: Vec::new(),
        }];
        save_journal(&journal, &work).unwrap();
        assert_eq!(load_journal(&journal).unwrap(), work);
        save_journal(&journal, &[]).unwrap();
        assert!(load_journal(&journal).unwrap().is_empty());
    }
}
