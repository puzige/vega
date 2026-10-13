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
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fixture: Option<DesktopFixture>,
}
#[cfg(any(test, feature = "test-support"))]
pub(super) struct DesktopFixture {
    pub windows: Vec<DesktopTarget>,
    pub results: std::collections::VecDeque<vega_mcp::ManagedResult>,
    pub actions: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub calls: Vec<(String, Value)>,
    pub permissions: std::collections::VecDeque<Value>,
}

#[cfg(any(test, feature = "test-support"))]
impl DesktopFixture {
    fn permissions_result(&mut self) -> vega_mcp::ManagedResult {
        let structured = self.permissions.front().cloned();
        if self.permissions.len() > 1 {
            self.permissions.pop_front();
        }
        vega_mcp::ManagedResult {
            structured,
            text: Vec::new(),
            images: Vec::new(),
            is_error: false,
        }
    }
}

enum CaptureMode {
    Observe { include_image: bool },
    Screenshot,
}

struct Desktop {
    daemon: Child,
    client: Option<ManagedClient>,
    root: PathBuf,
    socket: PathBuf,
    executable: PathBuf,
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

#[derive(Clone, Copy)]
enum DesktopPermissions {
    ScreenRecording,
    AccessibilityAndScreenRecording,
}

impl DesktopPermissions {
    fn for_call(name: &str, arguments: &Value) -> Self {
        if name == "ComputerScreenshot"
            || (name == "ComputerObserve" && arguments.get("target_id").is_none_or(Value::is_null))
        {
            Self::ScreenRecording
        } else {
            Self::AccessibilityAndScreenRecording
        }
    }
}

fn validate_permissions(
    result: vega_mcp::ManagedResult,
    required: DesktopPermissions,
) -> Result<(), Error> {
    let value = result.structured.ok_or_else(|| {
        diagnostics::failure(Capability::Desktop, Stage::Permissions, Class::Protocol)
    })?;
    match (
        value["accessibility"].as_bool(),
        value["screen_recording"].as_bool(),
    ) {
        (Some(accessibility), Some(screen_recording)) => {
            let needs_accessibility = matches!(
                required,
                DesktopPermissions::AccessibilityAndScreenRecording
            );
            let class = match (!accessibility && needs_accessibility, !screen_recording) {
                (false, false) => return Ok(()),
                (true, false) => Class::AccessibilityPermissionMissing,
                (false, true) => Class::ScreenRecordingPermissionMissing,
                (true, true) => Class::PermissionsMissing,
            };
            Err(diagnostics::failure(
                Capability::Desktop,
                Stage::Permissions,
                class,
            ))
        }
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
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn install_fixture(&mut self, fixture: DesktopFixture) {
        self.fixture = Some(fixture);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn fixture_calls(&self) -> Vec<(String, Value)> {
        self.fixture
            .as_ref()
            .map(|fixture| fixture.calls.clone())
            .unwrap_or_default()
    }

    async fn desktop_windows(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<Vec<DesktopTarget>, Error> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(fixture) = &self.fixture {
            return Ok(fixture.windows.clone());
        }
        self.desktop
            .as_mut()
            .ok_or(Error::NotReady)?
            .windows(cancel)
            .await
    }

    async fn desktop_call(
        &mut self,
        name: &str,
        args: Value,
        cancel: &CancellationToken,
    ) -> Result<vega_mcp::ManagedResult, vega_mcp::McpError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(fixture) = &mut self.fixture {
            if name == "check_permissions" {
                return Ok(fixture.permissions_result());
            }
            fixture.calls.push((name.into(), args.clone()));
            if name != "get_window_state" {
                fixture
                    .actions
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            return fixture
                .results
                .pop_front()
                .ok_or(vega_mcp::McpError::InvalidMessage);
        }
        self.desktop
            .as_mut()
            .and_then(|desktop| desktop.client.as_mut())
            .ok_or(vega_mcp::McpError::Transport)?
            .call(name, args, cancel)
            .await
    }

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
            if capability == Capability::Desktop {
                self.check_permissions(DesktopPermissions::AccessibilityAndScreenRecording, cancel)
                    .await?;
            }
            run.control.check_epoch(epoch)?;
            run.control.consume_observations();
            let dispatched = match capability {
                Capability::Desktop => self.desktop_call(tool, mapped, cancel).await,
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
                .observe(
                    run,
                    capability,
                    epoch,
                    call_id,
                    CaptureMode::Observe { include_image },
                    cancel,
                )
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
            if capability == Capability::Desktop {
                self.check_permissions(DesktopPermissions::for_call(name, &arguments), cancel)
                    .await?;
                run.control.check_epoch(epoch)?;
                let (target_id, cursor) = if name == "ComputerScreenshot" {
                    (Some(screenshot_arguments(&arguments)?), None)
                } else {
                    desktop_observe_arguments(&arguments)?
                };
                if let Some(target_id) = target_id {
                    run.control
                        .select_window(epoch, target_id)
                        .map_err(|error| {
                            if error == Error::StaleObservation {
                                Error::TargetExpired
                            } else {
                                error
                            }
                        })?;
                } else {
                    let id = self
                        .discover_desktop(run, epoch, call_id, cursor, cancel)
                        .await?;
                    return Ok(format!(
                        "completed; observation_id={id}; window directory is ephemeral and is not retained in history."
                    ));
                }
            }
            if !arguments.as_object().is_some_and(|object| {
                object.keys().all(|key| {
                    if capability == Capability::Browser {
                        key == "include_image"
                    } else {
                        key == "target_id" || key == "cursor"
                    }
                })
            }) {
                return Err(Error::InvalidAction);
            }
            let include_image = capability == Capability::Desktop
                || arguments["include_image"].as_bool().unwrap_or(false);
            let screenshot = name == "ComputerScreenshot";
            let id = self
                .observe(
                    run,
                    capability,
                    epoch,
                    call_id,
                    if screenshot {
                        CaptureMode::Screenshot
                    } else {
                        CaptureMode::Observe { include_image }
                    },
                    cancel,
                )
                .await?;
            if screenshot {
                let observation = run.control.observation(capability, &id)?;
                return run.retain_screenshot(&observation);
            }
            Ok(format!(
                "completed; observation_id={id}; observation images and page data are ephemeral and are not retained in history."
            ))
        }
    }

    async fn check_permissions(
        &mut self,
        required: DesktopPermissions,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let permissions = checked(
            Capability::Desktop,
            Stage::Permissions,
            self.desktop_call(
                "check_permissions",
                json!({"prompt":false,"probe_direct_capture":false}),
                cancel,
            )
            .await,
        )?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        validate_permissions(permissions, required)
    }

    pub(super) async fn authorize(
        &mut self,
        run: &ComputerUseRun,
        name: &str,
        arguments: &Value,
        epoch: u64,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let capability = ComputerUseRun::tool_capability(name).ok_or(Error::InvalidAction)?;
        self.ensure_connected(run, capability, epoch, cancel)
            .await?;
        if capability == Capability::Desktop {
            self.check_permissions(DesktopPermissions::for_call(name, arguments), cancel)
                .await?;
        }
        run.control.check_epoch(epoch)
    }

    pub(super) async fn ensure_connected(
        &mut self,
        run: &ComputerUseRun,
        capability: Capability,
        epoch: u64,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        #[cfg(any(test, feature = "test-support"))]
        if capability == Capability::Desktop && self.fixture.is_some() {
            if run.control.target(capability)?.is_none() {
                run.control.begin(capability)?;
                run.control
                    .connected(epoch, capability, Target::DesktopCatalog)?;
            }
            return Ok(());
        }
        if match capability {
            Capability::Desktop => self.desktop.is_some(),
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
                self.desktop = Some(Desktop::spawn(&components).await?);
                self.desktop
                    .as_mut()
                    .ok_or(Error::Unavailable)?
                    .connect(cancel)
                    .await?;
                run.control
                    .connected(epoch, capability, Target::DesktopCatalog)?;
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
        if capability == Capability::Desktop {
            let windows = self.desktop_windows(cancel).await?;
            validate_desktop_windows(&run.control, epoch, &windows)?;
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

    async fn discover_desktop(
        &mut self,
        run: &ComputerUseRun,
        epoch: u64,
        call_id: &str,
        cursor: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<String, Error> {
        if cursor.is_none() {
            let windows = self.desktop_windows(cancel).await?;
            run.control.set_windows(epoch, windows)?;
        }
        let page = run.control.catalog_page(epoch, cursor)?;
        let observation = Observation {
            id: ulid::Ulid::generate().to_string(),
            run_id: run.control.run_id().into(),
            call_id: call_id.into(),
            epoch,
            capability: Capability::Desktop,
            source: ToolObservationSource::CuaWindowCatalog,
            model_image: false,
            read_only: false,
            target: Target::DesktopCatalog,
            image: None,
            original_width: 0,
            original_height: 0,
            text: page.to_string(),
            snapshot_id: None,
            capture_id: None,
            elements: Value::Null,
            background_input: super::background::BackgroundInput::default(),
        };
        let id = observation.id.clone();
        run.control.publish(observation)?;
        Ok(id)
    }

    async fn observe(
        &mut self,
        run: &ComputerUseRun,
        capability: Capability,
        epoch: u64,
        call_id: &str,
        mode: CaptureMode,
        cancel: &CancellationToken,
    ) -> Result<String, Error> {
        let (include_image, screenshot_only) = match mode {
            CaptureMode::Observe { include_image } => (include_image, false),
            CaptureMode::Screenshot => (true, true),
        };
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
            read_only: false,
            target: target.clone(),
            image: None,
            original_width: 0,
            original_height: 0,
            text: String::new(),
            snapshot_id: None,
            capture_id: None,

            elements: Value::Null,
            background_input: super::background::BackgroundInput::default(),
        };
        match target {
            Target::DesktopCatalog => return Err(Error::NotReady),
            Target::Desktop(target) => {
                let result = self.desktop_call("get_window_state",json!({"pid":target.pid,"window_id":target.window_id,"include_screenshot":true,"include_accessibility_tree":!screenshot_only,"max_elements":200,"max_depth":12,"max_dimension":1600}),cancel).await.map_err(|error| diagnostics::transport(Capability::Desktop, Stage::Observe, error))?;
                super::desktop_observation::apply(&mut observation, &target, result)?;
                if screenshot_only {
                    observation.read_only = true;
                    observation.elements = Value::Null;
                    observation.background_input = super::background::BackgroundInput::default();
                    observation.text = "Explicit screenshot delivered to this conversation. This capture is read-only and cannot authorize actions.".into();
                }
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

const DESKTOP_TOOLS: &[&str] = &[
    "check_permissions",
    "list_apps",
    "list_windows",
    "get_window_state",
    "click",
    "type_text",
    "press_key",
    "hotkey",
    "scroll",
];

fn desktop_policy() -> Value {
    json!({"allow":{"tools":DESKTOP_TOOLS}})
}

fn desktop_policy_path(root: &Path) -> PathBuf {
    root.join("policy.yaml")
}

fn desktop_environment(root: &Path) -> Vec<(OsString, OsString)> {
    let mut values = environment(root);
    values.push((
        "CUA_DRIVER_POLICY_FILE".into(),
        desktop_policy_path(root).into_os_string(),
    ));
    values
}

fn desktop_daemon_arguments(socket: &Path) -> Vec<OsString> {
    [
        "serve",
        "--embedded",
        "--host-bundle-id",
        "ai.vega",
        "--socket",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([socket.as_os_str().to_owned()])
    .chain(
        ["--permission-mode", "standard"]
            .into_iter()
            .map(OsString::from),
    )
    .collect()
}

fn screenshot_arguments(arguments: &Value) -> Result<&str, Error> {
    let object = arguments
        .as_object()
        .ok_or(Error::InvalidArguments("target_id"))?;
    if object.len() != 1 || !object.contains_key("target_id") {
        return Err(Error::InvalidArguments("target_id"));
    }
    object["target_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or(Error::InvalidArguments("target_id"))
}

fn desktop_observe_arguments(arguments: &Value) -> Result<(Option<&str>, Option<&str>), Error> {
    let object = arguments
        .as_object()
        .ok_or(Error::InvalidArguments("target_id"))?;
    if object
        .keys()
        .any(|key| key != "target_id" && key != "cursor")
    {
        return Err(Error::InvalidArguments("target_id"));
    }
    let parse = |key: &'static str| match object.get(key) {
        Some(Value::String(value)) if !value.is_empty() && value.len() <= 128 => {
            Ok(Some(value.as_str()))
        }
        Some(Value::Null) | None => Ok(None),
        _ => Err(Error::InvalidArguments(key)),
    };
    let target = parse("target_id")?;
    let cursor = parse("cursor")?;
    if target.is_some() && cursor.is_some() {
        return Err(Error::InvalidArguments("cursor"));
    }
    Ok((target, cursor))
}

fn validate_desktop_windows(
    control: &super::ComputerUseControl,
    epoch: u64,
    windows: &[DesktopTarget],
) -> Result<(), Error> {
    control.check_epoch(epoch)?;
    let Some(Target::Desktop(target)) = control.target(Capability::Desktop)? else {
        return Err(Error::NotReady);
    };
    if !windows
        .iter()
        .any(|window| same_desktop_window(window, &target))
    {
        control.discard_observations();
        control.connected(epoch, Capability::Desktop, Target::DesktopCatalog)?;
        return Err(Error::TargetChanged);
    }
    Ok(())
}

fn same_desktop_window(left: &DesktopTarget, right: &DesktopTarget) -> bool {
    left.pid == right.pid && left.window_id == right.window_id && left.bundle_id == right.bundle_id
}

impl Desktop {
    async fn spawn(components: &ManagedComponents) -> Result<Self, Error> {
        let root = private_directory()?;
        let socket = root.join("driver.sock");
        let policy = desktop_policy_path(&root);
        std::fs::write(
            &policy,
            serde_json::to_vec(&desktop_policy()).map_err(|_| Error::Unavailable)?,
        )
        .map_err(|_| Error::Unavailable)?;
        let daemon = Command::new(&components.cua)
            .args(desktop_daemon_arguments(&socket))
            .env_clear()
            .envs(desktop_environment(&root))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                let _ = std::fs::remove_dir_all(&root);
                Error::Unavailable
            })?;
        Ok(Self {
            daemon,
            client: None,
            root,
            socket,
            executable: components.cua.clone(),
        })
    }

    async fn windows(&mut self, cancel: &CancellationToken) -> Result<Vec<DesktopTarget>, Error> {
        let client = self.client.as_mut().ok_or(Error::Unavailable)?;
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
        parse_windows(apps.structured.as_ref(), windows.structured.as_ref())
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
            .map_err(|error| {
                diagnostics::transport(Capability::Desktop, Stage::Initialize, error)
            })?;
        {
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
    }
    async fn current_target(&mut self, cancel: &CancellationToken) -> Result<Target, Error> {
        let result = checked(
            Capability::Browser,
            Stage::TargetList,
            self.client.current_browser_target(cancel).await,
        )?;
        parse_browser_target(&result.text.join("\n"), &self.connection)
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
    if windows.len() > 4096 || apps.len() > 4096 {
        return Err(diagnostics::failure(
            Capability::Desktop,
            Stage::ListWindows,
            Class::LimitExceeded,
        ));
    }
    for window in windows {
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
        let app = apps
            .iter()
            .find(|app| app["pid"].as_u64() == Some(u64::from(pid)));
        let bundle_id = app.and_then(|app| app["bundle_id"].as_str()).unwrap_or("");
        result.push(DesktopTarget {
            selection_id: ulid::Ulid::generate().to_string(),
            application: app
                .and_then(|app| app["name"].as_str())
                .or_else(|| window["app_name"].as_str())
                .unwrap_or("Application")
                .chars()
                .take(100)
                .collect(),
            bundle_id: bundle_id.to_owned(),
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
    let failure = |class| diagnostics::failure(Capability::Browser, Stage::TargetParse, class);
    if text.is_empty() || text == "No open tabs. Navigate to a URL to create one." {
        return Err(failure(Class::TargetCount));
    }
    if text.contains('\r') {
        return Err(failure(Class::TargetFormat));
    }
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line
            .strip_prefix("- ")
            .ok_or_else(|| failure(Class::TargetFormat))?;
        let (index, rest) = line
            .split_once(':')
            .ok_or_else(|| failure(Class::TargetFormat))?;
        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(failure(Class::TargetFormat));
        }
        let index = index
            .parse::<u32>()
            .map_err(|_| failure(Class::TargetFormat))?;
        rows.push((index, rest));
    }
    if rows.len() != 1 {
        return Err(failure(Class::TargetCount));
    }
    let (index, rest) = rows[0];
    if index != 0 {
        return Err(failure(Class::TargetUnavailable));
    }
    let rest = rest.strip_prefix(" (current) [").ok_or_else(|| {
        failure(if rest.starts_with(" [") {
            Class::TargetNotCurrent
        } else {
            Class::TargetFormat
        })
    })?;
    if rest.ends_with(" [crashed]") {
        return Err(failure(Class::TargetUnavailable));
    }
    let (title, url) = rest
        .split_once("](")
        .ok_or_else(|| failure(Class::TargetFormat))?;
    let url = url
        .strip_suffix(')')
        .ok_or_else(|| failure(Class::TargetFormat))?;
    if title.contains(['[', ']']) || url.contains(['(', ')']) {
        return Err(failure(Class::TargetFormat));
    }
    if !safe_url(url) {
        return Err(failure(Class::TargetUnavailable));
    }
    Ok(Target::Browser {
        connection: connection.to_owned(),
        index,
        title: title.to_owned(),
        url: url.to_owned(),
    })
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

pub(super) fn map_action(
    capability: Capability,
    observation: &Observation,
    args: &Value,
) -> Result<(&'static str, Value), Error> {
    if observation.read_only {
        return Err(Error::ReadOnlyObservation);
    }
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
    let text = |key: &'static str| {
        args[key]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 8192)
            .ok_or(Error::InvalidArguments(key))
    };
    match capability {
        Capability::Desktop => {
            let Target::Desktop(target) = &observation.target else {
                return Err(Error::InvalidAction);
            };
            if observation.snapshot_id.is_none() {
                return Err(Error::StaleObservation);
            }
            let element_token = match args.get("target") {
                None | Some(Value::Null) => None,
                Some(Value::String(token)) => Some(token.as_str()),
                _ => return Err(Error::InvalidAction),
            };
            let mut value = json!({"target":{"kind":"window","pid":target.pid,"window_id":target.window_id},"delivery_mode":"background"});
            if let Some(token) = element_token {
                if !observation.elements.as_array().is_some_and(|elements| {
                    elements
                        .iter()
                        .any(|element| element["element_token"].as_str() == Some(token))
                }) {
                    return Err(Error::StaleObservation);
                }
                value["element_token"] = json!(token);
                if !args["x"].is_null() || !args["y"].is_null() {
                    return Err(Error::CoordinateElementConflict);
                }
            }
            if (matches!(action, "hotkey" | "press_key")
                || (action == "scroll" && element_token.is_none()))
                && let Some(class) = observation.background_input.keyboard_refusal()
            {
                return Err(diagnostics::failure(
                    Capability::Desktop,
                    Stage::ActionPreflight,
                    class,
                ));
            }
            let tool = match action {
                "click" => {
                    if let Some(token) = element_token {
                        value["element_token"] = json!(token);
                    } else {
                        let (x, y) = observation.coordinates(
                            args["x"].as_f64().ok_or(Error::InvalidArguments("x"))?,
                            args["y"].as_f64().ok_or(Error::InvalidArguments("y"))?,
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
                        return Err(Error::InvalidArguments("key"));
                    }
                    value["keys"] = json!(keys);
                    "hotkey"
                }
                "scroll" => {
                    let direction = text("direction")?;
                    if !matches!(direction, "up" | "down" | "left" | "right") {
                        return Err(Error::InvalidArguments("direction"));
                    }
                    value["direction"] = json!(direction);
                    value["amount"] = json!(
                        args["amount"]
                            .as_u64()
                            .filter(|amount| (1..=10).contains(amount))
                            .ok_or(Error::InvalidArguments("amount"))?
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
                Some("accessibility_permission_missing"),
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
            let actual =
                validate_permissions(result, DesktopPermissions::AccessibilityAndScreenRecording);
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
        assert!(parse_browser_target("- 0: (current) [Inbox](https://example.com/)", "a").is_ok());
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
    #[test]
    fn issue75_browser_raw_target_parser_rejects_wrappers_and_unparsed_content() {
        let row = "- 0: (current) [Form](https://example.com/form)";
        assert!(parse_browser_target(row, "connection").is_ok());
        for text in [
            format!("### Result\n{row}"),
            format!(
                "### Result\n{row}\n### Page\n- Page URL: https://example.com/form\n- Page Title: Form"
            ),
            format!("### Result\n{row}\n### Events\n- New console entries: secret/path"),
            format!("{row}\nSECRET_EXTRA_CONTENT"),
            format!("SECRET_PREFIX\n{row}"),
            format!("{row}\n\n"),
        ] {
            assert!(parse_browser_target(&text, "connection").is_err());
        }
    }

    #[test]
    fn issue75_browser_raw_target_parser_reports_safe_parse_classes() {
        for (text, class) in [
            ("", "target_count"),
            (
                "No open tabs. Navigate to a URL to create one.",
                "target_count",
            ),
            (
                "- 0: (current) [SECRET_TITLE](https://example.com/SECRET_TOKEN)\n- 1: [Second](https://example.com/other)",
                "target_count",
            ),
            (
                "- 0: [SECRET_TITLE](https://example.com/SECRET_TOKEN)",
                "target_not_current",
            ),
            (
                "- SECRET_INDEX: (current) [SECRET_TITLE](https://example.com/SECRET_TOKEN)",
                "target_format",
            ),
            (
                "- 0: (current) [SECRET_TITLE](https://example.com/SECRET_TOKEN)\nSECRET_INPUT",
                "target_format",
            ),
            (
                "- 0: (current) [SECRET_TITLE](https://example.com/SECRET_TOKEN) [crashed]",
                "target_unavailable",
            ),
            (
                "- 0: (current) [SECRET_TITLE](chrome://SECRET_TOKEN)",
                "target_unavailable",
            ),
            (
                "- 1: (current) [SECRET_TITLE](https://example.com/SECRET_TOKEN)",
                "target_unavailable",
            ),
        ] {
            let error = parse_browser_target(text, "SECRET_CONNECTION").unwrap_err();
            let error = diagnostics::context(Capability::Browser, Stage::TargetList, error);
            let output = error.to_string();
            assert!(output.contains("executor=playwright"), "{output}");
            assert!(output.contains("stage=target_parse"), "{output}");
            assert!(output.contains(&format!("error_class={class}")), "{output}");
            assert!(!output.contains("SECRET"));
            assert!(!format!("{error:?}").contains("SECRET"));
        }
    }
    #[test]
    fn issue75_background_keyboard_preflight_preserves_ax_routes() {
        let control = super::super::ComputerUseControl::new();
        let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
        observation.elements = json!([{"element_token":"ax-fixture"}]);
        let report = json!({
            "exact_window":{"status":"matched","pid":1,"window_id":2},
            "routes":[
                {"route":"accessibility","status":"available"},
                {"route":"window_pointer","status":"available"},
                {"route":"pid_keyboard","status":"refused","reason":"same_pid_keyboard_ambiguity"}
            ]
        });
        observation.background_input =
            super::super::background::BackgroundInput::from_report(&report, 1, 2);
        assert!(
            observation
                .model_text()
                .contains("pid_keyboard=refused:same_pid_keyboard_ambiguity")
        );
        for arguments in [
            json!({"action":"hotkey","key":"cmd+end","target":null}),
            json!({"action":"hotkey","key":"cmd+end","target":"ax-fixture"}),
            json!({"action":"press_key","key":"end"}),
            json!({"action":"scroll","direction":"down","amount":1}),
        ] {
            let error = map_action(Capability::Desktop, &observation, &arguments).unwrap_err();
            let output = error.to_string();
            assert!(output.contains("stage=action_preflight"), "{output}");
            assert!(
                output.contains("error_class=same_pid_keyboard_ambiguity"),
                "{output}"
            );
            assert!(!output.contains("outcome_unknown"));
        }
        assert_eq!(
            map_action(
                Capability::Desktop,
                &observation,
                &json!({"action":"type_text","text":"fixture","target":"ax-fixture"})
            )
            .unwrap()
            .0,
            "type_text"
        );
        assert_eq!(
            map_action(
                Capability::Desktop,
                &observation,
                &json!({"action":"scroll","direction":"down","amount":1,"target":"ax-fixture"})
            )
            .unwrap()
            .0,
            "scroll"
        );
        for target in [
            json!(123),
            json!(true),
            json!({"token":"ax-fixture"}),
            json!(["ax-fixture"]),
        ] {
            assert!(
                map_action(
                    Capability::Desktop,
                    &observation,
                    &json!({"action":"scroll","direction":"down","amount":1,"target":target})
                )
                .is_err()
            );
        }
        let mut available = report;
        available["routes"][2] = json!({"route":"pid_keyboard","status":"available"});
        observation.background_input =
            super::super::background::BackgroundInput::from_report(&available, 1, 2);
        assert!(
            map_action(
                Capability::Desktop,
                &observation,
                &json!({"action":"hotkey","key":"cmd+end"})
            )
            .is_ok()
        );
        observation.background_input = super::super::background::BackgroundInput::default();
        assert!(observation.model_text().contains("pid_keyboard=unknown"));
        assert!(
            map_action(
                Capability::Desktop,
                &observation,
                &json!({"action":"hotkey","key":"cmd+end"})
            )
            .is_ok()
        );
    }

    #[test]
    fn issue75_background_available_report_never_bypasses_fresh_executor_refusal() {
        let run = ComputerUseRun::new(true, false);
        let epoch = run.control.begin(Capability::Desktop).unwrap();
        let mut observation = super::super::tests::fixture_observation(&run.control, epoch, 0);
        observation.background_input = super::super::background::BackgroundInput::from_report(
            &json!({
                "exact_window":{"status":"matched","pid":1,"window_id":2},
                "routes":[
                    {"route":"accessibility","status":"available"},
                    {"route":"window_pointer","status":"available"},
                    {"route":"pid_keyboard","status":"available"}
                ]
            }),
            1,
            2,
        );
        assert!(
            map_action(
                Capability::Desktop,
                &observation,
                &json!({"action":"hotkey","key":"cmd+end"})
            )
            .is_ok()
        );
        let error = checked(Capability::Desktop, Stage::Action, Ok(ManagedResult {
            text: vec!["SECRET_REASON".into()], images: Vec::new(),
            structured: Some(json!({"status":"refused","refusal":{"code":"same_pid_keyboard_ambiguity","reason":"SECRET_REASON"}})),
            is_error: true,
        })).err().unwrap();
        let output = action_unknown(&run, epoch, Capability::Desktop, Stage::Action, error);
        assert!(output.contains("outcome_unknown"));
        assert!(output.contains("Do not retry"));
        assert!(output.contains("error_class=same_pid_keyboard_ambiguity"));
        assert!(!output.contains("SECRET"));
        assert!(run.control.check_epoch(epoch).is_err());
        run.control.stop();
        run.control.finish_cleanup();
    }
    #[tokio::test]
    async fn issue75_catalog_identity_reaches_both_provider_protocols() {
        let run = ComputerUseRun::new(true, false);
        let epoch = run.control.begin(Capability::Desktop).unwrap();
        let apps = json!({"apps":[
            {"pid":1,"name":"Fixture","bundle_id":"org.example.fixture"},
            {"pid":2,"name":"ChatGPT","bundle_id":"com.openai.codex"}
        ]});
        let windows = json!({"windows":(0..201).map(|index| json!({
            "pid":if index == 200 {2} else {1},
            "window_id":index + 1,
            "title":"Fixture window"
        })).collect::<Vec<_>>()});
        let targets = parse_windows(Some(&apps), Some(&windows)).unwrap();
        let selected = targets[200].selection_id.clone();
        run.control.set_windows(epoch, targets).unwrap();
        let first = run.control.catalog_page(epoch, None).unwrap();
        let cursor = first["next_cursor"].as_str().unwrap();
        let mut backend = Backends::default();
        backend
            .discover_desktop(
                &run,
                epoch,
                "directory-page-2",
                Some(cursor),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let mut message = crate::ChatMessage::tool_result("directory-page-2", "completed");
        run.apply_observation("directory-page-2", &mut message);
        let mut request = crate::ChatRequest {
            model: "fixture".into(),
            messages: vec![
                crate::ChatMessage::assistant_with_tools(
                    "",
                    vec![crate::ChatToolCall {
                        id: "directory-page-2".into(),
                        name: "ComputerObserve".into(),
                        input_json: json!({"target_id":null,"cursor":cursor}).to_string(),
                    }],
                ),
                message,
            ],
            ..Default::default()
        };
        run.trim_messages(&mut request.messages);
        let chat = crate::openai::build_request_body(&request);
        assert_eq!(chat["messages"][1]["role"], "tool");
        assert_eq!(chat["messages"][1]["tool_call_id"], "directory-page-2");
        let content = chat["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("ChatGPT"));
        assert!(content.contains("com.openai.codex"));
        assert!(content.contains("CuaWindowCatalog"));
        assert!(!content.contains("org.example.fixture"));
        assert!(request.messages[1].images.is_empty());
        let responses = crate::openai::responses::build_body(&request).unwrap();
        let output = responses["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .unwrap();
        assert_eq!(output["call_id"], "directory-page-2");
        assert_eq!(output["output"], content);
        run.control.select_window(epoch, &selected).unwrap();
        let Some(Target::Desktop(target)) = run.control.target(Capability::Desktop).unwrap() else {
            panic!("selected desktop target");
        };
        assert_eq!(target.bundle_id, "com.openai.codex");
        run.trim_messages(&mut request.messages);
        assert_eq!(
            request.messages[1].content,
            super::super::INVALIDATED_OBSERVATION
        );
        assert!(
            !crate::openai::build_request_body(&request)
                .to_string()
                .contains("com.openai.codex")
        );
        assert!(
            !crate::openai::responses::build_body(&request)
                .unwrap()
                .to_string()
                .contains("com.openai.codex")
        );
        let outcome = crate::agent::outcome(
            Vec::new(),
            request.messages,
            String::new(),
            1,
            1,
            false,
            false,
        );
        assert!(
            !outcome
                .messages
                .iter()
                .any(|message| message.content.contains("com.openai.codex"))
        );
        run.control.stop();
        run.control.finish_cleanup();
    }

    #[test]
    fn issue75_catalog_identity_bounds_keep_full_internal_identity() {
        let boundary = "é".repeat(512);
        let long_a = format!("{boundary}a");
        let long_b = format!("{boundary}b");
        let apps = json!({"apps":[
            {"pid":1,"name":"Boundary","bundle_id":boundary},
            {"pid":2,"name":"Long A","bundle_id":long_a},
            {"pid":3,"name":"Long B","bundle_id":long_b},
            {"pid":4,"name":"Empty","bundle_id":""}
        ]});
        let windows = json!({"windows":(1..=5).map(|pid| json!({
            "pid":pid,"window_id":pid,"app_name":"Helper","title":"Fixture"
        })).collect::<Vec<_>>()});
        let targets = parse_windows(Some(&apps), Some(&windows)).unwrap();
        assert_eq!(targets.len(), 5);
        assert_eq!(targets[1].bundle_id, long_a);
        assert_eq!(targets[2].bundle_id, long_b);
        let mut other_identity = targets[1].clone();
        other_identity.bundle_id = targets[2].bundle_id.clone();
        assert!(!same_desktop_window(&targets[1], &other_identity));
        let control = super::super::ComputerUseControl::new();
        let epoch = control.begin(Capability::Desktop).unwrap();
        control.set_windows(epoch, targets).unwrap();
        let page = control.catalog_page(epoch, None).unwrap();
        assert!(page.to_string().len() <= 64 * 1024);
        assert_eq!(page["windows"][0]["bundle_id"], boundary);
        for item in page["windows"].as_array().unwrap().iter().skip(1) {
            assert_eq!(item.get("bundle_id"), Some(&Value::Null));
        }
        assert_eq!(page["windows"][4]["application"], "Helper");
        control.stop();
        control.finish_cleanup();
    }

    #[test]
    fn issue75_desktop_access_standard_policy_and_discovery_contract() {
        let args = desktop_daemon_arguments(Path::new("/owned/driver.sock"));
        assert!(args.windows(2).any(|pair| pair
            == [
                OsString::from("--permission-mode"),
                OsString::from("standard")
            ]));
        assert!(
            !args
                .iter()
                .any(|arg| arg.to_string_lossy().contains("manifest") || arg == "unrestricted")
        );
        let root = Path::new("/owned");
        let policy_path = desktop_policy_path(root);
        assert_eq!(policy_path.extension().unwrap(), "yaml");
        assert!(
            desktop_environment(root)
                .iter()
                .any(|(name, value)| name == "CUA_DRIVER_POLICY_FILE"
                    && value == policy_path.as_os_str())
        );
        assert!(
            !desktop_environment(root)
                .iter()
                .any(|(name, _)| name == "CUA_DRIVER_CAPABILITY_MANIFEST")
        );
        let policy = desktop_policy();
        assert_eq!(policy.as_object().unwrap().len(), 1);
        assert_eq!(policy["allow"]["tools"], json!(DESKTOP_TOOLS));
        assert!(!DESKTOP_TOOLS.contains(&"shell"));
        assert!(!DESKTOP_TOOLS.contains(&"launch_app"));
        let apps = json!({"apps":[{"pid":1,"name":"Vega","bundle_id":"ai.vega"},{"pid":2,"name":"External","bundle_id":"fixture.other"}]});
        let windows = json!({"windows":[{"pid":1,"window_id":10,"title":"First"},{"pid":2,"window_id":11,"title":"Second"}]});
        let parsed = parse_windows(Some(&apps), Some(&windows)).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].window_id, 11);
        let refreshed = parse_windows(Some(&apps), Some(&windows)).unwrap();
        assert_ne!(parsed[0].selection_id, refreshed[0].selection_id);
        assert!(same_desktop_window(&parsed[0], &refreshed[0]));
        assert!(!same_desktop_window(&parsed[0], &refreshed[1]));
        assert!(desktop_observe_arguments(&json!({"target_id":"one","cursor":"two"})).is_err());
        assert!(desktop_observe_arguments(&json!({"pid":2})).is_err());
        assert!(desktop_observe_arguments(&json!({"target_id":4})).is_err());
        assert_eq!(
            desktop_observe_arguments(&json!({"target_id":null,"cursor":null})).unwrap(),
            (None, None)
        );
    }

    #[test]
    fn issue75_desktop_access_missing_window_invalidates_without_fallback() {
        let control = super::super::ComputerUseControl::new();
        let epoch = control.begin(Capability::Desktop).unwrap();
        let target = DesktopTarget {
            selection_id: "first".into(),
            application: "Fixture".into(),
            bundle_id: "fixture".into(),
            pid: 1,
            window_id: 2,
            title: "Window".into(),
        };
        control.set_windows(epoch, vec![target.clone()]).unwrap();
        control.select_window(epoch, "first").unwrap();
        assert_eq!(
            validate_desktop_windows(&control, epoch, std::slice::from_ref(&target)),
            Ok(())
        );
        let mut replacement = target.clone();
        replacement.pid = 3;
        assert_eq!(
            validate_desktop_windows(&control, epoch, &[replacement]),
            Err(Error::TargetChanged)
        );
        assert_eq!(
            control.target(Capability::Desktop).unwrap(),
            Some(Target::DesktopCatalog)
        );
        assert_eq!(
            control.select_window(epoch, "first"),
            Err(Error::StaleObservation)
        );
        assert_eq!(control.status().phase, super::super::ControlPhase::Running);
        control.set_windows(epoch, vec![target]).unwrap();
        control.select_window(epoch, "first").unwrap();
        assert_eq!(
            validate_desktop_windows(&control, epoch, &[]),
            Err(Error::TargetChanged)
        );
        control.stop();
        assert_eq!(
            validate_desktop_windows(&control, epoch, &[]),
            Err(Error::Cancelled)
        );
        control.finish_cleanup();
    }

    #[test]
    fn issue75_desktop_access_discovers_helper_owned_windows() {
        let apps = json!({"apps":[]});
        let windows = json!({"windows":[{"pid":123,"window_id":456,"app_name":"Open and Save Panel","title":"Open"}]});
        let targets = parse_windows(Some(&apps), Some(&windows)).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].pid, 123);
        assert_eq!(targets[0].window_id, 456);
        assert_eq!(targets[0].application, "Open and Save Panel");
        assert_eq!(targets[0].bundle_id, "");
    }

    #[test]
    fn issue75_desktop_access_catalog_cannot_dispatch_action() {
        let control = super::super::ComputerUseControl::new();
        let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
        observation.target = Target::DesktopCatalog;
        observation.source = ToolObservationSource::CuaWindowCatalog;
        assert!(
            map_action(
                Capability::Desktop,
                &observation,
                &json!({"action":"type_text","text":"fixture","target":null})
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn issue75_observation_read_only_blocks_all_actions_before_dispatch() {
        for mut args in [
            json!({"action":"click","x":1,"y":1}),
            json!({"action":"click","target":"fixture"}),
            json!({"action":"type_text","target":"fixture","text":"fixture"}),
            json!({"action":"press_key","key":"end"}),
            json!({"action":"hotkey","key":"cmd+end"}),
            json!({"action":"scroll","target":"fixture","direction":"down","amount":1}),
        ] {
            let run = ComputerUseRun::new(true, false);
            let epoch = run.control.begin(Capability::Desktop).unwrap();
            let mut observation = super::super::tests::fixture_observation(&run.control, epoch, 0);
            observation.read_only = true;
            observation.elements = json!([{"element_token":"fixture"}]);
            let Target::Desktop(target) = observation.target.clone() else {
                panic!()
            };
            let actions = run.install_desktop_fixture(vec![target], Vec::new()).await;
            run.control
                .connected(epoch, Capability::Desktop, observation.target.clone())
                .unwrap();
            run.control.publish(observation.clone()).unwrap();
            args["observation_id"] = json!(observation.id);
            assert_eq!(
                run.execute(
                    "ComputerAct",
                    "readonly-action",
                    args,
                    &CancellationToken::new()
                )
                .await,
                Err(Error::ReadOnlyObservation)
            );
            assert_eq!(actions.load(std::sync::atomic::Ordering::SeqCst), 0);
            observation.read_only = false;
            assert!(
                map_action(
                    Capability::Desktop,
                    &observation,
                    &json!({"action":"click","x":1,"y":1})
                )
                .is_ok()
            );
            run.control.stop();
            run.control.finish_cleanup();
        }
    }

    #[tokio::test]
    async fn issue75_observation_action_postcheck_failure_stays_unknown_without_retry() {
        let run = ComputerUseRun::new(true, false);
        let epoch = run.control.begin(Capability::Desktop).unwrap();
        let observation = super::super::tests::fixture_observation(&run.control, epoch, 0);
        let Target::Desktop(target) = observation.target.clone() else {
            panic!()
        };
        run.control
            .connected(epoch, Capability::Desktop, observation.target.clone())
            .unwrap();
        run.control.publish(observation.clone()).unwrap();
        let actions = run
            .install_desktop_fixture(
                vec![target],
                vec![
                    vega_mcp::ManagedResult {
                        text: Vec::new(),
                        images: Vec::new(),
                        structured: None,
                        is_error: false,
                    },
                    vega_mcp::ManagedResult {
                        text: vec!["PRIVATE_CAPTURE_REASON".into()],
                        images: Vec::new(),
                        structured: Some(json!({"code":"px_capture_unavailable"})),
                        is_error: true,
                    },
                ],
            )
            .await;
        let outcome = run
            .execute(
                "ComputerAct",
                "action",
                json!({"action":"click","x":1,"y":1,
            "observation_id":observation.id}),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(outcome.starts_with("outcome_unknown"));
        assert!(outcome.contains("Do not retry"));
        assert!(!outcome.contains("PRIVATE"));
        assert_eq!(actions.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            run.control.status().phase,
            super::super::ControlPhase::Failed
        );
        assert!(run.wait_ready(&CancellationToken::new()).await.is_err());
        assert!(run.control.snapshot().observations.is_empty());
        run.control.stop();
        run.control.finish_cleanup();
    }
    #[tokio::test]
    async fn issue75_observation_mcp_error_cannot_hide_conflicting_metadata() {
        for (patch, class) in [
            (
                json!({"pid":9,"window_id":2}),
                Class::ObservationIdentityMismatch,
            ),
            (
                json!({"screenshot_frame_valid":false,"screenshot_error":{"code":"px_frame_mismatch"}}),
                Class::CaptureFrameMismatch,
            ),
            (
                json!({"screenshot_frame_valid":null}),
                Class::ScreenshotUnverified,
            ),
            (
                json!({"degraded":true,"degraded_reason":"PRIVATE_UNKNOWN"}),
                Class::DegradedUnknown,
            ),
        ] {
            let run = ComputerUseRun::new(true, false);
            let epoch = run.control.begin(Capability::Desktop).unwrap();
            let observation = super::super::tests::fixture_observation(&run.control, epoch, 0);
            let Target::Desktop(target) = observation.target.clone() else {
                panic!()
            };
            run.control
                .set_windows(epoch, vec![target.clone()])
                .unwrap();
            let mut structured = json!({"code":"px_capture_unavailable"});
            for (key, value) in patch.as_object().unwrap() {
                structured[key] = value.clone();
            }
            run.install_desktop_fixture(
                vec![target],
                vec![vega_mcp::ManagedResult {
                    text: vec!["PRIVATE_ERROR".into()],
                    images: Vec::new(),
                    structured: Some(structured),
                    is_error: true,
                }],
            )
            .await;
            let error = run
                .execute(
                    "ComputerObserve",
                    "error",
                    json!({"target_id":"fixture","cursor":null}),
                    &CancellationToken::new(),
                )
                .await
                .unwrap_err();
            assert_eq!(
                error,
                diagnostics::failure(Capability::Desktop, Stage::Observe, class)
            );
            assert_eq!(
                run.control.status().phase,
                super::super::ControlPhase::Failed
            );
            assert!(!error.to_string().contains("PRIVATE"));
            run.control.stop();
            run.control.finish_cleanup();
        }
    }

    #[test]
    fn issue75_readonly_permissions_are_scoped_and_strictly_typed() {
        for (ax, sr, expected_read, expected_input) in [
            (true, true, None, None),
            (false, true, None, Some("accessibility_permission_missing")),
            (
                true,
                false,
                Some("screen_recording_permission_missing"),
                Some("screen_recording_permission_missing"),
            ),
            (
                false,
                false,
                Some("screen_recording_permission_missing"),
                Some("permissions_missing"),
            ),
        ] {
            for (required, expected) in [
                (DesktopPermissions::ScreenRecording, expected_read),
                (
                    DesktopPermissions::AccessibilityAndScreenRecording,
                    expected_input,
                ),
            ] {
                let result = validate_permissions(
                    ManagedResult {
                        structured: Some(json!({"accessibility":ax,"screen_recording":sr})),
                        text: vec!["PRIVATE_BODY".into()],
                        images: Vec::new(),
                        is_error: false,
                    },
                    required,
                );
                match expected {
                    None => assert!(result.is_ok()),
                    Some(code) => {
                        let error = result.unwrap_err();
                        let feedback: Value = serde_json::from_str(&error.feedback()).unwrap();
                        assert_eq!(feedback["code"], code);
                        assert_eq!(feedback["retryable"], false);
                        assert_eq!(feedback["next_step"], "open_system_settings");
                        assert!(!error.user_message().contains("executor="));
                        assert!(!error.to_string().contains("PRIVATE"));
                    }
                }
            }
        }
        for malformed in [
            json!({"screen_recording":true}),
            json!({"accessibility":"true","screen_recording":true}),
            json!({"accessibility":false,"screen_recording":1}),
        ] {
            assert!(
                validate_permissions(
                    ManagedResult {
                        structured: Some(malformed),
                        text: Vec::new(),
                        images: Vec::new(),
                        is_error: false,
                    },
                    DesktopPermissions::ScreenRecording
                )
                .unwrap_err()
                .to_string()
                .contains("error_class=protocol")
            );
        }
    }

    #[tokio::test]
    async fn issue75_readonly_permissions_recheck_all_input_after_authorization() {
        for mut args in [
            json!({"action":"click","x":1,"y":1}),
            json!({"action":"type_text","target":"fixture","text":"fixture"}),
            json!({"action":"press_key","key":"end"}),
            json!({"action":"hotkey","key":"cmd+end"}),
            json!({"action":"scroll","target":"fixture","direction":"down","amount":1}),
        ] {
            for (ax, sr, expected) in [
                (false, true, Class::AccessibilityPermissionMissing),
                (true, false, Class::ScreenRecordingPermissionMissing),
                (false, false, Class::PermissionsMissing),
            ] {
                let run = ComputerUseRun::new(true, false);
                let epoch = run.control.begin(Capability::Desktop).unwrap();
                let mut observation =
                    super::super::tests::fixture_observation(&run.control, epoch, 0);
                observation.elements = json!([{"element_token":"fixture"}]);
                let Target::Desktop(target) = observation.target.clone() else {
                    panic!()
                };
                let actions = run.install_desktop_fixture(vec![target], Vec::new()).await;
                run.backend
                    .lock()
                    .await
                    .fixture
                    .as_mut()
                    .unwrap()
                    .permissions = vec![
                    json!({"accessibility":true,"screen_recording":true}),
                    json!({"accessibility":ax,"screen_recording":sr}),
                ]
                .into();
                run.control
                    .connected(epoch, Capability::Desktop, observation.target.clone())
                    .unwrap();
                run.control.publish(observation.clone()).unwrap();
                args["observation_id"] = json!(observation.id);
                assert!(
                    map_action(Capability::Desktop, &observation, &args).is_ok(),
                    "{args}"
                );
                let cancel = CancellationToken::new();
                run.authorize("ComputerAct", &args, &cancel).await.unwrap();
                assert_eq!(
                    run.execute("ComputerAct", "revoked", args.clone(), &cancel)
                        .await,
                    Err(diagnostics::failure(
                        Capability::Desktop,
                        Stage::Permissions,
                        expected
                    ))
                );
                assert_eq!(actions.load(std::sync::atomic::Ordering::SeqCst), 0);
                assert!(run.fixture_calls().await.is_empty());
                assert!(run.control.snapshot().observations.is_empty());
                run.control.stop();
                run.control.finish_cleanup();
            }
        }
    }

    #[tokio::test]
    async fn issue75_readonly_permissions_cached_connection_rechecks_capture_and_observe() {
        for (name, ax, sr, class) in [
            (
                "ComputerScreenshot",
                false,
                false,
                Class::ScreenRecordingPermissionMissing,
            ),
            (
                "ComputerObserve",
                false,
                true,
                Class::AccessibilityPermissionMissing,
            ),
        ] {
            let run = ComputerUseRun::new(true, false);
            let target = DesktopTarget {
                selection_id: "fixture".into(),
                application: "Fixture".into(),
                bundle_id: "test.fixture".into(),
                pid: 1,
                window_id: 2,
                title: "Owned".into(),
            };
            run.install_desktop_fixture(vec![target], Vec::new()).await;
            let cancel = CancellationToken::new();
            run.execute(
                "ComputerObserve",
                "directory",
                json!({"target_id":null,"cursor":null}),
                &cancel,
            )
            .await
            .unwrap();
            run.backend
                .lock()
                .await
                .fixture
                .as_mut()
                .unwrap()
                .permissions = vec![json!({"accessibility":ax,"screen_recording":sr})].into();
            let args = if name == "ComputerScreenshot" {
                json!({"target_id":"fixture"})
            } else {
                json!({"target_id":"fixture","cursor":null})
            };
            assert_eq!(
                run.execute(name, "capture", args, &cancel).await,
                Err(diagnostics::failure(
                    Capability::Desktop,
                    Stage::Permissions,
                    class
                ))
            );
            assert!(run.screenshot("capture").is_none());
            assert!(run.fixture_calls().await.is_empty());
            run.control.stop();
            run.control.finish_cleanup();
        }
    }

    #[tokio::test]
    async fn issue75_readonly_permissions_cancel_after_approval_cannot_capture() {
        for stop in [true, false] {
            let run = ComputerUseRun::new(true, false);
            let target = DesktopTarget {
                selection_id: "fixture".into(),
                application: "Fixture".into(),
                bundle_id: "test.fixture".into(),
                pid: 1,
                window_id: 2,
                title: "Owned".into(),
            };
            run.install_desktop_fixture(vec![target], Vec::new()).await;
            let cancel = CancellationToken::new();
            run.execute(
                "ComputerObserve",
                "directory",
                json!({"target_id":null,"cursor":null}),
                &cancel,
            )
            .await
            .unwrap();
            let args = json!({"target_id":"fixture"});
            run.authorize("ComputerScreenshot", &args, &cancel)
                .await
                .unwrap();
            if stop {
                cancel.cancel();
            } else {
                run.control.pause();
            }
            if !stop {
                run.control.finish_cleanup();
            }
            let task = run.execute("ComputerScreenshot", "cancelled", args, &cancel);
            if stop {
                assert!(task.await.is_err());
            } else {
                tokio::pin!(task);
                assert!(matches!(
                    futures::poll!(&mut task),
                    std::task::Poll::Pending
                ));
                cancel.cancel();
                assert!(task.await.is_err());
            }
            assert!(run.fixture_calls().await.is_empty());
            assert!(run.screenshot("cancelled").is_none());
            run.control.stop();
            run.control.finish_cleanup();
        }
    }
}
