#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gmail;
mod icons;
mod input;
mod labels;
mod label_ui;
mod message;
mod reader;
mod reader_check;
mod settings;
mod spaces;
mod space_ui;
mod theme;
mod triage;
mod triage_check;

use anyhow::{Context as _, Result};
use gmail::{Email, Session, SharedSession, SignIn};
use gpui::{
    App, Application, Bounds, Context, CursorStyle, DispatchPhase, Entity, FocusHandle, KeyBinding,
    MouseButton, MouseMoveEvent, MouseUpEvent, PathPromptOptions, Pixels, Point, PromptLevel,
    ScrollStrategy, SharedString, TitlebarOptions, UniformListScrollHandle, Window, WindowBounds,
    WindowOptions, actions, canvas, div, prelude::*, px, size, uniform_list,
};
use input::TextInput;
use label_ui::LabelMode;
use reader::Reader;
use settings::{ReaderPosition, Settings};
use std::{
    cell::Cell,
    rc::Rc,
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use theme::Palette;
use triage::{Action, Outcome, Row, Rule, Triage, rule_for, sender_address, sender_name};

actions!(
    mail,
    [Refresh, Apply, FocusFilter, ToggleRules, ToggleStaging, ToggleReader]
);

const DIVIDER: f32 = 6.;
const SCROLLBAR_WIDTH: f32 = 12.;
fn row_background(row: &Row, hovered: Option<&str>, active: bool, p: Palette) -> gpui::Hsla {
    let bg = if active { p.panel2.blend(p.accent.opacity(0.12)) } else if row.child { p.panel2 } else { p.canvas };
    let highlighted = hovered.is_some_and(|key| key == row.key
        || (row.child && key.strip_prefix("b:").is_some_and(|sender| sender == sender_address(&row.sender))));
    if highlighted { bg.blend(gpui::black().opacity(0.04)) } else { bg }
}
fn scrollbar_thumb(viewport: f32, content: f32, offset: f32) -> Option<(f32, f32)> {
    if viewport <= 0. || content <= viewport { return None; }
    let height = (viewport * viewport / content).max(24.).min(viewport);
    let top = offset.clamp(0., content - viewport) / (content - viewport) * (viewport - height);
    Some((top, height))
}
#[derive(Clone, Copy)]
struct InboxScrollGeometry {
    track: Bounds<Pixels>,
    top: f32,
    height: f32,
    max_offset: f32,
}
fn pane_widths(viewport: f32, reader: Option<f32>, staging: Option<f32>) -> (f32, f32) {
    let r_min = if reader.is_some() { 320. } else { 0. };
    let s_min = if staging.is_some() { 260. } else { 0. };
    let r = reader.map_or(0., |v| v.clamp(r_min, 8192.));
    let s = staging.map_or(0., |v| v.clamp(s_min, 8192.));
    let dividers = (reader.is_some() as u8 + staging.is_some() as u8) as f32 * DIVIDER;
    let budget = (viewport - 420. - dividers).max(r_min + s_min);
    let extra = r + s - r_min - s_min;
    if r + s > budget && extra > 0. {
        let scale = (budget - r_min - s_min) / extra;
        (r_min + (r - r_min) * scale, s_min + (s - s_min) * scale)
    } else {
        (r, s)
    }
}
fn reader_height(available: f32, preferred: f32) -> f32 {
    preferred.clamp(240., 8192.).min((available - 160. - DIVIDER).max(0.))
}
fn clamp_archive_fraction(fraction: f32, height: f32) -> f32 {
    let min = (80. / height.max(1.)).min(0.5);
    fraction.clamp(min, 1. - min)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ResizeAxis {
    Width,
    Reader,
    ReaderHeight,
    Bins,
}
#[derive(Clone, Copy)]
struct ResizeDrag {
    axis: ResizeAxis,
    start: Point<Pixels>,
    value: f32,
}

struct LabelHint(String);
impl Render for LabelHint {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        div().p_2().max_w(px(300.)).bg(p.panel).text_color(p.ink).text_xs().whitespace_normal()
            .border_1().border_color(p.line).child(self.0.clone())
    }
}
#[derive(Clone)]
struct MailDrag { ids: Vec<String>, senders: usize }
struct DragBadge { payload: MailDrag, offset: Point<Pixels> }
impl Render for DragBadge {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        div().pl(self.offset.x + px(12.)).pt(self.offset.y + px(12.)).child(
            div().p_2().bg(p.panel).border_1().border_color(p.accent).text_color(p.ink)
                .child(format!("{} messages / {} senders", self.payload.ids.len(), self.payload.senders)))
    }
}
struct RowMenu { row: Row, anchor: Bounds<Pixels>, selected: usize }
fn menu_position(anchor: Bounds<Pixels>, viewport: gpui::Size<Pixels>) -> Point<Pixels> {
    let x = f32::from(anchor.right()) - 360.;
    let below = f32::from(anchor.bottom());
    let y = if below + 252. <= f32::from(viewport.height) { below } else { f32::from(anchor.top()) - 252. };
    gpui::point(px(x.clamp(8., (f32::from(viewport.width) - 368.).max(8.))),
        px(y.clamp(8., (f32::from(viewport.height) - 260.).max(8.))))
}
struct Hint(&'static str);
impl Render for Hint {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        div()
            .p_2()
            .rounded_sm()
            .bg(p.panel)
            .text_color(p.ink)
            .border_1()
            .border_color(p.line)
            .text_xs()
            .child(self.0)
    }
}

#[derive(Clone)]
enum Command {
    Refresh(bool),
    Rules,
    Stage(Vec<String>, Action),
    SetRead(Vec<String>, bool),
    MarkReadArchive(Vec<String>),
    Expand(String),
    Open(String),
    RowMenu(String),
    Clear,
    PutBack(Vec<String>),
    Empty,
    Apply,
    Recheck,
    AutoApply,
    Theme,
    ToggleSidebar,
    ToggleReader,
    ToggleReaderPosition,
    EditRule(usize),
    DropRule(usize),
    SaveRule,
    CancelEdit,
    EditAction(Action),
    Import,
    Export,
    Peek(String, Action),
    ClosePeek,
    NewSpace,
    EditSpace(u64),
    SaveSpace,
    CancelSpace,
    SpacePicker,
    SpaceLabel(String),
    TopLabelPicker(LabelMode),
    RowLabelPicker(String),
    PickTopLabel(String),
    RemoveLabelFilter(String),
    RemoveRowLabel(String, String),
    ConfirmLabelRemoval,
    CancelLabelRemoval,
    AddLabelFilter(String),
    CloseLabelPicker,
}
struct InstanceLock {
    _file: std::fs::File,
}
impl gpui::Global for InstanceLock {}

