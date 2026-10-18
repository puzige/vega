use super::*;
use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentState {
    Running,
    Cancelling,
    Completed,
    Failed,
    Interrupted,
}

impl SubagentState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Interrupted)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "cancelling" => Some(Self::Cancelling),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentSnapshot {
    pub agent_id: String,
    pub parent_thread_id: String,
    pub parent_message_id: String,
    pub message_id: String,
    pub name: String,
    pub state: SubagentState,
    pub result: String,
    pub truncated: bool,
}

impl std::fmt::Debug for SubagentSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubagentSnapshot")
            .field("state", &self.state)
            .field("result_bytes", &self.result.len())
            .finish_non_exhaustive()
    }
}

pub struct SubagentAdmission {
    pub thread: Thread,
    pub snapshot: SubagentSnapshot,
    pub cancel: CancellationToken,
    pub permission_queue: crate::agent::PermissionQueue,
    pub user_questions: crate::agent::UserQuestionQueue,
    pub task: String,
    pub queue_generation: String,
}

pub trait SubagentHost: Send + Sync {
    fn admit(
        &self,
        admission: SubagentAdmission,
    ) -> BoxFuture<'static, Result<(), vega_runtime::VegaError>>;
    fn event(
        &self,
        thread_id: &str,
        message_id: &str,
        event: &ConversationEvent,
    ) -> Result<(), vega_runtime::VegaError>;
    fn updated(&self, snapshot: SubagentSnapshot) -> Result<(), vega_runtime::VegaError>;
    fn finished(
        &self,
        snapshot: SubagentSnapshot,
        usage_complete: bool,
    ) -> Result<(), vega_runtime::VegaError>;
}

#[derive(Clone, Debug)]
pub struct SubagentOpenRequested {
    pub source_thread_id: String,
    pub thread_id: String,
}

#[derive(Clone, Debug)]
pub struct SubagentStopRequested {
    pub source_thread_id: String,
    pub thread_id: String,
    pub message_id: String,
}
