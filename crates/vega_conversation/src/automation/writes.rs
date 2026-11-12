use super::*;

pub(super) struct ChangeAutomation {
    pub(super) thread: String,
    pub(super) command: AutomationCommand,
    pub(super) now: i64,
}

impl crate::database_writer::Sealed for ChangeAutomation {}

impl crate::database_writer::DatabaseCommand for ChangeAutomation {
    type Output = Result<Vec<AutomationTask>, String>;

    fn apply(self, store: &Store) -> Self::Output {
        command_on_writer(store, &self.thread, &self.command, self.now)
    }
}

pub(super) struct PauseAutomation {
    pub(super) thread: String,
    pub(super) reason: String,
}

impl crate::database_writer::Sealed for PauseAutomation {}

impl crate::database_writer::DatabaseCommand for PauseAutomation {
    type Output = Result<Vec<AutomationTask>, String>;

    fn apply(self, store: &Store) -> Self::Output {
        pause_all_on_writer(store, &self.thread, &self.reason)
    }
}

pub(super) struct PauseMatchingAutomation {
    pub(super) thread: String,
    pub(super) expected: Vec<AutomationTask>,
    pub(super) reason: String,
}

impl crate::database_writer::Sealed for PauseMatchingAutomation {}

impl crate::database_writer::DatabaseCommand for PauseMatchingAutomation {
    type Output = Result<(), ConversationError>;

    fn apply(self, store: &Store) -> Self::Output {
        pause_matching_on_writer(store, &self.thread, &self.expected, &self.reason)
    }
}

pub(super) struct RefreshAutomation {
    pub(super) thread: String,
    pub(super) now: i64,
}

impl crate::database_writer::Sealed for RefreshAutomation {}

impl crate::database_writer::DatabaseCommand for RefreshAutomation {
    type Output = Result<Vec<AutomationTask>, String>;

    fn apply(self, store: &Store) -> Self::Output {
        refresh_on_writer(store, &self.thread, self.now)
    }
}

pub(super) struct ReleaseAutomationClaim {
    pub(super) claim: AutomationRunClaim,
}

impl crate::database_writer::Sealed for ReleaseAutomationClaim {}

impl crate::database_writer::DatabaseCommand for ReleaseAutomationClaim {
    type Output = Result<(), String>;

    fn apply(self, store: &Store) -> Self::Output {
        release_claim_on_writer(store, &self.claim)
    }
}

pub(super) struct FinishAutomation {
    pub(super) claim: AutomationRunClaim,
    pub(super) success: bool,
    pub(super) usage_complete: bool,
    pub(super) elapsed_ms: u64,
    pub(super) now: i64,
}

impl crate::database_writer::Sealed for FinishAutomation {}

impl crate::database_writer::DatabaseCommand for FinishAutomation {
    type Output = Result<(), String>;

    fn apply(self, store: &Store) -> Self::Output {
        finish_on_writer(
            store,
            &self.claim,
            self.success,
            self.usage_complete,
            self.elapsed_ms,
            self.now,
        )
    }
}

pub(super) struct AuthorizeAutomation {
    pub(super) expected: AutomationPermissionAuthorization,
    pub(super) confirmed: bool,
    pub(super) now: i64,
}

impl crate::database_writer::Sealed for AuthorizeAutomation {}

impl crate::database_writer::DatabaseCommand for AuthorizeAutomation {
    type Output = Result<AutomationTask, String>;

    fn apply(self, store: &Store) -> Self::Output {
        authorize_full_access_on_writer(store, &self.expected, self.confirmed, self.now)
    }
}

pub(super) struct ClaimAutomation {
    pub(super) thread: String,
    pub(super) source: Option<AutomationKind>,
    pub(super) now: i64,
}

impl crate::database_writer::Sealed for ClaimAutomation {}

impl crate::database_writer::DatabaseCommand for ClaimAutomation {
    type Output = Result<Option<AutomationRunClaim>, String>;

    fn apply(self, store: &Store) -> Self::Output {
        claim_on_writer(store, &self.thread, self.source, self.now)
    }
}

pub(super) struct ActivateAutomation {
    pub(super) claim: AutomationRunClaim,
}

impl crate::database_writer::Sealed for ActivateAutomation {}

impl crate::database_writer::DatabaseCommand for ActivateAutomation {
    type Output = Result<Option<AutomationRunClaim>, String>;

    fn apply(self, store: &Store) -> Self::Output {
        activate_claim_on_writer(store, &self.claim)
    }
}
