use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

use crate::nsh::ProjectionId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::settings) struct DraftValues {
    pub(super) text: [String; 3],
    pub(super) composing: [bool; 4],
    pub(super) secret_present: bool,
    pub(super) secret_revision: u64,
    pub(super) revision: u64,
    pub(super) frozen: bool,
    pub(super) valid: bool,
}

#[derive(Clone)]
pub(in crate::settings) struct Draft {
    pub(super) session: ProjectionId,
    pub(super) key: u64,
    values: Arc<Mutex<DraftValues>>,
}

impl Draft {
    pub(super) fn new(text: [String; 3]) -> Self {
        Self {
            session: ProjectionId::new(),
            key: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            values: Arc::new(Mutex::new(DraftValues {
                text,
                composing: [false; 4],
                secret_present: false,
                secret_revision: 0,
                revision: 0,
                frozen: false,
                valid: true,
            })),
        }
    }

    pub(in crate::settings) fn read(&self) -> DraftValues {
        self.values
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(super) fn edit(&self, index: usize, text: Option<&str>, composing: Option<bool>) -> bool {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !values.valid || values.frozen || index >= 4 {
            return false;
        }
        if let Some(text) = text {
            if index == 3 {
                values.secret_present = !text.is_empty();
                values.secret_revision = values.secret_revision.wrapping_add(1);
            } else {
                values.text[index] = text.into();
            }
        }
        if let Some(composing) = composing {
            values.composing[index] = composing;
        }
        values.revision = values.revision.wrapping_add(1);
        true
    }

    pub(super) fn claim(&self, observed: &DraftValues) -> Option<[String; 3]> {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !values.valid
            || values.frozen
            || values.composing.contains(&true)
            || *values != *observed
        {
            return None;
        }
        values.frozen = true;
        Some(values.text.clone())
    }

    pub(super) fn consume_secret(&self) {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        values.secret_present = false;
    }

    pub(super) fn release(&self) {
        self.values
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .frozen = false;
    }

    pub(super) fn replace_capacity(&self, capacity: &[String; 2]) -> bool {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !values.valid || values.frozen || values.composing.contains(&true) {
            return false;
        }
        values.text[1] = capacity[0].clone();
        values.text[2] = capacity[1].clone();
        values.revision = values.revision.wrapping_add(1);
        true
    }

    pub(super) fn replace_observed(&self, stamp: &DraftStamp, index: usize, text: String) -> bool {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.session.matches(&stamp.session)
            || *values != stamp.values
            || !values.valid
            || values.frozen
            || values.composing.contains(&true)
            || index >= 3
        {
            return false;
        }
        values.text[index] = text;
        values.revision = values.revision.wrapping_add(1);
        true
    }

    pub(super) fn retire(&self) {
        self.values
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .valid = false;
    }

    pub(super) fn invalidate(&self, observed: &DraftValues) -> bool {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !values.valid
            || values.frozen
            || values.composing.contains(&true)
            || *values != *observed
        {
            return false;
        }
        values.valid = false;
        true
    }
}

#[derive(Clone)]
pub(in crate::settings) struct DraftStamp {
    session: ProjectionId,
    values: DraftValues,
}
impl PartialEq for DraftStamp {
    fn eq(&self, other: &Self) -> bool {
        self.session.matches(&other.session) && self.values == other.values
    }
}
impl Eq for DraftStamp {}
impl DraftStamp {
    #[cfg(test)]
    pub(in crate::settings) fn development_fixture() -> Self {
        Draft::new(["model".into(), "80".into(), "20".into()]).stamp()
    }

    #[cfg(test)]
    pub(in crate::settings) fn development_change(&mut self, composing: bool) {
        self.values.revision += 1;
        self.values.composing[0] = composing;
    }

    pub(in crate::settings) fn composing(&self) -> bool {
        !self.values.valid || self.values.frozen || self.values.composing.contains(&true)
    }
}
impl Draft {
    pub(super) fn invalidate_stamp(&self, stamp: &DraftStamp) -> bool {
        self.session.matches(&stamp.session) && self.invalidate(&stamp.values)
    }

    pub(in crate::settings) fn stamp(&self) -> DraftStamp {
        DraftStamp {
            session: self.session.clone(),
            values: self.read(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_input_and_composition_block_observed_submission() {
        let draft = Draft::new(["original".into(), String::new(), String::new()]);
        let old = draft.read();
        assert!(draft.edit(0, Some("newer"), None));
        assert!(draft.claim(&old).is_none());
        assert!(draft.edit(0, None, Some(true)));
        assert!(draft.claim(&draft.read()).is_none());
        assert!(draft.edit(0, None, Some(false)));
        let values = draft.claim(&draft.read()).unwrap();
        assert_eq!(values[0], "newer");
        assert!(!draft.edit(0, Some("overwrite"), None));
        draft.release();
        assert!(draft.edit(0, Some("recovered"), None));
        assert_eq!(draft.read().text[0], "recovered");
    }

    #[test]
    fn secret_submission_keeps_only_presence_and_revision() {
        let draft = Draft::new(Default::default());
        assert!(draft.edit(3, Some("one-time-secret"), None));
        let before = draft.read();
        assert!(before.secret_present);
        assert_eq!(before.text, [String::new(), String::new(), String::new()]);
        assert!(draft.claim(&before).is_some());
        draft.consume_secret();
        draft.release();
        assert!(!draft.read().secret_present);
        assert_eq!(draft.read().secret_revision, before.secret_revision);
        assert!(draft.edit(3, Some("replacement"), None));
        assert_ne!(draft.read().secret_revision, before.secret_revision);
        assert!(draft.claim(&before).is_none());
    }

    #[test]
    fn discard_invalidates_only_the_observed_session_and_cannot_be_reused() {
        let draft = Draft::new(Default::default());
        let old = draft.stamp();
        assert!(draft.edit(1, Some("latest"), None));
        assert!(!draft.invalidate_stamp(&old));
        assert!(draft.invalidate_stamp(&draft.stamp()));
        assert!(!draft.edit(1, Some("stale"), None));
        assert!(draft.claim(&draft.read()).is_none());
        let replacement = Draft::new(Default::default());
        assert!(!replacement.session.matches(&draft.session));
        assert!(replacement.edit(0, Some("fresh"), None));
    }
}
