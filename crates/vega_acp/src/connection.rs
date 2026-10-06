use crate::{
    EVENT_WEIGHT_BYTES, Error, MAX_EVENT_BYTES, MAX_EVENT_COUNT, MAX_FRAME_BYTES,
    MAX_PENDING_INBOUND, MAX_PENDING_OUTBOUND, MAX_SEEN_INBOUND_REQUEST_ID_BYTES,
    MAX_SEEN_INBOUND_REQUEST_IDS,
    framing::{FrameReader, parse_json_line, validate_frame_shape},
    protocol::{
        RequestId, RpcMessage, error_response, notification, parse_message, request, response,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    process::{Child, Command},
    sync::{Semaphore, mpsc, oneshot, watch},
    task::JoinHandle,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

const MAX_ARGUMENT_COUNT: usize = 128;
const MAX_ARGUMENT_BYTES: usize = 65_536;
pub(crate) const WRITER_QUEUE_CAPACITY: usize = 32;
const CLIENT_NAME: &str = "Vega";
const PROTOCOL_VERSION: u64 = 1;

type DynReader = Box<dyn AsyncRead + Send + Unpin>;

pub struct LaunchConfig {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl LaunchConfig {
    pub fn new(executable: PathBuf, args: Vec<String>, cwd: PathBuf) -> Self {
        Self {
            executable,
            args,
            cwd,
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        let encoded_argument_bytes = self.args.iter().map(String::len).sum::<usize>();
        if !self.executable.is_absolute()
            || !self.cwd.is_absolute()
            || self.args.len() > MAX_ARGUMENT_COUNT
            || encoded_argument_bytes > MAX_ARGUMENT_BYTES
        {
            return Err(Error::InvalidLaunchConfiguration);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InitializeResult {
    pub protocol_version: u64,
    pub agent_capabilities: Value,
    pub auth_methods: Value,
    pub agent_info: Option<Value>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub session_id: String,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    pub request_id: RequestId,
    pub session_id: String,
    pub tool_call: Value,
    pub options: Vec<PermissionOption>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Notification { method: String, params: Value },
    PermissionRequest(PermissionRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOutcome {
    Selected(String),
    Cancelled,
}

impl PermissionOutcome {
    pub fn option_id(&self) -> Option<&str> {
        match self {
            Self::Selected(option_id) => Some(option_id),
            Self::Cancelled => None,
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Selected(option_id) => {
                json!({"outcome":{"outcome":"selected","optionId":option_id}})
            }
            Self::Cancelled => json!({"outcome":{"outcome":"cancelled"}}),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptResult {
    pub stop_reason: String,
    pub raw: Value,
}

pub struct PromptHandle {
    receiver: oneshot::Receiver<Result<Value, Error>>,
}

impl PromptHandle {
    pub async fn wait(self) -> Result<PromptResult, Error> {
        let raw = self.receiver.await.map_err(|_| Error::ConnectionClosed)??;
        let stop_reason = raw
            .get("stopReason")
            .and_then(Value::as_str)
            .ok_or(Error::MissingStopReason)?
            .to_owned();
        Ok(PromptResult { stop_reason, raw })
    }
}

pub(crate) trait ChildControl: Send {
    fn force_terminate(self: Box<Self>);
    fn graceful_shutdown(self: Box<Self>) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

struct TokioChildControl(Child);

impl ChildControl for TokioChildControl {
    fn force_terminate(self: Box<Self>) {
        let mut child = self.0;
        let _ = child.start_kill();
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
    }

    fn graceful_shutdown(self: Box<Self>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            let mut child = self.0;
            if timeout(Duration::from_secs(2), child.wait()).await.is_err() {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        })
    }
}

struct WriteCommand {
    line: Vec<u8>,
    written: Option<oneshot::Sender<Result<(), Error>>>,
}

struct PendingOutgoing {
    sender: oneshot::Sender<Result<Value, Error>>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

struct PendingPermission {
    session_id: String,
    options: Vec<String>,
    reply_group: Arc<BatchReplyGroup>,
    response_index: usize,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

struct InboundRequestIdHistory {
    ids: HashSet<RequestId>,
    retained_bytes: usize,
}

struct QueuedEvent {
    event: Event,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

struct ConnectionInner {
    writer: mpsc::Sender<WriteCommand>,
    events: mpsc::Sender<QueuedEvent>,
    event_budget: Arc<Semaphore>,
    outbound_capacity: Arc<Semaphore>,
    inbound_capacity: Arc<Semaphore>,
    pending_outgoing: Mutex<HashMap<RequestId, PendingOutgoing>>,
    pending_permissions: Mutex<HashMap<RequestId, PendingPermission>>,
    seen_inbound_request_ids: Mutex<InboundRequestIdHistory>,
    initialized: Mutex<Option<InitializeResult>>,
    initialize_started: AtomicBool,
    next_request_id: AtomicU64,
    terminal: Mutex<Option<Error>>,
    terminal_sender: watch::Sender<Option<Error>>,
    cancellation: CancellationToken,
    child: Mutex<Option<Box<dyn ChildControl>>>,
}

impl ConnectionInner {
    fn active(&self) -> Result<(), Error> {
        if let Some(error) = lock(&self.terminal).clone() {
            return Err(error);
        }
        Ok(())
    }

    fn initialized(&self) -> Result<InitializeResult, Error> {
        lock(&self.initialized).clone().ok_or(Error::NotInitialized)
    }

    fn close(&self, error: Error, force: bool) -> Option<Box<dyn ChildControl>> {
        let mut terminal = lock(&self.terminal);
        if terminal.is_some() {
            return None;
        }
        *terminal = Some(error.clone());
        drop(terminal);
        self.terminal_sender.send_replace(Some(error.clone()));
        self.cancellation.cancel();
        let pending_outgoing = std::mem::take(&mut *lock(&self.pending_outgoing));
        for (_, pending) in pending_outgoing {
            let _ = pending.sender.send(Err(error.clone()));
        }
        lock(&self.pending_permissions).clear();
        let child = lock(&self.child).take();
        if force {
            if let Some(child) = child {
                child.force_terminate();
            }
            None
        } else {
            child
        }
    }

    fn fail(&self, error: Error) {
        let _ = self.close(error, true);
    }

    async fn write_value(&self, value: &Value) -> Result<(), Error> {
        self.active()?;
        let line = serde_json::to_vec(value).map_err(|_| Error::MalformedMessage)?;
        if line.len() > MAX_FRAME_BYTES {
            self.fail(Error::FrameTooLarge);
            return Err(Error::FrameTooLarge);
        }
        let (written, receiver) = oneshot::channel();
        let permit = self
            .writer
            .reserve()
            .await
            .map_err(|_| Error::ConnectionClosed)?;
        permit.send(WriteCommand {
            line,
            written: Some(written),
        });
        receiver.await.map_err(|_| Error::ConnectionClosed)??;
        Ok(())
    }

    fn enqueue_response_value(&self, value: &Value) -> Result<(), Error> {
        self.active()?;
        let line = serde_json::to_vec(value).map_err(|_| Error::MalformedMessage)?;
        if line.len() > MAX_FRAME_BYTES {
            self.fail(Error::FrameTooLarge);
            return Err(Error::FrameTooLarge);
        }
        match self.writer.try_send(WriteCommand {
            line,
            written: None,
        }) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.fail(Error::WriterQueueFull);
                Err(Error::WriterQueueFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.fail(Error::ConnectionClosed);
                Err(Error::ConnectionClosed)
            }
        }
    }

    fn respond_group(
        &self,
        group: &BatchReplyGroup,
        response_index: usize,
        value: Value,
    ) -> Result<(), Error> {
        let ready = {
            let mut state = lock(&group.state);
            if state.responses[response_index].is_some() {
                return Err(Error::DuplicateRequestId);
            }
            state.responses[response_index] = Some(value);
            state.remaining -= 1;
            if state.remaining == 0 {
                let responses = state
                    .responses
                    .iter_mut()
                    .filter_map(Option::take)
                    .collect::<Vec<_>>();
                Some(if state.is_batch {
                    Value::Array(responses)
                } else {
                    responses
                        .into_iter()
                        .next()
                        .ok_or(Error::MalformedMessage)?
                })
            } else {
                None
            }
        };
        if let Some(value) = ready {
            let inner = group.inner.upgrade().ok_or(Error::ConnectionClosed)?;
            inner.enqueue_response_value(&value)?;
        }
        Ok(())
    }

    async fn handle_line(self: &Arc<Self>, line: Vec<u8>) -> Result<(), Error> {
        let value = parse_json_line(&line)?;
        let (items, is_batch) = validate_frame_shape(value)?;
        let mut messages = Vec::with_capacity(items.len());
        let mut request_ids = HashSet::new();
        let mut response_ids = HashSet::new();
        for item in items {
            let message = parse_message(item)?;
            match &message {
                RpcMessage::Request { id, .. } => {
                    if !request_ids.insert(id.clone()) {
                        return Err(Error::DuplicateRequestId);
                    }
                }
                RpcMessage::Response { id, .. } => {
                    if !response_ids.insert(id.clone()) {
                        return Err(Error::DuplicateRequestId);
                    }
                }
                RpcMessage::Notification { .. } => {}
            }
            messages.push(message);
        }
        {
            let mut seen = lock(&self.seen_inbound_request_ids);
            if messages.iter().any(
                |message| matches!(message, RpcMessage::Request { id, .. } if seen.ids.contains(id)),
            ) {
                return Err(Error::DuplicateRequestId);
            }
            let request_count = messages
                .iter()
                .filter(|message| matches!(message, RpcMessage::Request { .. }))
                .count();
            let request_id_bytes = messages
                .iter()
                .try_fold(0usize, |total, message| {
                    if let RpcMessage::Request { id, .. } = message {
                        total.checked_add(id.retained_bytes())
                    } else {
                        Some(total)
                    }
                })
                .ok_or(Error::RequestIdHistoryFull)?;
            if seen.ids.len().saturating_add(request_count) > MAX_SEEN_INBOUND_REQUEST_IDS
                || seen.retained_bytes.saturating_add(request_id_bytes)
                    > MAX_SEEN_INBOUND_REQUEST_ID_BYTES
            {
                return Err(Error::RequestIdHistoryFull);
            }
            for message in &messages {
                if let RpcMessage::Request { id, .. } = message {
                    seen.ids.insert(id.clone());
                    seen.retained_bytes += id.retained_bytes();
                }
            }
        }
        {
            let pending = lock(&self.pending_permissions);
            if messages.iter().any(|message| {
                matches!(message, RpcMessage::Request { id, .. } if pending.contains_key(id))
            }) {
                return Err(Error::DuplicateRequestId);
            }
        }
        {
            let pending = lock(&self.pending_outgoing);
            if messages.iter().any(|message| {
                matches!(message, RpcMessage::Response { id, .. } if !pending.contains_key(id))
            }) {
                return Err(Error::UnknownRequestId);
            }
        }

        let request_count = messages
            .iter()
            .filter(|message| matches!(message, RpcMessage::Request { .. }))
            .count();
        let mut prepared = Vec::with_capacity(messages.len());
        let mut event_count = 0;
        for (index, message) in messages.into_iter().enumerate() {
            match message {
                RpcMessage::Request { id, method, params }
                    if method == "session/request_permission" =>
                {
                    let permission = parse_permission_request(id.clone(), params)?;
                    let inbound_permit = Arc::clone(&self.inbound_capacity)
                        .try_acquire_owned()
                        .map_err(|_| Error::TooManyPendingPermissions)?;
                    let serialized_bytes =
                        serde_json::to_vec(&message_value(&RpcMessage::Request {
                            id: id.clone(),
                            method,
                            params: permission.raw.clone(),
                        }))
                        .map_err(|_| Error::MalformedMessage)?
                        .len();
                    let permits = serialized_bytes.div_ceil(EVENT_WEIGHT_BYTES).max(1);
                    let event_permit = Arc::clone(&self.event_budget)
                        .try_acquire_many_owned(permits as u32)
                        .map_err(|_| Error::EventQueueOverflow)?;
                    prepared.push(PreparedMessage::Permission(PreparedPermission {
                        id,
                        request: permission,
                        _inbound_permit: inbound_permit,
                        event_permit,
                        response_index: index,
                    }));
                    event_count += 1;
                }
                RpcMessage::Notification { method, params } => {
                    let serialized_bytes =
                        serde_json::to_vec(&json!({"method":method,"params":params}))
                            .map_err(|_| Error::MalformedMessage)?
                            .len();
                    let permits = serialized_bytes.div_ceil(EVENT_WEIGHT_BYTES).max(1);
                    let event_permit = Arc::clone(&self.event_budget)
                        .try_acquire_many_owned(permits as u32)
                        .map_err(|_| Error::EventQueueOverflow)?;
                    prepared.push(PreparedMessage::Notification(PreparedNotification {
                        event: Event::Notification { method, params },
                        event_permit,
                    }));
                    event_count += 1;
                }
                RpcMessage::Request { id, .. } => {
                    prepared.push(PreparedMessage::MethodNotFound { id, index });
                }
                RpcMessage::Response { id, result } => {
                    prepared.push(PreparedMessage::Response { id, result });
                }
            }
        }
        if self.events.capacity() < event_count {
            return Err(Error::EventQueueOverflow);
        }
        let reply_group = (request_count > 0).then(|| {
            Arc::new(BatchReplyGroup::new(
                Arc::downgrade(self),
                prepared.len(),
                request_count,
                is_batch,
            ))
        });
        for message in prepared {
            match message {
                PreparedMessage::Response { id, result } => {
                    let pending = lock(&self.pending_outgoing)
                        .remove(&id)
                        .ok_or(Error::UnknownRequestId)?;
                    let result = result.map_err(|code| Error::AgentRejected { code });
                    let _ = pending.sender.send(result);
                }
                PreparedMessage::Notification(notification) => {
                    self.events
                        .try_send(QueuedEvent {
                            event: notification.event,
                            _permit: notification.event_permit,
                        })
                        .map_err(|_| Error::EventQueueOverflow)?;
                }
                PreparedMessage::Permission(permission) => {
                    let group = Arc::clone(reply_group.as_ref().ok_or(Error::MalformedMessage)?);
                    let options = permission
                        .request
                        .options
                        .iter()
                        .map(|option| option.option_id.clone())
                        .collect();
                    lock(&self.pending_permissions).insert(
                        permission.id,
                        PendingPermission {
                            session_id: permission.request.session_id.clone(),
                            options,
                            reply_group: group,
                            response_index: permission.response_index,
                            _permit: permission._inbound_permit,
                        },
                    );
                    self.events
                        .try_send(QueuedEvent {
                            event: Event::PermissionRequest(permission.request),
                            _permit: permission.event_permit,
                        })
                        .map_err(|_| Error::EventQueueOverflow)?;
                }
                PreparedMessage::MethodNotFound { id, index } => {
                    let group = Arc::clone(reply_group.as_ref().ok_or(Error::MalformedMessage)?);
                    self.respond_group(
                        &group,
                        index,
                        error_response(&id, -32601, "Method not found"),
                    )?;
                }
            }
        }
        Ok(())
    }
}

struct BatchReplyGroup {
    inner: Weak<ConnectionInner>,
    state: Mutex<BatchReplyState>,
}

struct BatchReplyState {
    responses: Vec<Option<Value>>,
    remaining: usize,
    is_batch: bool,
}

impl BatchReplyGroup {
    fn new(
        inner: Weak<ConnectionInner>,
        frame_size: usize,
        remaining: usize,
        is_batch: bool,
    ) -> Self {
        Self {
            inner,
            state: Mutex::new(BatchReplyState {
                responses: vec![None; frame_size],
                remaining,
                is_batch,
            }),
        }
    }
}

struct EventTasks {
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<usize>>,
}

pub struct Connection {
    inner: Arc<ConnectionInner>,
    events: tokio::sync::Mutex<mpsc::Receiver<QueuedEvent>>,
    terminal: tokio::sync::Mutex<watch::Receiver<Option<Error>>>,
    error_reported: AtomicBool,
    tasks: tokio::sync::Mutex<EventTasks>,
}

impl Connection {
    pub async fn spawn(config: LaunchConfig) -> Result<Self, Error> {
        let child = Self::validate_before_spawn(&config, || {
            let mut command = Command::new(&config.executable);
            command
                .args(&config.args)
                .current_dir(&config.cwd)
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            for name in ["HOME", "PATH", "LANG", "TMPDIR", "USER"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            command.spawn().map_err(|_| Error::LaunchFailed)
        })?;
        let mut child = child;
        let reader = child.stdout.take().ok_or(Error::LaunchFailed)?;
        let writer = child.stdin.take().ok_or(Error::LaunchFailed)?;
        let stderr = child.stderr.take().ok_or(Error::LaunchFailed)?;
        Ok(Self::from_parts(
            reader,
            writer,
            Some(Box::new(TokioChildControl(child))),
            Some(Box::new(stderr)),
        ))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_io<R, W>(reader: R, writer: W, child: Option<Box<dyn ChildControl>>) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Self::from_parts(reader, writer, child, None)
    }

    fn from_parts<R, W>(
        reader: R,
        writer: W,
        child: Option<Box<dyn ChildControl>>,
        stderr: Option<DynReader>,
    ) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (writer_sender, writer_receiver) = mpsc::channel(WRITER_QUEUE_CAPACITY);
        let (event_sender, event_receiver) = mpsc::channel(MAX_EVENT_COUNT);
        let (terminal_sender, terminal_receiver) = watch::channel(None);
        let inner = Arc::new(ConnectionInner {
            writer: writer_sender,
            events: event_sender,
            event_budget: Arc::new(Semaphore::new(MAX_EVENT_BYTES / EVENT_WEIGHT_BYTES)),
            outbound_capacity: Arc::new(Semaphore::new(MAX_PENDING_OUTBOUND)),
            inbound_capacity: Arc::new(Semaphore::new(MAX_PENDING_INBOUND)),
            pending_outgoing: Mutex::new(HashMap::new()),
            pending_permissions: Mutex::new(HashMap::new()),
            seen_inbound_request_ids: Mutex::new(InboundRequestIdHistory {
                ids: HashSet::new(),
                retained_bytes: 0,
            }),
            initialized: Mutex::new(None),
            initialize_started: AtomicBool::new(false),
            next_request_id: AtomicU64::new(1),
            terminal: Mutex::new(None),
            terminal_sender,
            cancellation: CancellationToken::new(),
            child: Mutex::new(child),
        });
        let reader_inner = Arc::clone(&inner);
        let reader_cancellation = inner.cancellation.clone();
        let reader_task = tokio::spawn(async move {
            reader_loop(reader_inner, reader_cancellation, reader).await;
        });
        let writer_inner = Arc::clone(&inner);
        let writer_cancellation = inner.cancellation.clone();
        let writer_task = tokio::spawn(async move {
            writer_loop(writer_inner, writer_cancellation, writer_receiver, writer).await;
        });
        let stderr_task = stderr.map(|reader| tokio::spawn(drain_stderr(reader)));
        Self {
            inner,
            events: tokio::sync::Mutex::new(event_receiver),
            terminal: tokio::sync::Mutex::new(terminal_receiver),
            error_reported: AtomicBool::new(false),
            tasks: tokio::sync::Mutex::new(EventTasks {
                reader: Some(reader_task),
                writer: Some(writer_task),
                stderr: stderr_task,
            }),
        }
    }

    pub(crate) fn validate_before_spawn<T>(
        config: &LaunchConfig,
        spawn: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        config.validate()?;
        spawn()
    }

    pub async fn initialize(&self) -> Result<InitializeResult, Error> {
        if self.inner.initialize_started.swap(true, Ordering::AcqRel) {
            return Err(Error::AlreadyInitialized);
        }
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": {},
            "clientInfo": {"name": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION")}
        });
        let raw = match self.request_value("initialize", params).await {
            Ok(raw) => raw,
            Err(error) => {
                self.inner.fail(error.clone());
                return Err(error);
            }
        };
        let result = (|| {
            let protocol_version = raw
                .get("protocolVersion")
                .and_then(Value::as_u64)
                .ok_or(Error::InvalidResult)?;
            if protocol_version != PROTOCOL_VERSION {
                return Err(Error::UnsupportedProtocolVersion {
                    offered: protocol_version,
                });
            }
            let agent_capabilities = raw
                .get("agentCapabilities")
                .cloned()
                .filter(Value::is_object)
                .ok_or(Error::InvalidResult)?;
            let auth_methods = raw
                .get("authMethods")
                .cloned()
                .filter(Value::is_array)
                .ok_or(Error::InvalidResult)?;
            let agent_info = match raw.get("agentInfo") {
                Some(agent_info) if agent_info.is_object() => Some(agent_info.clone()),
                Some(_) => return Err(Error::InvalidResult),
                None => None,
            };
            Ok(InitializeResult {
                protocol_version,
                agent_capabilities,
                auth_methods,
                agent_info,
                raw,
            })
        })();
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.inner.fail(error.clone());
                return Err(error);
            }
        };
        *lock(&self.inner.initialized) = Some(result.clone());
        Ok(result)
    }

    pub async fn new_session(&self, cwd: impl AsRef<Path>) -> Result<SessionInfo, Error> {
        self.inner.initialized()?;
        self.inner.active()?;
        let cwd = absolute_path(cwd.as_ref())?;
        self.session_operation("session/new", json!({"cwd":cwd,"mcpServers":[]}))
            .await
    }

    pub async fn load_session(
        &self,
        session_id: &str,
        cwd: impl AsRef<Path>,
    ) -> Result<SessionInfo, Error> {
        self.inner.initialized()?;
        self.inner.active()?;
        let cwd = absolute_path(cwd.as_ref())?;
        self.session_operation(
            "session/load",
            json!({"sessionId":session_id,"cwd":cwd,"mcpServers":[]}),
        )
        .await
    }

    pub async fn resume_session(
        &self,
        session_id: &str,
        cwd: impl AsRef<Path>,
    ) -> Result<SessionInfo, Error> {
        self.inner.initialized()?;
        self.inner.active()?;
        let cwd = absolute_path(cwd.as_ref())?;
        self.session_operation(
            "session/resume",
            json!({"sessionId":session_id,"cwd":cwd,"mcpServers":[]}),
        )
        .await
    }

    async fn session_operation(&self, method: &str, params: Value) -> Result<SessionInfo, Error> {
        let raw = self.request_value(method, params).await?;
        let session_id = raw
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidResult)?
            .to_owned();
        Ok(SessionInfo { session_id, raw })
    }

    pub async fn set_mode(&self, session_id: &str, mode_id: &str) -> Result<Value, Error> {
        self.inner.initialized()?;
        self.inner.active()?;
        self.request_value(
            "session/set_mode",
            json!({"sessionId":session_id,"modeId":mode_id}),
        )
        .await
    }

    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<PromptHandle, Error> {
        self.inner.initialized()?;
        self.inner.active()?;
        let mut waiter = self
            .start_request(
                "session/prompt",
                json!({"sessionId":session_id,"prompt":[{"type":"text","text":text}]}),
            )
            .await?;
        waiter._guard.disarm();
        Ok(PromptHandle {
            receiver: waiter.receiver,
        })
    }

    pub async fn cancel(&self, session_id: &str) -> Result<(), Error> {
        self.inner.initialized()?;
        self.inner.active()?;
        self.inner
            .write_value(&notification(
                "session/cancel",
                json!({"sessionId":session_id}),
            ))
            .await?;
        let pending = lock(&self.inner.pending_permissions)
            .iter()
            .filter_map(|(id, item)| (item.session_id == session_id).then_some(id.clone()))
            .collect::<Vec<_>>();
        for id in pending {
            self.respond_permission_outcome(id, PermissionOutcome::Cancelled)
                .await?;
        }
        Ok(())
    }

    pub async fn respond_permission(
        &self,
        request_id: RequestId,
        option_id: &str,
    ) -> Result<(), Error> {
        self.respond_permission_outcome(
            request_id,
            PermissionOutcome::Selected(option_id.to_owned()),
        )
        .await
    }

    async fn respond_permission_outcome(
        &self,
        request_id: RequestId,
        outcome: PermissionOutcome,
    ) -> Result<(), Error> {
        self.inner.active()?;
        let pending = {
            let mut requests = lock(&self.inner.pending_permissions);
            let Some(request) = requests.get(&request_id) else {
                return Err(Error::PermissionRequestClosed);
            };
            if let PermissionOutcome::Selected(option_id) = &outcome
                && !request
                    .options
                    .iter()
                    .any(|available| available == option_id)
            {
                return Err(Error::InvalidPermissionOption);
            }
            requests
                .remove(&request_id)
                .ok_or(Error::PermissionRequestClosed)?
        };
        self.inner.respond_group(
            &pending.reply_group,
            pending.response_index,
            response(&request_id, outcome.into_value()),
        )
    }

    pub async fn recv_event(&self) -> Result<Option<Event>, Error> {
        let mut events = self.events.lock().await;
        let mut terminal = self.terminal.lock().await;
        loop {
            match events.try_recv() {
                Ok(item) => return Ok(Some(item.event)),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return if let Some(error) = terminal.borrow().clone() {
                        self.report_terminal(error)
                    } else {
                        Err(Error::ConnectionClosed)
                    };
                }
                Err(mpsc::error::TryRecvError::Empty) => {}
            }
            if let Some(error) = terminal.borrow().clone() {
                return self.report_terminal(error);
            }
            tokio::select! {
                item = events.recv() => match item {
                    Some(item) => return Ok(Some(item.event)),
                    None => continue,
                },
                changed = terminal.changed() => {
                    if changed.is_err() {
                        return Err(Error::ConnectionClosed);
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_terminal_for_test(&self) -> Result<Error, Error> {
        let mut terminal = self.terminal.lock().await;
        loop {
            if let Some(error) = terminal.borrow().clone() {
                return Ok(error);
            }
            terminal
                .changed()
                .await
                .map_err(|_| Error::ConnectionClosed)?;
        }
    }

    fn report_terminal(&self, error: Error) -> Result<Option<Event>, Error> {
        if self.error_reported.swap(true, Ordering::AcqRel) {
            Ok(None)
        } else {
            Err(error)
        }
    }

    async fn request_value(&self, method: &str, params: Value) -> Result<Value, Error> {
        self.start_request(method, params).await?.wait().await
    }

    async fn start_request(&self, method: &str, params: Value) -> Result<RequestWaiter, Error> {
        self.inner.active()?;
        let permit = Arc::clone(&self.inner.outbound_capacity)
            .try_acquire_owned()
            .map_err(|_| Error::TooManyPendingRequests)?;
        let id_number = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
        let id = RequestId::Number(id_number.to_string());
        let message = request(&id, method, params);
        let line = serde_json::to_vec(&message).map_err(|_| Error::MalformedMessage)?;
        if line.len() > MAX_FRAME_BYTES {
            self.inner.fail(Error::FrameTooLarge);
            return Err(Error::FrameTooLarge);
        }
        let (sender, receiver) = oneshot::channel();
        lock(&self.inner.pending_outgoing).insert(
            id.clone(),
            PendingOutgoing {
                sender,
                _permit: permit,
            },
        );
        let guard = PendingRegistration {
            inner: Arc::downgrade(&self.inner),
            id,
            armed: true,
            enqueued: false,
        };
        let (written, written_receiver) = oneshot::channel();
        let slot = self
            .inner
            .writer
            .reserve()
            .await
            .map_err(|_| Error::ConnectionClosed)?;
        slot.send(WriteCommand {
            line,
            written: Some(written),
        });
        let mut guard = guard;
        guard.enqueued = true;
        written_receiver
            .await
            .map_err(|_| Error::ConnectionClosed)??;
        Ok(RequestWaiter {
            receiver,
            _guard: guard,
        })
    }

    pub async fn shutdown(&self) {
        let child = self.inner.close(Error::Interrupted, false);
        let mut tasks = self.tasks.lock().await;
        if let Some(writer) = tasks.writer.take() {
            let _ = writer.await;
        }
        if let Some(child) = child {
            child.graceful_shutdown().await;
        }
        if let Some(reader) = tasks.reader.take() {
            let _ = reader.await;
        }
        if let Some(stderr) = tasks.stderr.take() {
            let _ = stderr.await;
        }
    }

    #[cfg(test)]
    pub(crate) fn attach_child(&self, child: Box<dyn ChildControl>) {
        *lock(&self.inner.child) = Some(child);
    }

    #[cfg(test)]
    pub(crate) fn available_event_permits(&self) -> usize {
        self.inner.event_budget.available_permits()
    }

    #[cfg(test)]
    pub(crate) fn available_writer_slots_for_test(&self) -> usize {
        self.inner.writer.capacity()
    }

    #[cfg(test)]
    pub(crate) fn drain_stderr<R: AsyncRead + Unpin>(reader: R) -> impl Future<Output = usize> {
        drain_stderr(reader)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.inner.fail(Error::Interrupted);
    }
}

struct RequestWaiter {
    receiver: oneshot::Receiver<Result<Value, Error>>,
    _guard: PendingRegistration,
}

impl RequestWaiter {
    async fn wait(self) -> Result<Value, Error> {
        self.receiver.await.map_err(|_| Error::ConnectionClosed)?
    }
}

struct PendingRegistration {
    inner: Weak<ConnectionInner>,
    id: RequestId,
    armed: bool,
    enqueued: bool,
}

impl PendingRegistration {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingRegistration {
    fn drop(&mut self) {
        if self.armed
            && let Some(inner) = self.inner.upgrade()
        {
            let removed = lock(&inner.pending_outgoing).remove(&self.id).is_some();
            if removed && self.enqueued {
                inner.fail(Error::RequestAbandoned);
            }
        }
    }
}

struct PreparedPermission {
    id: RequestId,
    request: PermissionRequest,
    _inbound_permit: tokio::sync::OwnedSemaphorePermit,
    event_permit: tokio::sync::OwnedSemaphorePermit,
    response_index: usize,
}

struct PreparedNotification {
    event: Event,
    event_permit: tokio::sync::OwnedSemaphorePermit,
}

enum PreparedMessage {
    Response {
        id: RequestId,
        result: Result<Value, i64>,
    },
    Notification(PreparedNotification),
    Permission(PreparedPermission),
    MethodNotFound {
        id: RequestId,
        index: usize,
    },
}

async fn reader_loop<R>(inner: Arc<ConnectionInner>, cancellation: CancellationToken, reader: R)
where
    R: AsyncRead + Unpin,
{
    let mut reader = FrameReader::new(reader);
    loop {
        let frame = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return,
            frame = reader.next() => frame,
        };
        match frame {
            Ok(Some(line)) => {
                if let Err(error) = inner.handle_line(line).await {
                    inner.fail(error);
                    return;
                }
            }
            Ok(None) => {
                inner.fail(Error::TransportFailure);
                return;
            }
            Err(error) => {
                inner.fail(error);
                return;
            }
        }
    }
}

async fn writer_loop<W>(
    inner: Arc<ConnectionInner>,
    cancellation: CancellationToken,
    mut receiver: mpsc::Receiver<WriteCommand>,
    mut writer: W,
) where
    W: AsyncWrite + Unpin,
{
    loop {
        let command = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return,
            command = receiver.recv() => match command {
                Some(command) => command,
                None => return,
            },
        };
        let result = async {
            writer
                .write_all(&command.line)
                .await
                .map_err(|_| Error::TransportFailure)?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|_| Error::TransportFailure)
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(written) = command.written {
                    let _ = written.send(Ok(()));
                }
            }
            Err(error) => {
                if let Some(written) = command.written {
                    let _ = written.send(Err(error.clone()));
                }
                inner.fail(error);
                return;
            }
        }
    }
}

async fn drain_stderr<R: AsyncRead + Unpin>(mut reader: R) -> usize {
    let mut scratch = [0u8; crate::READ_SCRATCH_BYTES];
    loop {
        match reader.read(&mut scratch).await {
            Ok(0) | Err(_) => return 0,
            Ok(_) => {}
        }
    }
}

fn parse_permission_request(id: RequestId, params: Value) -> Result<PermissionRequest, Error> {
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or(Error::MalformedMessage)?
        .to_owned();
    let tool_call = params
        .get("toolCall")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or(Error::MalformedMessage)?;
    let raw_options = params
        .get("options")
        .and_then(Value::as_array)
        .filter(|options| !options.is_empty())
        .ok_or(Error::MalformedMessage)?;
    let mut options = Vec::with_capacity(raw_options.len());
    let mut ids = HashSet::new();
    for option in raw_options {
        let option_id = option
            .get("optionId")
            .and_then(Value::as_str)
            .ok_or(Error::MalformedMessage)?
            .to_owned();
        let name = option
            .get("name")
            .or_else(|| option.get("title"))
            .and_then(Value::as_str)
            .ok_or(Error::MalformedMessage)?
            .to_owned();
        if !ids.insert(option_id.clone()) {
            return Err(Error::MalformedMessage);
        }
        options.push(PermissionOption {
            option_id,
            name,
            kind: option
                .get("kind")
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
    }
    Ok(PermissionRequest {
        request_id: id,
        session_id,
        tool_call,
        options,
        raw: params,
    })
}

fn absolute_path(path: &Path) -> Result<String, Error> {
    if !path.is_absolute() {
        return Err(Error::InvalidLaunchConfiguration);
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(Error::InvalidLaunchConfiguration)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn message_value(message: &RpcMessage) -> Value {
    match message {
        RpcMessage::Request { id, method, params } => request(id, method, params.clone()),
        RpcMessage::Notification { method, params } => notification(method, params.clone()),
        RpcMessage::Response { id, result } => match result {
            Ok(value) => response(id, value.clone()),
            Err(code) => error_response(id, *code, "Agent request failed"),
        },
    }
}
