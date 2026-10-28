//! Durable sidebar organization contracts shared by the service and UI.
use super::Thread;
use serde::{Deserialize, Serialize};

/// Sidebar organization mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebarView {
    #[default]
    Projects,
    Groups,
}
/// Project projection mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebarProjectView {
    #[default]
    ByProject,
    Timeline,
}
/// Descending task timestamp used by project and timeline projections.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebarTaskSort {
    Updated,
    Created,
    #[default]
    Manual,
}
/// Semantic group color; UI maps these to theme tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebarGroupColor {
    #[default]
    Gray,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
}
/// Persisted projection preferences, independent of navigation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarPreferences {
    pub view: SidebarView,
    pub project_view: SidebarProjectView,
    pub sort: SidebarTaskSort,
    #[serde(default)]
    pub manual_thread_order: Vec<String>,
    #[serde(default)]
    pub hide_project_threads_from_recents: bool,
}
/// Local-calendar timeline section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebarTimelineBucket {
    Today,
    Yesterday,
    Last7Days,
    Last30Days,
    Earlier,
}
/// Durable collapse identity, scoped to the relevant projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebarCollapseTarget {
    Project(String),
    Group(String),
    Timeline(SidebarTimelineBucket),
    Pinned,
    Ungrouped,
}
/// Registered project metadata; no filesystem reads are performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarProject {
    pub id: String,
    pub name: String,
    pub path: String,
    pub git_default_branch: Option<String>,
    pub created_at: i64,
    pub last_opened_at: i64,
}
/// A named group in manual order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarGroup {
    pub id: String,
    pub name: String,
    pub color: SidebarGroupColor,
}
/// Membership entries appear in group-local manual order, including archived tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarMembership {
    pub thread_id: String,
    pub group_id: String,
}
/// Consistent metadata-only read, including archived tasks for restoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarOrganizationSnapshot {
    pub revision: u64,
    pub projects: Vec<SidebarProject>,
    pub threads: Vec<Thread>,
    pub groups: Vec<SidebarGroup>,
    pub memberships: Vec<SidebarMembership>,
    pub project_order: Vec<String>,
    pub preferences: SidebarPreferences,
    pub collapsed: Vec<SidebarCollapseTarget>,
}
/// Revision-checked mutation. `before_id: None` appends; self-moves are rejected.
#[derive(Debug, Clone)]
pub enum SidebarOrganizationAction {
    CreateGroup {
        name: String,
        color: SidebarGroupColor,
    },
    RenameGroup {
        group_id: String,
        name: String,
    },
    SetGroupColor {
        group_id: String,
        color: SidebarGroupColor,
    },
    DissolveGroup {
        group_id: String,
    },
    MoveGroup {
        group_id: String,
        before_id: Option<String>,
    },
    MoveProject {
        project_id: String,
        before_id: Option<String>,
    },
    ReorderThread {
        thread_id: String,
        before_id: Option<String>,
    },
    MoveThread {
        thread_id: String,
        group_id: Option<String>,
        before_id: Option<String>,
    },
    /// Reveal the actual registered folder without changing custom groups or ordering.
    RevealProject {
        project_id: String,
    },
    SetPreferences(SidebarPreferences),
    SetCollapsed {
        target: SidebarCollapseTarget,
        collapsed: bool,
    },
    CollapseAll,
    CreateThreadInGroup {
        group_id: String,
        draft: Box<super::Thread>,
    },
}
/// Stable distinguishable service failures; a conflict requires a fresh snapshot.
#[derive(Debug, thiserror::Error)]
pub enum SidebarOrganizationError {
    #[error("{}", vega_i18n::t_args("domain.organization.conflict", &[("expected", &.expected.to_string()), ("actual", &.actual.to_string())]))]
    Conflict { expected: u64, actual: u64 },
    #[error("{}", vega_i18n::t_args("domain.organization.invalid", &[("detail", .0)]))]
    Invalid(String),
    #[error("{}", vega_i18n::t_args("domain.organization.invalid", &[("detail", vega_i18n::t(.0))]))]
    InvalidKey(&'static str),
    #[error("{}", vega_i18n::t_args("domain.organization.limit", &[("detail", .0)]))]
    Limit(String),
    #[error("{}", vega_i18n::t_args("domain.organization.limit", &[("detail", vega_i18n::t(.0))]))]
    LimitKey(&'static str),
    #[error("{}", vega_i18n::t_args("domain.organization.store", &[("detail", .0)]))]
    Store(String),
}

/// Committed organization state and the precise task identity created by this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarOrganizationOutcome {
    pub snapshot: SidebarOrganizationSnapshot,
    pub created_thread: Option<Thread>,
}
