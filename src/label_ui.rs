use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LabelMode { Filter, Assign }

pub struct LabelRemoval {
    pub label: String,
    pub name: String,
    pub ids: Vec<String>,
    pub anchor: Bounds<Pixels>,
}

fn picker_bounds(anchor: Bounds<Pixels>, viewport: gpui::Size<Pixels>, height: f32) -> Bounds<Pixels> {
    let width = 440_f32.min((f32::from(viewport.width) - 16.).max(0.));
    let height = height.min((f32::from(viewport.height) - 16.).max(0.));
    let x = f32::from(anchor.left()).clamp(8., (f32::from(viewport.width) - width - 8.).max(8.));
    let y = (f32::from(anchor.bottom()) + 6.).clamp(8., (f32::from(viewport.height) - height - 8.).max(8.));
    Bounds::new(gpui::point(px(x), px(y)), size(px(width), px(height)))
}

impl MailApp {
    pub(super) fn request_label_removal(&mut self, key: &str, label: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_stage() { return; }
        if !spaces::available(std::slice::from_ref(&label), &self.labels) {
            self.error("Label unavailable. Refresh before removing labels.", cx); return;
        }
        let Some(row) = self.triage.rows(&self.filter.read(cx).content).into_iter().find(|row| row.key == key) else { return; };
        // Removal is row-local even if the row and unrelated messages are checked.
        let ids = self.triage.label_removal_ids(&row.ids, &label);
        if ids.is_empty() { return; }
        self.row_menu = None; self.peek = None; self.hovered = None;
        self.label_picker = None; self.label_row_targets = None;
        if row.ids.len() == 1 {
            self.write_label_change(ids, label, true, cx);
        } else {
            let Some(anchor) = self.row_bounds.get(key).copied() else { return; };
            let name = self.labels.entries[&label].name.clone();
            self.label_removal = Some(LabelRemoval { label, name, ids, anchor });
            if let Some(reader) = &self.reader { reader.read(cx).focus_host(); }
            self.label_removal_focus[0].focus(window);
        }
        cx.notify();
    }
    pub(super) fn confirm_label_removal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_stage() { return; }
        let Some(removal) = self.label_removal.take() else { return; };
        self.table_focus.focus(window);
        if self.triage.label_removal_ids(&removal.ids, &removal.label) != removal.ids {
            self.error("Messages changed. Reopen the label removal to review its targets.", cx); return;
        }
        self.write_label_change(removal.ids, removal.label, true, cx);
    }
    pub(super) fn open_label_picker(&mut self, mode: LabelMode, row: Option<Row>, window: &mut Window, cx: &mut Context<Self>) {
        let targets = row.map(|row| (row.key.clone(), self.triage.action_ids(&row)));
        let empty = targets.as_ref().map_or(self.triage.selection.is_empty(), |(_, ids)| ids.is_empty());
        if self.editing.is_some() || (mode == LabelMode::Assign && (!self.can_stage() || empty)) { return; }
        if self.label_picker == Some(mode) && self.label_row_targets.as_ref().map(|(key, _)| key) == targets.as_ref().map(|(key, _)| key) {
            self.label_picker = None;
            self.label_row_targets = None;
            self.table_focus.focus(window);
        } else {
            self.label_picker = Some(mode);
            self.label_row_targets = targets;
            self.label_picker_cursor = 0;
            self.label_query.update(cx, |input, cx| input.set("", cx));
            if let Some(reader) = &self.reader { reader.read(cx).focus_host(); }
            self.label_query.read(cx).focus_handle.focus(window);
            self.row_menu = None; self.peek = None; self.hovered = None;
        }
        cx.notify();
    }
    pub(super) fn change_label_filter(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.triage.label_filter.contains(&id) && !self.labels.entries.get(&id).is_some_and(|label| label.kind == "user") { return; }
        self.triage.toggle_label_filter(id);
        self.label_removal = None;
        self.cursor = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.row_menu = None; self.hovered_row = None;
        cx.notify();
    }
    pub(super) fn pick_top_label(&mut self, id: String, cx: &mut Context<Self>) {
        match self.label_picker {
            Some(LabelMode::Filter) => self.change_label_filter(id, cx),
            Some(LabelMode::Assign) => self.assign_label(id, cx),
            None => {}
        }
    }
    fn label_assignment_ids(&self) -> Vec<String> {
        let ids = self.label_row_targets.as_ref().map_or_else(
            || self.triage.selection.iter().cloned().collect(), |(_, ids)| ids.clone());
        self.triage.undecided_ids(&ids)
    }
    fn assign_label(&mut self, label: String, cx: &mut Context<Self>) {
        if !self.can_stage() { return; }
        if !spaces::available(std::slice::from_ref(&label), &self.labels) {
            self.error("Label unavailable. Refresh before assigning labels.", cx); return;
        }
        // Row pickers keep their opening targets; toolbar pickers use the current checked set.
        // Snapshot before I/O so later filter/selection changes cannot retarget a write.
        let ids: Vec<_> = self.label_assignment_ids()
            .into_iter().filter(|id| self.triage.emails.iter().any(|email| &email.id == id && !email.label_ids.contains(&label))).collect();
        self.write_label_change(ids, label, false, cx);
    }
    fn write_label_change(&mut self, ids: Vec<String>, label: String, remove: bool, cx: &mut Context<Self>) {
        if !self.can_stage() || ids.is_empty() { return; }
        if !spaces::available(std::slice::from_ref(&label), &self.labels) {
            self.error("Label unavailable. Refresh before changing labels.", cx); return;
        }
        self.row_menu = None;
        if self.demo {
            self.finish_label_change(&label, ids.into_iter().map(|id| (id, Outcome::Confirmed)).collect(), remove, cx);
            self.status.push_str(" Demo only; Gmail was not contacted.");
            return;
        }
        let Some(session) = self.session.clone() else { self.error("Connect Gmail before changing labels.", cx); return; };
        self.busy = true; self.failed = false;
        self.status = format!("{} label {} {} messages now. INBOX stays unchanged.", if remove { "Removing" } else { "Adding" }, if remove { "from" } else { "to" }, ids.len());
        cx.notify();
        let request_label = label.clone();
        let job = cx.background_executor().spawn(async move {
            ids.into_iter().map(|id| {
                let result = session.lock().map_err(|_| anyhow::anyhow!("Gmail worker failed"))
                    .and_then(|mut session| if remove { session.remove_label(&id, &request_label) } else { session.add_label(&id, &request_label) });
                let outcome = match result {
                    Ok(Some(outcome)) => outcome,
                    _ => Outcome::Failed("Connect Gmail and retry the label change.".into()),
                };
                (id, outcome)
            }).collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let results = job.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.finish_label_change(&label, results, remove, cx);
            });
        }).detach();
    }
    pub(super) fn finish_label_change(&mut self, label: &str, results: Vec<(String, Outcome)>, remove: bool, cx: &mut Context<Self>) {
        let confirmed: Vec<_> = results.iter().filter(|(_, outcome)| *outcome == Outcome::Confirmed).map(|(id, _)| id.clone()).collect();
        let failed = results.iter().filter(|(_, outcome)| matches!(outcome, Outcome::Failed(_))).count();
        let unknown = results.iter().filter(|(_, outcome)| matches!(outcome, Outcome::Unknown(_))).count();
        if remove {
            self.triage.remove_label(&confirmed, label);
            // A removed filter label can hide a message. Do not retain invisible checked targets.
            let visible: BTreeSet<_> = self.triage.rows(&self.filter.read(cx).content).into_iter().flat_map(|row| row.ids).collect();
            self.triage.selection.retain(|id| visible.contains(id));
        } else { self.triage.add_label(&confirmed, label); }
        self.failed = failed > 0 || unknown > 0;
        self.status = format!("Label {} / {} confirmed, {failed} failed, {unknown} unknown. INBOX and unread unchanged. {}", if remove { "removed" } else { "added" }, confirmed.len(),
            if unknown > 0 { "Refresh to check unknown results before retrying." }
            else if failed > 0 { "Failed messages were left unchanged. Retry explicitly." }
            else { "No Apply needed." });
        if !remove && !confirmed.is_empty() {
            let mut next = self.settings.clone();
            let usage = next.label_usage.entry(label.into()).or_default();
            usage.count = usage.count.saturating_add(1);
            usage.last_used = labels::usage_time();
            // History is optional: a save failure must not block mail operations or imply Gmail failed.
            if self.demo || next.save_config(&self.settings_path, &self.settings).is_ok() {
                self.settings = next;
            } else {
                self.status.push_str(" Quick-label history could not be saved.");
            }
        }
        cx.notify();
    }
    pub(super) fn label_picker_key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.label_picker.is_none() { return; }
        if matches!(event.keystroke.key.as_str(), "escape" | "tab") {
            self.command(Command::CloseLabelPicker, window, cx);
            cx.stop_propagation(); window.prevent_default(); return;
        }
        if !self.label_query.read(cx).focus_handle.is_focused(window) { return; }
        let results = spaces::find(&self.labels, &self.label_query.read(cx).content);
        match event.keystroke.key.as_str() {
            "down" => self.label_picker_cursor = (self.label_picker_cursor + 1).min(results.len().saturating_sub(1)),
            "up" => self.label_picker_cursor = self.label_picker_cursor.saturating_sub(1),
            "enter" => {
                let id = results.get(self.label_picker_cursor).map(|label| label.id.clone());
                if !self.busy && let Some(id) = id { self.pick_top_label(id, cx); }
            }
            _ => return,
        }
        cx.stop_propagation(); window.prevent_default(); cx.notify();
    }
    pub(super) fn row_label_strip(&self, row: &str, badges: &[labels::Badge], budget: f32, cx: &Context<Self>) -> gpui::Div {
        let p = Palette::current(cx);
        let shown = labels::visible_count(badges.len(), budget);
        let remaining = badges.len() - shown;
        let available = (budget - if remaining > 0 { 34. } else { 0. } - shown as f32 * 6.).max(0.);
        div().max_w(px(budget)).min_w_0().flex_shrink_0().flex().items_center().gap_1()
            .children(badges.iter().take(shown).map(|badge| {
                let width = available / shown.max(1) as f32;
                if let Some(id) = &badge.id {
                    let group: SharedString = format!("message-label-{row}-{id}").into();
                    let (ink, background) = badge.colors.unwrap_or((p.ink, p.panel2));
                    let width = width.min(badge.text.chars().count() as f32 * 7. + 30.);
                    div().group(group.clone()).flex().items_center().flex_shrink_0().min_w_0()
                        .w(px(width + 2.)).rounded_sm().bg(background)
                        .child(self.button(format!("filter-row-label-{row}-{id}"), "", Command::AddLabelFilter(id.clone()), !self.busy, cx)
                            .p_0().flex_1().min_w_0().bg(gpui::transparent_black()).border_color(gpui::transparent_black())
                            .child(labels::chip(badge, (width - 20.).max(0.), p)))
                        .child(self.button(format!("remove-row-label-{row}-{id}"), "", Command::RemoveRowLabel(row.into(), id.clone()), self.can_stage() && !self.labels.unavailable && (self.demo || self.session.is_some()), cx)
                            .p_0().size(px(20.)).flex_shrink_0().flex().items_center().justify_center().rounded_sm()
                            .bg(gpui::transparent_black()).border_color(gpui::transparent_black())
                            .child(icons::icon("icons/x.svg", ink)).opacity(0.)
                            .group_hover(group, |s| s.opacity(1.)).focus(|s| s.opacity(1.)))
                        .into_any_element()
                } else { labels::chip(badge, width, p).into_any_element() }
            }))
            .when(remaining > 0, |d| d.child(div().text_xs().text_color(p.ink_dim).child(format!("+{remaining}"))))
    }
    pub(super) fn label_removal_panel(&self, window: &Window, cx: &Context<Self>) -> gpui::Div {
        let Some(removal) = &self.label_removal else { return div(); };
        let p = Palette::current(cx);
        let bounds = picker_bounds(removal.anchor, window.viewport_size(), 340.);
        let view = cx.weak_entity();
        div().absolute().inset_0().occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.command(Command::CancelLabelRemoval, window, cx); }))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.command(Command::CancelLabelRemoval, window, cx); }))
            .on_mouse_down(MouseButton::Middle, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.command(Command::CancelLabelRemoval, window, cx); }))
            .child(div().id("confirm-label-removal").absolute().left(bounds.left()).top(bounds.top())
                .w(bounds.size.width).max_h(bounds.size.height).overflow_y_scroll().p_3().rounded_md().shadow_lg()
                .bg(p.panel).border_1().border_color(p.accent).flex().flex_col().gap_3()
                .child(canvas(move |bounds, _, cx| { let _ = view.update(cx, |this, _| this.label_removal_bounds = bounds); }, |_, _, _, _| {}).absolute().inset_0())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "escape" => this.command(Command::CancelLabelRemoval, window, cx),
                        "tab" => {
                            let next = if this.label_removal_focus[0].is_focused(window) { 1 } else { 0 };
                            this.label_removal_focus[next].focus(window);
                        }
                        _ => return,
                    }
                    cx.stop_propagation(); window.prevent_default();
                }))
                .child(div().whitespace_normal().child(format!("Remove {} from {} {} in this group?", removal.name, removal.ids.len(), if removal.ids.len() == 1 { "message" } else { "messages" })))
                .child(div().text_xs().text_color(p.ink_dim).whitespace_normal().child("Only displayed group members with this label. Other checked messages, INBOX and unread stay unchanged."))
                .child(div().flex().justify_end().gap_2()
                    .child(self.button("cancel-label-removal", "Cancel", Command::CancelLabelRemoval, true, cx).track_focus(&self.label_removal_focus[0]))
                    .child(self.button("confirm-label-removal-button", "Remove label", Command::ConfirmLabelRemoval, self.can_stage(), cx)
                        .track_focus(&self.label_removal_focus[1]).text_color(p.danger))))
    }
    pub(super) fn label_trigger_anchor(&self, mode: LabelMode, row: Option<String>, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        canvas(move |bounds, _, cx| {
            let _ = view.update(cx, |this, cx| {
                let active_row = this.label_row_targets.as_ref().map(|(key, _)| key);
                let active = this.label_picker == Some(mode) && active_row == row.as_ref();
                let anchor = if let Some(key) = row { this.label_row_anchors.entry(key).or_default() }
                    else { &mut this.label_picker_anchors[mode as usize] };
                if *anchor != bounds {
                    *anchor = bounds;
                    if active { cx.notify(); }
                }
            });
        }, |_, _, _, _| {}).absolute().inset_0()
    }
    pub(super) fn label_filter_controls(&self, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let p = Palette::current(cx);
        div().id("label-filter-bar").flex_shrink_0().flex().flex_wrap().items_center().gap_1()
            .px_3().py_1().border_b_1().border_color(p.line).bg(p.panel)
            .child(self.button("filter-labels", "", Command::TopLabelPicker(LabelMode::Filter), !self.busy, cx)
                .p_0().size(px(28.)).flex().items_center().justify_center().child(icons::icon("icons/label.svg", p.ink_dim))
                .relative().child(self.label_trigger_anchor(LabelMode::Filter, None, cx))
                .tooltip(|_, cx| cx.new(|_| Hint("Filter loaded Inbox by label / fuzzy find")).into()))
            .when(self.triage.label_filter.len() > 1, |d| d.child(div().text_xs().text_color(p.ink_dim).child("Any of")))
            .children(self.triage.label_filter.iter().map(|id| {
                let name = self.labels.entries.get(id).map(|label| label.name.clone()).unwrap_or_else(|| format!("Unavailable / {id}"));
                let group: SharedString = format!("label-filter-{id}").into();
                let hint = format!("Remove filter: {name}. Does not remove labels from emails.");
                div().group(group.clone()).flex().items_center().gap_1().px_1().rounded_sm().bg(p.accent.opacity(0.1))
                    .child(div().max_w(px(160.)).truncate().child(name))
                    .child(self.button(format!("remove-label-filter-{id}"), "", Command::RemoveLabelFilter(id.clone()), !self.busy, cx)
                        .p_0().size(px(20.)).rounded_full().flex().items_center().justify_center()
                        .child(icons::icon("icons/x.svg", p.ink_dim)).opacity(0.)
                        .group_hover(group, |s| s.opacity(1.)).focus(|s| s.opacity(1.))
                        .tooltip(move |_, cx| cx.new(|_| LabelHint(hint.clone())).into()))
            }))
    }
    pub(super) fn top_label_picker(&self, window: &Window, cx: &Context<Self>) -> gpui::Div {
        let p = Palette::current(cx);
        let Some(mode) = self.label_picker else { return div(); };
        let results = spaces::find(&self.labels, &self.label_query.read(cx).content);
        let selected = self.label_assignment_ids();
        let heading = if mode == LabelMode::Filter { "Filter labels / match any".into() }
            else if self.label_row_targets.is_some() { format!("Add label / {} messages", selected.len()) }
            else { format!("Add label / {} checked messages", selected.len()) };
        let anchor = self.label_row_targets.as_ref().and_then(|(key, _)| self.label_row_anchors.get(key)).copied()
            .unwrap_or(self.label_picker_anchors[mode as usize]);
        let quick = if mode == LabelMode::Assign { labels::quick_labels(&self.labels, &self.settings.label_usage, labels::usage_time()) } else { Vec::new() };
        let height = if quick.is_empty() { 340. } else {
            460_f32.min((f32::from(window.viewport_size().height - anchor.bottom()) - 14.).max(340.))
        };
        let bounds = picker_bounds(anchor, window.viewport_size(), height);
        let view = cx.weak_entity();
        let panel = div().id("floating-label-picker").absolute().left(bounds.left()).top(bounds.top())
            .w(bounds.size.width).max_h(bounds.size.height).overflow_y_scroll()
            .p_2().border_1().border_color(p.accent).rounded_md().shadow_lg().bg(p.panel).flex().flex_col().gap_2()
            .child(canvas(move |bounds, _, cx| { let _ = view.update(cx, |this, _| this.label_picker_bounds = bounds); }, |_, _, _, _| {}).absolute().inset_0())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(Self::label_picker_key))
            .child(div().flex().flex_shrink_0().items_center().gap_2()
                .child(div().flex_1().min_w_0().child(heading))
                .child(self.button("close-label-picker", "Close", Command::CloseLabelPicker, true, cx)
                    .bg(p.panel2.opacity(0.35))))
            .when(!quick.is_empty(), |d| d.child(div().flex().flex_col().flex_shrink_0().gap_1()
                .child(div().text_xs().text_color(p.ink_dim).child("Quick labels / score"))
                .child(div().flex().flex_wrap().gap_1().children(quick.iter().map(|(label, score)| {
                    let count = selected.iter().filter(|id| self.triage.emails.iter().any(|email| &email.id == *id && email.label_ids.contains(&label.id))).count();
                    let enabled = self.can_stage() && selected.len() > count && (self.demo || self.session.is_some());
                    let hint = format!("Add {} now. Score {score:.2}, {count}/{} already have label.", label.name, selected.len());
                    self.button(format!("quick-label-{}", label.id), "", Command::PickTopLabel(label.id.clone()), enabled, cx)
                        .w(px(((f32::from(bounds.size.width) - 24.) / 2.).max(0.))).min_w_0().text_xs()
                        .flex().items_center().gap_2().bg(p.panel2.opacity(0.35))
                        .child(div().flex_1().min_w_0().truncate().child(label.name.clone()))
                        .child(div().flex_shrink_0().text_color(p.ink_dim).child(format!("{score:.2}")))
                        .tooltip(move |_, cx| cx.new(|_| LabelHint(hint.clone())).into())
                })))))
            .child(self.label_query.clone())
            .child(div().text_xs().text_color(p.ink_dim).child(if mode == LabelMode::Assign { "Writes now. Keeps INBOX. Up/Down, Enter to choose." } else { "Combines with text filter. Up/Down, Enter to choose." }))
            .when(self.labels.unavailable, |d| d.child(div().text_color(p.danger).child("Labels unavailable. Cached filters still work; Refresh before assigning.")))
            .when(results.is_empty(), |d| d.child("No matching labels"))
            .children(results.iter().enumerate().skip(self.label_picker_cursor.saturating_sub(3)).take(4).map(|(index, label)| {
                let count = selected.iter().filter(|id| self.triage.emails.iter().any(|email| &email.id == *id && email.label_ids.contains(&label.id))).count();
                let checked = if mode == LabelMode::Filter { self.triage.label_filter.contains(&label.id) } else { !selected.is_empty() && count == selected.len() };
                let enabled = !self.busy && (mode == LabelMode::Filter || (self.can_stage() && !self.labels.unavailable && selected.len() > count && (self.demo || self.session.is_some())));
                self.button(format!("top-label-{}", label.id), "", Command::PickTopLabel(label.id.clone()), enabled, cx)
                    .bg(p.panel2.opacity(0.35))
                    .flex().flex_shrink_0().items_center().gap_2().w_full()
                    .child(icons::checkbox(checked, mode == LabelMode::Assign && count > 0 && !checked, p))
                    .child(div().flex_1().min_w_0().truncate().child(label.name.clone()))
                    .when(mode == LabelMode::Assign, |d| d.child(format!("{count}/{} have label", selected.len())))
                    .when(index == self.label_picker_cursor, |d| d.border_color(p.accent))
            }));
        div().absolute().inset_0().occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.command(Command::CloseLabelPicker, window, cx); }))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.command(Command::CloseLabelPicker, window, cx); }))
            .on_mouse_down(MouseButton::Middle, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.command(Command::CloseLabelPicker, window, cx); }))
            .child(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn floating_picker_is_anchored_and_clamped_to_viewport() {
        let anchor = Bounds::new(gpui::point(px(24.), px(90.)), size(px(28.), px(28.)));
        let bounds = picker_bounds(anchor, size(px(1100.), px(600.)), 340.);
        assert_eq!(bounds.left(), anchor.left());
        assert_eq!(bounds.top(), anchor.bottom() + px(6.));
        for (width, height) in [(1100., 600.), (1480., 850.), (320., 240.)] {
            let viewport = size(px(width), px(height));
            let edge = Bounds::new(gpui::point(px(width - 30.), px(height - 30.)), size(px(28.), px(28.)));
            let bounds = picker_bounds(edge, viewport, 460.);
            assert!(bounds.left() >= px(8.) && bounds.top() >= px(8.));
            assert!(bounds.right() <= viewport.width - px(8.) && bounds.bottom() <= viewport.height - px(8.));
        }
    }
}
