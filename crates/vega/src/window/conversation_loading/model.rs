use std::time::Instant;

use vega_theme::{Appearance, ThemeColors};
use vega_ui::nsh::ProjectionId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ResultPhase {
    Pending,
    Ready,
    Failed,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Observation {
    pub(super) thread_id: Option<String>,
    pub(super) generation: u64,
    pub(super) started: Option<Instant>,
    pub(super) worker_started: bool,
    pub(super) result: ResultPhase,
    pub(super) visit_started: Option<Instant>,
    pub(super) opening: Option<(String, Instant)>,
}

impl Observation {
    pub(super) fn failed(&self) -> bool {
        self.visit_started.is_none() && self.result == ResultPhase::Failed
    }
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub(super) revision: ProjectionId,
    pub(super) observed: Observation,
    pub(super) colors: ThemeColors,
    pub(super) appearance: Appearance,
    pub(super) language: &'static str,
    pub(super) focused: bool,
    pub(super) show_logo: bool,
    pub(super) phase: Option<f32>,
}

impl Snapshot {
    pub(super) fn same_frame(&self, other: &Self) -> bool {
        self.revision.matches(&other.revision)
            && self.observed == other.observed
            && self.appearance == other.appearance
            && self.language == other.language
            && self.focused == other.focused
            && self.show_logo == other.show_logo
    }
}

pub(super) enum Request {
    LaidOut(Snapshot, [f32; 4], [f32; 4]),
    Retry(Snapshot),
}
