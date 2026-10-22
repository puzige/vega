use super::controller::CheckTicket;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use tokio::sync::{mpsc as async_mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use vega_conversation::types::{ComputerUseError, Preparation, PreparationProbe};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CleanupStatus {
    #[default]
    Clear,
    Reclaiming,
    Failed(ComputerUseError),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceStatus {
    pub cleanup: CleanupStatus,
    pub last_close_error: Option<ComputerUseError>,
    pub last_inspection_error: Option<ComputerUseError>,
    pub failure: Option<String>,
}

impl ServiceStatus {
    pub fn can_start_automatically(&self) -> bool {
        self.cleanup == CleanupStatus::Clear
            && self.last_close_error.is_none()
            && self.last_inspection_error.is_none()
            && self.failure.is_none()
    }
}

#[derive(Clone, Copy)]
pub enum InspectionIntent {
    Automatic,
    Manual,
}

#[derive(Clone)]
enum Admission {
    Automatic(Arc<()>),
    Manual,
}

#[derive(Default)]
struct ServiceState {
    status: ServiceStatus,
    revision: Arc<()>,
    clients: Vec<ClientState>,
    reclaiming: bool,
}

struct ClientState {
    identity: Arc<()>,
    retiring: bool,
}

impl ServiceState {
    fn snapshot(&self) -> ServiceStatus {
        let mut status = self.status.clone();
        if status.cleanup == CleanupStatus::Clear
            && (self.reclaiming || self.clients.iter().any(|client| client.retiring))
        {
            status.cleanup = CleanupStatus::Reclaiming;
        }
        status
    }

    fn validate(
        &self,
        admission: &Admission,
        cancel: &CancellationToken,
    ) -> Result<(), ComputerUseError> {
        if cancel.is_cancelled() {
            return Err(ComputerUseError::Cancelled);
        }
        if self.status.failure.is_some() {
            return Err(ComputerUseError::Unavailable);
        }
        let status = self.snapshot();
        let blocked = match admission {
            Admission::Automatic(revision) => {
                !Arc::ptr_eq(revision, &self.revision) || !status.can_start_automatically()
            }
            Admission::Manual => self.clients.iter().any(|client| client.retiring),
        };
        if blocked {
            return Err(match status.cleanup {
                CleanupStatus::Failed(error) => error,
                _ => status
                    .last_close_error
                    .or(status.last_inspection_error)
                    .unwrap_or(ComputerUseError::NotReady),
            });
        }
        Ok(())
    }
}

#[derive(Clone)]
struct ClientLease {
    identity: Arc<()>,
    cancel: CancellationToken,
    state: Arc<Mutex<ServiceState>>,
}

impl ClientLease {
    fn cancel(&self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(client) = state
                .clients
                .iter_mut()
                .find(|client| Arc::ptr_eq(&client.identity, &self.identity))
                && !client.retiring
            {
                client.retiring = true;
                state.revision = Arc::new(());
            }
            self.cancel.cancel();
        } else {
            self.cancel.cancel();
        }
    }

    fn finish(&self) {
        if let Ok(mut state) = self.state.lock()
            && let Some(index) = state
                .clients
                .iter()
                .position(|client| Arc::ptr_eq(&client.identity, &self.identity))
        {
            state.clients.remove(index);
            state.revision = Arc::new(());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterError {
    Unavailable,
    Spawn(String),
    InvalidInspection,
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("准备检测服务不可用，请重启 Vega 后重试。"),
            Self::Spawn(error) => write!(f, "无法启动准备检测服务：{error}"),
            Self::InvalidInspection => f.write_str("准备检测请求不符合只读权限检查要求。"),
        }
    }
}

impl std::error::Error for AdapterError {}

pub struct InspectionResult {
    pub ticket: CheckTicket,
    pub result: Result<Preparation, ComputerUseError>,
}

#[derive(Clone)]
struct InspectionRequest {
    ticket: CheckTicket,
    admission: Admission,
}

pub struct ProbeClient {
    requests: watch::Sender<Option<InspectionRequest>>,
    responses: mpsc::Receiver<InspectionResult>,
    lease: ClientLease,
}

impl ProbeClient {
    pub fn open() -> Result<Self, AdapterError> {
        let service = service()?;
        if snapshot(&service.state).failure.is_some() {
            return Err(AdapterError::Unavailable);
        }
        let (requests, incoming) = watch::channel(None);
        let (responses, outgoing) = mpsc::channel();
        let lease = ClientLease {
            identity: Arc::new(()),
            cancel: service.cancel.child_token(),
            state: service.state,
        };
        {
            let mut state = lease.state.lock().map_err(|_| AdapterError::Unavailable)?;
            if lease.cancel.is_cancelled() || state.status.failure.is_some() {
                return Err(AdapterError::Unavailable);
            }
            state.clients.push(ClientState {
                identity: lease.identity.clone(),
                retiring: false,
            });
        }
        if service
            .commands
            .send(ServiceCommand::Attach {
                incoming,
                responses,
                lease: lease.clone(),
            })
            .is_err()
        {
            lease.cancel();
            lease.finish();
            return Err(AdapterError::Unavailable);
        }
        Ok(Self {
            requests,
            responses: outgoing,
            lease,
        })
    }

    pub fn inspect(
        &self,
        ticket: CheckTicket,
        prompt: bool,
        probe_direct_capture: bool,
        intent: InspectionIntent,
    ) -> Result<(), AdapterError> {
        if prompt || probe_direct_capture {
            return Err(AdapterError::InvalidInspection);
        }
        let state = self
            .lease
            .state
            .lock()
            .map_err(|_| AdapterError::Unavailable)?;
        if self.lease.cancel.is_cancelled() || state.status.failure.is_some() {
            return Err(AdapterError::Unavailable);
        }
        let admission = match intent {
            InspectionIntent::Automatic => Admission::Automatic(state.revision.clone()),
            InspectionIntent::Manual => Admission::Manual,
        };
        self.requests
            .send(Some(InspectionRequest { ticket, admission }))
            .map_err(|_| AdapterError::Unavailable)
    }

    pub fn try_recv(&self) -> Result<Option<InspectionResult>, AdapterError> {
        match self.responses.try_recv() {
            Ok(response) => Ok(Some(response)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(AdapterError::Unavailable),
        }
    }

    pub fn cancel(&self) {
        self.lease.cancel();
    }
}

impl Drop for ProbeClient {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Clone)]
struct Service {
    commands: async_mpsc::UnboundedSender<ServiceCommand>,
    state: Arc<Mutex<ServiceState>>,
    cancel: CancellationToken,
}

enum ServiceCommand {
    Attach {
        incoming: watch::Receiver<Option<InspectionRequest>>,
        responses: mpsc::Sender<InspectionResult>,
        lease: ClientLease,
    },
    Shutdown(oneshot::Sender<ServiceStatus>),
}

static SERVICE: OnceLock<Mutex<Option<Service>>> = OnceLock::new();

fn snapshot(state: &Mutex<ServiceState>) -> ServiceStatus {
    match state.lock() {
        Ok(state) => state.snapshot(),
        Err(_) => ServiceStatus {
            cleanup: CleanupStatus::Failed(ComputerUseError::Unavailable),
            last_close_error: None,
            last_inspection_error: None,
            failure: Some("准备检测服务状态不可用，请重启 Vega。".into()),
        },
    }
}

fn set_failure(state: &Mutex<ServiceState>, failure: String) {
    tracing::error!(%failure, "preparation service unavailable");
    if let Ok(mut state) = state.lock() {
        state.status.failure = Some(failure);
        state.revision = Arc::new(());
    }
}

pub fn service_status() -> ServiceStatus {
    let Some(service) = SERVICE.get() else {
        return ServiceStatus::default();
    };
    match service.lock() {
        Ok(service) => service
            .as_ref()
            .map_or_else(ServiceStatus::default, |service| snapshot(&service.state)),
        Err(_) => ServiceStatus {
            cleanup: CleanupStatus::Failed(ComputerUseError::Unavailable),
            last_close_error: None,
            last_inspection_error: None,
            failure: Some("准备检测服务状态不可用，请重启 Vega。".into()),
        },
    }
}

fn service() -> Result<Service, AdapterError> {
    let mut slot = SERVICE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| AdapterError::Unavailable)?;
    if let Some(service) = slot.as_ref() {
        return Ok(service.clone());
    }
    let (commands, incoming) = async_mpsc::unbounded_channel();
    let state = Arc::new(Mutex::new(ServiceState::default()));
    let cancel = CancellationToken::new();
    let service = Service {
        commands,
        state: state.clone(),
        cancel: cancel.clone(),
    };
    std::thread::Builder::new()
        .name("preparation-service".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    set_failure(&state, format!("无法启动准备检测服务：{error}"));
                    return;
                }
            };
            runtime.block_on(serve(incoming, state, cancel));
        })
        .map_err(|error| AdapterError::Spawn(error.to_string()))?;
    *slot = Some(service.clone());
    Ok(service)
}

