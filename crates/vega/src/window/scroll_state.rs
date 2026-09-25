use std::collections::VecDeque;

use vega_conversation::types::ThreadScrollAnchor;

const THREAD_SCROLL_STATE_CAPACITY: usize = 64;

#[derive(Default)]
pub(crate) struct ThreadScrollStateLru {
    states: VecDeque<(String, ThreadScrollAnchor)>,
}

impl ThreadScrollStateLru {
    pub(crate) fn remember(&mut self, thread_id: String, anchor: ThreadScrollAnchor) {
        if let Some(index) = self
            .states
            .iter()
            .position(|(cached_thread_id, _)| cached_thread_id == &thread_id)
        {
            self.states.remove(index);
        }
        self.states.push_back((thread_id, anchor));
        if self.states.len() > THREAD_SCROLL_STATE_CAPACITY {
            self.states.pop_front();
        }
    }

    pub(crate) fn get(&mut self, thread_id: &str) -> Option<ThreadScrollAnchor> {
        let index = self
            .states
            .iter()
            .position(|(cached_thread_id, _)| cached_thread_id == thread_id)?;
        let state = self.states.remove(index)?;
        let anchor = state.1.clone();
        self.states.push_back(state);
        Some(anchor)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(message_id: &str) -> ThreadScrollAnchor {
        ThreadScrollAnchor {
            identity: Some(format!("identity-{message_id}")),
            message_id: Some(message_id.to_string()),
            offset_in_item_px: 7.,
            following_tail: false,
        }
    }

    #[test]
    fn keeps_only_the_64_most_recent_threads_and_refreshes_recency_on_read() {
        let mut cache = ThreadScrollStateLru::default();
        for index in 0..64 {
            cache.remember(
                format!("thread-{index}"),
                anchor(&format!("message-{index}")),
            );
        }
        assert_eq!(cache.len(), 64);
        assert_eq!(
            cache.get("thread-0").and_then(|state| state.message_id),
            Some("message-0".to_string())
        );
        cache.remember("thread-64".into(), anchor("message-64"));
        assert_eq!(cache.len(), 64);
        assert!(cache.get("thread-1").is_none());
        assert_eq!(
            cache.get("thread-0").and_then(|state| state.message_id),
            Some("message-0".to_string())
        );
        assert_eq!(
            cache.get("thread-64").and_then(|state| state.message_id),
            Some("message-64".to_string())
        );
    }

    #[test]
    fn updating_a_thread_replaces_its_anchor_without_adding_an_entry() {
        let mut cache = ThreadScrollStateLru::default();
        cache.remember("thread-a".into(), anchor("message-old"));
        cache.remember("thread-b".into(), anchor("message-b"));
        cache.remember("thread-a".into(), anchor("message-new"));
        assert_eq!(cache.len(), 2);
        assert_eq!(
            cache.get("thread-a").and_then(|state| state.message_id),
            Some("message-new".to_string())
        );
    }
}
