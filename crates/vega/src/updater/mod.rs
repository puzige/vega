mod delta;
mod install;
mod network;
mod platform;
mod signature;

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use vega_conversation::types::{UpdatePhase, UpdateProjection, UpdateRequest};

pub(crate) use install::{acknowledge_startup, run_helper_if_requested};

pub(crate) const RELEASE_PAGE: &str = "https://github.com/puzige/vega/releases/latest";
static INSTALLING: AtomicBool = AtomicBool::new(false);
type UpdateResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn failure(message: &str) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(std::io::Error::other(message.to_string()))
}

pub(crate) fn installing() -> bool {
    INSTALLING.load(Ordering::SeqCst)
}
pub(crate) fn set_installing(value: bool) {
    INSTALLING.store(value, Ordering::SeqCst);
}

pub(crate) enum Event {
    State(UpdateProjection),
    Quit,
}

#[derive(Default)]
pub(crate) struct Updater {
    pub state: UpdateProjection,
    sender: Option<mpsc::SyncSender<UpdateRequest>>,
    busy: Arc<AtomicBool>,
}

impl Updater {
    pub(crate) fn start(&mut self) -> mpsc::Receiver<Event> {
        let (commands, receiver) = mpsc::sync_channel(1);
        let (events, updates) = mpsc::channel();
        self.sender = Some(commands);
        let busy = self.busy.clone();
        if std::thread::Builder::new()
            .name("vega-updater".into())
            .spawn(move || {
                let mut worker = Worker::new(events, busy);
                worker.run(receiver);
            })
            .is_err()
        {
            self.state.phase = UpdatePhase::Error;
            self.state.message = vega_i18n::t("app.update.start_failed").into();
            self.sender = None;
        }
        updates
    }

    pub(crate) fn request(&mut self, request: UpdateRequest) -> bool {
        if matches!(
            request,
            UpdateRequest::Check | UpdateRequest::Automatic(_) | UpdateRequest::Install
        ) && matches!(
            self.state.phase,
            UpdatePhase::Loading
                | UpdatePhase::Checking
                | UpdatePhase::Downloading { .. }
                | UpdatePhase::Installing
        ) {
            return false;
        }
        if matches!(request, UpdateRequest::Check)
            && (self.state.phase == UpdatePhase::Ready
                || self
                    .busy
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err())
        {
            return false;
        }
        if self
            .sender
            .as_ref()
            .is_none_or(|sender| sender.try_send(request).is_err())
        {
            if matches!(request, UpdateRequest::Check) {
                self.busy.store(false, Ordering::SeqCst);
            }
            return false;
        }
        match request {
            UpdateRequest::Check => self.state.phase = UpdatePhase::Checking,
            UpdateRequest::Automatic(_) => self.state.phase = UpdatePhase::Loading,
            UpdateRequest::Install => self.state.phase = UpdatePhase::Installing,
            UpdateRequest::Later | UpdateRequest::ReleasePage => {}
        }
        true
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    automatic: bool,
    last_checked: Option<u64>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            automatic: true,
            last_checked: None,
        }
    }
}

struct Worker {
    events: mpsc::Sender<Event>,
    state: UpdateProjection,
    preferences: Preferences,
    bundle: Option<platform::Bundle>,
    staging: Option<tempfile::TempDir>,
    release: Option<network::Release>,
    delta: bool,
    config: Option<PathBuf>,
    busy: Arc<AtomicBool>,
}

impl Worker {
    fn new(events: mpsc::Sender<Event>, busy: Arc<AtomicBool>) -> Self {
        let mut worker = Self {
            events,
            busy,
            state: UpdateProjection::default(),
            preferences: Preferences::default(),
            bundle: platform::discover().ok(),
            staging: None,
            release: None,
            delta: false,
            config: vega_store::paths::config_dir().map(|root| root.join("updater.json")),
        };
        match worker.load_preferences() {
            Ok(preferences) => worker.preferences = preferences,
            Err(_) => {
                worker.preferences.automatic = false;
                worker.state.message = vega_i18n::t("app.update.preferences_read").into();
            }
        }
        worker.state.automatic = worker.preferences.automatic;
        worker.state.last_checked = worker.preferences.last_checked;
        worker.state.current_version = worker
            .bundle
            .as_ref()
            .map(|bundle| bundle.version.clone())
            .unwrap_or_else(|| vega_i18n::t("app.update.development").into());
        worker.state.phase = UpdatePhase::Idle;
        if worker.state.message.is_empty() {
            worker.state.message = match &worker.bundle {
                Some(bundle) => bundle
                    .installation_error
                    .clone()
                    .unwrap_or_else(|| vega_i18n::t("app.update.verification_notice").into()),
                None => vega_i18n::t("app.update.development_check_only").into(),
            };
        }
        if std::env::args_os().any(|arg| arg == "--vega-update-failed") {
            worker.state.message = vega_i18n::t("app.update.rolled_back").into();
        }
        worker.publish();
        worker
    }

