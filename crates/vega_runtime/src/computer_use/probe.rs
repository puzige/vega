use super::{
    Capability, ComponentState, ComputerUseError as Error, ManagedComponents, Preparation,
    PromptMode,
    backend::{
        DESKTOP_TOOLS, desktop_daemon_arguments, desktop_environment, desktop_policy,
        desktop_policy_path, private_directory, server,
    },
    control,
    diagnostics::{self, Stage, checked},
    preparation_from_check_permissions, unknown_preparation,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;
use vega_mcp::ManagedClient;

pub struct PreparationProbe;

impl PreparationProbe {
    pub async fn detect(mode: PromptMode, cancel: &CancellationToken) -> Preparation {
        if cancel.is_cancelled() || control::owner_busy() {
            return unknown_preparation(ComponentState::Available);
        }
        let components = match ManagedComponents::detect() {
            Ok(components) => components,
            Err(error) => return unknown_preparation(component_state(error)),
        };
        let Ok(mut session) = Session::spawn(&components) else {
            return unknown_preparation(ComponentState::Available);
        };
        let probe = session.check_permissions(mode, cancel).await;
        session.reclaim().await;
        match probe {
            Ok(structured) => {
                preparation_from_check_permissions(ComponentState::Available, structured.as_ref())
            }
            Err(_) => unknown_preparation(ComponentState::Available),
        }
    }
}

fn component_state(error: Error) -> ComponentState {
    match error {
        Error::IncompatibleComponents => ComponentState::Incompatible,
        Error::UnsupportedPlatform => ComponentState::UnsupportedPlatform,
        _ => ComponentState::Missing,
    }
}

struct Session {
    daemon: Child,
    client: Option<ManagedClient>,
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

    async fn check_permissions(
        &mut self,
        mode: PromptMode,
        cancel: &CancellationToken,
    ) -> Result<Option<Value>, Error> {
        self.connect(cancel).await?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let prompt = mode.prompt();
        let result = checked(
            Capability::Desktop,
            Stage::Permissions,
            self.client
                .as_mut()
                .ok_or(Error::Unavailable)?
                .call(
                    "check_permissions",
                    json!({"prompt": prompt, "probe_direct_capture": prompt}),
                    cancel,
                )
                .await,
        )?;
        Ok(result.structured)
    }

    async fn connect(&mut self, cancel: &CancellationToken) -> Result<(), Error> {
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
            .map_err(|error| diagnostics::transport(Capability::Desktop, Stage::Initialize, error))
    }

    async fn reclaim(mut self) {
        if let Some(client) = self.client.as_mut() {
            let _ = tokio::time::timeout(Duration::from_secs(2), client.shutdown()).await;
        }
        let _ = self.daemon.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.daemon.wait()).await;
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.daemon.start_kill();
        let _ = std::fs::remove_dir_all(&self.root);
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
