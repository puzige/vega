use super::{
    Capability, ComputerUseError as Error, ComputerUseRun, DesktopTarget, ManagedComponents,
    Observation, Target, ToolObservationSource,
    diagnostics::{self, Class, Stage, checked},
    feedback::{ExecutionState, ToolFailure},
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
    #[cfg(any(test, debug_assertions))]
    pub(super) test_driver: Option<super::recovery_test_support::Driver>,
}
struct Dispatch<'a> {
    cancel: &'a CancellationToken,
    started: bool,
    input_started: bool,
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

pub(super) fn environment(root: &Path) -> Vec<(OsString, OsString)> {
    let mut env = vec![
        ("TMPDIR".into(), root.join("tmp").into_os_string()),
        ("LANG".into(), "en_US.UTF-8".into()),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        env.push(("HOME".into(), home));
    }
    env
}

pub(super) fn private_directory() -> Result<PathBuf, Error> {
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

pub(super) fn server(executable: PathBuf, args: Vec<String>, root: &Path) -> LocalServer {
    LocalServer {
        credential_policy: None,
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
) -> Error {
    let error = diagnostics::context(capability, stage, error);
    run.control.fail_dispatched_action(epoch, error);
    error
}

impl Backends {
    async fn desktop_windows(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<Vec<DesktopTarget>, Error> {
        #[cfg(any(test, debug_assertions))]
        if let Some(driver) = &self.test_driver {
            return driver.windows();
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
        #[cfg(any(test, debug_assertions))]
        if let Some(driver) = &self.test_driver {
            return driver.call(name, args);
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
        epoch: u64,
        name: &str,
        call_id: &str,
        arguments: Value,
        root_cancel: &CancellationToken,
    ) -> Result<String, ToolFailure> {
        run.check_request_epoch(epoch, root_cancel)
            .map_err(|error| ToolFailure::before_dispatch(name, error))?;
        let token = match run.control.action_token(epoch) {
            Ok(token) => token,
            Err(error) => {
                run.check_request_epoch(epoch, root_cancel)
                    .map_err(|error| ToolFailure::before_dispatch(name, error))?;
                return Err(ToolFailure::before_dispatch(name, error));
            }
        };
        let mut dispatch = Dispatch {
            cancel: &token,
            started: false,
            input_started: false,
        };
        let result = tokio::select! { biased;
            _ = root_cancel.cancelled() => { run.control.stop(); Err(Error::Cancelled) },
            result = self.execute_inner(run,epoch,name,call_id,arguments,&mut dispatch) => result,
        };
        if !dispatch.started && matches!(result, Err(Error::Cancelled | Error::NotReady)) {
            run.check_request_epoch(epoch, root_cancel)
                .map_err(|error| ToolFailure::before_dispatch(name, error))?;
        }
        result.map_err(|error| {
            let capability = ComputerUseRun::tool_capability(name).unwrap_or(Capability::Desktop);
            let error = if dispatch.input_started {
                let error = diagnostics::context(capability, Stage::Action, error);
                run.control.fail_dispatched_action(epoch, error);
                error
            } else {
                error
            };
            ToolFailure::new(
                error,
                capability,
                Stage::for_tool(name),
                if dispatch.input_started {
                    ExecutionState::OutcomeUnknown
                } else if dispatch.started {
                    ExecutionState::Failed
                } else {
                    ExecutionState::NotDispatched
                },
            )
        })
    }

    async fn execute_inner(
        &mut self,
        run: &ComputerUseRun,
        epoch: u64,
        name: &str,
        call_id: &str,
        arguments: Value,
        dispatch: &mut Dispatch<'_>,
    ) -> Result<String, Error> {
        let cancel = dispatch.cancel;
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
            run.consume_observations();
            dispatch.started = true;
            dispatch.input_started = true;
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
                    return Err(action_unknown(run, epoch, capability, Stage::Action, error));
                }
            };
            drop(result);
            if run.control.check_epoch(epoch).is_err() {
                return Err(action_unknown(
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
                return Err(action_unknown(
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
                    dispatch,
                )
                .await
            {
                Ok(id) => Ok(format!(
                    "completed; observation_id={id}; observation images and page data are ephemeral and are not retained in history."
                )),
                Err(error) => Err(action_unknown(
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
                    dispatch.started = true;
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
                    dispatch,
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
        #[cfg(any(test, debug_assertions))]
        if let Some(driver) = &self.test_driver {
            if run.control.status().phase == super::ControlPhase::Disconnected {
                driver.connected();
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
        dispatch: &mut Dispatch<'_>,
    ) -> Result<String, Error> {
        let cancel = dispatch.cancel;
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
                dispatch.started = true;
                let result = self.desktop_call("get_window_state",json!({"pid":target.pid,"window_id":target.window_id,"include_screenshot":true,"include_accessibility_tree":!screenshot_only,"max_elements":200,"max_depth":12,"max_dimension":1600}),cancel).await.map_err(|error| diagnostics::transport(Capability::Desktop, Stage::Observe, error))?;
                super::desktop_observation::apply(&mut observation, &target, result)?;
                if screenshot_only {
                    observation.read_only = true;
                    observation.elements = Value::Null;
                    observation.background_input = super::background::BackgroundInput::default();
                    observation.text = "The image in this result is available to inspect, read visible text, describe or compare. Use this image for the requested written answer as well as the attached screenshot; a separate AX observation is unnecessary for legible visible content. This capture is read-only and cannot authorize actions.".into();
                }
            }
            Target::Browser { .. } => {
                let browser = self.browser.as_mut().ok_or(Error::NotReady)?;
                dispatch.started = true;
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

pub(super) const DESKTOP_TOOLS: &[&str] = &[
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

pub(super) fn desktop_policy() -> Value {
    json!({"allow":{"tools":DESKTOP_TOOLS}})
}

pub(super) fn desktop_policy_path(root: &Path) -> PathBuf {
    root.join("policy.yaml")
}

pub(super) fn desktop_environment(root: &Path) -> Vec<(OsString, OsString)> {
    let mut values = environment(root);
    values.push((
        "CUA_DRIVER_POLICY_FILE".into(),
        desktop_policy_path(root).into_os_string(),
    ));
    values
}

pub(super) fn desktop_daemon_arguments(socket: &Path) -> Vec<OsString> {
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
