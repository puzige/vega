#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SummaryVariant {
    Control,
    Lean,
    Short,
    Capped,
}

impl SummaryVariant {
    pub(super) fn from_environment() -> Self {
        Self::parse(
            std::env::var("VEGA_COMPACTION_SUMMARY_VARIANT")
                .ok()
                .as_deref(),
        )
    }

    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("lean") => Self::Lean,
            Some("short") => Self::Short,
            Some("capped") => Self::Capped,
            _ => Self::Control,
        }
    }

    pub(super) fn instruction(self) -> &'static str {
        match self {
            Self::Control => "",
            Self::Lean => {
                "\nUse a lean, actionable checkpoint. Deduplicate repeated background and resolved exploration; preserve exact identifiers and decisions needed for the next action. All required safety and task-continuity information must remain."
            }
            Self::Short => {
                "\nUse a short, compact handoff focused on the active task, user corrections, completed work, pending work and the next concrete action. Omit redundant exposition, not constraints or unresolved risks."
            }
            Self::Capped => {
                "\nKeep the summary within 2000 words. Prioritize the active task, user corrections, exact continuation details, constraints, stop and permission boundaries, and unresolved side effects over background detail."
            }
        }
    }
}