async fn serve(
    mut commands: async_mpsc::UnboundedReceiver<ServiceCommand>,
    state: Arc<Mutex<ServiceState>>,
    cancel: CancellationToken,
) {
    let gate = Arc::new(tokio::sync::Mutex::new(()));
    let mut workers = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(ServiceCommand::Attach { incoming, responses, lease }) => {
                    workers.spawn(run_worker(incoming, responses, lease, gate.clone()));
                }
                Some(ServiceCommand::Shutdown(reply)) => {
                    cancel.cancel();
                    while let Some(result) = workers.join_next().await {
                        if let Err(error) = result {
                            set_failure(&state, format!("准备检测未正常结束：{error}"));
                        }
                    }
                    let _outcome = reclaim(None, &state, &gate).await;
                    let status = snapshot(&state);
                    if status.cleanup != CleanupStatus::Clear || status.failure.is_some() {
                        tracing::error!(?status, "preparation shutdown cleanup unresolved");
                    }
                    let _ = reply.send(status);
                    std::future::pending::<()>().await;
                }
                None => {
                    cancel.cancel();
                    while let Some(result) = workers.join_next().await {
                        if let Err(error) = result {
                            set_failure(&state, format!("准备检测未正常结束：{error}"));
                        }
                    }
                    let _outcome = reclaim(None, &state, &gate).await;
                    std::future::pending::<()>().await;
                }
            },
            result = workers.join_next(), if !workers.is_empty() => {
                if let Some(Err(error)) = result {
                    set_failure(&state, format!("准备检测未正常结束：{error}"));
                }
            }
        }
    }
}

