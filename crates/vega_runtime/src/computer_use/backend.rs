use super::{
    Capability, ComputerUseError as Error, ComputerUseRun, DesktopTarget, ManagedComponents,
    Observation, Target, ToolObservationSource,
    diagnostics::{self, Class, Stage, checked},
    media::decode_image,
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;
use vega_mcp::{LocalServer, ManagedClient};

#[derive(Default)]
pub(super) struct Backends {
    desktop: Option<Desktop>,
    browser: Option<Browser>,
}
struct Desktop {
    daemon: Child,
    client: Option<ManagedClient>,
    root: PathBuf,
    socket: PathBuf,
    executable: PathBuf,
    selected: bool,
}
struct Browser {
    client: ManagedClient,
    root: PathBuf,
    connection: String,
}

fn environment(root: &Path) -> Vec<(OsString, OsString)> {
    let mut env = vec![
        ("TMPDIR".into(), root.join("tmp").into_os_string()),
        ("LANG".into(), "en_US.UTF-8".into()),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        env.push(("HOME".into(), home));
    }
    env
}

fn private_directory() -> Result<PathBuf, Error> {
    let root = std::env::temp_dir().join(format!("vega-computer-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).map_err(|_| Error::Unavailable)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Unavailable)?;
    }
    std::fs::create_dir(root.join("tmp")).map_err(|_| Error::Unavailable)?;
    Ok(root)
}

fn server(executable: PathBuf, args: Vec<String>, root: &Path) -> LocalServer {
    LocalServer {
        executable,
        args,
        working_directory: root.to_owned(),
        environment: environment(root),
    }
}

fn validate_permissions(result: vega_mcp::ManagedResult) -> Result<(), Error> {
    let value = result.structured.ok_or_else(|| {
        diagnostics::failure(Capability::Desktop, Stage::Permissions, Class::Protocol)
    })?;
    match (
        value["accessibility"].as_bool(),
        value["screen_recording"].as_bool(),
    ) {
        (Some(true), Some(true)) => Ok(()),
        (Some(_), Some(_)) => Err(diagnostics::failure(
            Capability::Desktop,
            Stage::Permissions,
            Class::PermissionsMissing,
        )),
        _ => Err(diagnostics::failure(
            Capability::Desktop,
            Stage::Permissions,
            Class::Protocol,
        )),
    }
}

fn action_unknown(
    run: &ComputerUseRun,
    epoch: u64,
    capability: Capability,
    stage: Stage,
    error: Error,
) -> String {
    let error = diagnostics::context(capability, stage, error);
    run.control.fail(epoch, error);
    format!(
        "outcome_unknown: the action may have been dispatched. Do not retry. The user must inspect the target. {error}"
    )
}

impl Backends {
    pub(super) async fn execute(
        &mut self,
        run: &ComputerUseRun,
        name: &str,
        call_id: &str,
        arguments: Value,
        root_cancel: &CancellationToken,
    ) -> Result<String, Error> {
        let epoch = run.control.snapshot().epoch;
        let token = run.control.action_token(epoch)?;
        let result = tokio::select! { biased;
            _ = root_cancel.cancelled() => { run.control.stop(); Err(Error::Cancelled) },
            result = self.execute_inner(run,epoch,name,call_id,arguments,&token) => result,
        };
        result
    }

    async fn execute_inner(
        &mut self,
        run: &ComputerUseRun,
        epoch: u64,
        name: &str,
        call_id: &str,
        arguments: Value,
        cancel: &CancellationToken,
    ) -> Result<String, Error> {
        let capability = ComputerUseRun::tool_capability(name).ok_or(Error::InvalidAction)?;
        self.ensure_connected(run, capability, epoch, cancel)
            .await?;
        run.control.check_epoch(epoch)?;
        if name.ends_with("Act") {
            let observation_id = arguments["observation_id"]
                .as_str()
                .ok_or(Error::InvalidAction)?;
            let previous = run.control.observation(capability, observation_id)?;
            let (tool, mapped) = map_action(capability, &previous, &arguments)?;
            let include_image = previous.model_image;
            self.validate_target(run, capability, epoch, cancel).await?;
            run.control.check_epoch(epoch)?;
            run.control.discard_observations();
            let dispatched = match capability {
                Capability::Desktop => {
                    self.desktop
                        .as_mut()
                        .and_then(|desktop| desktop.client.as_mut())
                        .ok_or(Error::NotReady)?
                        .call(tool, mapped, cancel)
                        .await
                }
                Capability::Browser => {
                    self.browser
                        .as_mut()
                        .ok_or(Error::NotReady)?
                        .client
                        .call(tool, mapped, cancel)
                        .await
                }
            };
            let result = match checked(capability, Stage::Action, dispatched) {
                Ok(result) => result,
                Err(error) => {
                    return Ok(action_unknown(run, epoch, capability, Stage::Action, error));
                }
            };
            drop(result);
            if run.control.check_epoch(epoch).is_err() {
                return Ok(action_unknown(
                    run,
                    epoch,
                    capability,
                    Stage::Action,
                    Error::Cancelled,
                ));
            }
            if capability == Capability::Browser
                && let Err(error) = self.refresh_browser_target(run, epoch, cancel, true).await
            {
                return Ok(action_unknown(
                    run,
                    epoch,
                    capability,
                    Stage::TargetList,
                    error,
                ));
            }
            match self
                .observe(run, capability, epoch, call_id, include_image, cancel)
                .await
            {
                Ok(id) => Ok(format!(
                    "completed; observation_id={id}; observation images and page data are ephemeral and are not retained in history."
                )),
                Err(error) => Ok(action_unknown(
                    run,
                    epoch,
                    capability,
                    Stage::Observe,
                    error,
                )),
            }
        } else {
            if !arguments.as_object().is_some_and(|object| {
                object
                    .keys()
                    .all(|key| capability == Capability::Browser && key == "include_image")
            }) {
                return Err(Error::InvalidAction);
            }
            let include_image = capability == Capability::Desktop
                || arguments["include_image"].as_bool().unwrap_or(false);
            let id = self
                .observe(run, capability, epoch, call_id, include_image, cancel)
                .await?;
            Ok(format!(
                "completed; observation_id={id}; observation images and page data are ephemeral and are not retained in history."
            ))
        }
    }

    pub(super) async fn ensure_connected(
        &mut self,
        run: &ComputerUseRun,
        capability: Capability,
        epoch: u64,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        if match capability {
            Capability::Desktop => self
                .desktop
                .as_ref()
                .is_some_and(|desktop| desktop.selected),
            Capability::Browser => {
                self.browser.is_some() && run.control.target(capability)?.is_some()
            }
        } {
            return Ok(());
        }
        run.control.begin(capability)?;
        let components = ManagedComponents::detect()?;
        match capability {
            Capability::Desktop => {
                let target = match run.control.target(capability)? {
                    Some(Target::Desktop(target)) => target,
                    _ => {
                        self.desktop = Some(Desktop::spawn(&components, None).await?);
                        let desktop = self.desktop.as_mut().ok_or(Error::Unavailable)?;
                        desktop.connect(cancel, false).await?;
                        let client = desktop.client.as_mut().ok_or(Error::Unavailable)?;
                        let apps = checked(
                            Capability::Desktop,
                            Stage::ListApps,
                            client.call("list_apps", json!({}), cancel).await,
                        )?;
                        let windows = checked(
                            Capability::Desktop,
                            Stage::ListWindows,
                            client.call("list_windows", json!({}), cancel).await,
                        )?;
                        let targets =
                            parse_windows(apps.structured.as_ref(), windows.structured.as_ref())?;
                        run.control.offer_windows(epoch, targets)?;
                        await_grant(run, capability, epoch, cancel).await?;
                        match run.control.target(capability)? {
                            Some(Target::Desktop(target)) => target,
                            _ => return Err(Error::NotReady),
                        }
                    }
                };
                if let Some(desktop) = self.desktop.as_mut()
                    && !desktop.shutdown().await
                {
                    return Err(Error::Unavailable);
                }
                self.desktop = None;
                run.control.check_epoch(epoch)?;
                self.desktop = Some(Desktop::spawn(&components, Some(&target)).await?);
                self.desktop
                    .as_mut()
                    .ok_or(Error::Unavailable)?
                    .connect(cancel, true)
                    .await?;
                run.control
                    .connected(epoch, capability, Target::Desktop(target))?;
            }
            Capability::Browser => {
                let root = private_directory()?;
                let args = vec![
                    components.playwright.to_string_lossy().into_owned(),
                    "--extension".into(),
                    "--no-webmcp".into(),
                    "--image-responses".into(),
                    "allow".into(),
                    "--timeout-action".into(),
                    "5000".into(),
                    "--timeout-navigation".into(),
                    "30000".into(),
                    "--output-dir".into(),
                    root.to_string_lossy().into_owned(),
                ];
                let client = match ManagedClient::spawn(server(components.node, args, &root)) {
                    Ok(client) => client,
                    Err(_) => {
                        let _ = std::fs::remove_dir_all(&root);
                        return Err(Error::BrowserExtension);
                    }
                };
                self.browser = Some(Browser {
                    client,
                    root,
                    connection: ulid::Ulid::generate().to_string(),
                });
                let browser = self.browser.as_mut().ok_or(Error::Unavailable)?;
                browser
                    .client
                    .initialize(
                        "2025-11-25",
                        &[
                            "browser_tabs",
                            "browser_snapshot",
                            "browser_click",
                            "browser_type",
                            "browser_press_key",
                            "browser_navigate",
                            "browser_take_screenshot",
                        ],
                        cancel,
                    )
                    .await
                    .map_err(|error| {
                        diagnostics::transport(Capability::Browser, Stage::Initialize, error)
                    })?;
                for (name, properties) in [
                    ("browser_click", &["target"][..]),
                    ("browser_type", &["target", "text", "submit"][..]),
                    ("browser_snapshot", &["depth", "boxes"][..]),
                    (
                        "browser_take_screenshot",
                        &["type", "scale", "fullPage"][..],
                    ),
                ] {
                    browser
                        .client
                        .require_properties(name, properties)
                        .map_err(|error| {
                            diagnostics::transport(Capability::Browser, Stage::Initialize, error)
                        })?;
                }
                let target = browser.connect_target(cancel).await?;
                if let Some(previous) = run.control.target(capability)? {
                    if !same_browser_target(&previous, &target) {
                        return Err(Error::TargetChanged);
                    }
                    run.control.connected(epoch, capability, target)?;
                } else {
                    run.control.offer_browser(epoch, target)?;
                    await_grant(run, capability, epoch, cancel).await?;
                }
            }
        }
        Ok(())
    }

    async fn validate_target(
        &mut self,
        run: &ComputerUseRun,
        capability: Capability,
        epoch: u64,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        run.control.check_epoch(epoch)?;
        if capability == Capability::Browser {
            self.refresh_browser_target(run, epoch, cancel, false)
                .await?;
        }
        if run.control.target(capability)?.is_none() {
            return Err(Error::NotReady);
        }
        Ok(())
    }

    async fn refresh_browser_target(
        &mut self,
        run: &ComputerUseRun,
        epoch: u64,
        cancel: &CancellationToken,
        navigation: bool,
    ) -> Result<(), Error> {
        let browser = self.browser.as_mut().ok_or(Error::NotReady)?;
        let current = browser.current_target(cancel).await?;
        let previous = run
            .control
            .target(Capability::Browser)?
            .ok_or(Error::NotReady)?;
        let valid = if navigation {
            matches!((&previous,&current),(Target::Browser{connection:a,index:b,..},Target::Browser{connection:c,index:d,..}) if a==c && b==d)
        } else {
            previous == current
        };
        if !valid {
            return Err(Error::TargetChanged);
        }
        run.control.connected(epoch, Capability::Browser, current)
    }

    async fn observe(
        &mut self,
        run: &ComputerUseRun,
        capability: Capability,
        epoch: u64,
        call_id: &str,
        include_image: bool,
        cancel: &CancellationToken,
    ) -> Result<String, Error> {
        self.validate_target(run, capability, epoch, cancel).await?;
        let target = run.control.target(capability)?.ok_or(Error::NotReady)?;
        let mut observation = Observation {
            id: ulid::Ulid::generate().to_string(),
            run_id: run.control.run_id().into(),
            call_id: call_id.into(),
            epoch,
            capability,
            source: ToolObservationSource::CuaWindow,
            model_image: include_image,
            target: target.clone(),
            image: None,
            original_width: 0,
            original_height: 0,
            text: String::new(),
            snapshot_id: None,
            capture_id: None,

            elements: Value::Null,
        };
        match target {
            Target::Desktop(target) => {
                let result = checked(Capability::Desktop, Stage::Observe, self.desktop.as_mut().and_then(|desktop| desktop.client.as_mut()).ok_or(Error::NotReady)?.call("get_window_state",json!({"pid":target.pid,"window_id":target.window_id,"include_screenshot":true,"include_accessibility_tree":true,"max_elements":200,"max_depth":12,"max_dimension":1600}),cancel).await)?;
                let structured = result.structured.ok_or_else(|| {
                    diagnostics::failure(Capability::Desktop, Stage::Observe, Class::Protocol)
                })?;
                if structured["pid"].as_u64() != Some(u64::from(target.pid))
                    || structured["window_id"].as_u64() != Some(u64::from(target.window_id))
                    || structured["screenshot_frame_valid"] != true
                    || structured
                        .get("degraded")
                        .is_some_and(|value| value != &Value::Bool(false) && !value.is_null())
                {
                    return Err(diagnostics::failure(
                        Capability::Desktop,
                        Stage::Observe,
                        Class::ObservationUnavailable,
                    ));
                }
                observation.snapshot_id = Some(
                    structured["snapshot_id"]
                        .as_str()
                        .ok_or(Error::StaleObservation)?
                        .into(),
                );
                observation.capture_id = structured["capture_id"].as_str().map(str::to_owned);
                observation.elements = structured["elements"].clone();
                observation.text = structured["tree_markdown"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(65536)
                    .collect();
                let image = result.images.into_iter().next().ok_or(Error::Image)?;
                let (image, width, height) = decode_image(image)?;
                observation.image = Some(image);
                observation.original_width = width;
                observation.original_height = height;
            }
            Target::Browser { .. } => {
                let browser = self.browser.as_mut().ok_or(Error::NotReady)?;
                let snapshot = checked(
                    Capability::Browser,
                    Stage::Observe,
                    browser
                        .client
                        .call(
                            "browser_snapshot",
                            json!({"depth":12,"boxes":false}),
                            cancel,
                        )
                        .await,
                )?;
                let screenshot = checked(
                    Capability::Browser,
                    Stage::Screenshot,
                    browser
                        .client
                        .call(
                            "browser_take_screenshot",
                            json!({"type":"png","scale":"css","fullPage":false}),
                            cancel,
                        )
                        .await,
                )?;
                browser.clear_output()?;
                observation.source = ToolObservationSource::PlaywrightTab;
                observation.text = snapshot.text.join("\n").chars().take(65536).collect();
                observation.snapshot_id = Some(observation.id.clone());
                let image = screenshot.images.into_iter().next().ok_or(Error::Image)?;
                let (image, width, height) = decode_image(image)?;
                observation.image = Some(image);
                observation.original_width = width;
                observation.original_height = height;
                self.refresh_browser_target(run, epoch, cancel, false)
                    .await?;
            }
        }
        run.control.check_epoch(epoch)?;
        let id = observation.id.clone();
        run.control.publish(observation)?;
        Ok(id)
    }

    pub(super) async fn shutdown(&mut self) -> bool {
        let desktop_closed = match self.desktop.as_mut() {
            Some(desktop) => desktop.shutdown().await,
            None => true,
        };
        if desktop_closed {
            self.desktop = None;
        }
        let browser_closed = match self.browser.as_mut() {
            Some(browser) => browser.shutdown().await,
            None => true,
        };
        if browser_closed {
            self.browser = None;
        }
        desktop_closed && browser_closed
    }
}

impl Desktop {
    async fn spawn(
        components: &ManagedComponents,
        target: Option<&DesktopTarget>,
    ) -> Result<Self, Error> {
        let root = private_directory()?;
        let socket = root.join("driver.sock");
        let manifest = root.join("capabilities.json");
        let tools = if target.is_some() {
            json!([
                "check_permissions",
                "get_window_state",
                "click",
                "type_text",
                "press_key",
                "hotkey",
                "scroll"
            ])
        } else {
            json!(["list_apps", "list_windows", "check_permissions"])
        };
        let desktop = target.map_or_else(||json!({"display":true}),|target|json!({"display":false,"windows":[{"pid":target.pid,"window_id":target.window_id}]}));
        let policy = json!({"version":3,"expires_after":if target.is_some(){"2h"}else{"5m"},"idle_timeout":if target.is_some(){"15m"}else{"2m"},"allow":{"tools":tools},"resources":{"desktop":desktop}});
        std::fs::write(
            &manifest,
            serde_json::to_vec(&policy).map_err(|_| Error::Unavailable)?,
        )
        .map_err(|_| Error::Unavailable)?;
        let daemon = Command::new(&components.cua)
            .args([
                "serve",
                "--embedded",
                "--host-bundle-id",
                "ai.vega",
                "--socket",
            ])
            .arg(&socket)
            .args(["--permission-mode", "bounded", "--capability-manifest"])
            .arg(&manifest)
            .arg("--approve-capability-manifest")
            .env_clear()
            .envs(environment(&root))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            daemon,
            client: None,
            root,
            socket,
            executable: components.cua.clone(),
            selected: false,
        })
    }

    async fn connect(&mut self, cancel: &CancellationToken, selected: bool) -> Result<(), Error> {
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
        let tools: &[&str] = if selected {
            &[
                "check_permissions",
                "get_window_state",
                "click",
                "type_text",
                "press_key",
                "hotkey",
                "scroll",
            ]
        } else {
            &["list_apps", "list_windows", "check_permissions"]
        };
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
            .initialize("2025-06-18", tools, cancel)
            .await
            .map_err(|error| {
                diagnostics::transport(Capability::Desktop, Stage::Initialize, error)
            })?;
        if selected {
            let client = self.client.as_ref().ok_or(Error::Unavailable)?;
            client
                .require_properties("get_window_state", &["pid", "window_id", "max_dimension"])
                .map_err(|error| {
                    diagnostics::transport(Capability::Desktop, Stage::Initialize, error)
                })?;
            for name in ["click", "type_text", "press_key", "hotkey", "scroll"] {
                client
                    .require_properties(name, &["target", "delivery_mode"])
                    .map_err(|error| {
                        diagnostics::transport(Capability::Desktop, Stage::Initialize, error)
                    })?;
            }
        }
        let permissions = checked(
            Capability::Desktop,
            Stage::Permissions,
            self.client
                .as_mut()
                .ok_or(Error::Unavailable)?
                .call(
                    "check_permissions",
                    json!({"prompt":false,"probe_direct_capture":false}),
                    cancel,
                )
                .await,
        )?;
        validate_permissions(permissions)?;
        self.selected = selected;
        Ok(())
    }

    async fn shutdown(&mut self) -> bool {
        if let Some(pid) = self.daemon.id() {
            let stop = Command::new(&self.executable)
                .args(["stop", "--socket"])
                .arg(&self.socket)
                .arg("--expected-pid")
                .arg(pid.to_string())
                .env_clear()
                .envs(environment(&self.root))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .status();
            let _ = tokio::time::timeout(Duration::from_secs(2), stop).await;
        }
        let exited = match tokio::time::timeout(Duration::from_secs(2), self.daemon.wait()).await {
            Ok(result) => result.is_ok(),
            Err(_) => {
                let _ = self.daemon.start_kill();
                self.daemon.wait().await.is_ok()
            }
        };
        let proxy_closed = match self.client.as_mut() {
            Some(client) => client.shutdown().await,
            None => true,
        };
        if exited && proxy_closed {
            let _ = std::fs::remove_dir_all(&self.root);
        }
        exited && proxy_closed
    }
}

impl Browser {
    async fn connect_target(&mut self, cancel: &CancellationToken) -> Result<Target, Error> {
        let result = checked(
            Capability::Browser,
            Stage::TargetList,
            self.client.connect_browser_target(cancel).await,
        )?;
        parse_browser_target(&result.text.join("\n"), &self.connection)
            .map_err(|error| diagnostics::context(Capability::Browser, Stage::TargetList, error))
    }
    async fn current_target(&mut self, cancel: &CancellationToken) -> Result<Target, Error> {
        let result = checked(
            Capability::Browser,
            Stage::TargetList,
            self.client
                .call("browser_tabs", json!({"action":"list"}), cancel)
                .await,
        )?;
        parse_browser_target(&result.text.join("\n"), &self.connection)
            .map_err(|error| diagnostics::context(Capability::Browser, Stage::TargetList, error))
    }
    fn clear_output(&self) -> Result<(), Error> {
        let entries = std::fs::read_dir(&self.root).map_err(|_| Error::Unavailable)?;
        for entry in entries {
            let entry = entry.map_err(|_| Error::Unavailable)?;
            if entry.file_name() == "tmp" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(path).map_err(|_| Error::Unavailable)?;
            } else {
                std::fs::remove_file(path).map_err(|_| Error::Unavailable)?;
            }
        }
        Ok(())
    }
    async fn shutdown(&mut self) -> bool {
        let closed = self.client.shutdown().await;
        if closed {
            let _ = std::fs::remove_dir_all(&self.root);
        }
        closed
    }
}

