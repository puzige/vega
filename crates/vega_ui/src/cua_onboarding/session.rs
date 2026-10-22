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
}

pub struct ProbeClient {
    requests: watch::Sender<Option<InspectionRequest>>,
    responses: mpsc::Receiver<InspectionResult>,
    cancel: CancellationToken,
    state: Arc<Mutex<ServiceStatus>>,
}

impl ProbeClient {
    pub fn open() -> Result<Self, AdapterError> {
        let service = service()?;
        if snapshot(&service.state).failure.is_some() {
            return Err(AdapterError::Unavailable);
        }
        let (requests, incoming) = watch::channel(None);
        let (responses, outgoing) = mpsc::channel();
        let cancel = service.cancel.child_token();
        service
            .commands
            .send(ServiceCommand::Attach {
                incoming,
                responses,
                cancel: cancel.clone(),
            })
            .map_err(|_| AdapterError::Unavailable)?;
        Ok(Self {
            requests,
            responses: outgoing,
            cancel,
            state: service.state,
        })
    }

    pub fn inspect(
        &self,
        ticket: CheckTicket,
        prompt: bool,
        probe_direct_capture: bool,
    ) -> Result<(), AdapterError> {
        if prompt || probe_direct_capture {
            return Err(AdapterError::InvalidInspection);
        }
        if self.cancel.is_cancelled() || self.status().failure.is_some() {
            return Err(AdapterError::Unavailable);
        }
        self.requests
            .send(Some(InspectionRequest { ticket }))
            .map_err(|_| AdapterError::Unavailable)
    }

    pub fn try_recv(&self) -> Result<Option<InspectionResult>, AdapterError> {
        match self.responses.try_recv() {
            Ok(response) => Ok(Some(response)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(AdapterError::Unavailable),
        }
    }

    pub fn status(&self) -> ServiceStatus {
        snapshot(&self.state)
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
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
    state: Arc<Mutex<ServiceStatus>>,
    cancel: CancellationToken,
}

enum ServiceCommand {
    Attach {
        incoming: watch::Receiver<Option<InspectionRequest>>,
        responses: mpsc::Sender<InspectionResult>,
        cancel: CancellationToken,
    },
    Shutdown(oneshot::Sender<ServiceStatus>),
}

static SERVICE: OnceLock<Mutex<Option<Service>>> = OnceLock::new();

fn snapshot(state: &Mutex<ServiceStatus>) -> ServiceStatus {
    match state.lock() {
        Ok(state) => state.clone(),
        Err(_) => ServiceStatus {
            cleanup: CleanupStatus::Failed(ComputerUseError::Unavailable),
            last_close_error: None,
            last_inspection_error: None,
            failure: Some("准备检测服务状态不可用，请重启 Vega。".into()),
        },
    }
}

fn set_failure(state: &Mutex<ServiceStatus>, failure: String) {
    tracing::error!(%failure, "preparation service unavailable");
    if let Ok(mut state) = state.lock() {
        state.failure = Some(failure);
    }
}

fn set_cleanup(state: &Mutex<ServiceStatus>, cleanup: CleanupStatus) {
    if let Ok(mut state) = state.lock() {
        state.cleanup = cleanup;
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
    let state = Arc::new(Mutex::new(ServiceStatus::default()));
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
    state: Arc<Mutex<ServiceStatus>>,
    cancel: CancellationToken,
) {
    let gate = Arc::new(tokio::sync::Mutex::new(()));
    let mut workers = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(ServiceCommand::Attach { incoming, responses, cancel }) => {
                    workers.spawn(run_worker(incoming, responses, cancel, state.clone(), gate.clone()));
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
    state: &Mutex<ServiceStatus>,
    gate: &tokio::sync::Mutex<()>,
) -> Result<(), ComputerUseError> {
    let _guard = gate.lock().await;
    set_cleanup(state, CleanupStatus::Reclaiming);
    let close_error = if let Some(mut probe) = probe {
        let error = probe.close().await.err();
        drop(probe);
        error
    } else {
        None
    };
    let result = PreparationProbe::cleanup_pending().await;
    if let Ok(mut state) = state.lock() {
        state.cleanup = match result {
            Ok(()) => CleanupStatus::Clear,
            Err(error) => CleanupStatus::Failed(error),
        };
        state.last_close_error = close_error;
    }
    result
}

async fn run_worker(
    mut incoming: watch::Receiver<Option<InspectionRequest>>,
    responses: mpsc::Sender<InspectionResult>,
    cancel: CancellationToken,
    state: Arc<Mutex<ServiceStatus>>,
    gate: Arc<tokio::sync::Mutex<()>>,
) {
    let mut probe = None;
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            changed = incoming.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
        let Some(request) = incoming.borrow_and_update().clone() else {
            continue;
        };
        if snapshot(&state).failure.is_some() {
            let _ = responses.send(InspectionResult {
                ticket: request.ticket,
                result: Err(ComputerUseError::Unavailable),
            });
            continue;
        }
        let result = inspect(&mut probe, &cancel, &state, &gate).await;
        if !cancel.is_cancelled()
            && let Ok(mut state) = state.lock()
        {
            state.last_inspection_error = result.as_ref().err().copied();
        }
        if result.is_err() && probe.is_some() {
            let _outcome = reclaim(probe.take(), &state, &gate).await;
        }
        if cancel.is_cancelled()
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
    cancel.cancel();
    if probe.is_some() {
        let _outcome = reclaim(probe, &state, &gate).await;
    }
}

async fn inspect(
    probe: &mut Option<PreparationProbe>,
    cancel: &CancellationToken,
    state: &Mutex<ServiceStatus>,
    gate: &tokio::sync::Mutex<()>,
) -> Result<Preparation, ComputerUseError> {
    if probe.is_none() {
        reclaim(None, state, gate).await?;
        *probe = Some(PreparationProbe::open(cancel.clone())?);
    }
    match probe.as_mut() {
        Some(probe) => probe.check().await,
        None => Err(ComputerUseError::Unavailable),
    }
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