async fn reclaim(
    probe: Option<PreparationProbe>,
    state: &Mutex<ServiceState>,
    gate: &tokio::sync::Mutex<()>,
) -> Result<(), ComputerUseError> {
    let _guard = gate.lock().await;
    reclaim_locked(probe, state, None).await
}

async fn reclaim_locked(
    probe: Option<PreparationProbe>,
    state: &Mutex<ServiceState>,
    recovery: Option<&ClientLease>,
) -> Result<(), ComputerUseError> {
    {
        let mut state = state.lock().map_err(|_| ComputerUseError::Unavailable)?;
        if let Some(lease) = recovery {
            state.validate(&Admission::Manual, &lease.cancel)?;
        }
        state.reclaiming = true;
        state.revision = Arc::new(());
    }
    let retiring_probe = probe.is_some();
    let close_error = if let Some(mut probe) = probe {
        let error = probe.close().await.err();
        drop(probe);
        error
    } else {
        None
    };
    let result = PreparationProbe::cleanup_pending().await;
    if let Ok(mut state) = state.lock() {
        state.reclaiming = false;
        state.status.cleanup = match result {
            Ok(()) => CleanupStatus::Clear,
            Err(error) => CleanupStatus::Failed(error),
        };
        if close_error.is_some() {
            state.status.last_close_error = close_error;
        } else if !retiring_probe && result.is_ok() {
            state.status.last_close_error = None;
        }
        state.revision = Arc::new(());
    }
    result
}

async fn run_worker(
    mut incoming: watch::Receiver<Option<InspectionRequest>>,
    responses: mpsc::Sender<InspectionResult>,
    lease: ClientLease,
    gate: Arc<tokio::sync::Mutex<()>>,
) {
    let mut probe = None;
    loop {
        tokio::select! {
            biased;
            _ = lease.cancel.cancelled() => break,
            changed = incoming.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
        let Some(request) = incoming.borrow_and_update().clone() else {
            continue;
        };
        let result = inspect(&mut probe, &lease, &request.admission, &gate).await;
        if lease.cancel.is_cancelled()
            || responses
                .send(InspectionResult {
                    ticket: request.ticket,
                    result,
                })
                .is_err()
        {
            break;
        }
    }
    lease.cancel();
    if probe.is_some() {
        let _outcome = reclaim(probe, &lease.state, &gate).await;
    }
    lease.finish();
}

async fn inspect(
    probe: &mut Option<PreparationProbe>,
    lease: &ClientLease,
    admission: &Admission,
    gate: &tokio::sync::Mutex<()>,
) -> Result<Preparation, ComputerUseError> {
    let _guard = gate.lock().await;
    if probe.is_none() && matches!(admission, Admission::Manual) {
        reclaim_locked(None, &lease.state, Some(lease)).await?;
    }
    let opened = {
        let state = lease
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        state.validate(admission, &lease.cancel)?;
        if probe.is_none() {
            PreparationProbe::open(lease.cancel.clone()).map(|session| *probe = Some(session))
        } else {
            Ok(())
        }
    };
    let result = match opened {
        Ok(()) => match probe.as_mut() {
            Some(probe) => probe.check().await,
            None => Err(ComputerUseError::Unavailable),
        },
        Err(error) => Err(error),
    };
    if let Ok(mut state) = lease.state.lock()
        && !lease.cancel.is_cancelled()
    {
        state.status.last_inspection_error = result.as_ref().err().copied();
        state.revision = Arc::new(());
    }
    if result.is_err() && probe.is_some() {
        let _outcome = reclaim_locked(probe.take(), &lease.state, None).await;
    }
    result
}

struct QuitHookInstalled;

impl gpui_kit::Global for QuitHookInstalled {}

pub fn install_quit_hook(cx: &mut gpui_kit::App) {
    if cx.has_global::<QuitHookInstalled>() {
        return;
    }
    cx.set_global(QuitHookInstalled);
    cx.on_app_quit(|_| async move {
        let service = SERVICE.get().and_then(|slot| {
            slot.lock()
                .ok()
                .and_then(|service| service.as_ref().cloned())
        });
        if let Some(service) = service {
            service.cancel.cancel();
            let (reply, response) = oneshot::channel();
            if service
                .commands
                .send(ServiceCommand::Shutdown(reply))
                .is_err()
                || response.await.is_err()
            {
                set_failure(&service.state, "退出时无法确认准备检测已回收。".into());
            }
        }
    })
    .detach();
}
