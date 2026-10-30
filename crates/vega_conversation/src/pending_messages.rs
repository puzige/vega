use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use futures::{FutureExt, future::BoxFuture};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    ChatMessage, ChatRole, RuntimeFailureDetail, RuntimeFailureReason as Reason,
    RuntimeFailureStage as Stage, RuntimeSteeringInput, RuntimeSteeringSource, VegaError,
};
use vega_store::{Store, pending_messages as rows};

use crate::types::{
    ConversationError, ImageAttachment, PendingMessage, PendingMessageBehavior,
    PendingQueueProjection,
};

#[derive(Serialize, Deserialize)]
struct StoredSubmission {
    content: String,
    images: Vec<Vec<u8>>,
    #[serde(default)]
    permission_fence: Option<crate::types::PermissionFence>,
    #[serde(default)]
    references: Vec<String>,
    #[serde(default)]
    reference_mentions: Vec<crate::types::FileReferenceMention>,
}

fn failure() -> ConversationError {
    ConversationError::Store(vega_i18n::t("domain.pending.error.save").into())
}

pub fn enqueue(
    store: &Store,
    message: &PendingMessage,
) -> Result<Vec<PendingMessage>, ConversationError> {
    enqueue_cached(store, message, &[])
}

pub fn enqueue_cached(
    store: &Store,
    message: &PendingMessage,
    cached: &[PendingMessage],
) -> Result<Vec<PendingMessage>, ConversationError> {
    enqueue_cached_projection(store, message, cached).map(|projection| projection.messages)
}

pub fn enqueue_cached_projection(
    store: &Store,
    message: &PendingMessage,
    cached: &[PendingMessage],
) -> Result<PendingQueueProjection, ConversationError> {
    let _coordination = crate::permission_contracts::coordinate()?;
    crate::permission_contracts::require_admission_open(store)?;
    require_authority(store, message)?;
    crate::attachments::validate_images(&message.images).map_err(|_| failure())?;
    let payload = serde_json::to_string(&StoredSubmission {
        content: message.content.clone(),
        images: message
            .images
            .iter()
            .map(|image| image.bytes().to_vec())
            .collect(),
        permission_fence: message.permission_fence.clone(),
        references: message.references.clone(),
        reference_mentions: message.reference_mentions.clone(),
    })
    .map_err(|_| failure())?;
    if message.content.is_empty() && message.images.is_empty() {
        return Err(failure());
    }
    let tx = store.immediate_transaction().map_err(|_| failure())?;
    if rows::count(&tx, &message.thread_id).map_err(|_| failure())? >= 64 {
        return Err(ConversationError::Store(
            vega_i18n::t("domain.pending.error.full").into(),
        ));
    }
    rows::insert(
        &tx,
        &rows::PendingMessageRow {
            id: message.id.clone(),
            thread_id: message.thread_id.clone(),
            ordinal: 0,
            behavior: message.behavior.as_str().into(),
            generation: message.generation.clone(),
            payload,
            created_at: now_ms(),
        },
    )
    .map_err(|_| failure())?;
    let mut accepted = cached.to_vec();
    accepted.push(message.clone());
    let projection = load_cached_conn(&tx, &message.thread_id, &accepted)?;
    tx.commit().map_err(|_| failure())?;
    Ok(projection)
}

pub fn load(store: &Store, thread_id: &str) -> Result<Vec<PendingMessage>, ConversationError> {
    load_cached(store, thread_id, &[])
}

pub fn load_cached(
    store: &Store,
    thread_id: &str,
    cached: &[PendingMessage],
) -> Result<Vec<PendingMessage>, ConversationError> {
    load_cached_projection(store, thread_id, cached).map(|projection| projection.messages)
}

pub fn load_cached_projection(
    store: &Store,
    thread_id: &str,
    cached: &[PendingMessage],
) -> Result<PendingQueueProjection, ConversationError> {
    let tx = store
        .conn()
        .unchecked_transaction()
        .map_err(|_| failure())?;
    let projection = load_cached_conn(&tx, thread_id, cached)?;
    tx.commit().map_err(|_| failure())?;
    Ok(projection)
}

