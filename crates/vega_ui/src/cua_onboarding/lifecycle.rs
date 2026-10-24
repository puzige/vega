use super::CallbackIdentity;
use super::retention::{PrepareEntryKind, retained_entry};
use vega_conversation::types::ComputerUseError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskContext {
    task: String,
    generation: CallbackIdentity,
}

#[derive(Default)]
pub struct EntryLifecycle {
    current: Option<TaskContext>,
    retained: Option<PrepareEntryKind>,
    dismissed: bool,
}

impl EntryLifecycle {
    pub fn begin_task(&mut self, task: &str) -> TaskContext {
        let context = TaskContext {
            task: task.to_owned(),
            generation: CallbackIdentity::default(),
        };
        self.current = Some(context.clone());
        self.retained = None;
        self.dismissed = false;
        context
    }

    pub fn observe(&mut self, context: &TaskContext, error: ComputerUseError) {
        if self.current.as_ref() != Some(context) {
            return;
        }
        if let Some(entry) = retained_entry(&error, self.dismissed) {
            self.retained = Some(entry);
        }
    }

    pub fn detach(&mut self, context: &TaskContext, outgoing_error: Option<ComputerUseError>) {
        if let Some(error) = outgoing_error {
            self.observe(context, error);
        }
    }

    pub fn dismiss(&mut self, context: &TaskContext) {
        if self.current.as_ref() == Some(context) {
            self.dismissed = true;
            self.retained = None;
        }
    }

    pub fn visible(&self) -> Option<PrepareEntryKind> {
        self.retained
    }
}
