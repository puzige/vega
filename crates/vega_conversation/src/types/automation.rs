use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationKind {
    Goal,
    Loop,
}

impl AutomationKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Loop => "loop",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Goal => "持续目标",
            Self::Loop => "循环任务",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStatus {
    Active,
    Paused,
    Blocked,
    BudgetLimited,
    Complete,
    Stopped,
    Expired,
}

impl AutomationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "进行中",
            Self::Paused => "已暂停",
            Self::Blocked => "受阻",
            Self::BudgetLimited => "预算已用完",
            Self::Complete => "已完成",
            Self::Stopped => "已停止",
            Self::Expired => "已到期",
        }
    }
    pub fn replaceable(self) -> bool {
        matches!(self, Self::Complete | Self::Stopped)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationTask {
    pub thread_id: String,
    pub kind: AutomationKind,
    pub identity: String,
    pub revision: u64,
    pub instruction: String,
    pub status: AutomationStatus,
    pub token_budget: Option<u64>,
    pub used_tokens: u64,
    pub usage_complete: bool,
    pub elapsed_ms: u64,
    pub interval_seconds: Option<u64>,
    pub next_due_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub no_progress: u8,
    pub blocker_count: u8,
    pub blocker: Option<String>,
    pub reason: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutomationProjection {
    pub tasks: Vec<AutomationTask>,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationControl {
    Pause,
    Resume,
    Clear,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutomationCommand {
    Edit {
        kind: AutomationKind,
        identity: String,
        revision: u64,
        instruction: String,
    },
    Status(AutomationKind),
    Control(AutomationKind, AutomationControl),
    Create {
        kind: AutomationKind,
        instruction: String,
        token_budget: Option<u64>,
        interval_seconds: Option<u64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRunClaim {
    pub id: String,
    pub thread_id: String,
    pub source: Option<AutomationKind>,
    #[serde(default)]
    pub started: bool,
    pub tasks: Vec<AutomationTask>,
    pub usage_ids: Vec<i64>,
    pub tokens: u64,
    pub goal_report: Option<GoalReport>,
    pub wakeup: Option<WakeupReport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalReport {
    pub status: String,
    #[serde(default)]
    pub progress: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub blocker: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WakeupReport {
    #[serde(default)]
    pub delay_seconds: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub noop: bool,
    #[serde(default)]
    pub stop: bool,
}

impl AutomationTask {
    pub fn can_control(&self) -> bool {
        !self.status.replaceable()
            && self.status != AutomationStatus::BudgetLimited
            && !self
                .token_budget
                .is_some_and(|budget| self.used_tokens >= budget || !self.usage_complete)
    }
}