fn load_cached_conn(
    conn: &vega_store::Connection,
    thread_id: &str,
    cached: &[PendingMessage],
) -> Result<PendingQueueProjection, ConversationError> {
    let mut projection = PendingQueueProjection::default();
    for metadata in rows::list_metadata(conn, thread_id).map_err(|_| failure())? {
        projection
            .timestamps
            .insert(metadata.id.clone(), metadata.created_at);
        let message = if let Some(previous) = cached
            .iter()
            .find(|message| message.id == metadata.id && message.thread_id == thread_id)
        {
            let mut message = previous.clone();
            message.generation = metadata.generation;
            message.behavior = match metadata.behavior.as_str() {
                "queue" => PendingMessageBehavior::Queue,
                "steer" => PendingMessageBehavior::Steer,
                _ => return Err(failure()),
            };
            message
        } else {
            let row = rows::find(conn, thread_id, &metadata.id)
                .map_err(|_| failure())?
                .ok_or_else(failure)?;
            decode(row)?
        };
        projection.messages.push(message);
    }
    Ok(projection)
}

pub fn remove(
    store: &Store,
    thread_id: &str,
    id: &str,
) -> Result<PendingMessage, ConversationError> {
    let tx = store.immediate_transaction().map_err(|_| failure())?;
    let row = rows::find(&tx, thread_id, id)
        .map_err(|_| failure())?
        .ok_or_else(failure)?;
    let message = decode(row)?;
    if !rows::remove(&tx, thread_id, id).map_err(|_| failure())? {
        return Err(ConversationError::Store(
            vega_i18n::t("domain.pending.error.delivering").into(),
        ));
    }
    tx.commit().map_err(|_| failure())?;
    Ok(message)
}

pub fn promote(
    store: &Store,
    thread_id: &str,
    id: &str,
    generation: &str,
) -> Result<(), ConversationError> {
    if !rows::promote(store.conn(), thread_id, id, generation).map_err(|_| failure())? {
        return Err(failure());
    }
    Ok(())
}

pub fn recover(store: &Store, thread_id: &str) -> Result<Vec<PendingMessage>, ConversationError> {
    recover_cached(store, thread_id, &[])
}

pub fn recover_cached(
    store: &Store,
    thread_id: &str,
    cached: &[PendingMessage],
) -> Result<Vec<PendingMessage>, ConversationError> {
    recover_cached_projection(store, thread_id, cached).map(|projection| projection.messages)
}

pub fn recover_cached_projection(
    store: &Store,
    thread_id: &str,
    cached: &[PendingMessage],
) -> Result<PendingQueueProjection, ConversationError> {
    rows::release_claims(store.conn(), thread_id).map_err(|_| failure())?;
    load_cached_projection(store, thread_id, cached)
}

pub(crate) fn decode(row: rows::PendingMessageRow) -> Result<PendingMessage, ConversationError> {
    let payload: StoredSubmission = serde_json::from_str(&row.payload).map_err(|_| failure())?;
    let images = payload
        .images
        .into_iter()
        .map(ImageAttachment::from_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| failure())?;
    crate::attachments::validate_images(&images).map_err(|_| failure())?;
    let behavior = match row.behavior.as_str() {
        "queue" => PendingMessageBehavior::Queue,
        "steer" => PendingMessageBehavior::Steer,
        _ => return Err(failure()),
    };
    Ok(PendingMessage {
        id: row.id,
        thread_id: row.thread_id,
        generation: row.generation,
        behavior,
        content: payload.content,
        images,
        permission_fence: payload.permission_fence,
        references: payload.references,
        reference_mentions: payload.reference_mentions,
    })
}

pub fn capture_authority(
    store: &Store,
    source: &crate::types::Thread,
) -> Result<crate::types::PermissionFence, ConversationError> {
    let _coordination = crate::permission_contracts::coordinate()?;
    let row = vega_store::threads::find(store.conn(), &source.id)
        .map_err(|_| failure())?
        .ok_or_else(failure)?;
    let current = crate::threads::thread_from_row(&row)?;
    if current.project_id != source.project_id
        || current.mode != source.mode
        || current.permission_mode != source.permission_mode
        || current.permission_revision != source.permission_revision
        || current.full_access_grant != source.full_access_grant
        || current.backend != crate::types::TaskBackend::Native
        || current.backend != source.backend
        || current.status != crate::types::ThreadStatus::Active
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    crate::permission_contracts::capture_fence(store, &row)
}

