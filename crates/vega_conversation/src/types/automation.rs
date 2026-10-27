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
            Self::Goal => vega_i18n::t("domain.automation.goal"),
            Self::Loop => vega_i18n::t("domain.automation.loop"),
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
            Self::Active => vega_i18n::t("domain.automation.active"),
            Self::Paused => vega_i18n::t("domain.automation.paused"),
            Self::Blocked => vega_i18n::t("domain.automation.blocked"),
            Self::BudgetLimited => vega_i18n::t("domain.automation.budget_limited"),
            Self::Complete => vega_i18n::t("domain.automation.complete"),
            Self::Stopped => vega_i18n::t("domain.automation.stopped"),
            Self::Expired => vega_i18n::t("domain.automation.expired"),
        }
    }
    pub fn replaceable(self) -> bool {
        matches!(self, Self::Complete | Self::Stopped)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationReason {
    UserPaused,
    ThreadArchived,
    AppRestarted,
    PermissionReset,
    LoopExpired,
    RunIncomplete,
    UsageUnverified,
    BudgetExhausted,
    NoProgress,
    Stopped,
    StartFailed,
}

impl AutomationReason {
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "user_paused" => Some(Self::UserPaused),
            "thread_archived" => Some(Self::ThreadArchived),
            "app_restarted" => Some(Self::AppRestarted),
            "permission_reset" => Some(Self::PermissionReset),
            "loop_expired" => Some(Self::LoopExpired),
            "run_incomplete" => Some(Self::RunIncomplete),
            "usage_unverified" => Some(Self::UsageUnverified),
            "budget_exhausted" => Some(Self::BudgetExhausted),
            "no_progress" => Some(Self::NoProgress),
            "stopped" => Some(Self::Stopped),
            "start_failed" => Some(Self::StartFailed),
            _ => None,
        }
    }

    pub(crate) fn protocol_reason(self) -> &'static str {
        match self {
            Self::UserPaused => "已由你暂停",
            Self::ThreadArchived => "会话已归档",
            Self::AppRestarted => "应用重启后已暂停",
            Self::PermissionReset => "权限已重置；请检查任务后手动恢复",
            Self::LoopExpired => "循环已到期，恢复后将重新计时 7 天",
            Self::RunIncomplete => "本轮未完成，请检查会话中的错误或待处理事项后恢复",
            Self::UsageUnverified => "无法核实本轮完整用量，请清除后重新设置预算",
            Self::BudgetExhausted => "已达到 token 预算，后续回合已停止",
            Self::NoProgress => "连续三轮没有可核实的进展，请检查任务后恢复",
            Self::Stopped => "已停止，恢复任务后继续",
            Self::StartFailed => "自动任务未能开始，请检查模型和会话状态后恢复",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::UserPaused => vega_i18n::t("domain.automation.reason.user_paused"),
            Self::ThreadArchived => vega_i18n::t("domain.automation.reason.thread_archived"),
            Self::AppRestarted => vega_i18n::t("domain.automation.reason.app_restarted"),
            Self::PermissionReset => vega_i18n::t("domain.automation.reason.permission_reset"),
            Self::LoopExpired => vega_i18n::t("domain.automation.reason.loop_expired"),
            Self::RunIncomplete => vega_i18n::t("domain.automation.reason.run_incomplete"),
            Self::UsageUnverified => vega_i18n::t("domain.automation.reason.usage_unverified"),
            Self::BudgetExhausted => vega_i18n::t("domain.automation.reason.budget_exhausted"),
            Self::NoProgress => vega_i18n::t("domain.automation.reason.no_progress"),
            Self::Stopped => vega_i18n::t("domain.automation.reason.stopped"),
            Self::StartFailed => vega_i18n::t("domain.automation.reason.start_failed"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationTask {
    pub thread_id: String,
    pub kind: AutomationKind,
    pub identity: String,
    pub revision: u64,
    #[serde(default)]
    pub permission_authorization: Option<AutomationPermissionAuthorization>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_reason: Option<AutomationReason>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationPermissionAuthorization {
    pub kind: AutomationKind,
    pub identity: String,
    pub revision: u64,
    pub fence: super::PermissionFence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationPermissionChange {
    pub(crate) database: super::DatabaseIdentity,
    pub(crate) authorization: AutomationPermissionAuthorization,
    pub(crate) task: AutomationTask,
    pub(crate) resume: bool,
}

impl AutomationPermissionChange {
    pub fn task(&self) -> &AutomationTask {
        &self.task
    }
    pub fn resumes_task(&self) -> bool {
        self.resume
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationPermissionConfirmed {
    pub operation: u64,
    pub change: AutomationPermissionChange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationPermissionRequested {
    pub operation: u64,
    pub source: super::Thread,
    pub task: AutomationTask,
    pub resume: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationPermissionReloadRequested {
    pub operation: u64,
    pub thread_id: String,
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
    #[serde(default)]
    pub permission_fence: Option<super::PermissionFence>,
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
    pub fn reason_message(&self) -> Option<&str> {
        match self.system_reason {
            Some(reason) => Some(reason.message()),
            None => self.reason.as_deref(),
        }
    }

    pub fn is_user_paused(&self) -> bool {
        self.system_reason == Some(AutomationReason::UserPaused)
    }

    pub(crate) fn set_system_reason(&mut self, reason: AutomationReason) {
        self.reason = Some(reason.protocol_reason().into());
        self.system_reason = Some(reason);
    }

    pub(crate) fn set_pause_reason(&mut self, reason: &str) {
        self.system_reason = AutomationReason::from_code(reason);
        self.reason = Some(
            match self.system_reason {
                Some(reason) => reason.protocol_reason(),
                None => reason,
            }
            .into(),
        );
    }

    pub fn can_control(&self) -> bool {
        !self.status.replaceable()
            && self.status != AutomationStatus::BudgetLimited
            && !self
                .token_budget
                .is_some_and(|budget| self.used_tokens >= budget || !self.usage_complete)
    }
}
