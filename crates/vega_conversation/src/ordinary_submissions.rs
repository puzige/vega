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
    model: String,
    model_snapshot: Option<Box<ResolvedModel>>,
    reasoning: Option<FrozenReasoning>,
}

impl RawSubmission {
    pub fn new(
        text: String,
        images: Vec<ImageAttachment>,
        draft_revision: u64,
        model: String,
        model_snapshot: Option<Box<ResolvedModel>>,
        reasoning: Option<FrozenReasoning>,
    ) -> Self {
        Self {
            text,
            images: images.into(),
            draft_revision,
            model,
            model_snapshot,
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

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn model_snapshot(&self) -> Option<&ResolvedModel> {
        self.model_snapshot.as_deref()
    }

    pub fn reasoning(&self) -> Option<&FrozenReasoning> {
        self.reasoning.as_ref()
    }

    fn validate_selection(&self) -> Result<(), SubmissionError> {
        if self.model.trim().is_empty() {
            return Err(SubmissionError::PayloadMismatch);
        }
        if let Some(reasoning) = &self.reasoning {
            reasoning
                .validate()
                .map_err(|_| SubmissionError::PayloadMismatch)?;
            if reasoning.model != self.model {
                return Err(SubmissionError::PayloadMismatch);
            }
        }
        Ok(())
    }

    pub fn validate_model(&self, model: &ResolvedModel) -> Result<(), SubmissionError> {
        self.validate_selection()?;
        if model.identity.model != self.model
            || self
                .model_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.identity != model.identity)
            || self
                .reasoning
                .as_ref()
                .is_some_and(|reasoning| reasoning.provider != model.identity.provider)
        {
            return Err(SubmissionError::PayloadMismatch);
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), SubmissionError> {
        self.validate_selection()?;
        if let Some(model) = self.model_snapshot() {
            self.validate_model(model)?;
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct EffectiveAuthority {
    model: ResolvedModel,
    reasoning: FrozenReasoning,
}

impl EffectiveAuthority {
    pub fn model(&self) -> &ResolvedModel {
        &self.model
    }

    pub fn reasoning(&self) -> &FrozenReasoning {
        &self.reasoning
    }

    pub fn validate_current(&self, model: &ResolvedModel) -> Result<(), SubmissionError> {
        if &self.model != model {
            return Err(SubmissionError::PayloadMismatch);
        }
        vega_runtime::model_compat::freeze_choice(model, self.reasoning.choice.clone())
            .map_err(|_| SubmissionError::PayloadMismatch)?;
        Ok(())
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
pub struct PersistedPayload {
    text: String,
    images: Arc<[ImageAttachment]>,
}

impl PersistedPayload {
    pub fn new(text: String, images: Vec<ImageAttachment>) -> Self {
        Self {
            text,
            images: images.into(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn images(&self) -> &[ImageAttachment] {
        &self.images
    }
}

impl std::fmt::Debug for PersistedPayload {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PersistedPayload")
            .field("text_bytes", &self.text.len())
            .field("images", &self.images.len())
            .finish_non_exhaustive()
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
    NotCommitted,
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
pub enum RollbackFailure {
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
    pub persisted: Option<Arc<PersistedPayload>>,
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
    pub authority: Option<Arc<EffectiveAuthority>>,
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
    #[error("ordinary submission worker does not own the transaction")]
    CommitOwnerMismatch,
    #[error("ordinary submission workers have not converged")]
    WorkersActive,
    #[error("ordinary submission retry is blocked: {0:?}")]
    RetryBlocked(RetryBlocker),
    #[error("ordinary submission attempt counter is exhausted")]
    AttemptExhausted,
    #[error(transparent)]
    Admission(#[from] AdmissionError),
}

struct CommitAuthority {
    worker: Ulid,
    started: bool,
    rollback: Option<Result<(), RollbackFailure>>,
}

struct AttemptState {
    facts: AttemptSnapshot,
    workers: HashSet<Ulid>,
    commit_authority: Option<CommitAuthority>,
}

impl AttemptState {
    fn new(id: AttemptId, kind: AttemptKind, worker: Ulid) -> Self {
        Self {
            facts: AttemptSnapshot {
                id,
                assistant_message_id: Ulid::generate().to_string(),
                kind,
                commit: CommitFact::NotAttempted,
                persisted: None,
                primary: PrimaryFact::NotYetDriven,
                cleanup: CleanupFact::NotRequired,
                workers: 1,
                sealed: false,
                abnormal: false,
                history_changed: false,
                failure: None,
            },
            workers: HashSet::from([worker]),
            commit_authority: None,
        }
    }

    fn commit_authority(&self, worker: Ulid) -> Result<&CommitAuthority, SubmissionError> {
        let authority = self
            .commit_authority
            .as_ref()
            .ok_or(SubmissionError::InvalidTransition)?;
        if authority.worker != worker {
            return Err(SubmissionError::CommitOwnerMismatch);
        }
        Ok(authority)
    }

    fn make_unknown(&mut self) {
        self.facts.abnormal = true;
        if !matches!(
            self.facts.commit,
            CommitFact::Committed(_) | CommitFact::NotCommitted
        ) {
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
    authority: Option<Arc<EffectiveAuthority>>,
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
        raw.validate()?;
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
        let id = SubmissionId(Ulid::generate());
        let attempt = AttemptId {
            submission: id,
            number: 1,
        };
        let worker = Ulid::generate();
        let submission = Self(Arc::new(SubmissionInner {
            id,
            user_message_id: Ulid::generate().to_string(),
            database,
            thread_id,
            raw: Arc::new(raw),
            state: Mutex::new(SubmissionState {
                accepted: None,
                authority: None,
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
                if !matches!(
                    attempt.commit,
                    CommitFact::Committed(_) | CommitFact::NotCommitted
                ) {
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
            authority: (!unavailable).then(|| state.authority.clone()).flatten(),
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
        if matches!(current.facts.commit, CommitFact::CommitInFlight)
            || current
                .commit_authority
                .as_ref()
                .is_some_and(|authority| !authority.started && authority.rollback.is_none())
        {
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
        let worker = Ulid::generate();
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
        (
            CommitFact::NotAttempted | CommitFact::Preparing | CommitFact::NotCommitted,
            AttemptKind::CommittedRecovery(_),
        ) => RetryEligibility::CommittedNotStarted,
        (CommitFact::NotAttempted | CommitFact::Preparing | CommitFact::NotCommitted, _) => {
            RetryEligibility::Uncommitted
        }
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
        let worker = Ulid::generate();
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

    pub fn bind_effective_authority(
        &self,
        model: ResolvedModel,
        reasoning: FrozenReasoning,
    ) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        self.submission.raw().validate_model(&model)?;
        reasoning
            .validate()
            .map_err(|_| SubmissionError::PayloadMismatch)?;
        if reasoning.provider != model.identity.provider
            || reasoning.model != model.identity.model
            || self
                .submission
                .raw()
                .reasoning()
                .is_some_and(|raw| raw.choice != reasoning.choice)
            || vega_runtime::model_compat::freeze_choice(&model, reasoning.choice.clone())
                .map_err(|_| SubmissionError::PayloadMismatch)?
                != reasoning
        {
            return Err(SubmissionError::PayloadMismatch);
        }
        let authority = EffectiveAuthority { model, reasoning };
        if let Some(existing) = &state.authority {
            return if existing.as_ref() == &authority {
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
        ) || state.accepted.is_some()
        {
            return Err(SubmissionError::InvalidTransition);
        }
        state.authority = Some(Arc::new(authority));
        Ok(FactUpdate::Applied)
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

    pub fn bind_persisted_payload(
        &self,
        payload: PersistedPayload,
    ) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let accepted = state
            .accepted
            .as_ref()
            .ok_or(SubmissionError::InvalidTransition)?;
        if payload.images.as_ref() != accepted.images.as_ref() {
            return Err(SubmissionError::PayloadMismatch);
        }
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        if let Some(existing) = &current.facts.persisted {
            current.commit_authority(self.worker)?;
            return if existing.as_ref() == &payload {
                Ok(FactUpdate::Unchanged)
            } else {
                Err(SubmissionError::PayloadMismatch)
            };
        }
        if current.facts.abnormal
            || current.facts.failure.is_some()
            || current.facts.history_changed
            || !matches!(
                current.facts.commit,
                CommitFact::NotAttempted | CommitFact::Preparing
            )
        {
            return Err(SubmissionError::InvalidTransition);
        }
        if let AttemptKind::CommittedRecovery(original) = &current.facts.kind
            && (payload.text != original.persisted_text
                || payload.images.as_ref() != original.persisted_images.as_slice())
        {
            return Err(SubmissionError::PayloadMismatch);
        }
        current.facts.persisted = Some(Arc::new(payload));
        current.commit_authority = Some(CommitAuthority {
            worker: self.worker,
            started: false,
            rollback: None,
        });
        Ok(FactUpdate::Applied)
    }

    pub fn begin_commit(&self) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        let authority = current.commit_authority(self.worker)?;
        if authority.rollback.is_some()
            || current.facts.abnormal
            || current.facts.failure.is_some()
            || current.facts.history_changed
        {
            return Err(SubmissionError::InvalidTransition);
        }
        match current.facts.commit {
            CommitFact::NotAttempted | CommitFact::Preparing => {
                let authority = current
                    .commit_authority
                    .as_mut()
                    .ok_or(SubmissionError::InvalidTransition)?;
                authority.started = true;
                current.facts.commit = CommitFact::CommitInFlight;
                current.facts.cleanup = CleanupFact::Pending;
                Ok(FactUpdate::Applied)
            }
            CommitFact::CommitInFlight => Ok(FactUpdate::Unchanged),
            _ => Err(SubmissionError::InvalidTransition),
        }
    }

    pub fn record_not_committed(
        &self,
        result: Result<(), RollbackFailure>,
    ) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        let authority = current.commit_authority(self.worker)?;
        if authority.started {
            return Err(SubmissionError::InvalidTransition);
        }
        if let Some(existing) = authority.rollback {
            return if existing == result {
                Ok(FactUpdate::Unchanged)
            } else {
                Err(SubmissionError::InvalidTransition)
            };
        }
        if !matches!(
            current.facts.commit,
            CommitFact::NotAttempted | CommitFact::Preparing | CommitFact::Unknown
        ) {
            return Err(SubmissionError::InvalidTransition);
        }
        let authority = current
            .commit_authority
            .as_mut()
            .ok_or(SubmissionError::InvalidTransition)?;
        authority.rollback = Some(result);
        match result {
            Ok(()) => {
                current.facts.commit = CommitFact::NotCommitted;
                if !current.facts.abnormal {
                    current.facts.cleanup = CleanupFact::NotRequired;
                }
            }
            Err(_) => current.make_unknown(),
        }
        Ok(FactUpdate::Applied)
    }

    pub fn record_committed(
        &self,
        receipt: CommittedReceipt,
    ) -> Result<FactUpdate, SubmissionError> {
        let mut state = self.lock()?;
        let current = state
            .attempts
            .last()
            .ok_or(SubmissionError::InvalidTransition)?;
        let authority = current.commit_authority(self.worker)?;
        if !authority.started {
            return Err(SubmissionError::InvalidTransition);
        }
        if let CommitFact::Committed(existing) = &current.facts.commit {
            return if existing.as_ref() == &receipt {
                Ok(FactUpdate::Unchanged)
            } else {
                Err(SubmissionError::ReceiptMismatch)
            };
        }
        if !matches!(
            current.facts.commit,
            CommitFact::CommitInFlight | CommitFact::Unknown
        ) {
            return Err(SubmissionError::InvalidTransition);
        }
        let persisted = current
            .facts
            .persisted
            .as_ref()
            .ok_or(SubmissionError::InvalidTransition)?;
        validate_receipt(&self.submission, &current.facts, persisted, &receipt)?;
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
    persisted: &PersistedPayload,
    receipt: &CommittedReceipt,
) -> Result<(), SubmissionError> {
    if receipt.user_message_id != submission.user_message_id()
        || receipt.assistant_message_id != attempt.assistant_message_id
        || receipt.user_seq < 1
        || receipt.assistant_seq.checked_add(1) != Some(receipt.next_seq)
        || receipt.persisted_text != persisted.text
        || receipt.persisted_images.as_slice() != persisted.images.as_ref()
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
