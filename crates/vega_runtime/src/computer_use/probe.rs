use super::{
    Capability, ComponentState, ComputerUseError as Error, ManagedComponents, Preparation,
    PromptMode,
    backend::{
        DESKTOP_TOOLS, desktop_daemon_arguments, desktop_environment, desktop_policy,
        desktop_policy_path, private_directory, server,
    },
    control,
    diagnostics::{self, Stage, checked},
    unknown_preparation,
};
use serde_json::{Value, json};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;
use vega_mcp::ManagedClient;

pub(super) use super::preparation_session::{
    PreparationSession, PreparationTransport, PreparationTransportFactory,
};

pub struct PreparationProbe(PreparationSession<EmbeddedFactory>);

impl PreparationProbe {
    pub fn open(cancel: CancellationToken) -> Result<Self, Error> {
        PreparationSession::open(
            EmbeddedFactory,
            control::OwnershipAuthority::shared(),
            cancel,
        )
        .map(Self)
    }

    pub async fn check(&mut self) -> Result<Preparation, Error> {
        self.0.check().await
    }

    pub async fn close(&mut self) -> Result<(), Error> {
        self.0.close().await
    }

    pub async fn cleanup_pending() -> Result<(), Error> {
        control::OwnershipAuthority::shared()
            .cleanup_pending()
            .await
    }

    pub async fn detect(_mode: PromptMode, cancel: &CancellationToken) -> Preparation {
        let mut session = match Self::open(cancel.clone()) {
            Ok(session) => session,
            Err(error) => return unknown_preparation(component_state(error)),
        };
        let result = session.check().await;
        let _ = session.close().await;
        result.unwrap_or_else(|_| unknown_preparation(ComponentState::Available))
    }
}

struct EmbeddedFactory;

impl PreparationTransportFactory for EmbeddedFactory {
    type Transport = Session;

    fn open(&mut self) -> Result<Session, Error> {
        Session::spawn(&ManagedComponents::detect()?)
    }
}

impl PreparationTransport for Session {
    fn check_permissions(
        &mut self,
        _prompt: bool,
        _probe_direct_capture: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>, Error>> + Send + '_>> {
        Box::pin(async move {
            let cancel = CancellationToken::new();
            tokio::time::timeout(Duration::from_secs(15), self.inspect(&cancel))
                .await
                .map_err(|_| Error::Unavailable)?
        })
    }

    fn cleanup(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>> {
        Box::pin(self.reclaim())
    }
}

fn component_state(error: Error) -> ComponentState {
    match error {
        Error::IncompatibleComponents => ComponentState::Incompatible,
        Error::UnsupportedPlatform => ComponentState::UnsupportedPlatform,
        Error::MissingComponents => ComponentState::Missing,
        _ => ComponentState::Available,
    }
}

struct Session {
    daemon: Child,
    client: Option<ManagedClient>,
    initialized: bool,
    root: PathBuf,
    socket: PathBuf,
    executable: PathBuf,
}

impl Session {
    fn spawn(components: &ManagedComponents) -> Result<Self, Error> {
        let root = private_directory()?;
        let socket = root.join("driver.sock");
        match launch_daemon(components, &root, &socket) {
            Ok(daemon) => Ok(Self {
                daemon,
                client: None,
                initialized: false,
                root,
                socket,
                executable: components.cua.clone(),
            }),
            Err(error) => {
                let _ = std::fs::remove_dir_all(&root);
                Err(error)
            }
        }
    }

    async fn inspect(&mut self, cancel: &CancellationToken) -> Result<Option<Value>, Error> {
        self.connect(cancel).await?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let result = checked(
            Capability::Desktop,
            Stage::Permissions,
            self.client
                .as_mut()
                .ok_or(Error::Unavailable)?
                .call(
                    "check_permissions",
                    json!({"prompt": false, "probe_direct_capture": false}),
                    cancel,
                )
                .await,
        )?;
        Ok(result.structured)
    }

    async fn connect(&mut self, cancel: &CancellationToken) -> Result<(), Error> {
        if self.initialized {
            return Ok(());
        }
        let ready = async {
            for _ in 0..100 {
                if self
                    .daemon
                    .try_wait()
                    .map_err(|_| Error::Unavailable)?
                    .is_some()
                {
                    return Err(Error::Unavailable);
                }
                if self.socket.exists() {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(Error::Unavailable)
        };
        tokio::select! { biased; _=cancel.cancelled()=>return Err(Error::Cancelled),result=ready=>result? }
        self.client = Some(
            ManagedClient::spawn(server(
                self.executable.clone(),
                vec![
                    "mcp".into(),
                    "--embedded".into(),
                    "--host-bundle-id".into(),
                    "ai.vega".into(),
                    "--socket".into(),
                    self.socket.to_string_lossy().into_owned(),
                ],
                &self.root,
            ))
            .map_err(|error| {
                diagnostics::transport(Capability::Desktop, Stage::Initialize, error)
            })?,
        );
        self.client
            .as_mut()
            .ok_or(Error::Unavailable)?
            .initialize("2025-06-18", DESKTOP_TOOLS, cancel)
            .await
            .map_err(|error| {
                diagnostics::transport(Capability::Desktop, Stage::Initialize, error)
            })?;
        self.initialized = true;
        Ok(())
    }

    async fn reclaim(&mut self) -> Result<(), Error> {
        let _ = self.daemon.start_kill();
        if let Some(client) = self.client.as_mut() {
            let stopped = tokio::time::timeout(Duration::from_secs(2), client.shutdown())
                .await
                .map_err(|_| Error::Unavailable)?;
            if !stopped {
                return Err(Error::Unavailable);
            }
            self.client = None;
        }
        tokio::time::timeout(Duration::from_secs(2), self.daemon.wait())
            .await
            .map_err(|_| Error::Unavailable)?
            .map_err(|_| Error::Unavailable)?;
        match std::fs::remove_dir_all(&self.root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(Error::Unavailable),
        }
    }
}

fn launch_daemon(
    components: &ManagedComponents,
    root: &Path,
    socket: &Path,
) -> Result<Child, Error> {
    std::fs::write(
        desktop_policy_path(root),
        serde_json::to_vec(&desktop_policy()).map_err(|_| Error::Unavailable)?,
    )
    .map_err(|_| Error::Unavailable)?;
    Command::new(&components.cua)
        .args(desktop_daemon_arguments(socket))
        .env_clear()
        .envs(desktop_environment(root))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Error::Unavailable)
}