    fn load_preferences(&self) -> UpdateResult<Preferences> {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let path = self
            .config
            .as_ref()
            .ok_or_else(|| failure(vega_i18n::t("app.update.config_unavailable")))?;
        let mut file = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Preferences::default());
            }
            Err(error) => return Err(Box::new(error)),
        };
        if !file.metadata()?.is_file() || file.metadata()?.len() > 4096 {
            return Err(failure(vega_i18n::t("app.update.preferences_invalid")));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        serde_json::from_slice(&bytes)
            .map_err(|_| failure(vega_i18n::t("app.update.preferences_invalid")))
    }

    fn persist(&self) -> UpdateResult<()> {
        use std::io::Write;
        let path = self
            .config
            .as_ref()
            .ok_or_else(|| failure(vega_i18n::t("app.update.config_unavailable")))?;
        let root = path
            .parent()
            .ok_or_else(|| failure(vega_i18n::t("app.update.config_unavailable")))?;
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(
            &serde_json::to_vec(&self.preferences)
                .map_err(|_| failure(vega_i18n::t("app.update.preferences_invalid")))?,
        )?;
        file.as_file().sync_all()?;
        file.persist(path)
            .map_err(|_| failure(vega_i18n::t("app.update.preferences_save")))?;
        Ok(())
    }

    fn publish(&self) {
        let _ = self.events.send(Event::State(self.state.clone()));
    }

    fn run(&mut self, receiver: mpsc::Receiver<UpdateRequest>) {
        let started = std::time::Instant::now();
        loop {
            match receiver.recv_timeout(Duration::from_secs(10)) {
                Ok(UpdateRequest::Check) => self.check(true),
                Ok(UpdateRequest::Automatic(value)) => {
                    let old = self.preferences.automatic;
                    self.preferences.automatic = value;
                    if self.persist().is_err() {
                        self.preferences.automatic = old;
                        self.state.message = vega_i18n::t("app.update.preferences_retry").into();
                    } else {
                        self.state.automatic = value;
                    }
                    self.publish();
                }
                Ok(UpdateRequest::Install) => self.install(),
                Ok(UpdateRequest::Later) => {
                    self.state.message = vega_i18n::t("app.update.later").into();
                    self.publish();
                }
                Ok(UpdateRequest::ReleasePage) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if started.elapsed() >= Duration::from_secs(10)
                        && self.preferences.automatic
                        && self.staging.is_none()
                        && self
                            .preferences
                            .last_checked
                            .is_none_or(|last| now().saturating_sub(last) >= 24 * 60 * 60)
                        && self
                            .busy
                            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()
                    {
                        self.check(false);
                    }
                }
            }
        }
    }

    fn check(&mut self, manual: bool) {
        if self.staging.is_some() {
            self.busy.store(false, Ordering::SeqCst);
            self.publish();
            return;
        }
        self.preferences.last_checked = Some(now());
        self.state.last_checked = self.preferences.last_checked;
        self.state.phase = UpdatePhase::Checking;
        self.state.message = vega_i18n::t("app.update.checking").into();
        self.publish();
        let result = self.persist().and_then(|_| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(self.check_release())
        });
        if let Err(error) = result {
            let update_available = matches!(
                self.state.phase,
                UpdatePhase::Available | UpdatePhase::Downloading { .. }
            );
            self.state.phase = if update_available {
                UpdatePhase::Available
            } else if manual {
                UpdatePhase::Error
            } else {
                UpdatePhase::Idle
            };
            self.state.message = if manual || update_available {
                error.to_string()
            } else {
                vega_i18n::t("app.update.automatic_retry").into()
            };
            self.publish();
        }
        self.busy.store(false, Ordering::SeqCst);
    }

    async fn check_release(&mut self) -> UpdateResult<()> {
        let client = network::client()?;
        let release = network::latest(&client).await?;
        network::asset(&release, network::ASSET, network::DOWNLOAD_LIMIT)?;
        let version = release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name)
            .to_string();
        let latest = network::Version::parse(&version)?;
        self.state.latest_version = Some(version.clone());
        self.state.notes = release
            .body
            .as_deref()
            .unwrap_or_default()
            .chars()
            .take(16_000)
            .collect();
        if self
            .bundle
            .as_ref()
            .and_then(|bundle| network::Version::parse(&bundle.version).ok())
            .is_some_and(|current| latest <= current)
        {
            self.state.phase = UpdatePhase::Current;
            self.state.message = vega_i18n::t("app.update.current").into();
            self.publish();
            return Ok(());
        }
        self.state.phase = UpdatePhase::Available;
        self.state.message = vega_i18n::t("app.update.available_verifying").into();
        self.publish();
        let Some(bundle) = self.bundle.clone() else {
            self.state.message = vega_i18n::t("app.update.development_install").into();
            self.publish();
            return Ok(());
        };
        if let Some(error) = &bundle.installation_error {
            return Err(failure(error));
        }
        platform::validate_target_path(&bundle.path)?;
        let parent = bundle
            .path
            .parent()
            .ok_or_else(|| failure(vega_i18n::t("app.update.install_directory")))?;
        let staging = platform::private_tempdir_in(parent, ".vega-update-")
            .map_err(|_| failure(vega_i18n::t("app.update.install_read_only")))?;
        platform::private_dir(staging.path())?;
        self.state.phase = UpdatePhase::Downloading {
            received: 0,
            total: 0,
        };
        self.state.message = vega_i18n::t("app.update.downloading").into();
        self.publish();
        self.delta = network::download(
            &client,
            &release,
            staging.path(),
            &version,
            &bundle,
            true,
            |received, total| {
                self.state.phase = UpdatePhase::Downloading { received, total };
                self.publish();
            },
        )
        .await?;
        self.state.message = vega_i18n::t("app.update.verifying").into();
        self.publish();
        self.release = Some(release);
        self.staging = Some(staging);
        self.state.phase = UpdatePhase::Ready;
        self.state.message = vega_i18n::t("app.update.ready").into();
        self.publish();
        Ok(())
    }

    fn install(&mut self) {
        let mut discard_staging = false;
        let result = (|| -> UpdateResult<()> {
            if !installing() {
                return Err(failure(vega_i18n::t("app.update.restart_unconfirmed")));
            }
            let bundle = self
                .bundle
                .as_ref()
                .ok_or_else(|| failure(vega_i18n::t("app.update.cannot_install")))?;
            let staging = self
                .staging
                .as_ref()
                .ok_or_else(|| failure(vega_i18n::t("app.update.not_ready")))?;
            let version = self
                .state
                .latest_version
                .clone()
                .ok_or_else(|| failure(vega_i18n::t("app.update.version_unavailable")))?;
            self.state.phase = UpdatePhase::Installing;
            self.state.message = vega_i18n::t("app.update.restarting").into();
            self.publish();
            prepare_installation(
                &mut self.delta,
                &mut discard_staging,
                |delta| install::prepare(bundle, staging.path(), &version, delta),
                || {
                    let release = self
                        .release
                        .as_ref()
                        .ok_or_else(|| failure(vega_i18n::t("app.update.release_unavailable")))?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    runtime.block_on(async {
                        let client = network::client()?;
                        network::download(
                            &client,
                            release,
                            staging.path(),
                            &version,
                            bundle,
                            false,
                            |received, total| {
                                self.state.phase = UpdatePhase::Downloading { received, total };
                                let _ = self.events.send(Event::State(self.state.clone()));
                            },
                        )
                        .await
                    })?;
                    self.state.phase = UpdatePhase::Installing;
                    let _ = self.events.send(Event::State(self.state.clone()));
                    Ok(())
                },
            )?;
            if let Some(staging) = self.staging.take() {
                let _ = staging.keep();
            }
            let _ = self.events.send(Event::Quit);
            Ok(())
        })();
        if let Err(error) = result {
            set_installing(false);
            if discard_staging {
                self.staging = None;
            }
            self.state.phase = if self.staging.is_some() {
                UpdatePhase::Ready
            } else {
                UpdatePhase::Error
            };
            self.state.message = error.to_string();
            self.publish();
        }
    }
}

fn prepare_installation(
    delta: &mut bool,
    discard_staging: &mut bool,
    mut prepare: impl FnMut(bool) -> UpdateResult<()>,
    mut full: impl FnMut() -> UpdateResult<()>,
) -> UpdateResult<()> {
    match prepare(*delta) {
        Ok(()) => Ok(()),
        Err(error) if *delta => {
            tracing::info!(%error, "incremental preparation failed before exit; downloading full archive");
            *delta = false;
            *discard_staging = true;
            full()?;
            *discard_staging = false;
            prepare(false)
        }
        Err(error) => Err(error),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
