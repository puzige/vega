use crate::nsh::ProjectionId;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct Draft(pub Arc<Mutex<Value>>);

#[derive(Default)]
pub(super) struct Value {
    pub text: String,
    pub revision: u64,
    pub overflow: bool,
    pub composing: bool,
    pub editable: bool,
    pub page: Option<ProjectionId>,
    pub mount: std::sync::Weak<()>,
    pub visible: Arc<std::sync::atomic::AtomicBool>,
}

impl Value {
    pub fn replace(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        match self.revision.checked_add(1) {
            Some(revision) => self.revision = revision,
            None => self.overflow = true,
        }
        self.text.clear();
        self.text.push_str(text);
    }

    pub fn owns(&self, page: &ProjectionId) -> bool {
        self.page.as_ref().is_some_and(|owner| owner.matches(page))
            && self.mount.upgrade().is_some()
            && self.visible.load(std::sync::atomic::Ordering::Acquire)
    }
}

pub(super) fn finish_generated(
    model: &mut super::CommitPanelModel,
    token: &mut Option<(super::CommitOperationId, u64)>,
    value: &mut Value,
    operation: super::CommitOperationId,
    revision_current: bool,
    result: Result<String, super::CommitErrorCode>,
) -> bool {
    if !model.owns_pending(operation) || model.stage() != super::CommitPanelStage::Drafting {
        return false;
    }
    let unchanged = revision_current
        && !value.overflow
        && !value.composing
        && *token == Some((operation, value.revision));
    let text = result.as_ref().ok();
    let outcome = if unchanged {
        result.as_ref().map(|_| ()).map_err(|code| *code)
    } else {
        Err(super::CommitErrorCode::ChangedDuringRead)
    };
    if !model.finish_draft(operation, outcome) {
        return false;
    }
    *token = None;
    if unchanged && let Some(text) = text {
        value.replace(text);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn edits_keep_full_multiline_text_and_invalidate_generated_revision() {
        let mut draft = Value::default();
        draft.replace("subject\n\nbody\n");
        let generated_at = draft.revision;
        draft.replace("subject\n\nnewer body\n");
        assert_ne!(draft.revision, generated_at);
        assert_eq!(draft.text, "subject\n\nnewer body\n");
        let current = draft.revision;
        draft.replace("subject\n\nnewer body\n");
        assert_eq!(draft.revision, current);
    }

    #[test]
    fn hidden_or_retired_mount_rejects_retained_editor_callbacks() {
        use crate::nsh::ProjectionId;
        use std::sync::Arc;
        use std::sync::atomic::Ordering;

        let page = ProjectionId::new();
        let mount = Arc::new(());
        let draft = Value {
            page: Some(page.clone()),
            mount: Arc::downgrade(&mount),
            ..Value::default()
        };
        assert!(!draft.owns(&page));
        draft.visible.store(true, Ordering::Release);
        assert!(draft.owns(&page));
        assert!(!draft.owns(&ProjectionId::new()));
        drop(mount);
        assert!(!draft.owns(&page));
    }

    #[test]
    fn revision_exhaustion_never_revalidates_an_old_draft() {
        let mut draft = Value {
            revision: u64::MAX,
            ..Value::default()
        };
        draft.replace("new text");
        assert!(draft.overflow);
        assert_eq!(draft.revision, u64::MAX);
        assert_eq!(draft.text, "new text");
    }
}