struct MailApp {
    focus: FocusHandle,
    table_focus: FocusHandle,
    filter: Entity<TextInput>,
    filter_value: String,
    rule_input: Entity<TextInput>,
    space_name: Entity<TextInput>,
    space_query: Entity<TextInput>,
    space_draft: Option<space_ui::SpaceDraft>,
    label_query: Entity<TextInput>,
    label_picker: Option<LabelMode>,
    label_picker_cursor: usize,
    label_picker_anchors: [Bounds<Pixels>; 2],
    label_picker_bounds: Bounds<Pixels>,
    label_removal: Option<label_ui::LabelRemoval>,
    label_removal_bounds: Bounds<Pixels>,
    label_removal_focus: [FocusHandle; 2],
    label_row_targets: Option<(String, Vec<String>)>,
    label_row_anchors: BTreeMap<String, Bounds<Pixels>>,
    staging_scroll: gpui::ScrollHandle,
    session: Option<SharedSession>,
    reader: Option<Entity<Reader>>,
    active_message: Option<String>,
    reader_visible: bool,
    reader_width: f32,
    reader_height: f32,
    workspace_bounds: Bounds<Pixels>,
    triage: Triage,
    labels: labels::Catalog,
    label_revision: u64,
    row_menu: Option<RowMenu>,
    menu_bounds: Bounds<Pixels>,
    menu_focus: FocusHandle,
    row_focus: BTreeMap<String, FocusHandle>,
    label_focus: BTreeMap<String, FocusHandle>,
    row_bounds: BTreeMap<String, Bounds<Pixels>>,
    hovered_row: Option<String>,
    hovered_read_archive: Option<String>,
    mail_drag: Option<MailDrag>,
    drop_target: Option<Action>,
    suppress_row_click: bool,
    settings: Settings,
    settings_path: PathBuf,
    journal_path: PathBuf,
    busy: bool,
    blocked: bool,
    failed: bool,
    demo: bool,
    status: String,
    rules_view: bool,
    editing: Option<(Option<usize>, Action)>,
    peek: Option<(String, Action)>,
    hovered: Option<(String, Action)>,
    cursor: usize,
    scroll: UniformListScrollHandle,
    inbox_scrollbar: Rc<Cell<Option<InboxScrollGeometry>>>,
    inbox_scroll_drag: Option<f32>,
    sidebar_visible: bool,
    sidebar_width: f32,
    archive_fraction: f32,
    bin_bounds: Bounds<Pixels>,
    table_bounds: Bounds<Pixels>,
    resize_drag: Option<ResizeDrag>,
    width_focus: FocusHandle,
    reader_width_focus: FocusHandle,
    split_focus: FocusHandle,
}
impl MailApp {
    fn new(window: &mut Window, cx: &mut Context<Self>, demo: bool) -> Self {
        let filter = cx.new(|cx| {
            let mut input = TextInput::new("Filter messages...", cx);
            input.clearable = true;
            input
        });
        let rule_input = cx.new(|cx| TextInput::new("Sender match", cx));
        let space_name = cx.new(|cx| TextInput::new("Space name...", cx));
        let space_query = cx.new(|cx| TextInput::new("Fuzzy find labels...", cx));
        let label_query = cx.new(|cx| TextInput::new("Fuzzy find labels...", cx));
        cx.observe(&label_query, |this, _, cx| {
            this.label_picker_cursor = 0;
            cx.notify();
        }).detach();
        cx.observe(&space_name, |_, _, cx| cx.notify()).detach();
        cx.observe(&space_query, |this, _, cx| {
            if let Some(draft) = &mut this.space_draft { draft.cursor = 0; }
            cx.notify();
        }).detach();
        cx.observe(&filter, |this, input, cx| {
            let value = input.read(cx).content.to_string();
            if value != this.filter_value {
                this.filter_value = value;
                this.label_removal = None;
                this.triage.selection.clear();
                this.row_menu = None;
                this.hovered_row = None;
                this.cursor = 0;
                cx.notify();
            }
        })
        .detach();
        let mut error = None;
        let directory = settings::directory().unwrap_or_else(|e| {
            error = Some(format!("{e:#}"));
            PathBuf::new()
        });
        let instance_lock = if demo || error.is_some() {
            None
        } else {
            let lock = (|| -> Result<std::fs::File> {
                std::fs::create_dir_all(&directory)?;
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(directory.join("gpui.lock"))?;
                file.try_lock().context(
                    "Another Mado Mail instance is using these settings. Close it first.",
                )?;
                Ok(file)
            })();
            match lock {
                Ok(file) => Some(file),
                Err(e) => {
                    error = Some(format!("{e:#}"));
                    None
                }
            }
        };
        if let Some(file) = instance_lock {
            cx.set_global(InstanceLock { _file: file });
        }
        let settings_path = settings::config_path().unwrap_or_else(|e| {
            error = Some(format!("{e:#}"));
            PathBuf::new()
        });
        let journal_path = directory.join("gpui-submission.json");
        let settings = if demo || error.is_some() {
            Settings::default()
        } else {
            Settings::load_or_migrate(&settings_path, &directory.join("gpui-settings.json"))
                .unwrap_or_else(|e| {
                    error = Some(format!("Settings were not overwritten: {e:#}"));
                    Settings::default()
                })
        };
        settings.palette().apply(cx);
        let mut triage = if demo {
            triage::demo()
        } else {
            Triage::default()
        };
        if !demo {
            match settings::load_journal(&journal_path) {
                Ok(work) => triage.recover(work),
                Err(e) => error = Some(format!("Submission recovery blocked: {e:#}")),
            }
        }
        let focus = cx.focus_handle();
        let table_focus = cx.focus_handle();
        table_focus.focus(window);
        let status = if let Some(ref e) = error {
            e.clone()
        } else if demo {
            "Demo mode. Synthetic mail only; changes are not saved or sent to Gmail.".into()
        } else if triage.has_unknown() {
            "Interrupted submission found. Connect, then Recheck Gmail before retrying.".into()
        } else {
            "Connect Gmail to load your Inbox. Archive and Trash need fresh write consent.".into()
        };
        Self {
            focus,
            table_focus,
            filter,
            filter_value: String::new(),
            rule_input,
            space_name,
            space_query,
            space_draft: None,
            label_query,
            label_picker: None,
            label_picker_cursor: 0,
            label_picker_anchors: [Bounds::default(); 2],
            label_picker_bounds: Bounds::default(),
            label_removal: None,
            label_removal_bounds: Bounds::default(),
            label_removal_focus: [cx.focus_handle(), cx.focus_handle()],
            label_row_targets: None,
            label_row_anchors: BTreeMap::new(),
            staging_scroll: gpui::ScrollHandle::new(),
            session: None,
            reader: None,
            active_message: None,
            reader_visible: settings.pane_visibility.reader,
            sidebar_visible: settings.pane_visibility.staging,
            reader_width: settings.pane_widths.reader as f32,
            reader_height: settings.reader_height as f32,
            workspace_bounds: Bounds::default(),
            sidebar_width: settings.pane_widths.staging as f32,
            triage,
            labels: if demo { labels::demo() } else { labels::Catalog::default() },
            label_revision: 0,
            row_menu: None,
            menu_bounds: Bounds::default(),
            menu_focus: cx.focus_handle(),
            row_focus: BTreeMap::new(),
            label_focus: BTreeMap::new(),
            row_bounds: BTreeMap::new(),
            hovered_row: None,
            hovered_read_archive: None,
            mail_drag: None,
            drop_target: None,
            suppress_row_click: false,
            settings,
            settings_path,
            journal_path,
            busy: false,
            blocked: error.is_some(),
            failed: error.is_some(),
            demo,
            status,
            rules_view: false,
            editing: None,
            peek: None,
            hovered: None,
            cursor: 0,
            scroll: UniformListScrollHandle::new(),
            inbox_scrollbar: Rc::new(Cell::new(None)),
            inbox_scroll_drag: None,
            archive_fraction: 0.5,
            bin_bounds: Bounds::default(),
            table_bounds: Bounds::default(),
            resize_drag: None,
            width_focus: cx.focus_handle(),
            reader_width_focus: cx.focus_handle(),
            split_focus: cx.focus_handle(),
        }
    }
    fn error(&mut self, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        self.failed = true;
        self.status = error.to_string();
        cx.notify();
    }
    fn save_settings(&mut self, next: Settings, cx: &mut Context<Self>) -> bool {
        if self.blocked {
            return false;
        }
        let next = match serde_json::to_vec(&next)
            .map_err(anyhow::Error::from)
            .and_then(|b| Settings::decode(&b))
        {
            Ok(next) => next,
            Err(e) => {
                self.error(format!("Settings not changed: {e:#}"), cx);
                return false;
            }
        };
        if !self.demo
            && let Err(e) = next.save_config(&self.settings_path, &self.settings)
        {
            self.blocked = true;
            self.error(format!("Configuration could not be fully saved: {e:#}. Restart to reload saved rules and preferences before making further changes."), cx);
            return false;
        }
        self.reader_width = next.pane_widths.reader as f32;
        self.reader_height = next.reader_height as f32;
        self.sidebar_width = next.pane_widths.staging as f32;
        self.reader_visible = next.pane_visibility.reader;
        self.sidebar_visible = next.pane_visibility.staging;
        self.settings = next;
        self.settings.palette().apply(cx);
        cx.notify();
        true
    }
    fn load(&mut self, interactive: bool, run_rules: bool, cx: &mut Context<Self>) {
        if self.busy || self.blocked || self.editing.is_some() {
            return;
        }
        self.row_menu = None;
        self.label_revision += 1;
        if self.demo {
            if run_rules {
                self.triage.apply_rules(&self.settings.rules);
            }
            cx.notify();
            return;
        }
        self.busy = true;
        self.failed = false;
        self.status = "Loading Inbox... Pending decisions are kept.".into();
        cx.notify();
        let session = self.session.clone();
        // ponytail: one lock serializes token refresh, reader jobs and writes. An async client is the next step if contention is measured.
        let work = cx.background_executor().spawn(async move {
            let session = match session {
                Some(s) => Some(s),
                None => Session::restore()?.map(|s| Arc::new(Mutex::new(s))),
            };
            let Some(session) = session else {
                return anyhow::Ok(None);
            };
            let emails = session
                .lock()
                .map_err(|_| anyhow::anyhow!("Gmail worker failed"))?
                .inbox()?;
            Ok(emails.map(|emails| (session, emails)))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.loaded(result, interactive, run_rules, cx)
            });
        })
        .detach();
    }
    fn sign_in(&mut self, run_rules: bool, cx: &mut Context<Self>) {
        let sign_in = match SignIn::prepare() {
            Ok(s) => s,
            Err(e) => {
                self.error(format!("{e:#}"), cx);
                return;
            }
        };
        cx.open_url(sign_in.url.as_str());
        self.busy = true;
        self.status =
            "Complete Google sign-in in your browser. Permission allows Archive and Trash.".into();
        cx.notify();
        let work = cx.background_executor().spawn(async move {
            let mut session = sign_in.finish()?;
            let emails = session
                .inbox()?
                .context("Google rejected authorization; reconnect Gmail")?;
            anyhow::Ok(Some((Arc::new(Mutex::new(session)), emails)))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| this.loaded(result, false, run_rules, cx));
        })
        .detach();
    }
    fn loaded(
        &mut self,
        result: Result<Option<(SharedSession, Vec<Email>)>>,
        interactive: bool,
        run_rules: bool,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        match result {
            Ok(Some((session, emails))) => {
                let count = emails.len();
                if !self.session.as_ref().is_some_and(|old| Arc::ptr_eq(old, &session)) {
                    self.labels = labels::Catalog::default();
                }
                self.session = Some(session);
                self.triage.replace_inbox(emails);
                self.refresh_labels(cx);
                if run_rules {
                    self.triage.apply_rules(&self.settings.rules);
                    self.rules_view = false;
                }
                self.failed = false;
                self.status = if self.triage.has_unknown() {
                    "Connected. Recheck Gmail to resolve interrupted or uncertain submissions."
                        .into()
                } else {
                    format!(
                        "{count} Inbox messages loaded / maximum 400. Preview preserves unread status."
                    )
                };
            }
            Ok(None) => {
                self.session = None;
                if interactive {
                    self.sign_in(run_rules, cx);
                    return;
                }
                self.status="Connect Gmail. Saved read-only credentials are left untouched; write access needs fresh consent.".into();
            }
            Err(e) => self.error(
                format!("{e:#}. Existing mail and pending marks were kept. Refresh to retry."),
                cx,
            ),
        }
        cx.notify();
    }
    fn refresh_labels(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else { return; };
        self.label_revision += 1;
        let revision = self.label_revision;
        let ids: Vec<_> = self.triage.emails.iter().flat_map(|e| e.label_ids.iter().cloned()).collect::<BTreeSet<_>>().into_iter().collect();
        let worker_session = session.clone();
        let work = cx.background_executor().spawn(async move {
            worker_session.lock().map(|mut s| s.label_catalog()).unwrap_or_else(|_| labels::Catalog { unavailable: true, ..Default::default() })
        });
        cx.spawn(async move |this, cx| {
            let catalog = work.await;
            let fetch_colors = !catalog.unavailable;
            let current = this.update(cx, |this, cx| {
                if this.label_revision != revision || !this.session.as_ref().is_some_and(|s| Arc::ptr_eq(s, &session)) { return false; }
                this.labels = catalog; cx.notify(); true
            }).unwrap_or(false);
            if !current || !fetch_colors { return; }
            let worker_session = session.clone();
            let colors = cx.background_executor().spawn(async move {
                worker_session.lock().map(|mut s| s.label_colors(&ids)).ok()
            }).await;
            let _ = this.update(cx, |this, cx| {
                if this.label_revision == revision && this.session.as_ref().is_some_and(|s| Arc::ptr_eq(s, &session)) {
                    if let Some(colors) = colors { this.labels = colors; } else { this.labels.unavailable = true; }
                    cx.notify();
                }
            });
        }).detach();
    }
    fn row_labels(&self, row: &Row) -> Vec<labels::Badge> {
        labels::summarize(row.ids.iter().filter_map(|id| self.triage.emails.iter().find(|e| &e.id == id)).map(|e| e.label_ids.as_slice()), &self.labels, true)
    }
    fn can_stage(&self) -> bool { !self.busy && !self.blocked && !self.triage.has_unknown() && self.editing.is_none() }
    fn ensure_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Reader> {
        self.reader.get_or_insert_with(|| cx.new(|cx| Reader::new(window, cx))).clone()
    }
    fn open_message(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(email) = self.triage.emails.iter().find(|e| e.id == id).cloned() else {
            return;
        };
        if !self.demo && self.session.is_none() {
            self.error("Connect Gmail before opening a message.", cx);
            return;
        }
        let reader = self.ensure_reader(window, cx);
        self.active_message = Some(id.into());
        self.reader_visible = true;
        self.save_pane_layout(cx);
        self.peek = None;
        self.hovered = None;
        reader.update(cx, |view, cx| {
            if view.needs_load(id) {
                if self.demo {
                    view.demo_message(email, cx);
                } else if let Some(session) = self.session.clone() {
                    view.load(email, session, cx);
                }
            }
        });
        cx.notify();
    }
    fn set_read(&mut self, ids: Vec<String>, read: bool, archive: bool, cx: &mut Context<Self>) {
        if !self.can_stage() { return; }
        let targets = self.triage.undecided_ids(&ids);
        let archive_ids = if archive { targets.clone() } else { Vec::new() };
        let ids: Vec<_> = targets.into_iter().filter(|id| {
            self.triage.emails.iter().any(|e| &e.id == id && e.label_ids.iter().any(|label| label == "UNREAD") == read)
        }).collect();
        if ids.is_empty() {
            if archive { self.stage(archive_ids, Action::Archive, cx); }
            return;
        }
        self.row_menu = None;
        let state = if read { "read" } else { "unread" };
        if self.demo {
            self.triage.set_read(&ids, read);
            self.failed = false;
            self.status = format!("Marked {state} locally. Gmail was not contacted.");
            if archive { self.stage(archive_ids, Action::Archive, cx); }
            cx.notify();
            return;
        }
        let Some(session) = self.session.clone() else {
            self.error(format!("Connect Gmail before marking messages {state}."), cx);
            return;
        };
        self.busy = true;
        self.failed = false;
        self.status = format!("Marking {} messages {state}...", ids.len());
        cx.notify();
        let job = cx.background_executor().spawn(async move {
            ids.into_iter().map(|id| {
                let result = session.lock().map_err(|_| anyhow::anyhow!("Gmail worker failed"))
                    .and_then(|mut session| session.set_read(&id, read));
                (id, result)
            }).collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let results = job.await;
            let _ = this.update(cx, |this, cx| this.finish_read(results, read, archive_ids, cx));
        }).detach();
    }
    fn finish_read(&mut self, results: Vec<(String, Result<Option<Outcome>>)>, read: bool, archive_ids: Vec<String>, cx: &mut Context<Self>) {
        self.busy = false;
        let mut confirmed = Vec::new();
        let mut failed = 0;
        let mut unknown = 0;
        for (id, result) in results {
            match result {
                Ok(Some(Outcome::Confirmed)) => confirmed.push(id),
                Ok(Some(Outcome::Unknown(_))) => unknown += 1,
                _ => failed += 1,
            }
        }
        self.triage.set_read(&confirmed, read);
        self.failed = failed > 0 || unknown > 0;
        let state = if read { "read" } else { "unread" };
        self.status = format!("{} marked {state} / {failed} failed / {unknown} unknown. {}", confirmed.len(),
            if unknown > 0 { "Refresh to check Gmail read status before retrying." }
            else if failed > 0 { "Unconfirmed messages were left unchanged. Check Gmail connection and retry." }
            else { "Messages remain in the Inbox." });
        if !archive_ids.is_empty() {
            // Keep the batch in the Inbox until every read change is confirmed.
            if self.failed { self.status.push_str(" Nothing archived."); }
            else { self.stage(archive_ids, Action::Archive, cx); }
        }
        cx.notify();
    }
    fn submit(&mut self, recheck: bool, only: Option<Vec<String>>, cx: &mut Context<Self>) {
        if self.busy
            || self.blocked
            || self.editing.is_some()
            || (!recheck && self.triage.has_unknown())
        {
            return;
        }
        let work: Vec<_> = self
            .triage
            .work()
            .into_iter()
            .filter(|w| only.as_ref().is_none_or(|ids| ids.contains(&w.id)))
            .collect();
        if work.is_empty() {
            return;
        }
        if !recheck && work.iter().any(|w| matches!(w.action, Action::Space(_)) && !spaces::available(&w.label_ids, &self.labels)) {
            self.error("No changes sent. A space label is unavailable. Refresh or put back those messages before Apply.", cx);
            return;
        }
        if self.demo {
            self.triage
                .complete(work.into_iter().map(|w| (w, Outcome::Confirmed)).collect());
            self.status = "Demo changes applied locally. Gmail was not contacted.".into();
            cx.notify();
            return;
        }
        let Some(session) = self.session.clone() else {
            self.error("Connect Gmail before submitting or rechecking.", cx);
            return;
        };
        // Save submitted IDs before network work. Pending, unsubmitted marks are never persisted.
        if let Err(e) = settings::save_journal(&self.journal_path, &work) {
            self.error(
                format!("No action sent. Could not save submission journal: {e:#}"),
                cx,
            );
            return;
        }
        self.busy = true;
        self.failed = false;
        self.status = if recheck {
            "Checking each submitted message in Gmail. No writes sent.".into()
        } else {
            format!(
                "Applying {} decisions. Do not close the app until results arrive.",
                work.len()
            )
        };
        cx.notify();
        let job = cx.background_executor().spawn(async move {
            match session.lock() {
                Ok(mut session) => session.submit(&work, recheck),
                Err(_) => work
                    .into_iter()
                    .map(|w| {
                        (
                            w,
                            Outcome::Unknown("Gmail worker failed. Recheck Gmail.".into()),
                        )
                    })
                    .collect(),
            }
        });
        cx.spawn(async move |this,cx| {
            let results=job.await;
            let _=this.update(cx,|this,cx| {
                this.busy=false;
                let confirmed=results.iter().filter(|(_,o)|*o==Outcome::Confirmed).count();
                let failed=results.iter().filter(|(_,o)|matches!(o,Outcome::Failed(_))).count();
                let unknown:Vec<_>=results.iter().filter(|(_,o)|matches!(o,Outcome::Unknown(_))).map(|(w,_)|w.clone()).collect();
                this.triage.complete(results);
                if let Err(e)=settings::save_journal(&this.journal_path,&unknown) {
                    this.blocked=true;this.error(format!("Results received, but recovery journal could not be saved: {e:#}. Restart and recheck before new submissions."),cx);return;
                }
                this.failed=failed>0 || !unknown.is_empty();
                this.status=format!("{confirmed} confirmed / {failed} failed / {} unknown. {}",unknown.len(),if unknown.is_empty() {"Failed items stay staged. Apply retries them; Put back cancels their pending decision."} else {"Recheck Gmail before retrying. Unknown items cannot be put back yet."});cx.notify();
            });
        }).detach();
    }
    fn stage(&mut self, ids: Vec<String>, action: Action, cx: &mut Context<Self>) {
        if self.busy || self.blocked { return; }
        if self.triage.has_unknown() {
            self.error("Resolve unknown submissions with Recheck Gmail first.", cx);
            return;
        }
        let ids = self.triage.undecided_ids(&ids);
        if ids.is_empty() { return; }
        self.row_menu = None;
        if let Action::Space(id) = action {
            if !self.can_stage_action(action) { self.error("Space is being edited or its labels are unavailable.", cx); return; }
            let space = self.settings.spaces.iter().find(|space| space.id == id).unwrap();
            self.triage.stage_space(&ids, space);
        } else {
            self.triage.stage(&ids, action);
        }
        if self.settings.auto_apply && !matches!(action, Action::Space(_)) {
            self.submit(false, Some(ids), cx);
        } else {
            self.failed = false;
            self.status = "Decisions staged locally. Apply submits them to Gmail.".into();
        }
        cx.notify();
    }
    fn import(&mut self, cx: &mut Context<Self>) {
        let current = self.settings.clone();
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import Mado Mail rules or settings".into()),
        });
        cx.spawn(async move |this,cx| {
            let result=async {let paths=picker.await??;let Some(path)=paths.and_then(|p|p.into_iter().next()) else {return anyhow::Ok(None)};let bytes=settings::read_bounded(&path)?;Ok(Some(Settings::decode_import(&bytes, &current)?))}.await;
            let _=this.update(cx,|this,cx|match result {
                Ok(Some(settings))=>{
                    if this.busy || this.space_draft.is_some() || this.triage.marks.values().any(|mark| matches!(mark.action, Action::Space(_))) {this.error("Finish space edits and pending space messages before importing.",cx);return;}
                    // Back up preferences before replacing them; never export credentials.
                    if !this.demo {
                        let backup = settings::backup(&this.settings_path).and_then(|_| settings::backup(&settings::rules_path(&this.settings_path)));
                        if let Err(e)=backup {this.error(format!("Import cancelled; backup failed: {e:#}"),cx);return;}
                    }
                    let auto=settings.auto_apply;
                    if this.save_settings(settings,cx) {this.status=format!("Imported rules in original order. Previous settings backed up. Auto-apply is {}.",if auto {"ON; row actions write immediately"} else {"off"});this.failed=false;}
                }
                Ok(None)=>{},Err(e)=>this.error(format!("Import rejected: {e:#}"),cx),
            });
        }).detach();
    }
    fn export(&mut self, cx: &mut Context<Self>) {
        let directory = self
            .settings_path
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let picker = cx.prompt_for_new_path(directory, Some("mado-mail-settings.json"));
        let settings = self.settings.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                let Some(path) = picker.await?? else {
                    return anyhow::Ok(false);
                };
                settings.save(&path)?;
                Ok(true)
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(true) => {
                        this.status =
                            "Exported rules and preferences. No credentials or messages included."
                                .into();
                        this.failed = false;
                    }
                    Ok(false) => {}
                    Err(e) => this.error(format!("Export failed: {e:#}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn matching_sender_ids(&self, from: &str) -> Vec<String> {
        let pattern = sender_address(from);
        self.triage.emails.iter().filter(|e| !self.triage.marks.contains_key(&e.id)
            && e.from.to_lowercase().contains(&pattern)).map(|e| e.id.clone()).collect()
    }
    fn save_sender_rule(&mut self, from: String, action: Action, cx: &mut Context<Self>) -> bool {
        if self.busy || self.blocked || self.triage.has_unknown() { return false; }
        let address = sender_address(&from);
        let mut settings = self.settings.clone();
        if let Some(rule) = settings.rules.iter_mut().find(|r| r.from == address) { rule.action = action; }
        else { settings.rules.insert(0, Rule {
            id: format!("rule-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()),
            from: address.clone(), action,
        }); }
        if !self.save_settings(settings, cx) { return false; }
        self.editing = None;
        self.stage(self.matching_sender_ids(&address), action, cx);
        true
    }
    fn open_row_menu(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() || self.mail_drag.is_some() { return; }
        let rows = self.triage.rows(&self.filter.read(cx).content);
        let Some(index) = rows.iter().position(|r| r.key == key) else { return; };
        let Some(anchor) = self.row_bounds.get(key).copied() else { return; };
        self.cursor = index;
        self.row_menu = Some(RowMenu { row: rows[index].clone(), anchor, selected: if self.can_stage() { 0 } else { 3 } });
        self.peek = None; self.hovered = None;
        if let Some(reader) = &self.reader { reader.read(cx).focus_host(); }
        self.menu_focus.focus(window); cx.notify();
    }
    fn close_row_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.row_menu = None;
        let focus = self.table_focus.clone(); window.defer(cx, move |window, _| focus.focus(window)); cx.notify();
    }
    fn menu_action(&mut self, action: usize, window: &mut Window, cx: &mut Context<Self>) {
        if action < 3 && !self.can_stage() { return; }
        let Some(menu) = &self.row_menu else { return; };
        let row = menu.row.clone();
        let valid = self.triage.rows(&self.filter.read(cx).content).iter().any(|r| r.key == row.key && r.ids == row.ids);
        self.close_row_menu(window, cx);
        if !valid { return; }
        match action {
            0 | 1 => self.command(Command::Stage(self.triage.action_ids(&row), if action == 0 { Action::Archive } else { Action::Trash }), window, cx),
            2 => {
                self.editing = Some((None, Action::Archive));
                self.rule_input.update(cx, |i, cx| i.set(&sender_address(&row.sender), cx));
                let focus = self.rule_input.read(cx).focus_handle.clone();
                window.defer(cx, move |window, _| focus.focus(window));
            }
            3 => {
                if row.ids.len() == 1 { self.open_message(&row.ids[0], window, cx); }
                else { self.activate_row(&row, window, cx); }
            }
            4 => self.select(&row.key, cx),
            _ => {}
        }
        cx.notify();
    }
    fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some()
            && !matches!(
                command,
                Command::SaveRule | Command::CancelEdit | Command::EditAction(_)
            )
        {
            return;
        }
        if self.busy
            && !matches!(
                command,
                Command::Open(_)
                    | Command::RowMenu(_)
                    | Command::ClosePeek
                    | Command::Peek(_, _)
                    | Command::Rules
                    | Command::Expand(_)
                    | Command::Clear
                    | Command::ToggleSidebar
                    | Command::ToggleReader
                    | Command::CloseLabelPicker
                    | Command::CancelLabelRemoval
            )
        {
            return;
        }
        match command {
            Command::TopLabelPicker(mode) => self.open_label_picker(mode, None, window, cx),
            Command::RowLabelPicker(key) => {
                let row = self.triage.rows(&self.filter.read(cx).content).into_iter().find(|row| row.key == key);
                if let Some(row) = row { self.open_label_picker(LabelMode::Assign, Some(row), window, cx); }
            }
            Command::PickTopLabel(id) => self.pick_top_label(id, cx),
            Command::RemoveRowLabel(key, label) => self.request_label_removal(&key, label, window, cx),
            Command::ConfirmLabelRemoval => self.confirm_label_removal(window, cx),
            Command::CancelLabelRemoval => { self.label_removal = None; self.table_focus.focus(window); }
            Command::RemoveLabelFilter(id) => {
                if self.triage.label_filter.contains(&id) { self.change_label_filter(id, cx); }
            }
            Command::AddLabelFilter(id) => {
                if !self.triage.label_filter.contains(&id) { self.change_label_filter(id, cx); }
                self.label_picker = None;
                self.label_row_targets = None;
                self.table_focus.focus(window);
            }
            Command::CloseLabelPicker => { self.label_picker = None; self.label_row_targets = None; self.table_focus.focus(window); }
            Command::NewSpace => self.edit_space(None, window, cx),
            Command::EditSpace(id) => self.edit_space(Some(id), window, cx),
            Command::SaveSpace => self.save_space(window, cx),
            Command::CancelSpace => { self.space_draft = None; self.table_focus.focus(window); }
            Command::SpacePicker => {
                if let Some(draft) = &mut self.space_draft {
                    draft.picker = !draft.picker;
                    if draft.picker { self.space_query.read(cx).focus_handle.focus(window); }
                    else { self.space_name.read(cx).focus_handle.focus(window); }
                }
            }
            Command::SpaceLabel(id) => self.toggle_space_label(id, cx),
            Command::Refresh(rules) => self.load(true, rules, cx),
            Command::Rules => {
                if self.resize_drag.take().is_some() { self.save_pane_layout(cx); }
                self.rules_view = !self.rules_view;
                self.label_removal = None;
                self.label_picker = None;
                self.label_row_targets = None;
                self.row_menu = None;
                self.editing = None;
                self.peek = None;
                self.hovered = None;
                self.filter.update(cx, |i, cx| i.set("", cx));
            }
            Command::Stage(ids, action) => self.stage(ids, action, cx),
            Command::SetRead(ids, read) => self.set_read(ids, read, false, cx),
            Command::MarkReadArchive(ids) => self.set_read(ids, true, true, cx),
            Command::Expand(sender) => {
                let key = sender_address(&sender);
                if !self.triage.expanded.remove(&key) {
                    self.triage.expanded.insert(key);
                }
            }
            Command::Open(id) => self.open_message(&id, window, cx),
            Command::RowMenu(key) => self.open_row_menu(&key, window, cx),
            Command::Clear => self.triage.selection.clear(),
            Command::PutBack(ids) => {
                self.triage.unmark(&ids);
                self.peek = None;
                self.hovered = None;
            }
            Command::Empty => {
                let ids = self.triage.marks.keys().cloned().collect::<Vec<_>>();
                self.triage.unmark(&ids);
                self.peek = None;
                self.hovered = None;
            }
            Command::Apply => self.submit(false, None, cx),
            Command::Recheck => self.submit(true, None, cx),
            Command::AutoApply => {
                if self.settings.auto_apply {
                    let mut s = self.settings.clone();
                    s.auto_apply = false;
                    self.save_settings(s, cx);
                } else {
                    let answer=window.prompt(PromptLevel::Warning,"Enable auto-apply?",Some("Archive, Trash and confirmed sender rules will write to Gmail immediately. Space drops and existing staged items still require Apply."),&["Cancel","Enable"],cx);
                    cx.spawn(async move |this, cx| {
                        if answer.await == Ok(1) {
                            let _ = this.update(cx, |this, cx| {
                                if !this.busy {
                                    let mut s = this.settings.clone();
                                    s.auto_apply = true;
                                    this.save_settings(s, cx);
                                }
                            });
                        }
                    })
                    .detach();
                }
            }
            Command::ToggleReader => {
                if self.rules_view { return; }
                self.reader_visible = !self.reader_visible;
                if self.reader_visible { self.ensure_reader(window, cx); }
                else if let Some(reader) = &self.reader { reader.update(cx, |r, cx| r.set_visible(false, cx)); }
                self.resize_drag = None;
                self.save_pane_layout(cx);
                let focus = self.table_focus.clone();
                window.defer(cx, move |window, _| focus.focus(window));
            }
            Command::ToggleReaderPosition => {
                if self.rules_view { return; }
                self.resize_drag = None;
                self.row_menu = None;
                self.peek = None;
                self.hovered = None;
                let mut next = self.settings.clone();
                next.reader_position = if self.reader_below() { ReaderPosition::Beside } else { ReaderPosition::Below };
                self.save_settings(next, cx);
            }
            Command::ToggleSidebar => {
                if self.rules_view {
                    return;
                }
                self.sidebar_visible = !self.sidebar_visible;
                self.resize_drag = None;
                self.save_pane_layout(cx);
                self.peek = None;
                self.hovered = None;
                self.table_focus.focus(window);
            }
            Command::Theme => {
                let mut s = self.settings.clone();
                let names = s.theme_names();
                let i = names.iter().position(|t| *t == s.theme).unwrap_or(0);
                s.theme = names[(i + 1) % names.len()].into();
                self.save_settings(s, cx);
            }
            Command::EditRule(i) => {
                if let Some(r) = self.settings.rules.get(i) {
                    let from = r.from.clone();
                    self.editing = Some((Some(i), r.action));
                    self.rule_input.update(cx, |i, cx| i.set(&from, cx));
                    self.rule_input.read(cx).focus_handle.focus(window);
                }
            }
            Command::DropRule(i) => {
                let mut s = self.settings.clone();
                if i < s.rules.len() {
                    s.rules.remove(i);
                    self.save_settings(s, cx);
                }
            }
            Command::SaveRule => {
                if let Some((index, action)) = self.editing {
                    let from = self.rule_input.read(cx).content.trim().to_owned();
                    let saved = if let Some(i) = index {
                        let mut s = self.settings.clone();
                        s.rules[i].from = from; s.rules[i].action = action;
                        self.save_settings(s, cx)
                    } else { self.save_sender_rule(from, action, cx) };
                    if saved { self.editing = None; self.table_focus.focus(window); }
                }
            }
            Command::CancelEdit => {
                self.editing = None;
                self.table_focus.focus(window);
            }
            Command::EditAction(action) => {
                if let Some((_, a)) = &mut self.editing {
                    *a = action;
                }
            }
            Command::Import => self.import(cx),
            Command::Export => self.export(cx),
            Command::Peek(sender, action) => {
                let key = (sender, action);
                self.peek = if self.peek.as_ref() == Some(&key) {
                    None
                } else {
                    Some(key)
                };
            }
            Command::ClosePeek => {
                self.peek = None;
                self.hovered = None;
            }
        }
        cx.notify();
    }
    fn button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        command: Command,
        enabled: bool,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let p = Palette::current(cx);
        let label: SharedString = label.into();
        let chevron = match &command {
            Command::Expand(sender) => {
                Some(if self.triage.expanded.contains(&sender_address(sender)) {
                    "icons/chevron-down.svg"
                } else {
                    "icons/chevron-right.svg"
                })
            }
            Command::SpacePicker | Command::EditSpace(_) => Some("icons/label.svg"),
            Command::ToggleSidebar => Some("icons/sidebar-right.svg"),
            Command::ToggleReader => Some("icons/reader.svg"),
            Command::ToggleReaderPosition => Some(if self.reader_below() { "icons/reader-beside.svg" } else { "icons/reader-below.svg" }),
            _ => None,
        };
        let dock_toggle = matches!(command, Command::ToggleSidebar | Command::ToggleReader | Command::ToggleReaderPosition);
        let dock_active = match command {
            Command::ToggleSidebar => self.sidebar_visible,
            Command::ToggleReader => self.reader_visible,
            _ => false,
        };
        let dock_hint = match (&command, dock_active) {
            (Command::ToggleReader, true) => "Hide reader",
            (Command::ToggleReader, false) => "Show reader",
            (Command::ToggleReaderPosition, _) => if self.reader_below() { "Place reader beside the list" } else { "Place reader below the list" },
            (_, true) => "Hide staging pane",
            (_, false) => "Show staging pane",
        };
        let toggle = matches!(command, Command::AutoApply);
        let key_command = command.clone();
        let enabled = enabled
            && (self.editing.is_none()
                || matches!(
                    command,
                    Command::SaveRule | Command::CancelEdit | Command::EditAction(_)
                ));
        let primary = matches!(command, Command::Apply | Command::SaveRule | Command::SaveSpace);
        let row_action = label.is_empty() && matches!(command, Command::SetRead(..) | Command::Stage(_, Action::Archive));
        let destructive = matches!(
            command,
            Command::Stage(_, Action::Trash)
        );
        div()
            .id(id.into())
            .tab_index(0)
            .tab_stop(enabled)
            .px_2()
            .py_1()
            .rounded_sm()
            .border_1()
            .border_color(p.line)
            .bg(p.panel2)
            .when(chevron.is_some(), |d| {
                d.p_0()
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::transparent_black())
                    .border_color(gpui::transparent_black())
            })
            .when(matches!(command, Command::SpacePicker | Command::EditSpace(_)), |d| d.tooltip(|_, cx| cx.new(|_| Hint("Select space labels / fuzzy find")).into()))
            .when(dock_toggle, |d| {
                d.when(dock_active, |d| d.bg(p.accent.opacity(0.12)))
                    .tooltip(move |_, cx| cx.new(|_| Hint(dock_hint)).into())
            })
            .when(toggle, |d| {
                d.flex().items_center().gap_2().child(icons::checkbox(
                    self.settings.auto_apply,
                    false,
                    p,
                ))
            })
            .when(primary, |d| d.bg(p.accent).text_color(p.on_accent))
            .when(destructive, |d| d.text_color(p.danger))
            .cursor_pointer()
            .focus(|s| {
                let s = s.border_color(p.accent);
                if row_action { s.bg(p.accent.opacity(0.14)) } else { s }
            })
            .hover(|s| {
                let s = s.border_color(p.accent);
                if row_action { s.bg(p.accent.opacity(0.14)) } else { s }
            })
            .when(!enabled, |s| s.opacity(0.4))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                if enabled {
                    this.command(command.clone(), window, cx);
                }
            }))
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        cx.stop_propagation();
                        if enabled {
                            this.command(key_command.clone(), window, cx);
                        }
                    }
                }),
            )
            .on_key_up(|event, window, _| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") { window.prevent_default(); }
            })
            .when(chevron.is_none() && !label.is_empty(), |d| d.child(label))
            .when_some(chevron, |d, path| {
                d.child(icons::icon(
                    path,
                    if dock_toggle && dock_active {
                        p.accent
                    } else {
                        p.ink_dim
                    },
                ))
            })
    }
    fn row(&mut self, row: Row, index: usize, width: f32, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let email = self.triage.emails.iter().find(|e| e.id == row.ids[0]).unwrap();
        let subject = if row.header { "Expanded / click a message to read".into() }
            else if row.ids.len() > 1 { format!("{} messages / {}", row.ids.len(), email.subject) }
            else { email.subject.clone() };
        let date = email.date.clone();
        let sender = format!("{}{}", sender_name(&row.sender), if row.ids.len() > 1 { format!("  {}", row.ids.len()) } else { String::new() });
        let selected = self.triage.selected(&row);
        let partial = !selected && row.ids.iter().any(|id| self.triage.selection.contains(id));
        let active = row.ids.len() == 1 && self.active_message.as_ref() == row.ids.first();
        let unread = row.unread;
        let unread_hint = if row.ids.len() > 1 { "Contains unread messages" } else { "Unread" };
        let focus = self.row_focus.entry(row.key.clone()).or_insert_with(|| cx.focus_handle()).clone();
        let label_focus = self.label_focus.entry(row.key.clone()).or_insert_with(|| cx.focus_handle()).clone();
        let show_actions = self.hovered_row.as_ref() == Some(&row.key) || focus.contains_focused(window, cx)
            || (self.cursor == index && self.table_focus.is_focused(window))
            || self.row_menu.as_ref().is_some_and(|m| m.row.key == row.key)
            || self.label_row_targets.as_ref().is_some_and(|(key, _)| key == &row.key);
        let can_stage = self.can_stage();
        let combined_hover = self.hovered_read_archive.as_ref() == Some(&row.key);
        let combined_hover_key = row.key.clone();
        let bg = row_background(&row, self.hovered_row.as_deref(), active, p);
        let badges = self.row_labels(&row);
        let budget = ((width - 90.) * 0.4).clamp(34., 220.);
        let subject_line = div().flex_1().min_w_0().w_full().flex().items_center().gap_2()
            .when(!badges.is_empty(), |d| d.child(
                div().id(SharedString::from(format!("labels-{}", row.key))).tab_index(0).track_focus(&label_focus.clone().tab_index(0).tab_stop(self.editing.is_none()))
                    .flex_shrink_0().border_1().border_color(gpui::transparent_black()).focus(|s| s.border_color(p.accent))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_key_down(|event, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") { cx.stop_propagation(); window.prevent_default(); }
                    })
                    .child(self.row_label_strip(&row.key, &badges, budget, cx))))
            .child(div().flex_1().min_w_0().truncate().when(unread, |d| d.font_weight(gpui::FontWeight::BOLD)).child(subject));
        let action_ids = self.triage.action_ids(&row);
        let action_scope = if selected { format!("{} selected messages", action_ids.len()) } else { format!("{} messages in this row", action_ids.len()) };
        let read_hint = format!("Mark {action_scope} as {}", if unread { "read" } else { "unread" });
        let read_archive_hint = format!("Mark {action_scope} as read, then {} after confirmation", if self.settings.auto_apply { "archive now" } else { "stage Archive" });
        let archive_hint = format!("Archive {action_scope}");
        let trash_hint = format!("Trash {action_scope}");
        let label_hint = format!("Add label to {action_scope}");
        let ids = action_ids.clone();
        let senders = ids.iter().filter_map(|id| self.triage.emails.iter().find(|e| &e.id == id))
            .map(|e| sender_address(&e.from)).collect::<BTreeSet<_>>().len();
        let drag_view = cx.weak_entity();
        let content = div().id(SharedString::from(format!("content-{}", row.key))).flex_1().min_w_0().flex().flex_col().gap_2()
            .child(div().w(px((width - 90.).max(0.))).min_w_0().flex_shrink_0().flex().items_center().gap_2()
                .when(show_actions, |d| d.pr(px(224.)))
                .child(div().id(SharedString::from(format!("unread-{}", row.key))).size(px(6.)).flex_shrink_0().rounded_full()
                    .when(unread, |d| d.bg(p.accent).tooltip(move |_, cx| cx.new(|_| Hint(unread_hint)).into())))
                .child(div().flex_1().min_w_0().truncate()
                    .text_color(if active { p.accent } else if unread { p.ink } else { p.ink_dim })
                    .when(unread, |d| d.font_weight(gpui::FontWeight::BOLD)).child(sender))
                .when(!show_actions, |d| d.child(div().w(px(105.)).flex_shrink_0().truncate().text_xs().text_color(p.ink_dim).child(date))))
            .child(subject_line)
            .when(can_stage, |d| d.on_drag(MailDrag { ids, senders }, move |payload, offset, window, cx| {
                let _ = drag_view.update(cx, |this, cx| {
                    this.mail_drag = Some(payload.clone()); this.suppress_row_click = true; this.row_menu = None;
                    this.peek = None; this.hovered = None; this.table_focus.focus(window);
                    if let Some(reader) = &this.reader { reader.update(cx, |r, cx| r.set_visible(false, cx)); }
                    cx.notify();
                });
                cx.new(|_| DragBadge { payload: payload.clone(), offset })
            }));
        let key = row.key.clone(); let keyboard_key = key.clone(); let hover_key = key.clone(); let menu_key = key.clone();
        let middle_key = row.key.clone();
        let click_row = row.clone(); let bounds_key = key.clone(); let bounds_view = cx.weak_entity();
        div().id(SharedString::from(row.key.clone())).track_focus(&focus.clone().tab_stop(false)).relative()
            .w_full().h(px(68.)).flex().items_center().gap_2().px_2()
            .border_b_1().border_l_2().border_color(if active { p.accent } else { p.line }).bg(bg)
            .when(index == self.cursor, |d| d.border_t_1().border_color(p.accent.opacity(0.5)))
            .child(canvas(move |bounds, window, cx| {
                let changed = bounds_view.update(cx, |this, _| {
                    this.row_bounds.insert(bounds_key.clone(), bounds);
                    if let Some(menu) = &mut this.row_menu && menu.row.key == bounds_key && menu.anchor != bounds {
                        menu.anchor = bounds; true
                    } else { false }
                }).unwrap_or(false);
                if changed { window.request_animation_frame(); }
            }, |_, _, _, _| {}).absolute().inset_0())
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered { this.hovered_row = Some(hover_key.clone()); }
                else if this.hovered_row.as_ref() == Some(&hover_key) {
                    this.hovered_row = None;
                    this.hovered_read_archive = None;
                }
                cx.notify();
            }))
            .capture_any_mouse_down(cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                if event.button != MouseButton::Middle { return; }
                cx.stop_propagation();
                if this.editing.is_some() || this.mail_drag.is_some() { return; }
                let rows = this.triage.rows(&this.filter.read(cx).content);
                this.triage.select_all_or_clear(&rows, &middle_key);
                this.cursor = index;
                this.table_focus.focus(window);
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, _, window, cx| {
                cx.stop_propagation(); this.open_row_menu(&menu_key, window, cx);
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation(); if this.suppress_row_click { return; }
                this.table_focus.focus(window); this.cursor = index; this.activate_row(&click_row, window, cx);
            }))
            .child(div().id(SharedString::from(format!("check-{}", row.key))).tab_index(0).tab_stop(self.editing.is_none())
                .w(px(28.)).h_full().flex_shrink_0().flex().items_center().justify_center().border_1()
                .border_color(gpui::transparent_black()).bg(if selected || partial { gpui::rgb(0xe5f3ec).into() } else { gpui::transparent_black() })
                .focus(|s| s.border_color(p.accent)).cursor_pointer()
                .tooltip(|_, cx| cx.new(|_| Hint("Mark for bulk actions. Does not open the message.")).into())
                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.cursor = index; this.select(&key, cx); }))
                .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "space" | "enter") { cx.stop_propagation(); this.cursor = index; this.select(&keyboard_key, cx); }
                }))
                .on_key_up(|event, window, _| { if matches!(event.keystroke.key.as_str(), "space" | "enter") { window.prevent_default(); } })
                .child(icons::checkbox(selected, partial, Palette { accent: gpui::rgb(0x27634d).into(), on_accent: gpui::rgb(0xffffff).into(), ..p })))
            .child(div().w(px(28.)).flex_shrink_0().when(row.ids.len() > 1, |d| d.child(self.button(
                format!("expand-{}", row.key), "", Command::Expand(row.sender.clone()), true, cx))))
            .child(content)
            .when(show_actions && self.mail_drag.is_none(), |d| d.child(div().absolute().right(px(8.)).top(px(4.))
                .flex().items_center().gap_1().bg(bg)
                .child(self.button(format!("label-{}", row.key), "", Command::RowLabelPicker(row.key.clone()), can_stage && (self.demo || self.session.is_some()), cx)
                    .bg(p.panel2.opacity(0.35))
                    .p_0().size(px(28.)).flex().items_center().justify_center().relative()
                    .child(self.label_trigger_anchor(LabelMode::Assign, Some(row.key.clone()), cx))
                    .tooltip(move |_, cx| cx.new(|_| LabelHint(label_hint.clone())).into())
                    .child(icons::icon("icons/label.svg", p.ink_dim)))
                .child(div().h(px(28.)).flex().items_center().rounded_sm().overflow_hidden()
                    .border_1().border_color(if combined_hover { p.accent } else { p.line })
                    .bg(if combined_hover { p.accent.opacity(0.14) } else { p.panel2.opacity(0.35) })
                    .child(self.button(format!("read-{}", row.key), "", Command::SetRead(action_ids.clone(), unread), can_stage && (self.demo || self.session.is_some()), cx)
                        .bg(gpui::transparent_black()).border_0().rounded_none()
                        .p_0().w(px(30.)).h_full().flex().items_center().justify_center()
                        .tooltip(move |_, cx| cx.new(|_| LabelHint(read_hint.clone())).into())
                        .child(icons::icon(if unread { "icons/mark-read.svg" } else { "icons/mark-unread.svg" }, p.ink_dim)))
                    .child(self.button(format!("read-archive-{}", row.key), "+", Command::MarkReadArchive(action_ids.clone()), can_stage && (self.demo || self.session.is_some()), cx)
                        .bg(gpui::transparent_black()).text_color(p.ink_dim)
                        .border_0().border_l_1().border_r_1().rounded_none()
                        .p_0().w(px(26.)).h_full().flex().items_center().justify_center()
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered { this.hovered_read_archive = Some(combined_hover_key.clone()); }
                            else if this.hovered_read_archive.as_ref() == Some(&combined_hover_key) { this.hovered_read_archive = None; }
                            cx.notify();
                        }))
                        .tooltip(move |_, cx| cx.new(|_| LabelHint(read_archive_hint.clone())).into()))
                    .child(self.button(format!("archive-{}", row.key), "", Command::Stage(action_ids.clone(), Action::Archive), can_stage, cx)
                        .bg(gpui::transparent_black()).border_0().rounded_none()
                        .p_0().w(px(30.)).h_full().flex().items_center().justify_center()
                        .tooltip(move |_, cx| cx.new(|_| LabelHint(archive_hint.clone())).into())
                        .child(icons::icon("icons/archive.svg", p.ink_dim))))
                .child(self.button(format!("trash-{}", row.key), "", Command::Stage(action_ids, Action::Trash), can_stage, cx)
                    .bg(p.panel2.opacity(0.35))
                    .p_0().size(px(28.)).flex().items_center().justify_center()
                    .tooltip(move |_, cx| cx.new(|_| LabelHint(trash_hint.clone())).into())
                    .child(icons::icon("icons/trash.svg", p.danger)))
                .child(self.button(format!("menu-{}", row.key), "...", Command::RowMenu(row.key), true, cx)
                    .bg(p.panel2.opacity(0.35)))))
    }
    fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        let rows = self.triage.rows(&self.filter.read(cx).content);
        self.triage.select(&rows, key);
        cx.notify();
    }
    fn activate_row(&mut self, row: &Row, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() { return; }
        if row.ids.len() == 1 {
            if self.reader_visible { self.open_message(&row.ids[0], window, cx); }
        } else {
            self.command(Command::Expand(row.sender.clone()), window, cx);
        }
    }
    fn table_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.triage.rows(&self.filter.read(cx).content);
        if rows.is_empty() {
            return;
        }
        self.cursor = self.cursor.min(rows.len() - 1);
        let row = &rows[self.cursor];
        if event.keystroke.key == "f10" && event.keystroke.modifiers.shift {
            self.open_row_menu(&row.key, window, cx); cx.stop_propagation(); return;
        }
        match event.keystroke.key.as_str() {
            "up" => self.cursor = self.cursor.saturating_sub(1),
            "down" => self.cursor = (self.cursor + 1).min(rows.len() - 1),
            "home" => self.cursor = 0,
            "end" => self.cursor = rows.len() - 1,
            "enter" => self.activate_row(row, window, cx),
            "right" if row.ids.len() > 1 && !row.header => {
                self.command(Command::Expand(row.sender.clone()), window, cx)
            }
            "left" if row.header || row.child => {
                self.command(Command::Expand(row.sender.clone()), window, cx)
            }
            _ => return,
        }
        self.table_focus.focus(window);
        cx.stop_propagation();
        self.scroll
            .scroll_to_item(self.cursor, ScrollStrategy::Center);
        cx.notify();
    }
    fn inbox_scrollbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let scroll = self.scroll.clone();
        let geometry = self.inbox_scrollbar.clone();
        let dragging = self.inbox_scroll_drag.is_some();
        let p = Palette::current(cx);
        div().id("inbox-scrollbar").absolute().right_0().top_0().bottom_0().w(px(SCROLLBAR_WIDTH))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                if this.editing.is_some() { return; }
                let Some(bar) = this.inbox_scrollbar.get() else { return; };
                cx.stop_propagation();
                this.table_focus.focus(window);
                this.row_menu = None;
                this.hovered_row = None;
                let y = f32::from(event.position.y - bar.track.top());
                if y >= bar.top && y <= bar.top + bar.height {
                    this.inbox_scroll_drag = Some(y - bar.top);
                } else {
                    let current = -f32::from(this.scroll.0.borrow().base_handle.offset().y);
                    let page = f32::from(bar.track.size.height);
                    this.scroll_inbox_to(current + if y < bar.top { -page } else { page });
                }
                cx.notify();
            }))
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, _, cx| {
                if this.editing.is_some() { return; }
                let current = -f32::from(this.scroll.0.borrow().base_handle.offset().y);
                this.scroll_inbox_to(current - f32::from(event.delta.pixel_delta(px(20.)).y));
                cx.stop_propagation(); cx.notify();
            }))
            .child(canvas(move |bounds, _, _| {
                let scroll = scroll.0.borrow();
                let viewport = f32::from(bounds.size.height);
                let content = scroll.last_item_size.map_or(0., |s| f32::from(s.contents.height));
                let bar = scrollbar_thumb(viewport, content, -f32::from(scroll.base_handle.offset().y))
                    .map(|(top, height)| InboxScrollGeometry { track: bounds, top, height, max_offset: content - viewport });
                geometry.set(bar);
                bar
            }, move |_, bar, window, _| {
                if let Some(bar) = bar {
                    window.paint_quad(gpui::fill(bar.track, p.panel2));
                    let thumb = Bounds::new(bar.track.origin + gpui::point(px(2.), px(bar.top)), size(px(8.), px(bar.height)));
                    window.paint_quad(gpui::fill(thumb, p.ink_dim.opacity(if dragging { 0.85 } else { 0.55 })).corner_radii(px(4.)));
                }
            }).size_full())
    }
    fn scroll_inbox_to(&mut self, offset: f32) {
        let Some(bar) = self.inbox_scrollbar.get() else { return; };
        let mut scroll = self.scroll.0.borrow_mut();
        scroll.deferred_scroll_to_item = None;
        scroll.base_handle.set_offset(gpui::point(px(0.), px(-offset.clamp(0., bar.max_offset))));
    }
    fn move_inbox_scrollbar(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(grab) = self.inbox_scroll_drag else { return; };
        if event.pressed_button != Some(MouseButton::Left) {
            self.inbox_scroll_drag = None;
        } else if let Some(bar) = self.inbox_scrollbar.get() {
            let travel = f32::from(bar.track.size.height) - bar.height;
            if travel > 0. {
                let top = f32::from(event.position.y - bar.track.top()) - grab;
                self.scroll_inbox_to(top / travel * bar.max_offset);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn reader_below(&self) -> bool { self.settings.reader_position == ReaderPosition::Below }
    fn reader_height(&self) -> f32 {
        reader_height(self.workspace_bounds.size.height.into(), self.reader_height)
    }
    fn widths(&self, window: &Window) -> (f32, f32) {
        pane_widths(window.viewport_size().width.into(),
            (self.reader_visible && !self.reader_below()).then_some(self.reader_width),
            self.sidebar_visible.then_some(self.sidebar_width))
    }
    fn pane_width(&self, window: &Window) -> f32 { self.widths(window).1 }
    fn save_pane_layout(&mut self, cx: &mut Context<Self>) {
        let mut next = self.settings.clone();
        next.pane_widths = settings::PaneWidths {
            reader: self.reader_width.round() as u32,
            staging: self.sidebar_width.round() as u32,
        };
        next.reader_height = self.reader_height.round() as u32;
        next.pane_visibility = settings::PaneVisibility {
            reader: self.reader_visible,
            staging: self.sidebar_visible,
        };
        if next.pane_widths != self.settings.pane_widths
            || next.pane_visibility != self.settings.pane_visibility
            || next.reader_height != self.settings.reader_height
        {
            self.save_settings(next, cx);
        }
    }
    fn bin_height(&self) -> f32 {
        (f32::from(self.bin_bounds.size.height) - DIVIDER).max(1.)
    }
    fn resize_pane(
        &mut self,
        axis: ResizeAxis,
        value: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        match axis {
            ResizeAxis::Width | ResizeAxis::Reader => {
                let (reader, staging) = self.widths(window);
                let dividers = ((self.reader_visible && !self.reader_below()) as u8 + self.sidebar_visible as u8) as f32 * DIVIDER;
                let budget = f32::from(window.viewport_size().width) - 420. - dividers;
                if axis == ResizeAxis::Reader {
                    self.reader_width = value.clamp(320., (budget - staging).max(320.).min(8192.));
                } else {
                    self.sidebar_width = value.clamp(260., (budget - reader).max(260.).min(8192.));
                }
            }
            ResizeAxis::ReaderHeight => {
                self.reader_height = reader_height(self.workspace_bounds.size.height.into(), value).max(240.);
            }
            ResizeAxis::Bins => {
                self.archive_fraction = clamp_archive_fraction(value, self.bin_height())
            }
        }
        self.peek = None;
        self.hovered = None;
        cx.notify();
    }
    fn move_splitter(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inbox_scroll_drag.is_some() {
            self.move_inbox_scrollbar(event, window, cx);
            return;
        }
        let Some(drag) = self.resize_drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.resize_drag = None;
            self.save_pane_layout(cx);
            cx.notify();
        } else {
            let value = match drag.axis {
                ResizeAxis::Width | ResizeAxis::Reader => drag.value + f32::from(drag.start.x - event.position.x),
                ResizeAxis::ReaderHeight => drag.value + f32::from(drag.start.y - event.position.y),
                ResizeAxis::Bins => {
                    drag.value + f32::from(event.position.y - drag.start.y) / self.bin_height()
                }
            };
            self.resize_pane(drag.axis, value, window, cx);
        }
        cx.stop_propagation();
    }
    fn splitter(&self, axis: ResizeAxis, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let horizontal = matches!(axis, ResizeAxis::Width | ResizeAxis::Reader);
        let focus = match axis {
            ResizeAxis::Width => self.width_focus.clone(),
            ResizeAxis::Reader | ResizeAxis::ReaderHeight => self.reader_width_focus.clone(),
            ResizeAxis::Bins => self.split_focus.clone(),
        };
        let hint = match axis {
            ResizeAxis::Width => "Resize staging width. Drag or use Left/Right. Size is remembered.",
            ResizeAxis::Reader => "Resize reader width. Drag or use Left/Right. Size is remembered.",
            ResizeAxis::ReaderHeight => "Resize reader height. Drag or use Up/Down. Size is remembered.",
            ResizeAxis::Bins => "Resize Archive/Trash sections. Drag or use Up/Down.",
        };
        div()
            .id(match axis {
                ResizeAxis::Width => "staging-width-divider",
                ResizeAxis::Reader => "reader-width-divider",
                ResizeAxis::ReaderHeight => "reader-height-divider",
                ResizeAxis::Bins => "staging-bin-divider",
            })
            .tab_index(0)
            .track_focus(&focus.clone().tab_index(0).tab_stop(self.editing.is_none()))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(p.canvas)
            .when(horizontal, |d| {
                d.w(px(DIVIDER))
                    .h_full()
                    .cursor(CursorStyle::ResizeLeftRight)
            })
            .when(!horizontal, |d| {
                d.h(px(DIVIDER)).w_full().cursor(CursorStyle::ResizeUpDown)
            })
            .when(axis == ResizeAxis::ReaderHeight, |d| d.bg(p.line))
            .hover(|s| s.bg(p.accent.opacity(0.15)))
            .focus(|s| s.bg(p.accent.opacity(0.3)))
            .tooltip(move |_, cx| cx.new(|_| Hint(hint)).into())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    if this.editing.is_some() {
                        return;
                    }
                    focus.focus(window);
                    let value = match axis {
                        ResizeAxis::Width => this.pane_width(window),
                        ResizeAxis::Reader => this.widths(window).0,
                        ResizeAxis::ReaderHeight => this.reader_height(),
                        ResizeAxis::Bins => clamp_archive_fraction(this.archive_fraction, this.bin_height()),
                    };
                    this.resize_drag = Some(ResizeDrag {
                        axis,
                        start: event.position,
                        value,
                    });
                    this.peek = None;
                    this.hovered = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    let delta = match (axis, event.keystroke.key.as_str()) {
                        (ResizeAxis::Width | ResizeAxis::Reader, "left") | (ResizeAxis::Bins, "down") | (ResizeAxis::ReaderHeight, "up") => 16.,
                        (ResizeAxis::Width | ResizeAxis::Reader, "right") | (ResizeAxis::Bins, "up") | (ResizeAxis::ReaderHeight, "down") => -16.,
                        _ => return,
                    };
                    let value = match axis {
                        ResizeAxis::Width => this.pane_width(window) + delta,
                        ResizeAxis::Reader => this.widths(window).0 + delta,
                        ResizeAxis::ReaderHeight => this.reader_height() + delta,
                        ResizeAxis::Bins => this.archive_fraction + delta / this.bin_height(),
                    };
                    this.resize_pane(axis, value, window, cx);
                    this.save_pane_layout(cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .rounded_sm()
                    .bg(p.ink_dim.opacity(0.45))
                    .when(horizontal, |d| d.w(px(2.)).h(px(32.)))
                    .when(!horizontal, |d| d.h(px(2.)).w(px(32.))),
            )
    }
    fn bins(&self, width: f32, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let mut actions = vec![Action::Archive, Action::Trash];
        actions.extend(self.settings.spaces.iter().map(|space| Action::Space(space.id)));
        for mark in self.triage.marks.values() {
            if !actions.contains(&mark.action) { actions.push(mark.action); }
        }
        let with_spaces = actions.len() > 2 || self.space_draft.is_some();
        let stacked = with_spaces;
        let mut panel = div()
            .w(px(width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(p.panel)
            .border_l_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap_2()
                    .flex_shrink_0()
                    .p_3()
                    .child(format!("STAGING / {} pending", self.triage.marks.len()))
                    .child(self.button("new-space", "+ New space", Command::NewSpace, !self.busy && !self.blocked && self.space_draft.is_none() && !self.triage.has_unknown(), cx))
                    .child(self.button(
                        "empty",
                        "Empty all",
                        Command::Empty,
                        !self.busy && !self.triage.marks.is_empty(),
                        cx,
                    )),
            );
        let view = cx.weak_entity();
        let fraction = clamp_archive_fraction(self.archive_fraction, self.bin_height());
        let mut bins = div().id("staging-bins").relative().flex_1().min_h_0().flex().flex_col()
            .when(stacked, |d| d.overflow_y_scroll().track_scroll(&self.staging_scroll)).child(
            canvas(
                move |bounds, _, cx| {
                    let _ = view.update(cx, |this, cx| {
                        let changed = this.bin_bounds.size.height != bounds.size.height;
                        this.bin_bounds = bounds;
                        if changed {
                            cx.notify();
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );
        if self.space_draft.is_some() { bins = bins.child(self.space_editor(cx)); }
        for action in actions {
            if let Action::Space(id) = action {
                if self.space_draft.as_ref().is_some_and(|draft| draft.id == Some(id)) { continue; }
            }
            let mut groups: BTreeMap<String, Vec<&Email>> = BTreeMap::new();
            for email in &self.triage.emails {
                if self
                    .triage
                    .marks
                    .get(&email.id)
                    .is_some_and(|m| m.action == action)
                {
                    groups
                        .entry(sender_address(&email.from))
                        .or_default()
                        .push(email);
                }
            }
            let mut groups: Vec<_> = groups.into_iter().collect();
            groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
            let count: usize = groups.iter().map(|(_, g)| g.len()).sum();
            let mut list = div()
                .id(SharedString::from(match action {
                    Action::Archive => "archive-bin".into(),
                    Action::Trash => "trash-bin".into(),
                    Action::Space(id) => format!("space-bin-{id}"),
                }))
                .flex_1()
                .min_h_0()
                .when(stacked, |d| d.flex_initial().flex_none().max_h(px(180.)))
                .overflow_y_scroll();
            for (sender, emails) in groups {
                let ids: Vec<_> = emails.iter().map(|e| e.id.clone()).collect();
                let unknown = ids
                    .iter()
                    .any(|id| matches!(self.triage.marks[id].outcome, Some(Outcome::Unknown(_))));
                let failed = ids
                    .iter()
                    .any(|id| matches!(self.triage.marks[id].outcome, Some(Outcome::Failed(_))));
                let hover_sender = sender.clone();
                let label = format!(
                    "{}  {}{}{}",
                    emails.len(),
                    sender_name(&emails[0].from),
                    if !matches!(action, Action::Space(_)) && rule_for(&self.settings.rules, &emails[0].from).is_some() {
                        " / RULE"
                    } else {
                        ""
                    },
                    if unknown {
                        " / unknown"
                    } else if failed {
                        " / failed"
                    } else {
                        ""
                    }
                );
                list = list.child(
                    div()
                        .id(SharedString::from(format!("bin-{action:?}-{sender}")))
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_1()
                        .on_hover(cx.listener(move |this, hovered, _, cx| {
                            if this.resize_drag.is_some() || this.mail_drag.is_some() || this.row_menu.is_some() {
                                return;
                            }
                            this.hovered = if *hovered {
                                Some((hover_sender.clone(), action))
                            } else {
                                None
                            };
                            cx.notify();
                        }))
                        .child(
                            div()
                                .w(px(35.))
                                .h(px(12.))
                                .flex_shrink_0()
                                .flex()
                                .gap(px(1.))
                                .children((0..emails.len().min(9)).map(|_| {
                                    div().w(px(3.)).h_full().bg(if action != Action::Trash {
                                        p.accent
                                    } else {
                                        p.danger
                                    })
                                })),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(self.button(
                            format!("peek-{action:?}-{sender}"),
                            label,
                            Command::Peek(sender.clone(), action),
                            true,
                            cx,
                        )))
                        .child(self.button(
                            format!("back-{action:?}-{sender}"),
                            "Put back",
                            Command::PutBack(ids),
                            !self.busy && !unknown,
                            cx,
                        )),
                );
            }
            if action == Action::Trash && !stacked {
                bins = bins.child(self.splitter(ResizeAxis::Bins, cx));
            }
            let drop_count = self.mail_drag.as_ref().map_or(0, |drag| self.triage.undecided_ids(&drag.ids).len());
            let target_view = cx.weak_entity();
            bins = bins.child(
                div()
                    .id(SharedString::from(match action {
                        Action::Archive => "archive-drop-target".into(),
                        Action::Trash => "trash-drop-target".into(),
                        Action::Space(id) => format!("space-drop-target-{id}"),
                    }))
                    .border_1().border_color(gpui::transparent_black())
                    .on_drag_move::<MailDrag>(cx.listener(move |this, event: &gpui::DragMoveEvent<MailDrag>, _, cx| {
                        if event.bounds.contains(&event.event.position) {
                            if this.drop_target != Some(action) { this.drop_target = Some(action); cx.notify(); }
                        } else if this.drop_target == Some(action) { this.drop_target = None; cx.notify(); }
                    }))
                    .can_drop(move |value, _, cx| {
                        value.downcast_ref::<MailDrag>().is_some_and(|drag| target_view.read_with(cx, |this, _| {
                            this.can_stage_action(action) && this.sidebar_visible && !this.rules_view && !this.triage.undecided_ids(&drag.ids).is_empty()
                        }).unwrap_or(false))
                    })
                    .on_drop(cx.listener(move |this, drag: &MailDrag, window, cx| {
                        if this.can_stage_action(action) && this.sidebar_visible && !this.rules_view {
                            this.command(Command::Stage(drag.ids.clone(), action), window, cx);
                        }
                        this.mail_drag = None; this.drop_target = None; cx.stop_propagation(); cx.notify();
                    }))
                    .when(self.can_stage_action(action) && drop_count > 0, |d| d.drag_over::<MailDrag>(move |s, _, _, _| {
                        s.border_color(if action != Action::Trash { p.accent } else { p.danger })
                            .bg(if action != Action::Trash { p.accent.opacity(0.1) } else { p.danger.opacity(0.1) })
                    }))
                    .flex_1()
                    .map(|mut d| {
                        d.style().flex_grow = Some(if action == Action::Archive {
                            fraction
                        } else {
                            1. - fraction
                        });
                        d
                    })
                    .min_h_0()
                    .when(stacked, |d| d.flex_initial().flex_none().min_h(px(100.)).mb_2())
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .flex_shrink_0()
                            .border_y_1()
                            .border_color(p.line)
                            .text_color(if action != Action::Trash {
                                p.accent
                            } else {
                                p.danger
                            })
                            .child(format!("TO {} / {count}", self.action_name(action).to_uppercase())),
                    )
                    .when_some(if let Action::Space(id) = action { Some(id) } else { None }, |d, id| d.child(self.space_details(id, cx)))
                    .child(div().px_3().py_2().text_xs().text_color(p.ink_dim).child(
                        if self.mail_drag.is_none() { "Drop messages here".to_owned() }
                        else if !self.can_stage_action(action) || drop_count == 0 { "Drop unavailable".into() }
                        else if self.drop_target != Some(action) { format!("Drop {drop_count} messages here") }
                        else if self.settings.auto_apply && !matches!(action, Action::Space(_)) { format!("Release to {} {drop_count} now", action.label().to_lowercase()) }
                        else { format!("Release to stage {drop_count} for {}", self.action_name(action)) }
                    ))
                    .child(list.when(count == 0 && !stacked, |d| {
                        d.child(div().p_4().text_color(p.ink_dim).child("Empty"))
                    })),
            );
        }
        panel = panel.child(bins);
        let archive = self
            .triage
            .marks
            .values()
            .filter(|m| m.action == Action::Archive)
            .count();
        let trash = self.triage.marks.values().filter(|mark| mark.action == Action::Trash).count();
        let spaces = self.triage.marks.len() - archive - trash;
        panel.child(
            div()
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .border_t_1()
                .border_color(p.line)
                .flex_shrink_0()
                .child(
                    div()
                        .text_xs()
                        .text_color(p.ink_dim)
                        .child(if with_spaces { "Spaces add their labels and remove INBOX on Apply. Other labels and unread stay unchanged." } else { "Trash is not permanent deletion. Unread stays unchanged." }),
                )
                .when(self.triage.has_unknown(), |d| {
                    d.child(self.button(
                        "recheck",
                        "Recheck Gmail",
                        Command::Recheck,
                        !self.busy && !self.blocked,
                        cx,
                    ))
                })
                .child(self.button(
                    "apply",
                    if spaces > 0 { format!("Apply / {} pending", archive + trash + spaces) } else { format!("Apply / {archive} Archive, {trash} Trash") },
                    Command::Apply,
                    !self.busy
                        && !self.blocked
                        && !self.triage.has_unknown()
                        && archive + trash + spaces > 0,
                    cx,
                )),
        )
    }
    fn rules(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let filter = self.filter.read(cx).content.to_lowercase();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_3()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex_1()
                            .child("Sender rules / first matching rule wins"),
                    )
                    .child(self.button(
                        "import",
                        "Import rules/settings",
                        Command::Import,
                        !self.busy && !self.blocked && !self.demo,
                        cx,
                    ))
                    .child(self.button(
                        "export",
                        "Export settings",
                        Command::Export,
                        !self.busy,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .px_3()
                    .py_2()
                    .text_color(p.ink_dim)
                    .child(div().flex_1().child("FROM MATCH"))
                    .child(div().w(px(120.)).child("MARK"))
                    .child(div().w(px(180.)).child("ACTION")),
            )
            .child(
                div()
                    .id("rules-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(
                        self.settings
                            .rules
                            .iter()
                            .enumerate()
                            .filter(|(_, r)| r.from.to_lowercase().contains(&filter))
                            .map(|(i, r)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .px_3()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(p.line)
                                    .child(
                                        div().flex_1().min_w_0().truncate().child(r.from.clone()),
                                    )
                                    .child(
                                        div()
                                            .w(px(120.))
                                            .text_color(if r.action == Action::Archive {
                                                p.accent
                                            } else {
                                                p.danger
                                            })
                                            .child(r.action.label()),
                                    )
                                    .child(
                                        div()
                                            .w(px(180.))
                                            .flex()
                                            .gap_2()
                                            .child(self.button(
                                                format!("edit-{i}"),
                                                "Edit",
                                                Command::EditRule(i),
                                                !self.busy && !self.blocked,
                                                cx,
                                            ))
                                            .child(self.button(
                                                format!("drop-{i}"),
                                                "Drop rule",
                                                Command::DropRule(i),
                                                !self.busy && !self.blocked,
                                                cx,
                                            )),
                                    )
                            }),
                    )
                    .when(self.settings.rules.is_empty(), |d| {
                        d.child(
                            div()
                                .p_8()
                                .text_color(p.ink_dim)
                                .child("No sender rules. Open a message row menu and choose Sender rule..."),
                        )
                    }),
            )
    }
}
impl Render for MailApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.reader_visible && !self.rules_view { self.ensure_reader(window, cx); }
        let below = self.reader_below();
        let (reader_width, pane_width) = self.widths(window);
        let workspace_view = cx.weak_entity();
        let list_width = f32::from(window.viewport_size().width) - reader_width - pane_width
            - ((self.reader_visible && !below) as u8 + self.sidebar_visible as u8) as f32 * DIVIDER;
        let table_view = cx.weak_entity();
        let rows = self.triage.rows(&self.filter.read(cx).content);
        if rows.is_empty() { self.inbox_scrollbar.set(None); }
        let keys: BTreeSet<_> = rows.iter().map(|r| r.key.as_str()).collect();
        self.row_focus.retain(|key, _| keys.contains(key.as_str()));
        self.label_focus.retain(|key, _| keys.contains(key.as_str()));
        self.row_bounds.retain(|key, _| keys.contains(key.as_str()));
        self.label_row_anchors.retain(|key, _| keys.contains(key.as_str()));
        if self.row_menu.as_ref().is_some_and(|m| !rows.iter().any(|r| r.key == m.row.key && r.ids == m.row.ids)) {
            self.row_menu = None; self.table_focus.focus(window);
        }
        if let Some((key, _)) = self.row_focus.iter().find(|(_, f)| f.contains_focused(window, cx)) {
            if let Some(index) = rows.iter().position(|r| &r.key == key) { self.cursor = index; }
        }
        if self.row_menu.is_none() && self.menu_focus.is_focused(window) { self.table_focus.focus(window); }
        let label_popup = if !self.rules_view && self.editing.is_none() && self.row_menu.is_none() && self.mail_drag.is_none() && self.resize_drag.is_none() {
            self.label_focus.iter().find(|(_, f)| f.is_focused(window)).and_then(|(key, _)| {
                let row = rows.iter().find(|r| &r.key == key)?;
                Some((*self.row_bounds.get(key)?, labels::full_text(&self.row_labels(row))))
            })
        } else { None };
        if let Some(reader) = &self.reader {
            let visible = self.reader_visible && !self.rules_view && self.editing.is_none()
                && self.resize_drag.is_none() && self.peek.is_none() && self.hovered.is_none()
                && self.mail_drag.is_none() && self.inbox_scroll_drag.is_none()
                // The native WebView paints above GPUI overlays; keep its layout, but hide its body while picking.
                && self.label_picker.is_none() && self.label_removal.is_none()
                && !(below && (self.row_menu.is_some() || label_popup.is_some()));
            let badges = self.active_message.as_ref().and_then(|id| self.triage.emails.iter().find(|e| &e.id == id))
                .map(|email| labels::summarize(std::iter::once(email.label_ids.as_slice()), &self.labels, false));
            reader.update(cx, |reader, cx| { reader.set_visible(visible, cx); if let Some(badges) = badges { reader.set_labels(badges, cx); } });
        }
        for input in [&self.space_name, &self.space_query, &self.label_query] {
            input.update(cx, |input, _| input.tab_stop = self.editing.is_none());
        }
        self.filter.update(cx, |input, _| {
            input.tab_stop = self.editing.is_none();
            input.placeholder = if self.rules_view {
                "Filter sender rules..."
            } else {
                "Filter messages..."
            }
            .into();
        });
        let p = Palette::current(cx);
        let count: usize = rows.iter().filter(|r| !r.header).map(|r| r.ids.len()).sum();
        let senders = rows.iter().filter(|r| !r.child).count();
        let selected: Vec<_> = self.triage.selection.iter().cloned().collect();
        let can_stage = !self.busy && !self.blocked && !self.triage.has_unknown();
        let mut root = div()
            .key_context("MailApp")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.canvas)
            .text_color(p.ink)
            .font_family("Segoe UI")
            .text_size(px(12.))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.load(true, false, cx)))
            .on_action(cx.listener(|this, _: &Apply, _, cx| this.submit(false, None, cx)))
            .on_action(cx.listener(|this, _: &FocusFilter, window, cx| {
                if this.editing.is_none() {
                    this.filter.read(cx).focus_handle.focus(window);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleRules, window, cx| {
                this.command(Command::Rules, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleStaging, window, cx| {
                this.command(Command::ToggleSidebar, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleReader, window, cx| {
                this.command(Command::ToggleReader, window, cx)
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    cx.stop_propagation();
                    if event.keystroke.modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    cx.notify();
                }
                if event.keystroke.key == "escape" {
                    cx.stop_active_drag(window);
                    this.mail_drag = None; this.drop_target = None;
                    this.inbox_scroll_drag = None;
                    if this.label_removal.take().is_some() {
                        this.table_focus.focus(window); cx.stop_propagation(); cx.notify(); return;
                    }
                    if this.label_picker.take().is_some() {
                        this.label_row_targets = None;
                        this.table_focus.focus(window); cx.stop_propagation(); cx.notify(); return;
                    }
                    if this.space_draft.as_ref().is_some_and(|draft| draft.picker) {
                        this.space_draft.as_mut().unwrap().picker = false;
                        this.space_name.read(cx).focus_handle.focus(window);
                        cx.stop_propagation(); cx.notify(); return;
                    }
                    if this.row_menu.is_some() { this.close_row_menu(window, cx); cx.stop_propagation(); return; }
                    if this.resize_drag.take().is_some() { this.save_pane_layout(cx); }
                    this.editing = None;
                    this.peek = None;
                    this.hovered = None;
                    this.table_focus.focus(window);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(p.line)
                    .bg(p.panel)
                    .child(div().w(px(190.)).child(self.filter.clone()))
                    .child(self.button(
                        "query",
                        "Run query",
                        Command::Refresh(true),
                        !self.busy && !self.settings.rules.is_empty(),
                        cx,
                    ))
                    .child(self.button(
                        "rules",
                        if self.rules_view { "Inbox" } else { "Rules" },
                        Command::Rules,
                        true,
                        cx,
                    ))
                    .child(self.button(
                        "refresh",
                        if self.busy {
                            "Working..."
                        } else if self.session.is_some() || self.demo {
                            "Refresh"
                        } else {
                            "Connect Gmail"
                        },
                        Command::Refresh(false),
                        !self.busy && !self.blocked,
                        cx,
                    ))
                    .child(
                        div()
                            .text_color(p.ink_dim)
                            .child(format!("{senders} senders / {count} undecided")),
                    )
                    .when(!selected.is_empty() && !self.rules_view, |d| {
                        d.child(
                            div()
                                .text_color(p.accent)
                                .child(format!("{} checked", selected.len())),
                        )
                        .child(self.button("assign-labels", "Add label", Command::TopLabelPicker(LabelMode::Assign), can_stage && (self.demo || self.session.is_some()), cx)
                            .flex().items_center().gap_1().child(icons::icon("icons/label.svg", p.ink_dim))
                            .relative().child(self.label_trigger_anchor(LabelMode::Assign, None, cx))
                            .tooltip(|_, cx| cx.new(|_| Hint("Add a label to checked messages now. Keeps INBOX.")).into()))
                        .child(self.button(
                            "selection-a",
                            format!("Archive {}", selected.len()),
                            Command::Stage(selected.clone(), Action::Archive),
                            can_stage,
                            cx,
                        ))
                        .child(self.button(
                            "selection-d",
                            format!("Trash {}", selected.len()),
                            Command::Stage(selected.clone(), Action::Trash),
                            can_stage,
                            cx,
                        ))
                        .child(self.button(
                            "clear",
                            "Clear",
                            Command::Clear,
                            true,
                            cx,
                        ))
                    })
                    .child(div().flex_1())
                    .child(self.button(
                        "auto",
                        if self.settings.auto_apply {
                            "Auto-apply ON"
                        } else {
                            "Auto-apply"
                        },
                        Command::AutoApply,
                        !self.busy && !self.blocked,
                        cx,
                    ))
                    .child(self.button(
                        "theme",
                        format!("Theme: {}", self.settings.theme),
                        Command::Theme,
                        !self.busy && !self.blocked && self.settings.theme_names().len() > 1,
                        cx,
                    )),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(if self.failed { p.danger } else { p.ink_dim })
                    .child(div().flex_1().min_w_0().child(if self.labels.unavailable {
                        format!("{} Label metadata is unavailable; cached labels are kept. Refresh to retry.", self.status)
                    } else { self.status.clone() })),
            )
            .when(!self.rules_view, |d| d.child(self.label_filter_controls(cx)));
        root = if self.rules_view {
            root.child(self.rules(cx))
        } else {
            root.child(div().flex().flex_1().min_h_0().relative()
                .child(canvas(move |bounds, _, cx| {
                    cx.defer(move |cx| {
                        let _ = workspace_view.update(cx, |this, cx| {
                            if this.workspace_bounds != bounds { this.workspace_bounds = bounds; cx.notify(); }
                        });
                    });
                }, |_, _, _, _| {}).absolute().inset_0())
                .child(div().flex().flex_1().min_w_0().min_h_0().when(below, |d| d.flex_col())
                .child(div().flex_1().min_w_0().min_h_0().flex().flex_col()
                    .child(div().flex().gap_2().px_2().pr(px(8. + SCROLLBAR_WIDTH)).py_2().border_y_1().border_color(p.line).text_color(p.ink_dim)
                        .child(div().w(px(64.)))
                        .child(div().flex_1().child("SENDER / LABELS / SUBJECT"))
                        .child(div().w(px(105.)).child("DATE")))
                    .child(div().id("triage-table").tab_index(0).tab_stop(self.editing.is_none()).key_context("TriageTable").track_focus(&self.table_focus.clone().tab_index(0).tab_stop(self.editing.is_none())).flex_1().min_h_0().flex().flex_col()
                        .relative()
                        .child(canvas(move |bounds, _, cx| {
                            let _ = table_view.update(cx, |this, _| {
                                this.table_bounds = Bounds::new(bounds.origin, size((bounds.size.width - px(SCROLLBAR_WIDTH)).max(px(0.)), bounds.size.height));
                            });
                        }, |_, _, _, _| {}).absolute().inset_0())
                        .on_key_down(cx.listener(Self::table_key))
                        .when(rows.is_empty(), |d| d.child(div().absolute().inset_0().p_8().text_color(p.ink_dim).child(if self.filter.read(cx).content.is_empty() && self.triage.label_filter.is_empty() {"Nothing left undecided in the loaded Inbox."} else {"No messages match these filters."})))
                        .child(uniform_list("inbox",rows.len(),cx.processor(move |this,range:std::ops::Range<usize>,window,cx|range.map(|i|
                            this.row(rows[i].clone(),i,list_width - SCROLLBAR_WIDTH,window,cx).into_any_element()
                        ).collect())).track_scroll(self.scroll.clone()).mr(px(SCROLLBAR_WIDTH)).flex_1().min_h_0())
                        .child(self.inbox_scrollbar(cx))))
                .when(self.reader_visible, |d| {
                    let reader = self.reader.clone().expect("Visible reader is initialized");
                    let status = self.active_message.as_ref().map(|id| {
                        if let Some(mark) = self.triage.marks.get(id) {
                            format!("Staged for {} / preview preserved", self.action_name(mark.action))
                        } else if !self.triage.emails.iter().any(|e| &e.id == id) {
                            "No longer in the loaded Inbox / preview preserved".into()
                        } else { "Preview / unread unchanged".into() }
                    }).unwrap_or_else(|| "Select a message to read".into());
                    d.child(self.splitter(if below { ResizeAxis::ReaderHeight } else { ResizeAxis::Reader }, cx))
                        .child(div().flex_shrink_0().flex().flex_col().overflow_hidden()
                            .when(below, |d| d.w_full().h(px(self.reader_height())))
                            .when(!below, |d| d.w(px(reader_width)).h_full())
                            .child(div().px_3().py_1().text_xs().text_color(p.ink_dim).child(status))
                            .child(div().flex_1().min_h_0().child(reader)))
                }))
                .when(self.sidebar_visible, |d| d.child(self.splitter(ResizeAxis::Width, cx)).child(self.bins(pane_width, cx))))
        };
        root = root.child(div().h(px(34.)).flex_shrink_0().flex().items_center().gap_2().px_3()
            .border_t_1().border_color(p.line).bg(p.panel).text_xs().text_color(p.ink_dim)
            .child(div().flex_1().min_w_0().truncate().child("Row: read / expand   Checkbox: select   Middle-click: all / none   Right-click / Shift+F10: menu   Drag: stage"))
            .child(format!("{} pending", self.triage.marks.len()))
            .child(self.button("toggle-reader-position", "", Command::ToggleReaderPosition, !self.rules_view && !self.blocked, cx))
            .child(self.button("toggle-reader", "", Command::ToggleReader, !self.rules_view, cx))
            .child(self.button("toggle-staging", "", Command::ToggleSidebar, !self.rules_view, cx)));
        if self.sidebar_visible && !self.rules_view
            && let Some((sender, action)) = self.peek.as_ref().or(self.hovered.as_ref())
        {
            let emails: Vec<_> = self
                .triage
                .emails
                .iter()
                .filter(|e| {
                    sender_address(&e.from) == *sender
                        && self
                            .triage
                            .marks
                            .get(&e.id)
                            .is_some_and(|m| m.action == *action)
                })
                .collect();
            if !emails.is_empty() {
                root = root.child(
                    div()
                        .absolute()
                        .right(px(pane_width + DIVIDER + 8.))
                        .top(px(150.))
                        .w(px(370.))
                        .max_h(px(420.))
                        .p_3()
                        .bg(p.panel)
                        .border_1()
                        .border_color(p.accent)
                        .rounded_md()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .child(format!(
                                    "{} / {}",
                                    sender_name(&emails[0].from),
                                    self.action_name(*action)
                                ))
                                .child(self.button(
                                    "close-peek",
                                    "Close",
                                    Command::ClosePeek,
                                    true,
                                    cx,
                                )),
                        )
                        .child(div().text_color(p.ink_dim).child(sender.clone()))
                        .child(div().id("peek-scroll").overflow_y_scroll().children(
                            emails.into_iter().map(|e| {
                                div()
                                    .py_2()
                                    .border_t_1()
                                    .border_color(p.line)
                                    .child(self.button(
                                        format!("peek-open-{}", e.id),
                                        e.subject.clone(),
                                        Command::Open(e.id.clone()),
                                        self.demo || self.session.is_some(),
                                        cx,
                                    ))
                                    .child(div().text_color(p.ink_dim).child(e.snippet.clone()))
                                    .when_some(self.triage.marks[&e.id].outcome.as_ref(), |d, o| {
                                        match o {
                                            Outcome::Failed(s) | Outcome::Unknown(s) => {
                                                d.child(div().text_color(p.danger).child(s.clone()))
                                            }
                                            _ => d,
                                        }
                                    })
                            }),
                        )),
                );
            }
        }
        // Window-level capture keeps a drag active outside its narrow divider and ends it even outside the window.
        let move_resize = cx.listener(Self::move_splitter);
        let finish_resize = cx.listener(|this, event: &MouseUpEvent, _, cx| {
            if event.button == MouseButton::Left && this.inbox_scroll_drag.take().is_some() {
                cx.stop_propagation();
                cx.notify();
            }
            if event.button == MouseButton::Left && this.mail_drag.is_some() {
                let view = cx.weak_entity();
                cx.defer(move |cx| { let _ = view.update(cx, |this, cx| { this.mail_drag = None; this.drop_target = None; cx.notify(); }); });
            }
            if event.button == MouseButton::Left && this.resize_drag.take().is_some() {
                this.save_pane_layout(cx);
                cx.stop_propagation();
                cx.notify();
            }
        });
        let begin_click = cx.listener(|this, event: &gpui::MouseDownEvent, _, _| {
            if event.button == MouseButton::Left { this.suppress_row_click = false; }
        });
        root = root.child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture { begin_click(event, window, cx); }
                    });
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            move_resize(event, window, cx);
                        }
                    });
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            finish_resize(event, window, cx);
                        }
                    });
                },
            )
            .absolute()
            .inset_0(),
        );
        if let Some(drag) = self.resize_drag {
            root = root.child(div().absolute().inset_0().occlude().cursor(
                if matches!(drag.axis, ResizeAxis::Width | ResizeAxis::Reader) {
                    CursorStyle::ResizeLeftRight
                } else {
                    CursorStyle::ResizeUpDown
                },
            ));
        }
        if self.inbox_scroll_drag.is_some() {
            root = root.child(div().absolute().inset_0().occlude().cursor(CursorStyle::Arrow));
        }
        if let Some((anchor, text)) = label_popup {
            let position = menu_position(anchor, window.viewport_size());
            root = root.child(div().id("label-details").absolute().left(position.x).top(position.y).w(px(360.))
                .max_h(px(252.)).overflow_y_scroll().p_3().bg(p.panel).border_1().border_color(p.line)
                .text_xs().whitespace_normal().child(text));
        }
        if self.label_picker.is_some() && !self.rules_view && self.editing.is_none() {
            root = root.child(self.top_label_picker(window, cx));
        }
        if self.label_removal.is_some() && !self.rules_view && self.editing.is_none() {
            root = root.child(self.label_removal_panel(window, cx));
        }
        if let Some(menu) = &self.row_menu {
            let position = menu_position(menu.anchor, window.viewport_size());
            let count = menu.row.ids.len();
            let chosen = menu.selected;
            let menu_view = cx.weak_entity();
            let scope = if self.triage.selected(&menu.row) {
                format!("{} selected messages", self.triage.action_ids(&menu.row).len())
            } else { format!("these {count} messages") };
            let items = vec![format!("Archive {scope}"), format!("Trash {scope}"),
                "Sender rule...".into(), if count == 1 { "Open message".into() } else if menu.row.header { "Collapse messages".into() } else { "Expand messages".into() },
                format!("{} these {count} messages", if self.triage.selected(&menu.row) { "Uncheck" } else { "Select" })];
            root = root.child(div().absolute().inset_0().occlude()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.close_row_menu(window, cx); }))
                .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.close_row_menu(window, cx); }))
                .on_scroll_wheel(cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.close_row_menu(window, cx); }))
                .child(div().id("row-menu").track_focus(&self.menu_focus).absolute().left(position.x).top(position.y)
                    .w(px(360.)).h(px(252.)).bg(p.panel).text_color(p.ink).border_1().border_color(p.accent)
                    .child(canvas(move |bounds, _, cx| { let _ = menu_view.update(cx, |this, _| this.menu_bounds = bounds); }, |_, _, _, _| {}).absolute().inset_0())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        cx.stop_propagation();
                        match event.keystroke.key.as_str() {
                            "up" => { if let Some(m) = &mut this.row_menu { m.selected = (m.selected + 4) % 5; } }
                            "down" => { if let Some(m) = &mut this.row_menu { m.selected = (m.selected + 1) % 5; } }
                            "home" => { if let Some(m) = &mut this.row_menu { m.selected = 0; } }
                            "end" => { if let Some(m) = &mut this.row_menu { m.selected = 4; } }
                            "enter" | "space" => { if let Some(m) = &this.row_menu { this.menu_action(m.selected, window, cx); } }
                            "escape" | "tab" => this.close_row_menu(window, cx),
                            _ => {}
                        }
                        cx.notify();
                    }))
                    .child(div().h(px(48.)).px_3().py_2().text_xs().text_color(p.ink_dim).truncate()
                        .child(format!("{} / {count} messages", sender_name(&menu.row.sender))))
                    .children(items.into_iter().enumerate().map(|(index, label)| {
                        let enabled = index >= 3 || can_stage;
                        div().id(("row-menu-item", index)).h(px(34.)).px_3().flex().items_center().cursor_pointer()
                            .when(index == chosen, |d| d.bg(p.accent.opacity(0.12)))
                            .when(index == 1, |d| d.text_color(p.danger)).when(!enabled, |d| d.opacity(0.4))
                            .on_hover(cx.listener(move |this, over, _, cx| { if *over { if let Some(m) = &mut this.row_menu { m.selected = index; } cx.notify(); } }))
                            .on_click(cx.listener(move |this, _, window, cx| { cx.stop_propagation(); if enabled { this.menu_action(index, window, cx); } }))
                            .child(label)
                    }))
                    .child(div().px_3().py_1().text_xs().text_color(p.ink_dim).child("Arrows: navigate / Enter: act / Esc: close"))));
        }
        if let Some((rule_index, action)) = self.editing {
            root = root.child(
                div()
                    .absolute()
                    .occlude()
                    .inset_0()
                    .bg(gpui::rgba(0x000000aa))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(470.))
                            .p_5()
                            .bg(p.panel)
                            .border_1()
                            .border_color(p.accent)
                            .rounded_md()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(if rule_index.is_some() { "Edit sender rule / From match" } else { "New sender rule / From match" })
                            .when(rule_index.is_none(), |d| d.child(div().text_xs().text_color(p.ink_dim).child(format!(
                                "Save will {} {} current matches{}.", action.label().to_lowercase(),
                                self.matching_sender_ids(&self.rule_input.read(cx).content).len(),
                                if self.settings.auto_apply { " immediately" } else { " into staging" }
                            ))))
                            .child(self.rule_input.clone())
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .children([
                                        ("rule-archive", Action::Archive, "icons/archive.svg"),
                                        ("rule-trash", Action::Trash, "icons/trash.svg"),
                                    ].into_iter().map(|(id, choice, icon)| {
                                        let selected = action == choice;
                                        let color = if selected { p.accent } else { p.ink_dim };
                                        self.button(id, "", Command::EditAction(choice), true, cx)
                                            .h(px(36.)).px_3().flex().items_center().gap_2()
                                            .bg(if selected { p.accent.opacity(0.08) } else { p.panel })
                                            .border_color(if selected { p.accent } else { p.line })
                                            .text_color(color)
                                            .child(icons::icon(icon, color))
                                            .child(choice.label())
                                            .child(div().size(px(16.)).flex().items_center().justify_center()
                                                .when(selected, |d| d.child(icons::icon("icons/check.svg", color))))
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .child(self.button(
                                        "rule-cancel",
                                        "Cancel",
                                        Command::CancelEdit,
                                        true,
                                        cx,
                                    ))
                                    .child(self.button(
                                        "rule-save",
                                        if rule_index.is_some() { "Save" } else { "Save rule and apply to matches" },
                                        Command::SaveRule,
                                        true,
                                        cx,
                                    )),
                            ),
                    ),
            );
        }
        root
    }
}
fn main() {
    // ponytail: GPUI 0.2.2 composes above child HWNDs; keep its DirectX HWND path until upstream supports child-safe composition.
    // SAFETY: Set startup configuration before GPUI, WebView2, or workers initialize.
    unsafe { std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1") };
    Application::new()
        .with_assets(icons::Assets)
        .run(|cx: &mut App| {
            cx.set_global(Palette::default());
            input::init(cx);
            cx.bind_keys([
                KeyBinding::new("alt-i", reader::ToggleImages, Some("MailReader")),
                KeyBinding::new("ctrl-r", Refresh, Some("MailApp")),
                KeyBinding::new("ctrl-enter", Apply, Some("MailApp && !TextInput")),
                KeyBinding::new("ctrl-f", FocusFilter, Some("MailApp")),
                KeyBinding::new("ctrl-l", ToggleRules, Some("MailApp")),
                KeyBinding::new("ctrl-shift-b", ToggleStaging, Some("MailApp")),
                KeyBinding::new("alt-r", ToggleReader, Some("MailApp")),
            ]);
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            if std::env::args().any(|a| a == "--reader-demo" || a == "--reader-check") {
                let reader = Reader::open(cx, "Mado Mail - Message reader demo").expect("Could not open reader");
                reader
                    .update(cx, |reader, _, cx| reader.demo(cx))
                    .expect("Could not load reader demo");
                cx.activate(true);
                return;
            }
            let check = std::env::args().any(|a| a == "--triage-check");
            let demo = check || std::env::args().any(|a| a == "--demo");
            let bounds = Bounds::centered(None, size(px(1480.), px(850.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1100.), px(600.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(
                            if demo {
                                "Mado Mail - Demo"
                            } else {
                                "Mado Mail"
                            }
                            .into(),
                        ),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    cx.new(|cx| {
                        let mut app = MailApp::new(window, cx, demo);
                        if !demo {
                            app.load(false, false, cx);
                        }
                        if check {
                            if std::env::args().any(|arg| arg == "--detach-only") {
                                reader::detach_check::start(window.window_handle(), cx);
                            } else {
                                triage_check::start(window.window_handle(), cx);
                            }
                        }
                        app
                    })
                },
            )
            .expect("Could not open Mado Mail");
            cx.activate(true);
        });
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn hover_darkens_only_the_item_or_its_parent_group() {
        let p = Palette::default();
        let rows = triage::demo().rows("");
        let parent = rows.iter().find(|row| row.header).unwrap();
        let child = rows.iter().find(|row| row.child).unwrap();
        let sibling = rows.iter().filter(|row| row.child).nth(1).unwrap();
        let unrelated = rows.iter().find(|row| !row.child && !row.header).unwrap();
        let tint = gpui::black().opacity(0.04);
        for row in [parent, child, unrelated] {
            let base = if row.child { p.panel2 } else { p.canvas };
            assert_eq!(row_background(row, None, false, p), base);
            assert_eq!(row_background(row, Some(&row.key), false, p), base.blend(tint));
        }
        for row in [child, sibling] {
            assert_eq!(row_background(row, Some(&parent.key), false, p), p.panel2.blend(tint));
        }
        assert_eq!(row_background(parent, Some(&child.key), false, p), p.canvas);
        assert_eq!(row_background(sibling, Some(&child.key), false, p), p.panel2);
        assert_eq!(row_background(unrelated, Some(&parent.key), false, p), p.canvas);
        let active = p.panel2.blend(p.accent.opacity(0.12));
        assert_eq!(row_background(child, None, true, p), active);
        assert_eq!(row_background(child, Some(&child.key), true, p), active.blend(tint));
        assert_eq!(row_background(child, Some(&parent.key), true, p), active.blend(tint));
    }
    #[test]
    fn inbox_scrollbar_tracks_overflow_and_clamps_at_both_ends() {
        assert_eq!(scrollbar_thumb(0., 100., 0.), None);
        assert_eq!(scrollbar_thumb(400., 0., 0.), None);
        assert_eq!(scrollbar_thumb(400., 400., 0.), None);
        assert_eq!(scrollbar_thumb(400., 800., 0.), Some((0., 200.)));
        assert_eq!(scrollbar_thumb(400., 800., 200.), Some((100., 200.)));
        assert_eq!(scrollbar_thumb(400., 800., 900.), Some((200., 200.)));
        assert_eq!(scrollbar_thumb(400., 800., -20.), Some((0., 200.)));
        assert_eq!(scrollbar_thumb(400., 40000., 0.), Some((0., 24.)));
        assert_eq!(scrollbar_thumb(12., 800., 500.), Some((0., 12.)));
        for viewport in [120., 300., 700.] {
            let (top, height) = scrollbar_thumb(viewport, 2000., 2000. - viewport).unwrap();
            assert!((top + height - viewport).abs() < 0.001);
        }
    }
    #[test]
    fn row_menu_stays_in_view_at_each_edge() {
        let viewport = size(px(1100.), px(600.));
        for (x, y) in [(0., 0.), (1040., 550.), (0., 550.), (1040., 0.)] {
            let anchor = Bounds::new(gpui::point(px(x), px(y)), size(px(60.), px(44.)));
            let position = menu_position(anchor, viewport);
            assert!(position.x >= px(8.) && position.x + px(360.) <= viewport.width - px(8.));
            assert!(position.y >= px(8.) && position.y + px(252.) <= viewport.height - px(8.));
        }
    }
    #[test]
    fn bottom_reader_reserves_list_space_and_clamps_preferred_height() {
        assert_eq!(reader_height(730., 360.), 360.);
        assert_eq!(reader_height(470., 360.), 304.);
        assert_eq!(reader_height(730., 20.), 240.);
        assert_eq!(reader_height(100., 360.), 0.);
        for height in [470., 730., 1200.] {
            for preferred in [240., 360., 8192.] {
                let shown = reader_height(height, preferred);
                assert!(shown >= 240.);
                assert!(height - shown - DIVIDER >= 160.);
            }
        }
    }
    #[test]
    fn sidebar_limits_keep_inbox_and_both_bins_usable() {
        assert_eq!(pane_widths(1480., None, Some(320.)), (0., 320.));
        assert_eq!(pane_widths(1480., Some(480.), Some(320.)), (480., 320.));
        assert_eq!(pane_widths(1100., None, None), (0., 0.));
        assert_eq!(pane_widths(1100., Some(10.), None), (320., 0.));
        for viewport in [1100., 1480., 1920.] {
            for reader in [None, Some(480.), Some(8000.)] {
                for staging in [None, Some(320.), Some(8000.)] {
                    let (r, s) = pane_widths(viewport, reader, staging);
                    let dividers = (reader.is_some() as u8 + staging.is_some() as u8) as f32 * DIVIDER;
                    assert!(r + s + dividers <= viewport - 420. + 0.01);
                    assert!(reader.is_none() || r >= 320.);
                    assert!(staging.is_none() || s >= 260.);
                }
            }
        }
        assert_eq!(clamp_archive_fraction(-1., 320.), 0.25);
        assert_eq!(clamp_archive_fraction(2., 320.), 0.75);
        assert_eq!(clamp_archive_fraction(0.3, 0.), 0.5);
        assert_eq!(clamp_archive_fraction(0.8, 120.), 0.5);
    }
}
