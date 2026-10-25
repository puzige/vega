use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use ulid::Ulid;
use vega_store::thread_admission::{self, AdmissionError, DatabaseIdentity, ThreadAdmission};

use crate::types::{FrozenReasoning, ImageAttachment, ResolvedModel};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SubmissionId(Ulid);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AttemptId {
    submission: SubmissionId,
    number: u64,
}

impl AttemptId {
    pub fn submission(self) -> SubmissionId {
        self.submission
    }

    pub fn number(self) -> u64 {
        self.number
    }
}

pub struct RawSubmission {
    text: String,
    images: Arc<[ImageAttachment]>,
    draft_revision: u64,
    model: ResolvedModel,
    reasoning: Option<FrozenReasoning>,
}

impl RawSubmission {
    pub fn new(
        text: String,
        images: Vec<ImageAttachment>,
        draft_revision: u64,
        model: ResolvedModel,
        reasoning: Option<FrozenReasoning>,
    ) -> Self {
        Self {
            text,
            images: images.into(),
            draft_revision,
            model,
            reasoning,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn images(&self) -> &[ImageAttachment] {
        &self.images
    }

    pub fn draft_revision(&self) -> u64 {
        self.draft_revision
    }

    pub fn model(&self) -> &ResolvedModel {
        &self.model
    }

    pub fn reasoning(&self) -> Option<&FrozenReasoning> {
        self.reasoning.as_ref()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AcceptedPayload {
    text: String,
    images: Arc<[ImageAttachment]>,
}

impl AcceptedPayload {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn images(&self) -> &[ImageAttachment] {
        &self.images
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CommittedReceipt {
    pub user_message_id: String,
    pub user_seq: i64,
    pub assistant_message_id: String,
    pub assistant_seq: i64,
    pub next_seq: i64,
    pub persisted_text: String,
    pub persisted_images: Vec<ImageAttachment>,
}

impl std::fmt::Debug for CommittedReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommittedReceipt")
            .field("user_seq", &self.user_seq)
            .field("assistant_seq", &self.assistant_seq)
            .field("next_seq", &self.next_seq)
            .field("text_bytes", &self.persisted_text.len())
            .field("images", &self.persisted_images.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitFact {
    NotAttempted,
    Preparing,
    CommitInFlight,
    Committed(Arc<CommittedReceipt>),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryFact {
    NotYetDriven,
    NotStarted,
    Started,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupFact {
    NotRequired,
    Pending,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmissionFailure {
    PreflightRejected,
    PreparationFailed,
    SpawnFailed,
    CommitFailed,
    LaunchFailed,
    Cancelled,
    WorkerLost,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupFailure {
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactUpdate {
    Applied,
    Unchanged,
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryBlocker {
    Unsealed,
    WorkersActive,
    Unknown,
    PrimaryStarted,
    CleanupIncomplete,
    NoFailure,
    HistoryChanged,
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryEligibility {
    Uncommitted,
    CommittedNotStarted,
    Blocked(RetryBlocker),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptKind {
    Initial,
    UncommittedRetry,
    CommittedRecovery(Arc<CommittedReceipt>),
}

#[derive(Clone, Debug)]
pub struct AttemptSnapshot {
    pub id: AttemptId,
    pub assistant_message_id: String,
    pub kind: AttemptKind,
    pub commit: CommitFact,
    pub primary: PrimaryFact,
    pub cleanup: CleanupFact,
    pub workers: usize,
    pub sealed: bool,
    pub abnormal: bool,
    pub history_changed: bool,
    pub failure: Option<SubmissionFailure>,
}

pub struct SubmissionSnapshot {
    pub id: SubmissionId,
    pub user_message_id: String,
    pub database: DatabaseIdentity,
    pub thread_id: String,
    pub raw: Arc<RawSubmission>,
    pub accepted: Option<Arc<AcceptedPayload>>,
    pub attempts: Vec<AttemptSnapshot>,
    pub retry: RetryEligibility,
    pub unavailable: bool,
    pub retired: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SubmissionError {
    #[error("ordinary submission facts are unavailable")]
    Unavailable,
    #[error("ordinary submission admission does not match")]
    AdmissionMismatch,
    #[error("thread already has an active ordinary submission")]
    Busy,
    #[error("ordinary submission attempt is stale")]
    StaleAttempt,
    #[error("ordinary submission attempt is sealed")]
    Sealed,
    #[error("ordinary submission fact transition is invalid")]
    InvalidTransition,
    #[error("ordinary submission payload does not match")]
    PayloadMismatch,
    #[error("ordinary submission commit receipt does not match")]
    ReceiptMismatch,
    #[error("ordinary submission workers have not converged")]
    WorkersActive,
    #[error("ordinary submission retry is blocked: {0:?}")]
    RetryBlocked(RetryBlocker),
    #[error("ordinary submission attempt counter is exhausted")]
    AttemptExhausted,
    #[error(transparent)]
    Admission(#[from] AdmissionError),
}

struct AttemptState {
    facts: AttemptSnapshot,
    workers: HashSet<Ulid>,
}

impl AttemptState {
    fn new(id: AttemptId, kind: AttemptKind, worker: Ulid) -> Self {
        Self {
            facts: AttemptSnapshot {
                id,
                assistant_message_id: Ulid::new().to_string(),
                kind,
                commit: CommitFact::NotAttempted,
                primary: PrimaryFact::NotYetDriven,
                cleanup: CleanupFact::NotRequired,
                workers: 1,
                sealed: false,
                abnormal: false,
                history_changed: false,
                failure: None,
            },
            workers: HashSet::from([worker]),
        }
    }

    fn make_unknown(&mut self) {
        self.facts.abnormal = true;
        if !matches!(self.facts.commit, CommitFact::Committed(_)) {
            self.facts.commit = CommitFact::Unknown;
        }
        if self.facts.primary != PrimaryFact::Started {
            self.facts.primary = PrimaryFact::Unknown;
        }
        if matches!(
            self.facts.cleanup,
            CleanupFact::NotRequired | CleanupFact::Pending
        ) {
            self.facts.cleanup = CleanupFact::Unknown;
        }
    }
}

struct SubmissionState {
    accepted: Option<Arc<AcceptedPayload>>,
    attempts: Vec<AttemptState>,
    active: Option<ThreadAdmission>,
    retired: bool,
}

struct SubmissionInner {
    id: SubmissionId,
    user_message_id: String,
    database: DatabaseIdentity,
    thread_id: String,
    raw: Arc<RawSubmission>,
    state: Mutex<SubmissionState>,
}

#[derive(Clone)]
pub struct OrdinarySubmission(Arc<SubmissionInner>);

impl std::fmt::Debug for OrdinarySubmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OrdinarySubmission")
            .field("id", &self.0.id)
            .finish_non_exhaustive()
    }
}

#[must_use]
pub struct OrdinaryWorker {
    submission: OrdinarySubmission,
    attempt: AttemptId,
    worker: Ulid,
    admission: ThreadAdmission,
    completed: bool,
}

#[derive(Clone, Eq, Hash, PartialEq)]
struct SubmissionKey {
    database: DatabaseIdentity,
    thread_id: String,
}

type Submissions = HashMap<SubmissionKey, Vec<OrdinarySubmission>>;

fn submissions() -> &'static Mutex<Submissions> {
    static SUBMISSIONS: OnceLock<Mutex<Submissions>> = OnceLock::new();
    SUBMISSIONS.get_or_init(Default::default)
}

pub fn for_thread(
    database: DatabaseIdentity,
    thread_id: &str,
) -> Result<Vec<OrdinarySubmission>, SubmissionError> {
    let registry = submissions()
        .lock()
        .map_err(|_| SubmissionError::Unavailable)?;
    Ok(registry
        .get(&SubmissionKey {
            database,
            thread_id: thread_id.to_owned(),
        })
        .cloned()
        .unwrap_or_default())
}

impl OrdinarySubmission {
    pub fn begin(
        database: DatabaseIdentity,
        thread_id: String,
        raw: RawSubmission,
        admission: ThreadAdmission,
    ) -> Result<(Self, OrdinaryWorker), SubmissionError> {
        if !admission.verifies(database, &thread_id) {
            return Err(SubmissionError::AdmissionMismatch);
        }
        if raw.reasoning.as_ref().is_some_and(|reasoning| {
            reasoning.provider != raw.model.identity.provider
                || reasoning.model != raw.model.identity.model
        }) {
            return Err(SubmissionError::PayloadMismatch);
        }
        let mut registry = submissions()
            .lock()
            .map_err(|_| SubmissionError::Unavailable)?;
        let key = SubmissionKey {
            database,
            thread_id: thread_id.clone(),
        };
        if let Some(existing) = registry.get(&key) {
            for submission in existing {
                if submission.lock()?.active.is_some() {
                    return Err(SubmissionError::Busy);
                }
            }
        }
        let id = SubmissionId(Ulid::new());
        let attempt = AttemptId {
            submission: id,
            number: 1,
        };
        let worker = Ulid::new();
        let submission = Self(Arc::new(SubmissionInner {
            id,
            user_message_id: Ulid::new().to_string(),
            database,
            thread_id,
            raw: Arc::new(raw),
            state: Mutex::new(SubmissionState {
                accepted: None,
                attempts: vec![AttemptState::new(attempt, AttemptKind::Initial, worker)],
                active: Some(admission.clone()),
                retired: false,
            }),
        }));
        registry.entry(key).or_default().push(submission.clone());
        let guard = OrdinaryWorker {
            submission: submission.clone(),
            attempt,
            worker,
            admission,
            completed: false,
        };
        Ok((submission, guard))
    }

    fn lock(&self) -> Result<MutexGuard<'_, SubmissionState>, SubmissionError> {
        self.0
            .state
            .lock()
            .map_err(|_| SubmissionError::Unavailable)
    }

    pub fn id(&self) -> SubmissionId {
        self.0.id
    }

    pub fn user_message_id(&self) -> &str {
        &self.0.user_message_id
    }

    pub fn raw(&self) -> &Arc<RawSubmission> {
        &self.0.raw
    }

    pub fn snapshot(&self) -> SubmissionSnapshot {
        let (state, unavailable) = match self.0.state.lock() {
            Ok(state) => (state, false),
            Err(poisoned) => (poisoned.into_inner(), true),
        };
        let mut attempts: Vec<_> = state
            .attempts
            .iter()
            .map(|attempt| attempt.facts.clone())
            .collect();
        if unavailable {
            for attempt in &mut attempts {
                attempt.abnormal = true;
                if !matches!(attempt.commit, CommitFact::Committed(_)) {
                    attempt.commit = CommitFact::Unknown;
                }
                if attempt.primary != PrimaryFact::Started {
                    attempt.primary = PrimaryFact::Unknown;
                }
                if matches!(
                    attempt.cleanup,
                    CleanupFact::NotRequired | CleanupFact::Pending
                ) {
                    attempt.cleanup = CleanupFact::Unknown;
                }
            }
        }
        let retry = if unavailable {
            RetryEligibility::Blocked(RetryBlocker::Unknown)
        } else {
            retry_eligibility(&state)
        };
        SubmissionSnapshot {
            id: self.0.id,
            user_message_id: self.0.user_message_id.clone(),
            database: self.0.database,
            thread_id: self.0.thread_id.clone(),
            raw: self.0.raw.clone(),
            accepted: state.accepted.clone(),
            attempts,
            retry,
            unavailable,
            retired: state.retired,
        }
    }

    pub fn seal(&self, attempt: AttemptId) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let Some(current) = state.attempts.last_mut() else {
            return Err(SubmissionError::InvalidTransition);
        };
        if current.facts.id != attempt {
            return Ok(FactUpdate::Stale);
        }
        if current.facts.sealed {
            return Ok(FactUpdate::Unchanged);
        }
        if !current.workers.is_empty() {
            return Err(SubmissionError::WorkersActive);
        }
        if matches!(current.facts.commit, CommitFact::CommitInFlight) {
            current.make_unknown();
        }
        if !current.facts.abnormal && current.facts.primary == PrimaryFact::NotYetDriven {
            current.facts.primary = PrimaryFact::NotStarted;
        }
        current.facts.sealed = true;
        state.active = None;
        Ok(FactUpdate::Applied)
    }

    pub fn invalidate_recovery(&self, attempt: AttemptId) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let Some(current) = state.attempts.last_mut() else {
            return Err(SubmissionError::InvalidTransition);
        };
        if current.facts.id != attempt {
            return Ok(FactUpdate::Stale);
        }
        if current.facts.history_changed {
            return Ok(FactUpdate::Unchanged);
        }
        current.facts.history_changed = true;
        Ok(FactUpdate::Applied)
    }

    pub fn try_retry(&self, attempt: AttemptId) -> Result<OrdinaryWorker, SubmissionError> {
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last()
            .ok_or(SubmissionError::InvalidTransition)?;
        if current.facts.id != attempt {
            return Err(SubmissionError::StaleAttempt);
        }
        let kind = match retry_eligibility(&state) {
            RetryEligibility::Uncommitted => AttemptKind::UncommittedRetry,
            RetryEligibility::CommittedNotStarted => {
                let receipt = match (&current.facts.commit, &current.facts.kind) {
                    (CommitFact::Committed(receipt), _) => receipt,
                    (_, AttemptKind::CommittedRecovery(receipt)) => receipt,
                    _ => return Err(SubmissionError::InvalidTransition),
                };
                AttemptKind::CommittedRecovery(receipt.clone())
            }
            RetryEligibility::Blocked(blocker) => {
                return Err(SubmissionError::RetryBlocked(blocker));
            }
        };
        let attempt = AttemptId {
            submission: self.0.id,
            number: attempt
                .number
                .checked_add(1)
                .ok_or(SubmissionError::AttemptExhausted)?,
        };
        let admission = thread_admission::try_acquire(self.0.database, &self.0.thread_id)?
            .ok_or(SubmissionError::Busy)?;
        let worker = Ulid::new();
        state
            .attempts
            .push(AttemptState::new(attempt, kind, worker));
        state.active = Some(admission.clone());
        Ok(OrdinaryWorker {
            submission: self.clone(),
            attempt,
            worker,
            admission,
            completed: false,
        })
    }

    pub fn retire_after_history_handoff(
        &self,
        attempt: AttemptId,
    ) -> Result<FactUpdate, SubmissionError> {
        let mut registry = submissions()
            .lock()
            .map_err(|_| SubmissionError::Unavailable)?;
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last()
            .ok_or(SubmissionError::InvalidTransition)?;
        if current.facts.id != attempt {
            return Ok(FactUpdate::Stale);
        }
        if state.retired {
            return Ok(FactUpdate::Unchanged);
        }
        if !current.facts.sealed
            || !current.workers.is_empty()
            || current.facts.abnormal
            || current.facts.primary != PrimaryFact::Started
            || !matches!(current.facts.commit, CommitFact::Committed(_))
        {
            return Err(SubmissionError::InvalidTransition);
        }
        state.retired = true;
        let key = SubmissionKey {
            database: self.0.database,
            thread_id: self.0.thread_id.clone(),
        };
        if let Some(records) = registry.get_mut(&key) {
            records.retain(|record| !Arc::ptr_eq(&record.0, &self.0));
            if records.is_empty() {
                registry.remove(&key);
            }
        }
        Ok(FactUpdate::Applied)
    }
}

fn retry_eligibility(state: &SubmissionState) -> RetryEligibility {
    let blocked = |reason| RetryEligibility::Blocked(reason);
    if state.retired {
        return blocked(RetryBlocker::Retired);
    }
    let Some(attempt) = state.attempts.last() else {
        return blocked(RetryBlocker::Unknown);
    };
    let facts = &attempt.facts;
    if !attempt.workers.is_empty() {
        return blocked(RetryBlocker::WorkersActive);
    }
    if !facts.sealed {
        return blocked(RetryBlocker::Unsealed);
    }
    if facts.abnormal
        || matches!(
            facts.commit,
            CommitFact::Unknown | CommitFact::CommitInFlight
        )
    {
        return blocked(RetryBlocker::Unknown);
    }
    if facts.primary == PrimaryFact::Started {
        return blocked(RetryBlocker::PrimaryStarted);
    }
    if facts.primary != PrimaryFact::NotStarted {
        return blocked(RetryBlocker::Unknown);
    }
    if facts.failure.is_none() {
        return blocked(RetryBlocker::NoFailure);
    }
    if facts.history_changed {
        return blocked(RetryBlocker::HistoryChanged);
    }
    match (&facts.commit, &facts.kind) {
        (CommitFact::NotAttempted | CommitFact::Preparing, AttemptKind::CommittedRecovery(_)) => {
            RetryEligibility::CommittedNotStarted
        }
        (CommitFact::NotAttempted | CommitFact::Preparing, _) => RetryEligibility::Uncommitted,
        (CommitFact::Committed(_), _) if facts.cleanup == CleanupFact::Succeeded => {
            RetryEligibility::CommittedNotStarted
        }
        (CommitFact::Committed(_), _) => blocked(RetryBlocker::CleanupIncomplete),
        _ => blocked(RetryBlocker::Unknown),
    }
}

impl OrdinaryWorker {
    pub fn attempt_id(&self) -> AttemptId {
        self.attempt
    }

    pub fn admission(&self) -> &ThreadAdmission {
        &self.admission
    }

    fn lock(&self) -> Result<MutexGuard<'_, SubmissionState>, SubmissionError> {
        let state = self.submission.lock()?;
        let current = state
            .attempts
            .last()
            .ok_or(SubmissionError::InvalidTransition)?;
        if current.facts.id != self.attempt {
            return Err(SubmissionError::StaleAttempt);
        }
        if current.facts.sealed || state.retired {
            return Err(SubmissionError::Sealed);
        }
        if !current.workers.contains(&self.worker)
            || !state
                .active
                .as_ref()
                .is_some_and(|active| active.same_owner(&self.admission))
        {
            return Err(SubmissionError::AdmissionMismatch);
        }
        Ok(state)
    }

    pub fn register_worker(&self) -> Result<Self, SubmissionError> {
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        let worker = Ulid::new();
        current.workers.insert(worker);
        current.facts.workers = current.workers.len();
        Ok(Self {
            submission: self.submission.clone(),
            attempt: self.attempt,
            worker,
            admission: self.admission.clone(),
            completed: false,
        })
    }

    pub fn begin_preparing(&self) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        match current.facts.commit {
            CommitFact::NotAttempted => {
                current.facts.commit = CommitFact::Preparing;
                Ok(FactUpdate::Applied)
            }
            CommitFact::Preparing => Ok(FactUpdate::Unchanged),
            _ => Err(SubmissionError::InvalidTransition),
        }
    }

    pub fn bind_accepted_payload(&self, text: String) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        if let Some(accepted) = &state.accepted {
            return if accepted.text == text {
                Ok(FactUpdate::Unchanged)
            } else {
                Err(SubmissionError::PayloadMismatch)
            };
        }
        let current = state
            .attempts
            .last()
            .ok_or(SubmissionError::InvalidTransition)?;
        if !matches!(
            current.facts.commit,
            CommitFact::NotAttempted | CommitFact::Preparing
        ) {
            return Err(SubmissionError::InvalidTransition);
        }
        state.accepted = Some(Arc::new(AcceptedPayload {
            text,
            images: self.submission.0.raw.images.clone(),
        }));
        Ok(FactUpdate::Applied)
    }

    pub fn begin_commit(&self) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        if state.accepted.is_none() {
            return Err(SubmissionError::InvalidTransition);
        }
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        if current.facts.abnormal
            || current.facts.failure.is_some()
            || current.facts.history_changed
        {
            return Err(SubmissionError::InvalidTransition);
        }
        match current.facts.commit {
            CommitFact::NotAttempted | CommitFact::Preparing => {
                current.facts.commit = CommitFact::CommitInFlight;
                current.facts.cleanup = CleanupFact::Pending;
                Ok(FactUpdate::Applied)
            }
            CommitFact::CommitInFlight => Ok(FactUpdate::Unchanged),
            _ => Err(SubmissionError::InvalidTransition),
        }
    }

    pub fn record_committed(
        &self,
        receipt: CommittedReceipt,
    ) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let accepted = state
            .accepted
            .as_ref()
            .ok_or(SubmissionError::InvalidTransition)?;
        let current = state
            .attempts
            .last()
            .ok_or(SubmissionError::InvalidTransition)?;
        if let CommitFact::Committed(existing) = &current.facts.commit {
            return if existing.as_ref() == &receipt {
                Ok(FactUpdate::Unchanged)
            } else {
                Err(SubmissionError::ReceiptMismatch)
            };
        }
        if !matches!(current.facts.commit, CommitFact::CommitInFlight) {
            return Err(SubmissionError::InvalidTransition);
        }
        validate_receipt(&self.submission, &current.facts, accepted, &receipt)?;
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        current.facts.commit = CommitFact::Committed(Arc::new(receipt));
        Ok(FactUpdate::Applied)
    }

    pub fn record_cleanup(
        &self,
        result: Result<(), CleanupFailure>,
    ) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        let next = match result {
            Ok(()) => CleanupFact::Succeeded,
            Err(CleanupFailure::Failed) => CleanupFact::Failed,
            Err(CleanupFailure::Unknown) => CleanupFact::Unknown,
        };
        if current.facts.cleanup == next {
            return Ok(FactUpdate::Unchanged);
        }
        if current.facts.cleanup != CleanupFact::Pending {
            return Err(SubmissionError::InvalidTransition);
        }
        current.facts.cleanup = next;
        Ok(FactUpdate::Applied)
    }

    pub fn mark_primary_started(&self) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        if current.facts.primary == PrimaryFact::Started {
            return Ok(FactUpdate::Unchanged);
        }
        if current.facts.abnormal
            || current.facts.history_changed
            || current.facts.failure.is_some()
            || !matches!(current.facts.commit, CommitFact::Committed(_))
            || current.facts.primary != PrimaryFact::NotYetDriven
            || current.facts.cleanup != CleanupFact::Pending
        {
            return Err(SubmissionError::InvalidTransition);
        }
        current.facts.primary = PrimaryFact::Started;
        Ok(FactUpdate::Applied)
    }

    pub fn complete(
        mut self,
        result: Result<(), SubmissionFailure>,
    ) -> Result<FactUpdate, SubmissionError> {
        {
            let mut state = self.lock()?;
            let current = state
                .attempts
                .last_mut()
                .ok_or(SubmissionError::InvalidTransition)?;
            if let Err(failure) = result {
                current.facts.failure.get_or_insert(failure);
                if matches!(
                    failure,
                    SubmissionFailure::WorkerLost | SubmissionFailure::Unknown
                ) {
                    current.make_unknown();
                }
            }
            current.workers.remove(&self.worker);
            current.facts.workers = current.workers.len();
        }
        self.completed = true;
        Ok(FactUpdate::Applied)
    }
}

impl Drop for OrdinaryWorker {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let mut state = match self.submission.0.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(attempt) = state
            .attempts
            .iter_mut()
            .find(|attempt| attempt.facts.id == self.attempt)
            && attempt.workers.remove(&self.worker)
        {
            attempt.facts.workers = attempt.workers.len();
            attempt
                .facts
                .failure
                .get_or_insert(SubmissionFailure::WorkerLost);
            attempt.make_unknown();
        }
    }
}

fn validate_receipt(
    submission: &OrdinarySubmission,
    attempt: &AttemptSnapshot,
    accepted: &AcceptedPayload,
    receipt: &CommittedReceipt,
) -> Result<(), SubmissionError> {
    if receipt.user_message_id != submission.user_message_id()
        || receipt.assistant_message_id != attempt.assistant_message_id
        || receipt.user_seq < 1
        || receipt.assistant_seq.checked_add(1) != Some(receipt.next_seq)
        || receipt.persisted_text != accepted.text
        || receipt.persisted_images.as_slice() != accepted.images.as_ref()
    {
        return Err(SubmissionError::ReceiptMismatch);
    }
    let boundary_matches = match &attempt.kind {
        AttemptKind::Initial | AttemptKind::UncommittedRetry => {
            receipt.user_seq.checked_add(1) == Some(receipt.assistant_seq)
        }
        AttemptKind::CommittedRecovery(original) => {
            receipt.user_seq == original.user_seq
                && receipt.assistant_seq == original.next_seq
                && receipt.persisted_text == original.persisted_text
                && receipt.persisted_images == original.persisted_images
        }
    };
    if !boundary_matches {
        return Err(SubmissionError::ReceiptMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use vega_store::Store;

    fn model() -> ResolvedModel {
        use vega_runtime::model_compat::{
            ModelApi, ModelRoute, decode_model_catalog, resolve_model,
        };
        let catalog = decode_model_catalog(br#"{"version":1,"models":[{"provider":"provider","id":"model","api":"openai-completions","baseUrl":"https://example.invalid/v1","input":["text"]}]}"#).unwrap();
        let definition = catalog
            .lookup(
                &ModelRoute::new(ModelApi::ChatCompletions, "https://example.invalid/v1").unwrap(),
                &crate::types::ModelIdentity {
                    provider: "provider".into(),
                    model: "model".into(),
                },
            )
            .unwrap();
        resolve_model(&definition, None).unwrap()
    }

    fn begin() -> (OrdinarySubmission, OrdinaryWorker) {
        let store = Store::open(":memory:").unwrap();
        let database = store.database_identity();
        let admission = thread_admission::try_acquire(database, "thread")
            .unwrap()
            .unwrap();
        OrdinarySubmission::begin(
            database,
            "thread".into(),
            RawSubmission::new("raw reference".into(), Vec::new(), 7, model(), None),
            admission,
        )
        .unwrap()
    }

    fn commit(submission: &OrdinarySubmission, worker: &OrdinaryWorker) -> CommittedReceipt {
        worker
            .bind_accepted_payload("expanded payload".into())
            .unwrap();
        worker.begin_commit().unwrap();
        let snapshot = submission.snapshot();
        let attempt = snapshot.attempts.last().unwrap();
        let (user_seq, assistant_seq) = match &attempt.kind {
            AttemptKind::CommittedRecovery(original) => (original.user_seq, original.next_seq),
            _ => (1, 2),
        };
        let receipt = CommittedReceipt {
            user_message_id: snapshot.user_message_id,
            user_seq,
            assistant_message_id: attempt.assistant_message_id.clone(),
            assistant_seq,
            next_seq: assistant_seq + 1,
            persisted_text: "expanded payload".into(),
            persisted_images: Vec::new(),
        };
        assert_eq!(
            worker.record_committed(receipt.clone()).unwrap(),
            FactUpdate::Applied
        );
        assert_eq!(
            worker.record_committed(receipt.clone()).unwrap(),
            FactUpdate::Unchanged
        );
        receipt
    }

    #[test]
    fn view_drop_keeps_payload_and_registered_workers_hold_admission() {
        let (submission, worker) = begin();
        let snapshot = submission.snapshot();
        let attempt = worker.attempt_id();
        let child = worker.register_worker().unwrap();
        worker
            .complete(Err(SubmissionFailure::SpawnFailed))
            .unwrap();
        assert!(matches!(
            submission.seal(attempt),
            Err(SubmissionError::WorkersActive)
        ));
        drop(submission);
        let restored = for_thread(snapshot.database, &snapshot.thread_id)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(restored.id(), snapshot.id);
        assert_eq!(restored.raw().text(), "raw reference");
        assert_eq!(restored.raw().draft_revision(), 7);
        assert!(
            thread_admission::try_acquire(snapshot.database, &snapshot.thread_id)
                .unwrap()
                .is_none()
        );
        child.complete(Ok(())).unwrap();
        assert_eq!(
            restored.snapshot().retry,
            RetryEligibility::Blocked(RetryBlocker::Unsealed)
        );
        restored.seal(attempt).unwrap();
        assert_eq!(restored.snapshot().retry, RetryEligibility::Uncommitted);
        assert!(
            thread_admission::try_acquire(snapshot.database, &snapshot.thread_id)
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            restored.retire_after_history_handoff(attempt),
            Err(SubmissionError::InvalidTransition)
        ));
    }

    #[test]
    fn concurrent_retry_consumes_qualification_once_and_stale_events_do_nothing() {
        let (submission, worker) = begin();
        let old = worker.attempt_id();
        let user_id = submission.user_message_id().to_owned();
        worker.begin_preparing().unwrap();
        worker
            .complete(Err(SubmissionFailure::PreflightRejected))
            .unwrap();
        submission.seal(old).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let requests: Vec<_> = (0..2)
            .map(|_| {
                let submission = submission.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    submission.try_retry(old)
                })
            })
            .collect();
        let mut guards = Vec::new();
        for request in requests {
            match request.join().unwrap() {
                Ok(guard) => guards.push(guard),
                Err(SubmissionError::StaleAttempt) => {}
                _ => panic!("unexpected retry outcome"),
            }
        }
        assert_eq!(guards.len(), 1);
        let next = guards.pop().unwrap();
        assert_eq!(next.attempt_id().number(), old.number() + 1);
        assert_eq!(submission.user_message_id(), user_id);
        assert_eq!(submission.seal(old).unwrap(), FactUpdate::Stale);
        assert_eq!(
            submission.invalidate_recovery(old).unwrap(),
            FactUpdate::Stale
        );
        assert_eq!(submission.snapshot().attempts.len(), 2);
        next.complete(Err(SubmissionFailure::Cancelled)).unwrap();
        assert_eq!(
            submission.snapshot().retry,
            RetryEligibility::Blocked(RetryBlocker::Unsealed)
        );
    }

    #[test]
    fn ambiguous_commit_and_abnormal_worker_drop_never_create_retry_proof() {
        for in_flight in [false, true] {
            let (submission, worker) = begin();
            let attempt = worker.attempt_id();
            worker.begin_preparing().unwrap();
            if in_flight {
                worker.bind_accepted_payload("accepted".into()).unwrap();
                worker.begin_commit().unwrap();
                worker
                    .complete(Err(SubmissionFailure::CommitFailed))
                    .unwrap();
            } else {
                drop(worker);
            }
            submission.seal(attempt).unwrap();
            let snapshot = submission.snapshot();
            assert!(matches!(
                snapshot.attempts.last().unwrap().commit,
                CommitFact::Unknown
            ));
            assert_eq!(
                snapshot.retry,
                RetryEligibility::Blocked(RetryBlocker::Unknown)
            );
            assert!(matches!(
                submission.try_retry(attempt),
                Err(SubmissionError::RetryBlocked(RetryBlocker::Unknown))
            ));
        }
    }

    #[test]
    fn committed_recovery_requires_cleanup_seal_and_unchanged_history() {
        for cleanup in [
            None,
            Some(Ok(())),
            Some(Err(CleanupFailure::Failed)),
            Some(Err(CleanupFailure::Unknown)),
        ] {
            let (submission, worker) = begin();
            let attempt = worker.attempt_id();
            commit(&submission, &worker);
            if let Some(result) = cleanup {
                worker.record_cleanup(result).unwrap();
            }
            worker
                .complete(Err(SubmissionFailure::LaunchFailed))
                .unwrap();
            assert!(matches!(
                submission.try_retry(attempt),
                Err(SubmissionError::RetryBlocked(RetryBlocker::Unsealed))
            ));
            submission.seal(attempt).unwrap();
            if cleanup == Some(Ok(())) {
                assert_eq!(
                    submission.snapshot().retry,
                    RetryEligibility::CommittedNotStarted
                );
                submission.invalidate_recovery(attempt).unwrap();
                assert!(matches!(
                    submission.try_retry(attempt),
                    Err(SubmissionError::RetryBlocked(RetryBlocker::HistoryChanged))
                ));
            } else {
                assert!(matches!(
                    submission.try_retry(attempt),
                    Err(SubmissionError::RetryBlocked(
                        RetryBlocker::CleanupIncomplete
                    ))
                ));
            }
        }
    }

    #[test]
    fn started_without_output_cannot_be_recovered_as_unstarted() {
        let (submission, worker) = begin();
        let attempt = worker.attempt_id();
        commit(&submission, &worker);
        worker.mark_primary_started().unwrap();
        worker.record_cleanup(Ok(())).unwrap();
        worker
            .complete(Err(SubmissionFailure::LaunchFailed))
            .unwrap();
        submission.seal(attempt).unwrap();
        assert_eq!(
            submission.snapshot().attempts.last().unwrap().primary,
            PrimaryFact::Started
        );
        assert!(matches!(
            submission.try_retry(attempt),
            Err(SubmissionError::RetryBlocked(RetryBlocker::PrimaryStarted))
        ));
        submission.retire_after_history_handoff(attempt).unwrap();
        let snapshot = submission.snapshot();
        assert!(
            for_thread(snapshot.database, &snapshot.thread_id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn recovery_keeps_user_and_accepted_payload_across_precommit_failure() {
        let (submission, worker) = begin();
        let original_attempt = worker.attempt_id();
        let original_receipt = commit(&submission, &worker);
        worker.record_cleanup(Ok(())).unwrap();
        worker
            .complete(Err(SubmissionFailure::LaunchFailed))
            .unwrap();
        submission.seal(original_attempt).unwrap();
        let recovery = submission.try_retry(original_attempt).unwrap();
        let recovery_attempt = recovery.attempt_id();
        assert!(matches!(
            recovery.bind_accepted_payload("changed reference".into()),
            Err(SubmissionError::PayloadMismatch)
        ));
        recovery
            .complete(Err(SubmissionFailure::PreparationFailed))
            .unwrap();
        submission.seal(recovery_attempt).unwrap();
        assert_eq!(
            submission.snapshot().retry,
            RetryEligibility::CommittedNotStarted
        );
        let next = submission.try_retry(recovery_attempt).unwrap();
        let next_attempt = next.attempt_id();
        let recovered_receipt = commit(&submission, &next);
        assert_eq!(
            original_receipt.user_message_id,
            recovered_receipt.user_message_id
        );
        assert_eq!(original_receipt.user_seq, recovered_receipt.user_seq);
        assert_eq!(
            original_receipt.persisted_text,
            recovered_receipt.persisted_text
        );
        assert_ne!(
            original_receipt.assistant_message_id,
            recovered_receipt.assistant_message_id
        );
        assert_eq!(recovered_receipt.assistant_seq, original_receipt.next_seq);
        next.record_cleanup(Ok(())).unwrap();
        next.complete(Err(SubmissionFailure::LaunchFailed)).unwrap();
        submission.seal(next_attempt).unwrap();
    }

    #[test]
    fn mismatched_receipt_cannot_authorize_primary_launch() {
        let (submission, worker) = begin();
        let attempt = worker.attempt_id();
        worker.bind_accepted_payload("accepted".into()).unwrap();
        worker.begin_commit().unwrap();
        let mut receipt = CommittedReceipt {
            user_message_id: submission.user_message_id().to_owned(),
            user_seq: 1,
            assistant_message_id: submission
                .snapshot()
                .attempts
                .last()
                .unwrap()
                .assistant_message_id
                .clone(),
            assistant_seq: 2,
            next_seq: 3,
            persisted_text: "display projection".into(),
            persisted_images: Vec::new(),
        };
        assert!(matches!(
            worker.record_committed(receipt.clone()),
            Err(SubmissionError::ReceiptMismatch)
        ));
        assert!(matches!(
            worker.mark_primary_started(),
            Err(SubmissionError::InvalidTransition)
        ));
        receipt.persisted_text = "accepted".into();
        worker.record_committed(receipt).unwrap();
        worker.record_cleanup(Ok(())).unwrap();
        worker
            .complete(Err(SubmissionFailure::LaunchFailed))
            .unwrap();
        submission.seal(attempt).unwrap();
        assert_eq!(
            submission.snapshot().retry,
            RetryEligibility::CommittedNotStarted
        );
    }

    #[test]
    fn accepted_images_survive_view_drop_and_wrong_bytes_cannot_confirm() {
        fn image(value: u8) -> ImageAttachment {
            let pixels = image::RgbaImage::from_pixel(1, 1, image::Rgba([value, 0, 0, 255]));
            let mut encoded = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(pixels)
                .write_to(&mut encoded, image::ImageFormat::Png)
                .unwrap();
            ImageAttachment::from_bytes(encoded.into_inner()).unwrap()
        }
        let original = image(10);
        let wrong = image(20);
        let bytes = original.bytes().to_vec();
        let store = Store::open(":memory:").unwrap();
        let database = store.database_identity();
        let admission = thread_admission::try_acquire(database, "thread")
            .unwrap()
            .unwrap();
        let (submission, worker) = OrdinarySubmission::begin(
            database,
            "thread".into(),
            RawSubmission::new(String::new(), vec![original], 8, model(), None),
            admission,
        )
        .unwrap();
        let attempt = worker.attempt_id();
        worker.bind_accepted_payload(String::new()).unwrap();
        worker.begin_commit().unwrap();
        let mut receipt = CommittedReceipt {
            user_message_id: submission.user_message_id().to_owned(),
            user_seq: 1,
            assistant_message_id: submission
                .snapshot()
                .attempts
                .last()
                .unwrap()
                .assistant_message_id
                .clone(),
            assistant_seq: 2,
            next_seq: 3,
            persisted_text: String::new(),
            persisted_images: vec![wrong],
        };
        assert!(matches!(
            worker.record_committed(receipt.clone()),
            Err(SubmissionError::ReceiptMismatch)
        ));
        drop(submission);
        let restored = for_thread(database, "thread").unwrap().pop().unwrap();
        assert!(restored.raw().text().is_empty());
        assert_eq!(restored.raw().images()[0].bytes(), bytes);
        receipt.persisted_images = restored.raw().images().to_vec();
        worker.record_committed(receipt).unwrap();
        worker.record_cleanup(Ok(())).unwrap();
        worker
            .complete(Err(SubmissionFailure::LaunchFailed))
            .unwrap();
        restored.seal(attempt).unwrap();
        assert_eq!(
            restored.snapshot().retry,
            RetryEligibility::CommittedNotStarted
        );
    }

    #[test]
    fn invalidated_history_and_recorded_failure_block_primary_launch() {
        for history_changed in [false, true] {
            let (submission, worker) = begin();
            let attempt = worker.attempt_id();
            commit(&submission, &worker);
            if history_changed {
                submission.invalidate_recovery(attempt).unwrap();
            } else {
                worker
                    .register_worker()
                    .unwrap()
                    .complete(Err(SubmissionFailure::Cancelled))
                    .unwrap();
            }
            assert!(matches!(
                worker.mark_primary_started(),
                Err(SubmissionError::InvalidTransition)
            ));
            worker.record_cleanup(Ok(())).unwrap();
            worker
                .complete(Err(SubmissionFailure::LaunchFailed))
                .unwrap();
            submission.seal(attempt).unwrap();
        }
    }

    #[test]
    fn poisoned_facts_preserve_payload_but_fail_closed() {
        let (submission, worker) = begin();
        let attempt = worker.attempt_id();
        let poisoned = submission.clone();
        assert!(
            std::thread::spawn(move || {
                let _state = poisoned.0.state.lock().unwrap();
                panic!("poison submission facts");
            })
            .join()
            .is_err()
        );
        let snapshot = submission.snapshot();
        assert!(snapshot.unavailable);
        assert_eq!(snapshot.raw.text(), "raw reference");
        assert_eq!(
            snapshot.retry,
            RetryEligibility::Blocked(RetryBlocker::Unknown)
        );
        assert!(matches!(
            worker.complete(Err(SubmissionFailure::PreparationFailed)),
            Err(SubmissionError::Unavailable)
        ));
        assert!(matches!(
            submission.seal(attempt),
            Err(SubmissionError::Unavailable)
        ));
        assert!(matches!(
            submission.try_retry(attempt),
            Err(SubmissionError::Unavailable)
        ));
    }
}
