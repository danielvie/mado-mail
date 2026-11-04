use super::*;
use crate::spaces::Space;

pub struct SpaceDraft {
    pub id: Option<u64>,
    pub labels: BTreeSet<String>,
    pub picker: bool,
    pub cursor: usize,
}

impl MailApp {
    pub(super) fn action_name(&self, action: Action) -> String {
        match action {
            Action::Space(id) => self.settings.spaces.iter().find(|space| space.id == id)
                .map(|space| space.name.clone()).unwrap_or_else(|| format!("Space {id} / recovered")),
            _ => action.label().into(),
        }
    }
    pub(super) fn can_stage_action(&self, action: Action) -> bool {
        self.can_stage() && match action {
            Action::Space(id) => self.settings.spaces.iter().find(|space| space.id == id)
                .is_some_and(|space| spaces::available(&space.label_ids, &self.labels)
                    && self.triage.marks.values().filter(|mark| mark.action == action).all(|mark| mark.label_ids == space.label_ids))
                && !self.space_draft.as_ref().is_some_and(|draft| draft.id == Some(id)),
            _ => true,
        }
    }
    pub(super) fn edit_space(&mut self, id: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.blocked || self.triage.has_unknown() || self.space_draft.is_some() { return; }
        let space = id.and_then(|id| self.settings.spaces.iter().find(|space| space.id == id));
        if let Some(id) = id {
            if space.is_none() || self.triage.marks.values().any(|mark| mark.action == Action::Space(id)) {
                self.error("Put back or apply this space's pending messages before editing its labels.", cx);
                return;
            }
        } else if self.settings.spaces.len() >= 100 { self.error("At most 100 spaces are supported.", cx); return; }
        let name = space.map_or("", |space| space.name.as_str()).to_owned();
        let labels = space.map(|space| space.label_ids.iter().cloned().collect()).unwrap_or_default();
        self.space_draft = Some(SpaceDraft { id, labels, picker: id.is_some(), cursor: 0 });
        self.sidebar_visible = true;
        self.save_pane_layout(cx);
        self.staging_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.space_name.update(cx, |input, cx| input.set(&name, cx));
        self.space_query.update(cx, |input, cx| input.set("", cx));
        self.row_menu = None; self.peek = None; self.hovered = None;
        if id.is_some() { self.space_query.read(cx).focus_handle.clone().focus(window, cx); }
        else { self.space_name.read(cx).focus_handle.clone().focus(window, cx); }
        cx.notify();
    }
    pub(super) fn can_remove_space(&self, id: u64) -> bool {
        !self.busy && !self.blocked && !self.triage.has_unknown() && self.space_draft.is_none()
            && self.mail_drag.is_none() && self.settings.spaces.iter().any(|space| space.id == id)
            && !self.triage.marks.values().any(|mark| mark.action == Action::Space(id))
    }
    pub(super) fn remove_space(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_remove_space(id) { return; }
        let mut next = self.settings.clone();
        next.spaces.retain(|space| space.id != id);
        if self.save_settings(next, cx) {
            self.peek = None; self.hovered = None;
            self.failed = false;
            self.status = "Space removed locally. Gmail labels and messages are unchanged.".into();
            self.table_focus.focus(window, cx);
        }
    }
    pub(super) fn save_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.blocked || self.triage.has_unknown() { return; }
        let Some(draft) = &self.space_draft else { return; };
        let labels: Vec<_> = draft.labels.iter().cloned().collect();
        if !spaces::available(&labels, &self.labels) { self.error("Select available Gmail user labels. Refresh if the catalog is unavailable.", cx); return; }
        let id = match draft.id {
            Some(id) => id,
            None => match self.settings.spaces.iter().map(|space| space.id).max().unwrap_or(0).checked_add(1) {
                Some(id) => id,
                None => { self.error("No space IDs available.", cx); return; }
            },
        };
        if self.triage.marks.values().any(|mark| mark.action == Action::Space(id)) {
            self.error("Pending messages lock this space's labels.", cx); return;
        }
        let space = Space { id, name: self.space_name.read(cx).content.trim().into(), label_ids: labels };
        let mut next = self.settings.clone();
        if let Some(existing) = next.spaces.iter_mut().find(|s| s.id == id) { *existing = space; }
        else { next.spaces.push(space); }
        if self.save_settings(next, cx) {
            self.space_draft = None;
            self.staging_scroll.scroll_to_bottom();
            self.failed = false;
            self.status = "Space saved locally. Drop messages or use Move selected, then Apply.".into();
            self.table_focus.focus(window, cx);
        }
    }
    pub(super) fn toggle_space_label(&mut self, id: String, cx: &mut Context<Self>) {
        if self.busy || self.blocked || self.labels.unavailable { return; }
        let Some(draft) = &mut self.space_draft else { return; };
        if draft.labels.remove(&id) { cx.notify(); return; }
        if self.labels.entries.get(&id).is_some_and(|label| label.kind == "user") && draft.labels.len() < 100 {
            draft.labels.insert(id); cx.notify();
        }
    }
    pub(super) fn space_picker_key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.space_query.read(cx).focus_handle.is_focused(window) { return; }
        let Some(draft) = &mut self.space_draft else { return; };
        if !draft.picker { return; }
        let results = spaces::find(&self.labels, &self.space_query.read(cx).content);
        match event.keystroke.key.as_str() {
            "down" => draft.cursor = (draft.cursor + 1).min(results.len().saturating_sub(1)),
            "up" => draft.cursor = draft.cursor.saturating_sub(1),
            "enter" => {
                let id = results.get(draft.cursor).map(|label| label.id.clone());
                if let Some(id) = id { self.toggle_space_label(id, cx); }
            }
            "escape" => { draft.picker = false; self.space_name.read(cx).focus_handle.clone().focus(window, cx); }
            _ => return,
        }
        window.prevent_default(); cx.stop_propagation(); cx.notify();
    }
    pub(super) fn space_editor(&self, cx: &Context<Self>) -> gpui::Div {
        let p = Palette::current(cx);
        let Some(draft) = &self.space_draft else { return div(); };
        let ids: Vec<_> = draft.labels.iter().cloned().collect();
        let valid = spaces::validate(&[Space { id: 1, name: self.space_name.read(cx).content.trim().into(), label_ids: ids.clone() }]).is_ok()
            && spaces::available(&ids, &self.labels);
        let enabled = !self.busy && !self.blocked && !self.triage.has_unknown();
        let mut editor = div().flex_shrink_0().m_2().p_2().border_2().border_color(p.accent)
            .flex().flex_col().gap_2()
            .child(if draft.id.is_some() { "Edit space" } else { "New space / draft" })
            .child(self.space_name.clone())
            .child(div().flex().items_center().gap_2()
                .child(self.button("space-label-icon", "", Command::SpacePicker, enabled, cx))
                .child("Labels"))
            .child(div().flex().flex_wrap().gap_1().children(ids.iter().map(|id| {
                let label = self.labels.entries.get(id).map(|label| label.name.as_str()).unwrap_or(id);
                self.button(format!("space-chip-{id}"), format!("{label} ×"), Command::SpaceLabel(id.clone()), enabled, cx)
                    .max_w_full().overflow_hidden()
            })));
        if draft.picker {
            let results = spaces::find(&self.labels, &self.space_query.read(cx).content);
            // Keep the focused result visible without moving the surrounding staging pane.
            let start = draft.cursor.saturating_sub(5);
            editor = editor.child(div().flex().flex_col().gap_1().border_1().border_color(p.line).p_1()
                .on_key_down(cx.listener(Self::space_picker_key))
                .child(self.space_query.clone())
                .child(div().text_xs().text_color(p.ink_dim).child("Fuzzy find / Up, Down, Enter. Esc closes."))
                .when(self.labels.unavailable, |d| d.child("Labels unavailable. Refresh to retry."))
                .when(results.is_empty(), |d| d.child("No matching labels"))
                .children(results.iter().enumerate().skip(start).take(6).map(|(index, label)| {
                    self.button(format!("space-label-{}", label.id), label.name.clone(), Command::SpaceLabel(label.id.clone()), enabled && !self.labels.unavailable, cx)
                        .flex().items_center().gap_2().max_w_full().overflow_hidden()
                        .child(icons::checkbox(draft.labels.contains(&label.id), false, p))
                        .when(index == draft.cursor, |d| d.border_color(p.accent))
                })));
        }
        editor.child(div().flex().gap_2()
            .child(self.button("space-cancel", "Cancel", Command::CancelSpace, !self.busy, cx))
            .child(self.button("space-save", "Save space", Command::SaveSpace, enabled && valid, cx)))
            .child(div().text_xs().text_color(p.ink_dim).child("Name + label required. Drafts cannot receive mail."))
    }
    pub(super) fn space_details(&self, id: u64, cx: &Context<Self>) -> gpui::Div {
        let p = Palette::current(cx);
        let labels = self.triage.marks.values().find(|mark| mark.action == Action::Space(id)).map(|mark| mark.label_ids.clone())
            .or_else(|| self.settings.spaces.iter().find(|space| space.id == id).map(|space| space.label_ids.clone())).unwrap_or_default();
        let pending = self.triage.marks.values().any(|mark| mark.action == Action::Space(id));
        let badges = labels::summarize(std::iter::once(labels.as_slice()), &self.labels, false);
        div().px_2().pb_2().flex().flex_col().gap_1()
            .child(div().flex().flex_wrap().items_center().gap_1()
                .child(self.button(format!("edit-space-{id}"), "", Command::EditSpace(id), !self.busy && !self.blocked && !pending && self.space_draft.is_none(), cx))
                .children(badges.iter().map(|badge| labels::chip(badge, 220., p))))
            .when(!spaces::available(&labels, &self.labels), |d| d.child(div().text_color(p.danger).child("Label unavailable. Refresh or edit before Apply.")))
            .when(pending, |d| d.child(div().text_xs().text_color(p.ink_dim).child("Labels locked while mail is pending.")))
            .child(self.button(format!("move-space-{id}"), format!("Move selected / {}", self.triage.selection.len()), Command::Stage(self.triage.selection.iter().cloned().collect(), Action::Space(id)), self.can_stage_action(Action::Space(id)) && !self.triage.selection.is_empty(), cx))
    }
}
