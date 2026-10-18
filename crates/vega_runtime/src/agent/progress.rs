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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatRole, project_model_messages};

    fn has_reminder(message: &ChatMessage) -> bool {
        project_model_messages(std::slice::from_ref(message)).unwrap()[0]
            .content
            .contains("<vega_runtime_notice>")
    }

    #[tokio::test(start_paused = true)]
    async fn reminder_is_initial_then_throttled_by_visible_updates_and_reminders() {
        let mut reporter = ProgressReporter::new();
        let mut first = ChatMessage::tool_result("first", "observed");
        reporter.after_tool_batch(Some(&mut first));
        assert!(has_reminder(&first));
        tokio::time::advance(Duration::from_secs(59)).await;
        let mut early = ChatMessage::tool_result("early", "observed");
        reporter.after_tool_batch(Some(&mut early));
        assert!(!has_reminder(&early));
        tokio::time::advance(Duration::from_secs(1)).await;
        let mut due = ChatMessage::tool_result("due", "observed");
        reporter.after_tool_batch(Some(&mut due));
        assert!(has_reminder(&due));
        tokio::time::advance(Duration::from_secs(59)).await;
        reporter.observe_visible(Some(Instant::now()));
        tokio::time::advance(Duration::from_secs(1)).await;
        let mut visible = ChatMessage::tool_result("visible", "observed");
        reporter.after_tool_batch(Some(&mut visible));
        assert!(!has_reminder(&visible));
        assert_eq!(first.content, "observed");
    }

    #[tokio::test(start_paused = true)]
    async fn non_tool_boundaries_do_not_consume_the_initial_reminder() {
        let mut reporter = ProgressReporter::new();
        let mut user = ChatMessage::new(ChatRole::User, "new request");
        reporter.after_tool_batch(Some(&mut user));
        reporter.after_tool_batch(None);
        assert_eq!(user.content, "new request");
        let mut tool = ChatMessage::tool_result("tool", "observed");
        reporter.after_tool_batch(Some(&mut tool));
        assert!(has_reminder(&tool));
    }
}
