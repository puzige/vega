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
}
