use super::CallbackIdentity;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskContext {
    task: String,
    generation: CallbackIdentity,
}

impl TaskContext {
    pub(crate) fn new(task: &str) -> Self {
        Self {
            task: task.to_owned(),
            generation: CallbackIdentity::default(),
        }
    }
}