pub(crate) fn require_authority(
    store: &Store,
    message: &PendingMessage,
) -> Result<crate::types::PermissionFence, ConversationError> {
    let saved = message
        .permission_fence
        .as_ref()
        .filter(|fence| fence.is_valid() && fence.thread_id == message.thread_id)
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    let row = vega_store::threads::find(store.conn(), &message.thread_id)
        .map_err(|_| failure())?
        .ok_or_else(failure)?;
    if row.backend != "native"
        || row.status != "active"
        || crate::permission_contracts::capture_fence(store, &row)? != *saved
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(saved.clone())
}

pub fn validate_authority(
    store: &Store,
    message: &PendingMessage,
) -> Result<(), ConversationError> {
    let _coordination = crate::permission_contracts::coordinate()?;
    crate::permission_contracts::require_admission_open(store)?;
    require_authority(store, message).map(|_| ())
}

pub fn prepare_dispatch(
    store: &Store,
    thread: &str,
    id: &str,
    content: &str,
    images: &[ImageAttachment],
    references: &[String],
) -> Result<crate::types::PendingInputAuthority, ConversationError> {
    let _coordination = crate::permission_contracts::coordinate()?;
    let row = rows::find_dispatchable(store.conn(), thread, id)
        .map_err(|_| failure())?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    let message = decode(row)?;
    if message.content != content || message.images != images || message.references != references {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    require_authority(store, &message)?;
    Ok(crate::types::PendingInputAuthority {
        database: store.database_identity(),
        message,
    })
}

pub(crate) fn verify_dispatch(
    store: &Store,
    accepted: &crate::types::PendingInputAuthority,
    thread: &str,
    id: &str,
) -> Result<crate::types::PermissionFence, ConversationError> {
    if accepted.database != store.database_identity()
        || accepted.message.thread_id != thread
        || accepted.message.id != id
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let row = rows::find_dispatchable(store.conn(), thread, id)
        .map_err(|_| failure())?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    let message = decode(row)?;
    if message != accepted.message {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    require_authority(store, &message)
}

pub struct QueueSteeringSource {
    database: PathBuf,
    workspace: PathBuf,
    thread_id: String,
    generation: String,
    credentials: vega_runtime::CredentialReader,
    exceptions: Option<vega_runtime::CredentialExceptionFilter>,
    cancel: CancellationToken,
    admission: Option<crate::types::ThreadAdmission>,
    authority: Arc<OnceLock<crate::types::PermissionFence>>,
}

impl QueueSteeringSource {
    pub(crate) fn authority_slot(&self) -> Arc<OnceLock<crate::types::PermissionFence>> {
        self.authority.clone()
    }

    pub fn with_thread_admission(mut self, admission: crate::types::ThreadAdmission) -> Self {
        self.admission = Some(admission);
        self
    }

    pub fn with_credential_exceptions(
        mut self,
        filter: vega_runtime::CredentialExceptionFilter,
    ) -> Self {
        self.exceptions = Some(filter);
        self
    }
    pub fn new(
        database: PathBuf,
        workspace: PathBuf,
        thread_id: String,
        generation: String,
        credentials: vega_runtime::CredentialReader,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            database,
            workspace,
            thread_id,
            generation,
            credentials,
            exceptions: None,
            cancel,
            admission: None,
            authority: Arc::new(OnceLock::new()),
        }
    }
}

impl RuntimeSteeringSource for QueueSteeringSource {
    fn ready(&self) -> BoxFuture<'_, Result<Option<String>, VegaError>> {
        let database = self.database.clone();
        let thread = self.thread_id.clone();
        let generation = self.generation.clone();
        let cancel = self.cancel.clone();
        async move {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            tokio::task::spawn_blocking(move || {
                let store = Store::open_read_only(database)
                    .map_err(|error| VegaError::from(error).at_runtime_stage(Stage::PendingRead))?;
                rows::next_steer_metadata(store.conn(), &thread, &generation)
                    .map_err(|error| VegaError::from(error).at_runtime_stage(Stage::PendingRead))
            })
            .await
            .map_err(|_| pending_failure(Stage::PendingWorker, Reason::Worker))?
        }
        .boxed()
    }

    fn take(&self) -> BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        let database = self.database.clone();
        let workspace = self.workspace.clone();
        let thread_id = self.thread_id.clone();
        let generation = self.generation.clone();
        let credentials = Arc::clone(&self.credentials);
        let exceptions = self.exceptions.clone();
        let cancel = self.cancel.clone();
        let admission = self.admission.clone();
        let authority = self.authority.get().cloned();
        async move {
            tokio::task::spawn_blocking(move || {
                if cancel.is_cancelled() {
                    return Ok(None);
                }
                let store = Store::open(database)
                    .map_err(|error| VegaError::from(error).at_runtime_stage(Stage::PendingRead))?;
                let _admission = match admission {
                    Some(admission)
                        if admission.verifies(store.database_identity(), &thread_id) =>
                    {
                        admission
                    }
                    Some(_) => {
                        return Err(pending_failure(Stage::PendingAdmission, Reason::Admission));
                    }
                    None => crate::thread_access::acquire(&store, &thread_id)
                        .map_err(|_| pending_failure(Stage::PendingAdmission, Reason::Admission))?,
                };
                let row = rows::next_steer(store.conn(), &thread_id, &generation)
                    .map_err(|error| VegaError::from(error).at_runtime_stage(Stage::PendingRead))?;
                let Some(row) = row else {
                    return Ok(None);
                };
                let message = decode(row.clone())
                    .map_err(|_| pending_failure(Stage::PendingDecode, Reason::PendingDecode))?;
                let accepted = authority
                    .as_ref()
                    .ok_or_else(|| pending_failure(Stage::PendingAdmission, Reason::Admission))?;
                {
                    let _coordination = crate::permission_contracts::coordinate()
                        .map_err(|_| pending_failure(Stage::PendingAdmission, Reason::Admission))?;
                    if require_authority(&store, &message).as_ref().ok() != Some(accepted) {
                        return Err(pending_failure(Stage::PendingAdmission, Reason::Admission));
                    }
                }
                let refs = vega_tools::reference::resolve_references(
                    &workspace,
                    &message.references,
                    vega_tools::reference::REFERENCE_MAX_FILES,
                    vega_tools::reference::REFERENCE_MAX_FILE_BYTES,
                    vega_tools::reference::REFERENCE_MAX_TOTAL_BYTES,
                )
                .map_err(|error| match error {
                    vega_tools::ToolError::Io(error) => {
                        VegaError::Io(error).at_runtime_stage(Stage::PendingReferences)
                    }
                    _ => pending_failure(Stage::PendingReferences, Reason::References),
                })?;
                let original_content = message.content.clone();
                let content = if refs.is_empty() {
                    message.content.clone()
                } else {
                    format!(
                        "{}\n\n{}",
                        vega_tools::reference::render_reference_block(&refs),
                        message.content
                    )
                };
                let values = credentials().map_err(|_| VegaError::CredentialCheckUnavailable)?;
                let content = vega_runtime::redact_sensitive_credential_text_with_policy(
                    &content,
                    &values,
                    exceptions.as_ref(),
                );
                if cancel.is_cancelled() {
                    return Ok(None);
                }
                let _coordination = crate::permission_contracts::coordinate()
                    .map_err(|_| pending_failure(Stage::PendingAdmission, Reason::Admission))?;
                if require_authority(&store, &message).as_ref().ok() != Some(accepted) {
                    return Err(pending_failure(Stage::PendingAdmission, Reason::Admission));
                }
                let tx = store.immediate_transaction()?;
                if cancel.is_cancelled()
                    || rows::next_steer(&tx, &thread_id, &generation)?.as_ref() != Some(&row)
                {
                    return Ok(None);
                }
                if !rows::claim_steer(&tx, &thread_id, &message.id, &generation)? {
                    return Ok(None);
                }
                let seq = vega_store::messages::next_seq(&tx, &thread_id)?;
                tx.commit()?;
                let mut chat = ChatMessage::new(ChatRole::User, content);
                chat.images = message.images;
                Ok(Some(RuntimeSteeringInput {
                    original_content,
                    id: message.id,
                    assistant_message_id: ulid::Ulid::generate().to_string(),
                    user_message_seq: seq,
                    message: chat,
                }))
            })
            .await
            .map_err(|_| pending_failure(Stage::PendingWorker, Reason::Worker))?
            .map_err(|error: VegaError| error.at_runtime_stage(Stage::PendingClaim))
        }
        .boxed()
    }
}

fn pending_failure(stage: Stage, reason: Reason) -> VegaError {
    VegaError::RuntimeFailure(RuntimeFailureDetail::new(reason)).at_runtime_stage(stage)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_millis()).ok())
        .unwrap_or(0)
}
