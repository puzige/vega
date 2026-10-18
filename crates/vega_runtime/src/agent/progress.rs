use std::time::Duration;

use tokio::time::Instant;

use crate::ChatMessage;

const REMINDER_INTERVAL: Duration = Duration::from_secs(60);

pub(super) struct ProgressReporter {
    last_visible: Instant,
    last_reminder: Option<Instant>,
    initial_pending: bool,
}

impl ProgressReporter {
    pub(super) fn new() -> Self {
        Self {
            last_visible: Instant::now(),
            last_reminder: None,
            initial_pending: true,
        }
    }

    pub(super) fn observe_visible(&mut self, visible: Option<Instant>) {
        if let Some(visible) = visible {
            self.last_visible = visible;
            self.initial_pending = false;
        }
    }

    pub(super) fn after_tool_batch(&mut self, last: Option<&mut ChatMessage>) {
        let now = Instant::now();
        let last_update = self.last_reminder.map_or(self.last_visible, |reminder| {
            reminder.max(self.last_visible)
        });
        if !self.initial_pending && now.duration_since(last_update) < REMINDER_INTERVAL {
            return;
        }
        if last.is_some_and(ChatMessage::attach_progress_reminder) {
            self.initial_pending = false;
            self.last_reminder = Some(now);
        }
    }
}
