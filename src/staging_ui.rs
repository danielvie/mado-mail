use super::*;

pub(super) fn tray_height(available: f32) -> f32 {
    200_f32.min((available * 0.4).max(0.))
}

impl MailApp {
    fn staging_actions(&self) -> Vec<Action> {
        let mut actions = vec![Action::Archive, Action::Trash];
        actions.extend(self.settings.spaces.iter().map(|space| Action::Space(space.id)));
        for mark in self.triage.marks.values() {
            if !actions.contains(&mark.action) { actions.push(mark.action); }
        }
        actions
    }
    fn staging_target(&self, action: Action, cx: &Context<Self>) -> gpui::Stateful<gpui::Div> {
        let p = Palette::current(cx);
        let count = self.triage.marks.values().filter(|mark| mark.action == action).count();
        let view = cx.weak_entity();
        let measured = cx.weak_entity();
        let color = if action == Action::Trash { p.danger } else { p.accent };
        let immediate = self.settings.auto_apply && !matches!(action, Action::Space(_));
        let name = self.action_name(action);
        let hint = format!("{name}. {}", if immediate { "Dropping here writes to Gmail immediately. Auto-apply is on." } else { "Drop messages to stage this destination. Apply submits them." });
        div().id(SharedString::from(format!("staging-target-{action:?}")))
            .relative().flex_shrink_0().max_w(px(190.)).flex().items_center().gap_1().px_2().py_1()
            .rounded_sm().border_1().border_color(p.line).text_color(color)
            .child(div().min_w_0().max_w(px(140.)).truncate().child(name))
            .child(count.to_string())
            .child(canvas(move |bounds, _, cx| {
                let _ = measured.update(cx, |app, _| {
                    if let Some((_, existing)) = app.staging_targets.iter_mut().find(|(a, _)| *a == action) { *existing = bounds; }
                    else { app.staging_targets.push((action, bounds)); }
                });
            }, |_, _, _, _| {}).absolute().inset_0())
            .on_drag_move::<MailDrag>(cx.listener(move |app, event: &gpui::DragMoveEvent<MailDrag>, _, cx| {
                let next = event.bounds.contains(&event.event.position).then_some(action);
                if next.is_some() || app.drop_target == Some(action) {
                    if app.drop_target != next { app.drop_target = next; cx.notify(); }
                }
            }))
            .can_drop(move |value, _, cx| value.downcast_ref::<MailDrag>().is_some_and(|drag| view.read_with(cx, |app, _| {
                app.can_stage_action(action) && !app.rules_view && !app.triage.undecided_ids(&drag.ids).is_empty()
            }).unwrap_or(false)))
            .on_drop(cx.listener(move |app, drag: &MailDrag, window, cx| {
                if app.can_stage_action(action) && !app.rules_view { app.command(Command::Stage(drag.ids.clone(), action), window, cx); }
                app.mail_drag = None; app.drop_target = None; cx.stop_propagation(); cx.notify();
            }))
            .when(self.can_stage_action(action), |d| d.drag_over::<MailDrag>(move |s, _, _, _| s.border_color(color).bg(color.opacity(0.12))))
            .tooltip(move |_, cx| { let text = hint.clone(); cx.new(move |_| LabelHint(text)).into() })
    }
    pub(super) fn staging_counts(&self) -> (usize, usize, usize) {
        let failed = self.triage.marks.values().filter(|mark| matches!(mark.outcome, Some(Outcome::Failed(_)))).count();
        let unknown = self.triage.marks.values().filter(|mark| matches!(mark.outcome, Some(Outcome::Unknown(_)))).count();
        (self.triage.marks.len(), failed, unknown)
    }
    pub(super) fn staging_can_apply(&self) -> bool { self.can_stage() && !self.triage.marks.is_empty() }
    pub(super) fn staging_line(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let (pending, failed, unknown) = self.staging_counts();
        let measured = cx.weak_entity();
        div().id("staging-line").relative().h(px(48.)).flex_shrink_0().w_full().flex().items_center().gap_2().px_3()
            .border_t_1().border_color(p.line).bg(p.panel)
            .child(canvas(move |bounds, _, cx| { let _ = measured.update(cx, |app, _| app.staging_line_bounds = bounds); }, |_, _, _, _| {}).absolute().inset_0())
            .child(format!("{pending} pending"))
            .when(failed > 0, |d| d.child(div().text_color(p.danger).child(format!("{failed} failed"))))
            .when(unknown > 0, |d| d.child(div().text_color(p.danger).child(format!("{unknown} unknown"))))
            .child(div().id("staging-destinations").flex_1().min_w_0().overflow_x_scroll().flex().items_center().gap_2()
                .children(self.staging_actions().into_iter().map(|action| self.staging_target(action, cx))))
            .when(unknown > 0, |d| d.child(self.button("recheck", "Recheck Gmail", Command::Recheck, !self.busy && !self.blocked, cx)))
            .child(self.button("toggle-staging", if self.sidebar_visible { "Collapse" } else { "Review" }, Command::ToggleSidebar, !self.rules_view, cx))
            .child(self.button("apply", format!("Apply {pending}"), Command::Apply, self.staging_can_apply(), cx))
    }
    pub(super) fn staging_review(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::current(cx);
        let view = cx.weak_entity();
        let mut contents = div().id("staging-review-scroll").overflow_y_scroll().track_scroll(&self.staging_scroll)
            .flex_1().min_h_0().flex().flex_col();
        if self.space_draft.is_some() { contents = contents.child(self.space_editor(cx)); }
        for action in self.staging_actions() {
            let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (id, mark) in &self.triage.marks {
                if mark.action != action { continue; }
                let sender = self.triage.emails.iter().find(|email| &email.id == id)
                    .map(|email| sender_address(&email.from)).unwrap_or_else(|| "Recovered messages".into());
                groups.entry(sender).or_default().push(id.clone());
            }
            if let Action::Space(id) = action {
                let editing = self.space_draft.as_ref().is_some_and(|draft| draft.id == Some(id));
                if !editing {
                    contents = contents.child(div().flex_shrink_0().border_b_1().border_color(p.line).px_2().py_1()
                        .child(div().flex().items_center().gap_2()
                            .child(self.action_name(action))
                            .child(self.button(format!("remove-space-{id}"), "Remove space", Command::RemoveSpace(id), self.can_remove_space(id), cx)))
                        .child(self.space_details(id, cx)));
                }
            }
            for (sender, ids) in groups {
                let emails: Vec<_> = self.triage.emails.iter().filter(|email| ids.contains(&email.id)).collect();
                let mut name = emails.first().map(|email| sender_name(&email.from)).unwrap_or(&sender).to_owned();
                if emails.first().is_some_and(|email| rule_for(&self.settings.rules, &email.from).is_some()) { name.push_str(" / Rule"); }
                let unknown = ids.iter().any(|id| matches!(self.triage.marks[id].outcome, Some(Outcome::Unknown(_))));
                let outcomes: BTreeSet<_> = ids.iter().filter_map(|id| match &self.triage.marks[id].outcome {
                    Some(Outcome::Unknown(reason)) => Some(format!("Unknown: {reason}")),
                    Some(Outcome::Failed(reason)) => Some(format!("Failed: {reason}")),
                    _ => None,
                }).collect();
                let outcome = if outcomes.is_empty() { "Pending, not submitted".into() } else { outcomes.into_iter().collect::<Vec<_>>().join("; ") };
                let outcome_color = if ids.iter().any(|id| self.triage.marks[id].outcome.is_some()) { p.danger } else { p.ink_dim };
                contents = contents.child(div().id(SharedString::from(format!("review-{action:?}-{sender}")))
                    .min_h(px(36.)).flex_shrink_0().flex().items_center().gap_2().px_3().py_1().border_b_1().border_color(p.line)
                    .child(div().w(px(40.)).child(ids.len().to_string()))
                    .child(div().w(px(180.)).min_w_0().truncate().child(self.button(format!("peek-{action:?}-{sender}"), name,
                        Command::Peek(sender, action), !emails.is_empty(), cx)))
                    .child(div().w(px(140.)).truncate().child(self.action_name(action)))
                    .child(div().flex_1().min_w_0().whitespace_normal().text_xs().text_color(outcome_color).child(outcome))
                    .child(self.button(format!("back-{action:?}-{}", ids[0]), "Put back", Command::PutBack(ids), !self.busy && !unknown, cx)
                        .tooltip(|_, cx| cx.new(|_| Hint("Cancel pending decisions only. This does not reverse Gmail writes; unknown outcomes require Recheck Gmail.")).into())));
            }
        }
        if self.triage.marks.is_empty() && self.settings.spaces.is_empty() && self.space_draft.is_none() {
            contents = contents.child(div().p_3().text_color(p.ink_dim).child("No pending decisions. Drop messages on a destination below to stage them."));
        }
        div().id("staging-review").relative().h(px(tray_height(self.workspace_bounds.size.height.into()))).flex_shrink_0()
            .w_full().flex().flex_col().bg(p.panel).border_t_1().border_color(p.line)
            .child(canvas(move |bounds, _, cx| { let _ = view.update(cx, |app, _| app.bin_bounds = bounds); }, |_, _, _, _| {}).absolute().inset_0())
            .child(div().h(px(36.)).flex_shrink_0().flex().items_center().gap_2().px_3().border_b_1().border_color(p.line)
                .child("Review pending decisions")
                .child(self.button("new-space", "+ New space", Command::NewSpace, !self.busy && !self.blocked && self.space_draft.is_none() && !self.triage.has_unknown(), cx))
                .child(self.button("empty", "Put back all", Command::Empty, !self.busy && self.triage.marks.values().any(|mark| !matches!(mark.outcome, Some(Outcome::Unknown(_)))), cx))
                .child(div().flex_1())
                .child(div().text_xs().text_color(p.ink_dim).child("Put back cancels pending decisions, not Gmail writes.")))
            .child(contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shallow_tray_preserves_reading_space_in_small_windows() {
        assert_eq!(tray_height(800.), 200.);
        assert_eq!(tray_height(400.), 160.);
        assert_eq!(tray_height(0.), 0.);
        assert_eq!(tray_height(-10.), 0.);
    }
}
