use super::*;

const HISTORY_LIMIT: usize = 128;

#[derive(Clone)]
pub(super) struct EditorSnapshot {
    content: SharedString,
    mentions: Vec<Mention>,
    skill_mentions: Vec<SkillMention>,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

impl TextInput {
    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            content: self.content.clone(),
            mentions: self.mentions.clone(),
            skill_mentions: self.skill_mentions(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
        }
    }

    pub(super) fn record_edit(&mut self) {
        if self.composition_history_active {
            return;
        }
        let snapshot = self.snapshot();
        if self.undo_history.len() == HISTORY_LIMIT {
            self.undo_history.pop_front();
        }
        self.undo_history.push_back(snapshot);
        self.redo_history.clear();
    }

    fn restore_snapshot(&mut self, snapshot: EditorSnapshot, cx: &mut Context<Self>) {
        self.content = snapshot.content;
        self.mentions = snapshot.mentions;
        self.skill_mentions = snapshot.skill_mentions;
        self.selected_range = snapshot.selected_range;
        self.selection_reversed = snapshot.selection_reversed;
        self.marked_range = None;
        self.composition_history_active = false;
        self.prediction = None;
        self.mouse_selection = None;
        self.vertical_goal = None;
        self.editor_scroll = None;
        self.scroll_handle.reset();
        self.content_revision = self.content_revision.wrapping_add(1);
        self.update_logical_viewport();
        cx.notify();
    }

    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_composing() {
            cx.propagate();
            return;
        }
        if let Some(snapshot) = self.undo_history.pop_back() {
            self.redo_history.push_back(self.snapshot());
            self.restore_snapshot(snapshot, cx);
        }
    }

    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_composing() {
            cx.propagate();
            return;
        }
        if let Some(snapshot) = self.redo_history.pop_back() {
            self.undo_history.push_back(self.snapshot());
            self.restore_snapshot(snapshot, cx);
        }
    }
}