async fn await_grant(
    run: &ComputerUseRun,
    capability: Capability,
    epoch: u64,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    let mut changes = run.control.subscribe();
    loop {
        run.control.check_epoch(epoch)?;
        if run.control.target(capability)?.is_some() {
            return Ok(());
        }
        tokio::select! {biased;_=cancel.cancelled()=>return Err(Error::Cancelled),result=changes.changed()=>{result.map_err(|_|Error::Cancelled)?;}}
    }
}

fn parse_windows(
    apps: Option<&Value>,
    windows: Option<&Value>,
) -> Result<Vec<DesktopTarget>, Error> {
    let apps = apps
        .and_then(|value| value["apps"].as_array())
        .ok_or(Error::Protocol)?;
    let windows = windows
        .and_then(|value| value["windows"].as_array())
        .ok_or(Error::Protocol)?;
    let mut result = Vec::new();
    for window in windows.iter().take(200) {
        let Some(pid) = window["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid > 0)
        else {
            continue;
        };
        let Some(window_id) = window["window_id"]
            .as_u64()
            .and_then(|id| u32::try_from(id).ok())
        else {
            continue;
        };
        let Some(app) = apps
            .iter()
            .find(|app| app["pid"].as_u64() == Some(u64::from(pid)))
        else {
            continue;
        };
        let bundle_id = app["bundle_id"].as_str().unwrap_or("");
        if bundle_id == "ai.vega" || bundle_id.is_empty() {
            continue;
        }
        result.push(DesktopTarget {
            selection_id: ulid::Ulid::generate().to_string(),
            application: app["name"]
                .as_str()
                .unwrap_or("Application")
                .chars()
                .take(100)
                .collect(),
            bundle_id: bundle_id.into(),
            pid,
            window_id,
            title: window["title"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect(),
        });
    }
    Ok(result)
}

fn parse_browser_target(text: &str, connection: &str) -> Result<Target, Error> {
    let rows: Vec<_> = text.lines().filter(|line| line.starts_with("- ")).collect();
    if rows.len() != 1 {
        return Err(Error::BrowserExtension);
    }
    let mut current = None;
    for line in rows {
        let line = line.strip_prefix("- ").ok_or(Error::Protocol)?;
        let (index, rest) = line.split_once(':').ok_or(Error::Protocol)?;
        let index = index.parse::<u32>().map_err(|_| Error::Protocol)?;
        if index != 0 {
            return Err(Error::TargetChanged);
        }
        if let Some(rest) = rest.strip_prefix(" (current) [") {
            let (title, url) = rest.split_once("](").ok_or(Error::Protocol)?;
            let url = url.strip_suffix(')').ok_or(Error::TargetChanged)?;
            if current.is_some()
                || title.contains(['[', ']', '\n', '\r'])
                || url.contains(['(', ')', '\n', '\r'])
                || !safe_url(url)
            {
                return Err(Error::TargetChanged);
            }
            current = Some(Target::Browser {
                connection: connection.into(),
                index,
                title: title.into(),
                url: url.into(),
            });
        }
    }
    current.ok_or(Error::BrowserExtension)
}

fn same_browser_target(a: &Target, b: &Target) -> bool {
    matches!((a,b),(Target::Browser{index:i,title:t,url:u,..},Target::Browser{index:j,title:v,url:w,..}) if i==j && t==v && u==w)
}
fn safe_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn map_action(
    capability: Capability,
    observation: &Observation,
    args: &Value,
) -> Result<(&'static str, Value), Error> {
    let object = args.as_object().ok_or(Error::InvalidAction)?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "observation_id"
                | "action"
                | "target"
                | "text"
                | "key"
                | "url"
                | "x"
                | "y"
                | "direction"
                | "amount"
        )
    }) {
        return Err(Error::InvalidAction);
    }
    let action = args["action"].as_str().ok_or(Error::InvalidAction)?;
    let text = |key: &str| {
        args[key]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 8192)
            .ok_or(Error::InvalidAction)
    };
    match capability {
        Capability::Desktop => {
            let Target::Desktop(target) = &observation.target else {
                return Err(Error::InvalidAction);
            };
            if observation.snapshot_id.is_none() {
                return Err(Error::StaleObservation);
            }
            let mut value = json!({"target":{"kind":"window","pid":target.pid,"window_id":target.window_id},"delivery_mode":"background"});
            if let Some(token) = args["target"].as_str() {
                if !observation.elements.as_array().is_some_and(|elements| {
                    elements
                        .iter()
                        .any(|element| element["element_token"].as_str() == Some(token))
                }) {
                    return Err(Error::StaleObservation);
                }
                value["element_token"] = json!(token);
                if !args["x"].is_null() || !args["y"].is_null() {
                    return Err(Error::InvalidAction);
                }
            }
            let tool = match action {
                "click" => {
                    if let Some(token) = args["target"].as_str() {
                        value["element_token"] = json!(token);
                    } else {
                        let (x, y) = observation.coordinates(
                            args["x"].as_f64().ok_or(Error::InvalidAction)?,
                            args["y"].as_f64().ok_or(Error::InvalidAction)?,
                        )?;
                        value["x"] = json!(x);
                        value["y"] = json!(y);
                        value["capture_id"] = json!(
                            observation
                                .capture_id
                                .as_ref()
                                .ok_or(Error::StaleObservation)?
                        );
                    }
                    "click"
                }
                "type_text" => {
                    value["text"] = json!(text("text")?);
                    "type_text"
                }
                "press_key" => {
                    value["key"] = json!(text("key")?);
                    "press_key"
                }
                "hotkey" => {
                    let keys: Vec<_> = text("key")?.split('+').collect();
                    if keys.len() < 2 || keys.len() > 4 {
                        return Err(Error::InvalidAction);
                    }
                    value["keys"] = json!(keys);
                    "hotkey"
                }
                "scroll" => {
                    let direction = text("direction")?;
                    if !matches!(direction, "up" | "down" | "left" | "right") {
                        return Err(Error::InvalidAction);
                    }
                    value["direction"] = json!(direction);
                    value["amount"] = json!(
                        args["amount"]
                            .as_u64()
                            .filter(|amount| (1..=10).contains(amount))
                            .ok_or(Error::InvalidAction)?
                    );
                    value["by"] = json!("line");
                    "scroll"
                }
                _ => return Err(Error::InvalidAction),
            };
            Ok((tool, value))
        }
        Capability::Browser => {
            let target = || {
                let target = text("target")?;
                if !target.strip_prefix('e').is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
                }) || !observation.text.contains(&format!("[ref={target}]"))
                {
                    return Err(Error::StaleObservation);
                }
                Ok(target)
            };
            match action {
                "click" => Ok(("browser_click", json!({"target":target()?}))),
                "type_text" => Ok((
                    "browser_type",
                    json!({"target":target()?,"text":text("text")?,"submit":false}),
                )),
                "press_key" => {
                    let key = text("key")?;
                    if !matches!(
                        key,
                        "Enter"
                            | "Tab"
                            | "Escape"
                            | "Backspace"
                            | "Delete"
                            | "ArrowUp"
                            | "ArrowDown"
                            | "ArrowLeft"
                            | "ArrowRight"
                            | "PageUp"
                            | "PageDown"
                            | "Home"
                            | "End"
                            | "Space"
                    ) {
                        return Err(Error::InvalidAction);
                    }
                    Ok(("browser_press_key", json!({"key":key})))
                }
                "scroll" => {
                    let key = match text("direction")? {
                        "up" => "PageUp",
                        "down" => "PageDown",
                        _ => return Err(Error::InvalidAction),
                    };
                    Ok(("browser_press_key", json!({"key":key})))
                }
                "navigate" => {
                    let url = text("url")?;
                    if !safe_url(url) {
                        return Err(Error::InvalidAction);
                    }
                    Ok(("browser_navigate", json!({"url":url})))
                }
                _ => Err(Error::InvalidAction),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vega_mcp::ManagedResult;
    #[test]
    fn issue75_runtime_fixes_unknown_executor_errors_keep_safe_diagnostics() {
        let result = ManagedResult {
            text: vec!["SECRET_PAGE /Users/private token=SECRET_TOKEN permission arbitrary".into()],
            images: Vec::new(),
            structured: Some(json!({"reason":"SECRET_INPUT"})),
            is_error: true,
        };
        let error = checked(Capability::Browser, Stage::TargetList, Ok(result))
            .err()
            .unwrap();
        let output = error.to_string();
        assert!(output.contains("executor=playwright"), "{output}");
        assert!(output.contains("stage=target_list"), "{output}");
        assert!(output.contains("error_class=unknown"), "{output}");
        assert!(!output.contains("SECRET"));
        assert!(!format!("{error:?}").contains("SECRET"));
    }

    #[test]
    fn issue75_runtime_fixes_action_failure_keeps_unknown_and_does_not_restore_revoked_epoch() {
        let run = ComputerUseRun::new(true, false);
        let epoch = run.control.begin(Capability::Desktop).unwrap();
        run.control.pause();
        let output = action_unknown(
            &run,
            epoch,
            Capability::Desktop,
            Stage::Action,
            diagnostics::failure(
                Capability::Desktop,
                Stage::Action,
                Class::BackgroundUnavailable,
            ),
        );
        assert!(output.starts_with("outcome_unknown:"));
        assert!(output.contains("Do not retry"));
        assert!(output.contains("executor=cua stage=action error_class=background_unavailable"));
        assert_eq!(
            run.control.status().phase,
            super::super::ControlPhase::Pausing
        );
        assert!(run.control.check_epoch(epoch).is_err());
        assert!(run.control.snapshot().observations.is_empty());
        run.control.stop();
        run.control.finish_cleanup();
    }

    #[test]
    fn issue75_runtime_fixes_permissions_require_explicit_valid_booleans() {
        for (structured, expected) in [
            (
                json!({"accessibility":false,"screen_recording":true}),
                Some("permissions_missing"),
            ),
            (
                json!({"accessibility":"false","screen_recording":true}),
                Some("protocol"),
            ),
            (json!({"accessibility":true,"screen_recording":true}), None),
        ] {
            let result = ManagedResult {
                text: vec!["SECRET_RAW".into()],
                structured: Some(structured),
                images: Vec::new(),
                is_error: false,
            };
            let actual = validate_permissions(result);
            if let Some(expected) = expected {
                let output = actual.unwrap_err().to_string();
                assert!(output.contains("executor=cua stage=permissions"));
                assert!(output.contains(&format!("error_class={expected}:")));
                assert!(!output.contains("SECRET"));
            } else {
                assert!(actual.is_ok());
            }
        }
    }

    #[test]
    fn issue75_driver_action_maps_only_typed_window_target_and_original_png_coordinates() {
        let control = super::super::ComputerUseControl::new();
        let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
        observation.original_width = 8;
        observation.original_height = 6;
        observation.elements = json!([{"element_token":"ax-1","role":"AXButton","label":"Send"}]);
        let (name, mapped) = map_action(
            Capability::Desktop,
            &observation,
            &json!({"action":"click","x":2.0,"y":1.5}),
        )
        .unwrap();
        assert_eq!(name, "click");
        assert_eq!(mapped["x"], 4.0);
        assert_eq!(mapped["y"], 3.0);
        assert!(mapped.get("pid").is_none());
        assert!(mapped.get("window_id").is_none());
        assert!(mapped.get("scope").is_none());
        assert_eq!(
            mapped["target"],
            json!({"kind":"window","pid":1,"window_id":2})
        );
        assert_eq!(mapped["delivery_mode"], "background");
        assert_eq!(mapped["capture_id"], "capture-0");
        assert!(observation.model_text().contains("ax-1"));
        let (_, mapped) = map_action(
            Capability::Desktop,
            &observation,
            &json!({"action":"type_text","target":"ax-1","text":"typed"}),
        )
        .unwrap();
        assert_eq!(mapped["element_token"], "ax-1");
        assert_eq!(mapped["text"], "typed");
        assert!(
            map_action(
                Capability::Desktop,
                &observation,
                &json!({"action":"click","target":"Send"})
            )
            .is_err()
        );
        assert!(
            map_action(
                Capability::Desktop,
                &observation,
                &json!({"action":"click","target":"ax-1","x":2,"y":1})
            )
            .is_err()
        );
    }

    #[test]
    fn issue75_browser_refs_are_exact_and_navigation_preserves_only_the_single_connection() {
        let control = super::super::ComputerUseControl::new();
        let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
        observation.text = "- button [ref=e10]\n- text ref=button".into();
        assert!(
            map_action(
                Capability::Browser,
                &observation,
                &json!({"action":"click","target":"e1"})
            )
            .is_err()
        );
        assert!(
            map_action(
                Capability::Browser,
                &observation,
                &json!({"action":"click","target":"button"})
            )
            .is_err()
        );
        assert_eq!(
            map_action(
                Capability::Browser,
                &observation,
                &json!({"action":"click","target":"e10"})
            )
            .unwrap()
            .1,
            json!({"target":"e10"})
        );
        assert!(
            parse_browser_target(
                "- 0: (current) [First](https://example.com/first)",
                "connection"
            )
            .is_ok()
        );
        assert!(
            parse_browser_target(
                "- 0: (current) [Second](https://example.com/second)",
                "connection"
            )
            .is_ok()
        );
        assert!(parse_browser_target("- 0: [First](https://example.com/first)\n- 1: (current) [Popup](https://example.com/second)","connection").is_err());
    }

    #[test]
    fn issue75_browser_tab_parser_rejects_ambiguous_or_privileged_targets() {
        assert!(
            parse_browser_target(
                "### Result\n- 0: (current) [Inbox](https://example.com/)",
                "a"
            )
            .is_ok()
        );
        assert!(
            parse_browser_target(
                "- 0: (current) [A](https://a.test)\n- 1: (current) [B](https://b.test)",
                "a"
            )
            .is_err()
        );
        assert!(parse_browser_target("- 0: (current) [A](chrome://extensions)", "a").is_err());
        assert!(parse_browser_target("- 0: (current) [New tab](about:blank)", "a").is_err());
        assert!(!safe_url("javascript:alert(1)"));
        assert!(!safe_url("file:///tmp/private"));
    }
}
