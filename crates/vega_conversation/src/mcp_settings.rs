use std::collections::{HashMap, HashSet};
use std::net::TcpListener as StdTcpListener;
use vega_runtime::{CredentialReadError, CredentialReadReason, CredentialReadStage};
type OAuthCallbackStream = tokio::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::types::{
    McpConnectionTest, McpEnvironmentVariable, McpOAuthDiscovery, McpOAuthPreparation,
    McpOAuthRegistration, McpOAuthStart, McpOAuthStepUpOffer, McpRemoteAuthorization,
    McpServerDiagnostic, McpServerForm, McpServerHealth, McpServerTransport, McpServerView,
};
use vega_runtime::{McpReadyServer, McpRevocationLease};
use vega_store::Store;
use vega_store::mcp_servers::{self, McpEnvSlot, McpServerDraft, McpServerRow, McpStoreError};

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum McpSettingsError {
    #[error("{}", vega_i18n::t("domain.mcp.invalid"))]
    Invalid,
    #[error("{}", vega_i18n::t("domain.mcp.conflict"))]
    Conflict,
    #[error("{}", vega_i18n::t("domain.mcp.notfound"))]
    NotFound,
    #[error("{}", vega_i18n::t("domain.mcp.capacity"))]
    Capacity,
    #[error("{}", vega_i18n::t("domain.mcp.store"))]
    Store,
    #[error("{}", vega_i18n::t("domain.mcp.credential"))]
    Credential,
    #[error(transparent)]
    CredentialRead(CredentialReadError),
    #[error("{}", vega_i18n::t("domain.mcp.credentialbindingrequired"))]
    CredentialBindingRequired,
    #[error("{}", vega_i18n::t("domain.mcp.authorizationrequired"))]
    AuthorizationRequired,
    #[error("{}", vega_i18n::t("domain.mcp.cimdunavailable"))]
    CimdUnavailable,
    #[error("{}", vega_i18n::t("domain.mcp.authorizationfailed"))]
    AuthorizationFailed,
    #[error("{}", vega_i18n::t("domain.mcp.connection"))]
    Connection,
    #[error("{}", vega_i18n::t("domain.mcp.unsupportedtransport"))]
    UnsupportedTransport,
    #[error("{}", vega_i18n::t("domain.mcp.confirmationrequired"))]
    ConfirmationRequired,
}

impl McpSettingsError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Invalid => "invalid_config",
            Self::Conflict => "configuration_changed",
            Self::NotFound => "not_found",
            Self::Capacity => "capacity_exceeded",
            Self::Store => "storage_failed",
            Self::Credential => "credential_missing",
            Self::CredentialRead(_) => "credential_check_unavailable",
            Self::CredentialBindingRequired => "credential_binding_required",
            Self::AuthorizationRequired => "authorization_required",
            Self::CimdUnavailable => "cimd_unavailable",
            Self::AuthorizationFailed => "authorization_failed",
            Self::Connection => "connection_failed",
            Self::UnsupportedTransport => "unsupported_transport",
            Self::ConfirmationRequired => "confirmation_required",
        }
    }
}

impl From<McpStoreError> for McpSettingsError {
    fn from(error: McpStoreError) -> Self {
        match error {
            McpStoreError::Invalid => Self::Invalid,
            McpStoreError::Configuration => Self::Store,
            McpStoreError::ConfigurationFailure(failure) => {
                Self::CredentialRead(configuration_failure(failure))
            }
            McpStoreError::ConfigurationBusy(_) => Self::Conflict,
            McpStoreError::ConfigurationIo(kind) => match kind {
                std::io::ErrorKind::WouldBlock => Self::Conflict,
                std::io::ErrorKind::InvalidData => Self::Invalid,
                _ => Self::Store,
            },
            McpStoreError::Conflict => Self::Conflict,
            McpStoreError::NotFound => Self::NotFound,
            McpStoreError::Capacity => Self::Capacity,
            McpStoreError::Sql(_) => Self::Store,
        }
    }
}

impl From<CredentialReadError> for McpSettingsError {
    fn from(error: CredentialReadError) -> Self {
        Self::CredentialRead(error)
    }
}

fn cleanup_pending(store: &Store, path: &std::path::Path) -> Result<bool, McpStoreError> {
    let snapshot = store.conn().unchecked_transaction()?;
    vega_store::mcp_config::projection_current(&snapshot, path)?;
    let pending = !mcp_servers::pending_cleanup(&snapshot)?.is_empty()
        || mcp_servers::list(&snapshot)?.iter().any(|row| row.deleting);
    snapshot.commit()?;
    Ok(pending)
}

fn configuration_failure(
    failure: vega_store::yaml_config::ConfigurationFailure,
) -> CredentialReadError {
    use vega_store::yaml_config::ConfigurationFailureStage as Stage;
    let mut error = CredentialReadError::configuration_io(CredentialReadStage::Mcp, failure.kind);
    error.reason = match failure.stage {
        Stage::Poisoned => CredentialReadReason::ConfigurationPoisoned,
        Stage::Directory => CredentialReadReason::ConfigurationDirectory,
        Stage::LockOpen => CredentialReadReason::ConfigurationLockOpen,
        Stage::LockValidation => CredentialReadReason::ConfigurationLockValidation,
        Stage::LockAcquire => CredentialReadReason::ConfigurationLockAcquire,
        Stage::Read => CredentialReadReason::ConfigurationRead,
        Stage::Parse => CredentialReadReason::ConfigurationParse,
    };
    error
}

fn mcp_read_error(error: McpStoreError) -> CredentialReadError {
    match error {
        McpStoreError::Sql(error) => CredentialReadError::database(CredentialReadStage::Mcp, error),
        McpStoreError::ConfigurationFailure(failure) => configuration_failure(failure),
        McpStoreError::ConfigurationBusy(busy) => {
            let mut error = CredentialReadError::new(
                CredentialReadStage::Mcp,
                CredentialReadReason::Configuration,
            );
            error.configuration_lock = Some(busy.as_str());
            error
        }
        McpStoreError::ConfigurationIo(kind) => {
            CredentialReadError::configuration_io(CredentialReadStage::Mcp, kind)
        }
        _ => CredentialReadError::new(
            CredentialReadStage::Mcp,
            CredentialReadReason::Configuration,
        ),
    }
}

/// New-run read authority plus content-free diagnostics for enabled servers
/// that could not connect. A failed server contributes zero tools.
pub struct McpRunReadiness {
    pub ready_servers: Vec<McpReadyServer>,
    pub unavailable: Vec<McpServerDiagnostic>,
}

#[derive(Default)]
struct RegistryState {
    epochs: HashMap<String, u64>,
    // Only revocation authority lives across Settings and run runtimes. A
    // strong ready handle would retain a child pipe or reqwest pool bound to
    // an already-dropped Tokio runtime.
    leases: HashMap<String, Vec<(u64, McpRevocationLease)>>,
    suspended: HashSet<String>,
    connecting: HashMap<String, CancellationToken>,
    prepared_oauth: HashMap<String, PreparedOAuth>,
    pending_oauth: HashMap<String, PendingOAuth>,
    active_oauth: HashMap<String, ActiveOAuth>,
    step_up_challenges: HashMap<String, BoundStepUpChallenge>,
}

const OAUTH_FLOW_LIFETIME: Duration = Duration::from_secs(180);
const MAX_OAUTH_FLOWS: usize = 8;
const MCP_CONNECT_DISCOVER_TIMEOUT: Duration = Duration::from_secs(15);

struct PreparedOAuth {
    server_id: String,
    revision: u64,
    epoch: u64,
    created: Instant,
    listener: StdTcpListener,
    resource: vega_mcp::ResourceAuthorization,
    server: vega_mcp::AuthorizationServer,
    redirect_uri: String,
    requested_scopes: Vec<String>,
    step_up: Option<(vega_mcp::OAuthTokens, Arc<vega_mcp::ScopeChallenge>)>,
}

struct PendingOAuth {
    server_id: String,
    revision: u64,
    epoch: u64,
    created: Instant,
    listener: StdTcpListener,
    resource: vega_mcp::ResourceAuthorization,
    client: vega_mcp::OAuthClient,
    request: vega_mcp::AuthorizationRequest,
}

struct ActiveOAuth {
    server_id: String,
    created: Instant,
    cancel: CancellationToken,
}

struct BoundStepUpChallenge {
    revision: u64,
    epoch: u64,
    challenge: Arc<vega_mcp::ScopeChallenge>,
}

struct ConnectedRow {
    ready: McpReadyServer,
    /// Deferred until the registry lock and exact revision CAS are held.
    refreshed_oauth: Option<(String, String)>,
}

type ScopeChallengeSink = Arc<dyn Fn(vega_mcp::ScopeChallenge) + Send + Sync>;

impl RegistryState {
    fn expire_oauth(&mut self) {
        self.prepared_oauth
            .retain(|_, pending| pending.created.elapsed() < OAUTH_FLOW_LIFETIME);
        self.pending_oauth
            .retain(|_, pending| pending.created.elapsed() < OAUTH_FLOW_LIFETIME);
        self.active_oauth.retain(|_, active| {
            if active.created.elapsed() < OAUTH_FLOW_LIFETIME {
                true
            } else {
                active.cancel.cancel();
                false
            }
        });
    }

    fn revoke(&mut self, id: &str) {
        self.suspended.insert(id.to_owned());
        self.invalidate_connections(id);
    }

    fn invalidate_connections(&mut self, id: &str) {
        self.step_up_challenges.remove(id);
        self.prepared_oauth
            .retain(|_, pending| pending.server_id != id);
        self.pending_oauth
            .retain(|_, pending| pending.server_id != id);
        self.active_oauth.retain(|_, active| {
            if active.server_id == id {
                active.cancel.cancel();
                false
            } else {
                true
            }
        });
        let epoch = self.epochs.entry(id.to_owned()).or_default();
        *epoch = epoch.wrapping_add(1);
        if let Some(leases) = self.leases.remove(id) {
            for (_, lease) in leases {
                lease.revoke();
            }
        }
        if let Some(inflight) = self.connecting.remove(id) {
            inflight.cancel();
        }
    }

    /// The OAuth callback's commit is the cancellation boundary. Detach its
    /// exact flow before revoking older server authority, otherwise `revoke`
    /// cancels the token watched by `finish_oauth` itself and its biased select
    /// can drop the browser socket after installing the credential but before
    /// acknowledging completion. A later UI Cancel cannot undo a committed
    /// credential; all other same-server flows and ready handles are revoked.
    fn revoke_for_oauth_install(
        &mut self,
        server_id: &str,
        flow_id: &str,
    ) -> Result<(), McpSettingsError> {
        let matching = self
            .active_oauth
            .get(flow_id)
            .is_some_and(|active| active.server_id == server_id && !active.cancel.is_cancelled());
        if !matching {
            return Err(McpSettingsError::Conflict);
        }
        self.active_oauth.remove(flow_id);
        self.revoke(server_id);
        Ok(())
    }

    fn epoch(&self, id: &str) -> u64 {
        self.epochs.get(id).copied().unwrap_or_default()
    }

    fn connection_token(&mut self, id: &str) -> CancellationToken {
        self.connecting.entry(id.to_owned()).or_default().clone()
    }
}

/// Clone and share one service instance between Settings and run workers.
/// Calling `new` separately would create independent revocation domains.
#[derive(Clone)]
pub struct McpServerSettingsService {
    database_path: PathBuf,
    config_root: PathBuf,
    registry: Arc<Mutex<RegistryState>>,
    configuration_cancel: CancellationToken,
    configuration_budget: Option<Arc<Mutex<ConfigurationRetryBudget>>>,
    configuration_retry_enabled: bool,
    configuration_invalidation: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
struct ConfigurationRetryBudget {
    started: Option<Instant>,
    attempts: u64,
    wait_ms: u64,
    last_failure: Option<CredentialReadError>,
    exhausted: bool,
}

impl ConfigurationRetryBudget {
    fn check_acquisition(&mut self, now: Instant) -> Result<(), CredentialReadError> {
        if let Some(error) = self.last_failure
            && (self.exhausted
                || self.started.is_some_and(|started| {
                    now.saturating_duration_since(started) >= Duration::from_millis(500)
                }))
        {
            self.exhausted = true;
            return Err(error);
        }
        Ok(())
    }

    fn next_delay(
        &mut self,
        busy: vega_store::yaml_config::ConfigurationLockBusy,
        now: Instant,
    ) -> Result<Duration, CredentialReadError> {
        let elapsed = now.saturating_duration_since(*self.started.get_or_insert(now));
        self.attempts += 1;
        let attempts = self.attempts;
        let previous_wait_ms = self.wait_ms;
        let error = |wait_ms| {
            let mut error = mcp_read_error(McpStoreError::ConfigurationBusy(busy));
            error.configuration_lock_attempts = Some(attempts);
            error.configuration_lock_wait_ms = Some(wait_ms);
            error
        };
        tracing::debug!(
            configuration_lock = busy.as_str(),
            configuration_lock_attempts = attempts,
            configuration_lock_wait_ms = previous_wait_ms,
            "configuration lock acquisition busy"
        );
        self.last_failure = Some(error(previous_wait_ms));
        if self.attempts >= 6 || elapsed >= Duration::from_millis(500) {
            self.exhausted = true;
            return Err(error(previous_wait_ms));
        }
        let delay = Duration::from_millis(10 << (self.attempts - 1));
        if elapsed + delay > Duration::from_millis(500) {
            self.exhausted = true;
            return Err(error(previous_wait_ms));
        }
        self.wait_ms += delay.as_millis() as u64;
        self.last_failure = Some(error(self.wait_ms));
        Ok(delay)
    }
}

impl McpServerSettingsService {
    pub fn new(database_path: PathBuf, config_root: PathBuf) -> Self {
        Self {
            database_path,
            config_root,
            registry: Arc::new(Mutex::new(RegistryState::default())),
            configuration_cancel: CancellationToken::new(),
            configuration_budget: None,
            configuration_retry_enabled: false,
            configuration_invalidation: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn store(&self) -> Result<Store, McpSettingsError> {
        if self.configuration_retry_enabled {
            return self
                .configuration_operation()
                .credential_store()
                .map_err(Into::into);
        }
        let store = Store::open(&self.database_path).map_err(|_| McpSettingsError::Store)?;
        store.migrate().map_err(|_| McpSettingsError::Store)?;
        let mut state = self.lock()?;
        match vega_store::mcp_config::reconcile(
            store.conn(),
            &self.config_root.join("config.yaml"),
            validate_yaml_row,
        ) {
            Ok(changed) => {
                for id in changed {
                    state.revoke(&id);
                }
            }
            Err(error) => {
                let ids = state
                    .epochs
                    .keys()
                    .chain(state.leases.keys())
                    .chain(state.connecting.keys())
                    .cloned()
                    .collect::<HashSet<_>>();
                for id in ids {
                    state.invalidate_connections(&id);
                }
                return Err(error.into());
            }
        }
        drop(state);
        Ok(store)
    }

    pub fn with_configuration_cancel(mut self, cancel: CancellationToken) -> Self {
        self.configuration_cancel = cancel;
        self.configuration_budget = None;
        self.configuration_retry_enabled = true;
        self
    }

    fn configuration_operation(&self) -> Self {
        let mut operation = self.clone();
        operation.configuration_retry_enabled = true;
        if operation.configuration_budget.is_none() {
            operation.configuration_budget =
                Some(Arc::new(Mutex::new(ConfigurationRetryBudget::default())));
        }
        operation
    }

    fn configuration_guard(
        &self,
    ) -> Result<
        (
            std::sync::MutexGuard<'_, RegistryState>,
            vega_store::yaml_config::Edit,
        ),
        CredentialReadError,
    > {
        let budget = self
            .configuration_budget
            .clone()
            .unwrap_or_else(|| Arc::new(Mutex::new(ConfigurationRetryBudget::default())));
        loop {
            if self.configuration_cancel.is_cancelled() {
                return Err(crate::credential_errors::state_error(
                    CredentialReadStage::Mcp,
                ));
            }
            let mut retry = budget
                .lock()
                .map_err(|_| crate::credential_errors::state_error(CredentialReadStage::Mcp))?;
            if let Err(error) = retry.check_acquisition(Instant::now()) {
                drop(retry);
                return Err(self.configuration_exhausted(error));
            }
            let busy = match self.registry.try_lock() {
                Ok(mut state) => {
                    if self
                        .configuration_invalidation
                        .swap(false, std::sync::atomic::Ordering::SeqCst)
                    {
                        Self::invalidate_configuration(&mut state);
                    }
                    match vega_store::yaml_config::try_begin(&self.config_root.join("config.yaml"))
                    {
                        Ok(document) => {
                            if retry.attempts > 0 {
                                tracing::debug!(
                                    configuration_lock_attempts = retry.attempts,
                                    configuration_lock_wait_ms = retry.wait_ms,
                                    "configuration lock acquisition recovered"
                                );
                            }
                            return Ok((state, document));
                        }
                        Err(error) => {
                            let busy = vega_store::yaml_config::lock_busy(&error);
                            if let Some(busy) = busy {
                                drop(state);
                                busy
                            } else {
                                let mut state = state;
                                Self::invalidate_configuration(&mut state);
                                return Err(mcp_read_error(vega_store::mcp_config::io_error(
                                    error,
                                )));
                            }
                        }
                    }
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    vega_store::yaml_config::ConfigurationLockBusy::Registry
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(crate::credential_errors::state_error(
                        CredentialReadStage::Mcp,
                    ));
                }
            };
            let result = retry.next_delay(busy, Instant::now());
            drop(retry);
            let delay = match result {
                Ok(delay) => delay,
                Err(error) => return Err(self.configuration_exhausted(error)),
            };
            let until = Instant::now() + delay;
            while Instant::now() < until {
                if self.configuration_cancel.is_cancelled() {
                    return Err(crate::credential_errors::state_error(
                        CredentialReadStage::Mcp,
                    ));
                }
                std::thread::sleep(
                    until
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(10)),
                );
            }
        }
    }

    fn configuration_exhausted(&self, error: CredentialReadError) -> CredentialReadError {
        if self.configuration_cancel.is_cancelled() {
            return crate::credential_errors::state_error(CredentialReadStage::Mcp);
        }
        self.configuration_invalidation
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut state) = self.registry.try_lock() {
            Self::invalidate_configuration(&mut state);
            self.configuration_invalidation
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        error
    }

    fn invalidate_configuration(state: &mut RegistryState) {
        let ids = state
            .epochs
            .keys()
            .chain(state.leases.keys())
            .chain(state.connecting.keys())
            .cloned()
            .collect::<HashSet<_>>();
        for id in ids {
            state.invalidate_connections(&id);
        }
    }

    fn credential_store(&self) -> Result<Store, CredentialReadError> {
        let store = Store::open(&self.database_path)
            .map_err(|error| CredentialReadError::database(CredentialReadStage::Mcp, error))?;
        store
            .migrate()
            .map_err(|error| CredentialReadError::database(CredentialReadStage::Mcp, error))?;
        let (mut state, document) = self.configuration_guard()?;
        match vega_store::mcp_config::reconcile_with_edit(
            store.conn(),
            &self.config_root.join("config.yaml"),
            validate_yaml_row,
            document,
        ) {
            Ok(changed) => {
                for id in changed {
                    state.revoke(&id);
                }
            }
            Err(error) => {
                Self::invalidate_configuration(&mut state);
                return Err(mcp_read_error(error));
            }
        }
        Ok(store)
    }

    pub fn reconcile_declarations(&self) -> Result<(), McpSettingsError> {
        self.store().map(|_| ())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, RegistryState>, McpSettingsError> {
        let mut state = self.registry.lock().map_err(|_| McpSettingsError::Store)?;
        if self
            .configuration_invalidation
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            Self::invalidate_configuration(&mut state);
        }
        Ok(state)
    }

    fn enabled_known_credentials(&self) -> Result<Vec<String>, CredentialReadError> {
        let store = self.credential_store()?;
        let mut known = mcp_servers::list(store.conn())
            .map_err(mcp_read_error)?
            .into_iter()
            .filter(|row| row.enabled)
            .map(|row| known_credentials_from_row(&self.config_root, &row))
            .collect::<Result<Vec<_>, _>>()
            .map(|groups| groups.into_iter().flatten().collect::<Vec<_>>())?;
        // The run authority includes all owner-held values, not only the
        // currently selected provider or enabled servers: a malicious server
        // can echo another provider's key or a disabled server's old key.
        known.extend(all_owner_credentials(&self.config_root, &store)?);
        Ok(known)
    }

    /// Owner-only current values for the final provider projection boundary.
    /// This is called again before every model request, including summaries;
    /// it intentionally includes disabled servers and other Provider keys.
    pub fn current_owner_credential_values(&self) -> Result<Vec<String>, CredentialReadError> {
        self.configuration_operation().enabled_known_credentials()
    }

    /// Re-read owner-held values at the final lower-trust Settings boundary.
    /// A remote scope or metadata string is not safe merely because its syntax
    /// is valid: a server may echo a short credential in a UI-visible field.
    fn reject_owner_secret_in_ui<'a>(
        &self,
        fields: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), McpSettingsError> {
        let store = self.store()?;
        let fields = fields.into_iter().collect::<Vec<_>>();
        let known = all_owner_credentials(&self.config_root, &store)?;
        if known
            .iter()
            .any(|secret| !secret.is_empty() && fields.iter().any(|field| field.contains(secret)))
        {
            return Err(McpSettingsError::AuthorizationFailed);
        }
        Ok(())
    }

    /// Read-only list; no process launch, network access or secret retrieval.
    pub fn list(&self) -> Result<Vec<McpServerView>, McpSettingsError> {
        let store = self.store()?;
        let state = self.lock()?;
        mcp_servers::list(store.conn())?
            .iter()
            .map(|row| view(row, &state, &self.config_root))
            .collect()
    }

    /// Save a new disabled server, with no implicit activation.
    pub fn create(&self, form: McpServerForm) -> Result<McpServerView, McpSettingsError> {
        let draft = draft_from_form(&form)?;
        let store = self.store()?;
        let state = self.lock()?;
        let row = mcp_servers::create(store.conn(), &draft)?;
        drop(state);
        self.reconcile_pending()?;
        let state = self.lock()?;
        view(&row, &state, &self.config_root)
    }

    /// Full-form compare-and-swap edit. Any previous ready handle is revoked
    /// before the new configuration can be persisted or later rediscovered.
    pub fn replace(
        &self,
        id: &str,
        expected_revision: u64,
        form: McpServerForm,
    ) -> Result<McpServerView, McpSettingsError> {
        let draft = draft_from_form(&form)?;
        let store = self.store()?;
        let mut state = self.lock()?;
        state.revoke(id);
        let row = mcp_servers::replace(store.conn(), id, expected_revision, &draft)?;
        drop(state);
        self.reconcile_pending()?;
        let state = self.lock()?;
        view(&row, &state, &self.config_root)
    }

    /// Rotate a manually entered bearer. The value never enters a DTO, DB or
    /// log. Rotation disables/revisions the server; explicit re-enable is
    /// required before the new credential may be used.
    pub fn set_bearer_secret(
        &self,
        id: &str,
        expected_revision: u64,
        secret: String,
    ) -> Result<McpServerView, McpSettingsError> {
        if secret.is_empty() || secret.len() > 64 * 1024 {
            return Err(McpSettingsError::Invalid);
        }
        let store = self.store()?;
        let mut state = self.lock()?;
        state.revoke(id);
        let old = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
        if old.config_revision != expected_revision
            || old.transport != "remote"
            || old.remote_auth_mode.as_deref() != Some("bearer")
        {
            return Err(McpSettingsError::Conflict);
        }
        let draft = draft_from_row(&old);
        let row = mcp_servers::replace(store.conn(), id, expected_revision, &draft)?;
        let reference = row
            .remote_credential_ref
            .as_deref()
            .ok_or(McpSettingsError::Store)?;
        vega_store::keystore::set_key(&self.config_root, reference, &secret)
            .map_err(|_| McpSettingsError::Credential)?;
        drop(state);
        self.reconcile_pending()?;
        let state = self.lock()?;
        view(&row, &state, &self.config_root)
    }

    pub fn local_credential_binding_template(&self, id: &str) -> Result<String, McpSettingsError> {
        let store = self.store()?;
        let row = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
        let slots = env_slots_from_row(&row)?;
        if slots.is_empty() {
            return Ok(String::from("credential_bindings: []\n"));
        }
        let mut template = String::from("credential_bindings:\n");
        for slot in slots {
            let reference =
                serde_json::to_string(&slot.credential_ref).map_err(|_| McpSettingsError::Store)?;
            let variable =
                serde_json::to_string(&slot.variable).map_err(|_| McpSettingsError::Store)?;
            template.push_str(&format!("  - key_ref: {reference}\n    env: {variable}\n    origin: \"\"\n    header: authorization\n    prefix: \"Bearer \"\n    path: /\n"));
        }
        Ok(template)
    }

    /// Install/rotate one local child environment value into a slot owned by
    /// this exact server. The old value is durably queued for deletion before
    /// the new one is written, and the server remains disabled until reapproved.
    pub fn set_local_env_secret(
        &self,
        id: &str,
        expected_revision: u64,
        variable: &str,
        secret: String,
    ) -> Result<McpServerView, McpSettingsError> {
        if secret.is_empty() || secret.len() > 64 * 1024 {
            return Err(McpSettingsError::Invalid);
        }
        let (row, reference) = {
            let store = self.store()?;
            let mut state = self.lock()?;
            state.revoke(id);
            mcp_servers::begin_local_secret_rotation(store.conn(), id, expected_revision, variable)?
        };
        self.reconcile_pending()?;
        vega_store::keystore::set_key(&self.config_root, &reference, &secret)
            .map_err(|_| McpSettingsError::Credential)?;
        let state = self.lock()?;
        view(&row, &state, &self.config_root)
    }

    /// Explicit, bounded OAuth metadata discovery. This never opens a browser,
    /// registers a client or changes persisted authority.
    pub async fn discover_oauth(
        &self,
        id: &str,
        expected_revision: u64,
        confirmed: bool,
    ) -> Result<McpOAuthDiscovery, McpSettingsError> {
        if !confirmed {
            return Err(McpSettingsError::ConfirmationRequired);
        }
        let store = self.store()?;
        let row = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
        let (endpoint, allow_loopback_http) = oauth_endpoint(&row, expected_revision)?;
        let resource = vega_mcp::ResourceAuthorization::discover(endpoint, allow_loopback_http)
            .await
            .map_err(map_oauth_error)?;
        self.reject_owner_secret_in_ui(
            std::iter::once(resource.resource())
                .chain(resource.authorization_servers().iter().map(String::as_str))
                .chain(resource.requested_scopes().iter().map(String::as_str)),
        )?;
        Ok(McpOAuthDiscovery {
            resource: resource.resource().to_owned(),
            issuers: resource.authorization_servers().to_vec(),
            requested_scopes: resource.requested_scopes().to_vec(),
        })
    }

    /// Current bound 403 offers. Challenges are deliberately memory-only:
    /// restart preserves the failed call audit, but never replays it or an old
    /// external scope request. The next explicit call may re-trigger one.
    pub fn step_up_offers(&self) -> Result<Vec<McpOAuthStepUpOffer>, McpSettingsError> {
        let store = self.store()?;
        let state = self.lock()?;
        let mut offers = Vec::new();
        for (id, bound) in &state.step_up_challenges {
            let Some(row) = mcp_servers::find(store.conn(), id)? else {
                continue;
            };
            if row.enabled
                && !row.deleting
                && row.remote_auth_mode.as_deref() == Some("oauth")
                && row.config_revision == bound.revision
                && state.epoch(id) == bound.epoch
                && !state.suspended.contains(id)
            {
                offers.push(McpOAuthStepUpOffer {
                    server_id: id.clone(),
                    config_revision: bound.revision,
                    added_scopes: bound.challenge.scopes().to_vec(),
                });
            }
        }
        drop(state);
        offers.sort_by(|left, right| left.server_id.cmp(&right.server_id));
        self.reject_owner_secret_in_ui(
            offers
                .iter()
                .flat_map(|offer| offer.added_scopes.iter().map(String::as_str)),
        )?;
        Ok(offers)
    }

    /// Prepare a verified step-up using the exact token/challenge identity.
    /// No failed call is replayed and no browser/DCR action happens here.
    pub async fn prepare_verified_step_up(
        &self,
        id: &str,
        expected_revision: u64,
    ) -> Result<McpOAuthPreparation, McpSettingsError> {
        let (issuer, challenge) = {
            let store = self.store()?;
            let state = self.lock()?;
            let row = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
            let bound = state
                .step_up_challenges
                .get(id)
                .ok_or(McpSettingsError::Conflict)?;
            if !row.enabled
                || row.deleting
                || row.config_revision != expected_revision
                || bound.revision != expected_revision
                || bound.epoch != state.epoch(id)
            {
                return Err(McpSettingsError::Conflict);
            }
            (
                row.remote_oauth_issuer
                    .ok_or(McpSettingsError::AuthorizationRequired)?,
                bound.challenge.clone(),
            )
        };
        self.prepare_oauth_inner(id, expected_revision, &issuer, Some(challenge))
            .await
    }

    /// Bind the exact callback port and inspect only the issuer the user
    /// selected. This is a pre-consent preview: no DCR or browser launch.
    pub async fn prepare_oauth(
        &self,
        id: &str,
        expected_revision: u64,
        selected_issuer: &str,
    ) -> Result<McpOAuthPreparation, McpSettingsError> {
        self.prepare_oauth_inner(id, expected_revision, selected_issuer, None)
            .await
    }

    async fn prepare_oauth_inner(
        &self,
        id: &str,
        expected_revision: u64,
        selected_issuer: &str,
        step_up: Option<Arc<vega_mcp::ScopeChallenge>>,
    ) -> Result<McpOAuthPreparation, McpSettingsError> {
        let (row, epoch) = {
            let store = self.store()?;
            let mut state = self.lock()?;
            state.expire_oauth();
            if state.prepared_oauth.len() + state.active_oauth.len() >= MAX_OAUTH_FLOWS
                || state
                    .prepared_oauth
                    .values()
                    .any(|flow| flow.server_id == id)
                || state.active_oauth.values().any(|flow| flow.server_id == id)
            {
                return Err(McpSettingsError::Capacity);
            }
            let row = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
            let _ = oauth_endpoint(&row, expected_revision)?;
            (row, state.epoch(id))
        };
        let (endpoint, allow_loopback_http) = oauth_endpoint(&row, expected_revision)?;
        let resource = vega_mcp::ResourceAuthorization::discover(endpoint, allow_loopback_http)
            .await
            .map_err(map_oauth_error)?;
        let server = resource
            .discover_server(selected_issuer)
            .await
            .map_err(map_oauth_error)?;
        if row
            .remote_oauth_issuer
            .as_deref()
            .is_some_and(|old_issuer| old_issuer != selected_issuer)
        {
            return Err(McpSettingsError::Invalid);
        }
        let listener = StdTcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| McpSettingsError::AuthorizationFailed)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| McpSettingsError::AuthorizationFailed)?;
        let port = listener
            .local_addr()
            .map_err(|_| McpSettingsError::AuthorizationFailed)?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");
        let step_up_state = if let Some(challenge) = step_up {
            let reference = row
                .remote_credential_ref
                .as_deref()
                .ok_or(McpSettingsError::AuthorizationRequired)?;
            if !is_owned_secret_ref(id, reference) || !reference.ends_with("-oauth") {
                return Err(McpSettingsError::Credential);
            }
            let generation = reference
                .strip_prefix(&format!("mcp-{id}-r"))
                .and_then(|suffix| suffix.strip_suffix("-oauth"))
                .and_then(|suffix| suffix.parse::<u64>().ok())
                .ok_or(McpSettingsError::Credential)?;
            let client_id = row
                .remote_oauth_client_id
                .as_deref()
                .ok_or(McpSettingsError::AuthorizationRequired)?;
            let client = server
                .pre_registered(client_id, selected_issuer, &redirect_uri)
                .map_err(map_oauth_error)?;
            let envelope = vega_store::keystore::get_key(&self.config_root, reference)
                .map_err(|_| McpSettingsError::Credential)?;
            let tokens = client
                .restore_owner_only_tokens(&envelope, &resource, id, generation)
                .map_err(map_oauth_error)?;
            let scopes = client
                .verified_step_up_scopes(&tokens, &challenge)
                .map_err(map_oauth_error)?;
            Some((tokens, challenge, scopes))
        } else {
            None
        };
        let registration = match row.remote_oauth_client_id.as_deref() {
            Some(_) => McpOAuthRegistration::PreRegistered,
            None if server.supports_dcr() => McpOAuthRegistration::DynamicRegistration,
            None if server.advertises_cimd() => McpOAuthRegistration::CimdUnavailable,
            None => McpOAuthRegistration::Unavailable,
        };
        let requested_scopes = step_up_state
            .as_ref()
            .map(|(_, _, scopes)| scopes.clone())
            .unwrap_or_else(|| resource.requested_scopes().to_vec());
        let step_up_added_scopes = step_up_state
            .as_ref()
            .map(|(_, challenge, _)| challenge.scopes().to_vec())
            .unwrap_or_default();
        let resource_name = resource.resource().to_owned();
        let registration_endpoint = server.registration_endpoint().map(str::to_owned);
        self.reject_owner_secret_in_ui(
            std::iter::once(resource_name.as_str())
                .chain(std::iter::once(selected_issuer))
                .chain(std::iter::once(redirect_uri.as_str()))
                .chain(requested_scopes.iter().map(String::as_str))
                .chain(step_up_added_scopes.iter().map(String::as_str))
                .chain(registration_endpoint.iter().map(String::as_str)),
        )?;
        let flow_id = ulid::Ulid::generate().to_string();
        {
            let store = self.store()?;
            let mut state = self.lock()?;
            let current = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
            state.expire_oauth();
            if current.config_revision != expected_revision
                || current.deleting
                || state.epoch(id) != epoch
                || state.prepared_oauth.len() + state.active_oauth.len() >= MAX_OAUTH_FLOWS
                || state
                    .prepared_oauth
                    .values()
                    .any(|flow| flow.server_id == id)
                || state.active_oauth.values().any(|flow| flow.server_id == id)
                || step_up_state.as_ref().is_some_and(|(_, challenge, _)| {
                    state
                        .step_up_challenges
                        .get(id)
                        .is_none_or(|bound| !Arc::ptr_eq(&bound.challenge, challenge))
                })
            {
                return Err(McpSettingsError::Conflict);
            }
            state.prepared_oauth.insert(
                flow_id.clone(),
                PreparedOAuth {
                    server_id: id.to_owned(),
                    revision: expected_revision,
                    epoch,
                    created: Instant::now(),
                    listener,
                    resource,
                    server,
                    redirect_uri: redirect_uri.clone(),
                    requested_scopes: requested_scopes.clone(),
                    step_up: step_up_state.map(|(tokens, challenge, _)| (tokens, challenge)),
                },
            );
        }
        // The UI may close without calling begin/finish. An OS timer, not an
        // operation-scoped Tokio task, releases the bound listener even when
        // the caller's short-lived runtime has already been dropped.
        let weak_registry = Arc::downgrade(&self.registry);
        let expiry_flow_id = flow_id.clone();
        if std::thread::Builder::new()
            .name("vega-mcp-oauth-expiry".into())
            .spawn(move || {
                std::thread::sleep(OAUTH_FLOW_LIFETIME);
                if let Some(registry) = weak_registry.upgrade()
                    && let Ok(mut state) = registry.lock()
                {
                    state.prepared_oauth.remove(&expiry_flow_id);
                    state.pending_oauth.remove(&expiry_flow_id);
                    if let Some(active) = state.active_oauth.remove(&expiry_flow_id) {
                        active.cancel.cancel();
                    }
                }
            })
            .is_err()
        {
            self.cancel_oauth(&flow_id)?;
            return Err(McpSettingsError::AuthorizationFailed);
        }
        if let Err(error) = self.reject_owner_secret_in_ui(
            std::iter::once(resource_name.as_str())
                .chain(std::iter::once(selected_issuer))
                .chain(std::iter::once(redirect_uri.as_str()))
                .chain(requested_scopes.iter().map(String::as_str))
                .chain(step_up_added_scopes.iter().map(String::as_str))
                .chain(registration_endpoint.iter().map(String::as_str)),
        ) {
            self.cancel_oauth(&flow_id)?;
            return Err(error);
        }
        Ok(McpOAuthPreparation {
            flow_id,
            resource: resource_name,
            issuer: selected_issuer.to_owned(),
            redirect_uri,
            requested_scopes,
            step_up_added_scopes,
            registration,
            registration_endpoint,
        })
    }

    /// After the UI shows the exact prepared issuer, scope, redirect and DCR
    /// deprecation warning, this one-shot action may register/open the browser.
    pub async fn begin_oauth(
        &self,
        flow_id: &str,
        confirmed: bool,
    ) -> Result<McpOAuthStart, McpSettingsError> {
        if !confirmed {
            return Err(McpSettingsError::ConfirmationRequired);
        }
        let prepared = {
            let mut state = self.lock()?;
            state.expire_oauth();
            state
                .prepared_oauth
                .remove(flow_id)
                .ok_or(McpSettingsError::Conflict)?
        };
        if prepared.created.elapsed() >= OAUTH_FLOW_LIFETIME {
            return Err(McpSettingsError::Conflict);
        }
        // A Provider key can be configured after the preview but before the
        // user confirms. Recheck before registration or browser handoff.
        self.reject_owner_secret_in_ui(
            std::iter::once(prepared.resource.resource())
                .chain(std::iter::once(prepared.server.issuer()))
                .chain(std::iter::once(prepared.redirect_uri.as_str()))
                .chain(prepared.requested_scopes.iter().map(String::as_str)),
        )?;
        let (row, epoch, cancel) = {
            let store = self.store()?;
            let mut state = self.lock()?;
            if state.epoch(&prepared.server_id) != prepared.epoch {
                return Err(McpSettingsError::Conflict);
            }
            state.revoke(&prepared.server_id);
            let current = mcp_servers::find(store.conn(), &prepared.server_id)?
                .ok_or(McpSettingsError::NotFound)?;
            let _ = oauth_endpoint(&current, prepared.revision)?;
            let row = mcp_servers::set_enabled(
                store.conn(),
                &prepared.server_id,
                prepared.revision,
                false,
            )?;
            let cancel = CancellationToken::new();
            let epoch = state.epoch(&prepared.server_id);
            state.active_oauth.insert(
                flow_id.to_owned(),
                ActiveOAuth {
                    server_id: prepared.server_id.clone(),
                    created: prepared.created,
                    cancel: cancel.clone(),
                },
            );
            (row, epoch, cancel)
        };
        self.reconcile_pending()?;
        let client = match row.remote_oauth_client_id.as_deref() {
            Some(client_id) => prepared
                .server
                .pre_registered(client_id, prepared.server.issuer(), &prepared.redirect_uri)
                .map_err(map_oauth_error),
            None => tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(McpSettingsError::Conflict),
                result = prepared.server.register_dcr(&prepared.redirect_uri, true) =>
                    result.map_err(map_oauth_error),
            },
        };
        let client = match client {
            Ok(client) => client,
            Err(error) => {
                self.lock()?.active_oauth.remove(flow_id);
                return Err(error);
            }
        };
        let request_result = match prepared.step_up.as_ref() {
            Some((tokens, challenge)) => client.begin_verified_step_up(tokens, challenge, true),
            None => client.begin_authorization(),
        };
        let request = match request_result.map_err(map_oauth_error) {
            Ok(request) => request,
            Err(error) => {
                self.lock()?.active_oauth.remove(flow_id);
                return Err(error);
            }
        };
        let authorization_url = request.authorization_url().to_owned();
        let requested_scopes = prepared.requested_scopes.clone();
        let issuer = prepared.server.issuer().to_owned();
        if let Err(error) = self.reject_owner_secret_in_ui(
            std::iter::once(authorization_url.as_str())
                .chain(std::iter::once(issuer.as_str()))
                .chain(requested_scopes.iter().map(String::as_str)),
        ) {
            self.cancel_oauth(flow_id)?;
            return Err(error);
        }
        let insertion = (|| -> Result<(), McpSettingsError> {
            let store = self.store()?;
            let mut state = self.lock()?;
            let current = mcp_servers::find(store.conn(), &prepared.server_id)?
                .ok_or(McpSettingsError::NotFound)?;
            if current.config_revision != row.config_revision
                || current.deleting
                || state.epoch(&prepared.server_id) != epoch
                || cancel.is_cancelled()
                || !state.active_oauth.contains_key(flow_id)
            {
                state.active_oauth.remove(flow_id);
                return Err(McpSettingsError::Conflict);
            }
            state.pending_oauth.insert(
                flow_id.to_owned(),
                PendingOAuth {
                    server_id: prepared.server_id.clone(),
                    revision: row.config_revision,
                    epoch,
                    created: prepared.created,
                    listener: prepared.listener,
                    resource: prepared.resource,
                    client,
                    request,
                },
            );
            Ok(())
        })();
        if insertion.is_err() {
            self.lock()?.active_oauth.remove(flow_id);
        }
        insertion?;
        Ok(McpOAuthStart {
            flow_id: flow_id.to_owned(),
            authorization_url,
            issuer,
            requested_scopes,
        })
    }

    /// Explicit Settings close/Cancel action; the callback cannot complete
    /// afterwards even if it was already being awaited on another worker.
    pub fn cancel_oauth(&self, flow_id: &str) -> Result<(), McpSettingsError> {
        let mut state = self.lock()?;
        state.prepared_oauth.remove(flow_id);
        state.pending_oauth.remove(flow_id);
        if let Some(active) = state.active_oauth.remove(flow_id) {
            active.cancel.cancel();
        }
        Ok(())
    }

    /// Wait once for the exact loopback callback, exchange the code using
    /// PKCE/state/issuer validation, then install only into a new revisioned
    /// owner-only slot. A stale callback cannot mutate a later edit.
    pub async fn finish_oauth(&self, flow_id: &str) -> Result<McpServerView, McpSettingsError> {
        let (pending, cancel) = {
            let mut state = self.lock()?;
            state.expire_oauth();
            let pending = state
                .pending_oauth
                .remove(flow_id)
                .ok_or(McpSettingsError::Conflict)?;
            let cancel = state
                .active_oauth
                .get(flow_id)
                .ok_or(McpSettingsError::Conflict)?
                .cancel
                .clone();
            (pending, cancel)
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(McpSettingsError::Conflict),
            result = self.finish_oauth_pending(flow_id, pending, &cancel) => result,
        };
        self.lock()?.active_oauth.remove(flow_id);
        result
    }

    async fn finish_oauth_pending(
        &self,
        flow_id: &str,
        pending: PendingOAuth,
        cancel: &CancellationToken,
    ) -> Result<McpServerView, McpSettingsError> {
        let remaining = OAUTH_FLOW_LIFETIME
            .checked_sub(pending.created.elapsed())
            .ok_or(McpSettingsError::Conflict)?;
        let (callback, mut browser) = receive_oauth_callback(pending.listener, remaining).await?;
        let tokens = pending
            .client
            .finish_authorization(pending.request, &callback)
            .await
            .map_err(map_oauth_error)?;
        let _ = tokens
            .bearer_credential(&pending.resource)
            .map_err(map_oauth_error)?;
        let current = {
            let store = self.store()?;
            mcp_servers::find(store.conn(), &pending.server_id)?
                .ok_or(McpSettingsError::NotFound)?
        };
        let (endpoint, allow_loopback_http) = oauth_endpoint(&current, pending.revision)?;
        let fresh_resource =
            vega_mcp::ResourceAuthorization::discover(endpoint, allow_loopback_http)
                .await
                .map_err(map_oauth_error)?;
        let _ = tokens
            .bearer_credential(&fresh_resource)
            .map_err(map_oauth_error)?;
        let row = {
            let store = self.store()?;
            let mut state = self.lock()?;
            if cancel.is_cancelled() || state.epoch(&pending.server_id) != pending.epoch {
                return Err(McpSettingsError::Conflict);
            }
            state.revoke_for_oauth_install(&pending.server_id, flow_id)?;
            let (row, reference) = mcp_servers::begin_oauth_install(
                store.conn(),
                &pending.server_id,
                pending.revision,
                pending.client.server_issuer(),
                pending.client.client_id(),
            )?;
            let envelope = tokens
                .to_owner_only_envelope(&pending.server_id, row.config_revision)
                .map_err(map_oauth_error)?;
            vega_store::keystore::set_key(&self.config_root, &reference, &envelope)
                .map_err(|_| McpSettingsError::Credential)?;
            row
        };
        self.reconcile_pending()?;
        let body = b"Vega authorization complete. Return.";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = browser.write_all(response.as_bytes()).await;
        let _ = browser.write_all(body).await;
        let state = self.lock()?;
        view(&row, &state, &self.config_root)
    }

    /// Local credential disconnect only; remote grant revocation is not
    /// claimed. A cleanup failure leaves a durable disabled row for retry.
    pub fn disconnect_oauth(
        &self,
        id: &str,
        expected_revision: u64,
        confirmed: bool,
    ) -> Result<McpServerView, McpSettingsError> {
        if !confirmed {
            return Err(McpSettingsError::ConfirmationRequired);
        }
        let row = {
            let store = self.store()?;
            let mut state = self.lock()?;
            state.revoke(id);
            mcp_servers::begin_oauth_disconnect(store.conn(), id, expected_revision)?
        };
        self.reconcile_pending()?;
        let state = self.lock()?;
        view(&row, &state, &self.config_root)
    }

    /// Explicit enable/disable. Disabling revokes even if persistence fails,
    /// so an old active run can never dispatch after the user's action.
    pub async fn set_enabled(
        &self,
        id: &str,
        expected_revision: u64,
        enabled: bool,
        confirmed: bool,
    ) -> Result<McpServerView, McpSettingsError> {
        if enabled && !confirmed {
            return Err(McpSettingsError::ConfirmationRequired);
        }
        let row = {
            let store = self.store()?;
            let mut state = self.lock()?;
            state.revoke(id);
            let row = mcp_servers::set_enabled(store.conn(), id, expected_revision, enabled)?;
            if enabled {
                state.suspended.remove(id);
            }
            row
        };
        self.reconcile_pending()?;
        if enabled {
            // Settings validation is deliberately transient. The next user
            // run reconnects inside its own Tokio runtime; retaining this
            // pipe/client would make a closed Settings runtime look Ready.
            let code = self
                .test_connection(id, row.config_revision, true)
                .await
                .err()
                .map(McpSettingsError::code);
            let store = self.store()?;
            let _ = mcp_servers::set_last_error(store.conn(), id, row.config_revision, code);
        }
        let store = self.store()?;
        let row = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
        let state = self.lock()?;
        view(&row, &state, &self.config_root)
    }

    /// Explicit test of a saved server. A disabled server's test handle is
    /// never inserted into the run registry.
    pub async fn test_connection(
        &self,
        id: &str,
        expected_revision: u64,
        confirmed: bool,
    ) -> Result<McpConnectionTest, McpSettingsError> {
        if !confirmed {
            return Err(McpSettingsError::ConfirmationRequired);
        }
        let store = self.store()?;
        let row = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
        if row.config_revision != expected_revision || row.deleting {
            return Err(McpSettingsError::Conflict);
        }
        let (epoch, connecting) = {
            let mut state = self.lock()?;
            (state.epoch(id), state.connection_token(id))
        };
        let connected = tokio::select! {
            biased;
            _ = connecting.cancelled() => return Err(McpSettingsError::Conflict),
            ready = connect_row_with_timeout(&self.config_root, &row, None, MCP_CONNECT_DISCOVER_TIMEOUT) => ready?,
        };
        let store = self.store()?;
        let state = self.lock()?;
        let current = mcp_servers::find(store.conn(), id)?.ok_or(McpSettingsError::NotFound)?;
        if state.epoch(id) != epoch
            || current.deleting
            || current.config_revision != expected_revision
        {
            connected.ready.revoke();
            return Err(McpSettingsError::Conflict);
        }
        if let Some((reference, envelope)) = connected.refreshed_oauth.as_ref() {
            if current.remote_credential_ref.as_deref() != Some(reference.as_str()) {
                connected.ready.revoke();
                return Err(McpSettingsError::Conflict);
            }
            if vega_store::keystore::set_key(&self.config_root, reference, envelope).is_err() {
                connected.ready.revoke();
                return Err(McpSettingsError::Credential);
            }
        }
        let ready = connected
            .ready
            .with_known_credentials(all_owner_credentials(&self.config_root, &store)?);
        if ready.catalog_contains_known_credential() {
            ready.revoke();
            return Err(McpSettingsError::Connection);
        }
        let result = McpConnectionTest {
            tool_names: ready.tool_names(),
            rejected_tools: ready.rejected_tools().to_vec(),
        };
        ready.revoke();
        Ok(result)
    }

    /// A new run uses only enabled, actually connected rows. Reconnection may
    /// refresh discovery, but it cannot replay any historical tool call.
    pub async fn ready_for_run(&self) -> Result<McpRunReadiness, McpSettingsError> {
        self.configuration_operation()
            .ready_for_run_operation()
            .await
    }

    async fn ready_for_run_operation(&self) -> Result<McpRunReadiness, McpSettingsError> {
        self.reconcile_pending_for_run()?;
        let store = self.credential_store()?;
        let rows: Vec<_> = mcp_servers::list(store.conn())
            .map_err(mcp_read_error)?
            .into_iter()
            .filter(|row| row.enabled)
            .collect();
        // Discover independently: one unresponsive server must not serialize
        // the first user message behind up to seven more connection timeouts.
        // `join_all` preserves stored row order for deterministic aliases and
        // diagnostics; each `ensure_ready` still CAS-checks its own revision.
        let results =
            futures::future::join_all(rows.iter().map(|row| self.ensure_ready(row))).await;
        // A server can echo a different enabled server's key. The cross-server
        // guard therefore includes every currently owner-held credential, not
        // only those belonging to transports that became ready. Read after
        // connection so a just-refreshed OAuth token is included as well.
        let known_credentials = self.enabled_known_credentials()?;
        let credential_owner = self.clone();
        let credential_reader: Arc<
            dyn Fn() -> Result<Vec<String>, vega_runtime::CredentialReadError> + Send + Sync,
        > = Arc::new(move || {
            let mut owner = credential_owner.clone();
            owner.configuration_budget = None;
            owner.current_owner_credential_values()
        });
        let mut ready_servers = Vec::new();
        let mut unavailable = Vec::new();
        for (row, result) in rows.into_iter().zip(results) {
            match result {
                Ok(ready) => ready_servers.push(
                    ready
                        .with_known_credentials(known_credentials.clone())
                        .with_known_credentials_reader(credential_reader.clone()),
                ),
                Err(error) => unavailable.push(McpServerDiagnostic {
                    server_id: row.id,
                    code: error.code().to_owned(),
                }),
            }
        }
        Ok(McpRunReadiness {
            ready_servers,
            unavailable,
        })
    }

    /// Explicit remove, preserving historical message/tool audit. Secret
    /// deletion is local only; UI must disclose remote grant revocation.
    pub fn remove(
        &self,
        id: &str,
        expected_revision: u64,
        confirmed: bool,
    ) -> Result<(), McpSettingsError> {
        if !confirmed {
            return Err(McpSettingsError::ConfirmationRequired);
        }
        {
            let store = self.store()?;
            let mut state = self.lock()?;
            state.revoke(id);
            mcp_servers::begin_remove(store.conn(), id, expected_revision)?;
        }
        self.reconcile_pending()?;
        let store = self.store()?;
        if mcp_servers::find(store.conn(), id)?.is_some() {
            return Err(McpSettingsError::Credential);
        }
        Ok(())
    }

    /// Retry any interrupted owner-only secret deletion after restart. The
    /// outbox is durable and acknowledges only after deletion/Missing.
    pub fn reconcile_pending(&self) -> Result<(), McpSettingsError> {
        let store = self.store()?;
        let _state = self.lock()?;
        let path = self.config_root.join("config.yaml");
        let _document =
            vega_store::yaml_config::begin(&path).map_err(|_| McpSettingsError::Conflict)?;
        if !cleanup_pending(&store, &path)? {
            return Ok(());
        }
        let _permit = vega_store::write_coordination::acquire(
            store.conn(),
            vega_store::write_coordination::WriteCategory::McpServers,
        )
        .map_err(|_| McpSettingsError::Store)?;
        vega_store::mcp_config::projection_current(store.conn(), &path)?;
        for entry in mcp_servers::pending_cleanup(store.conn())? {
            if !is_owned_secret_ref(&entry.server_id, &entry.credential_ref)
                || !mcp_servers::cleanup_target_is_stale(store.conn(), &entry)?
            {
                return Err(McpSettingsError::Store);
            }
            match vega_store::keystore::delete_key(&self.config_root, &entry.credential_ref) {
                Ok(()) | Err(vega_store::keystore::Error::Missing) => {}
                Err(_) => return Err(McpSettingsError::Credential),
            }
            mcp_servers::acknowledge_cleanup(store.conn(), &entry)?;
        }
        for row in mcp_servers::list(store.conn())?
            .into_iter()
            .filter(|row| row.deleting)
        {
            let _ = mcp_servers::finish_remove(store.conn(), &row.id)?;
        }
        Ok(())
    }

    fn reconcile_pending_for_run(&self) -> Result<(), CredentialReadError> {
        let store = self.credential_store()?;
        let (_state, _document) = self.configuration_guard()?;
        let path = self.config_root.join("config.yaml");
        if !cleanup_pending(&store, &path).map_err(mcp_read_error)? {
            return Ok(());
        }
        let _permit = vega_store::write_coordination::acquire(
            store.conn(),
            vega_store::write_coordination::WriteCategory::McpServers,
        )
        .map_err(|error| CredentialReadError::database(CredentialReadStage::Mcp, error))?;
        vega_store::mcp_config::projection_current(store.conn(), &path).map_err(mcp_read_error)?;
        for entry in mcp_servers::pending_cleanup(store.conn()).map_err(mcp_read_error)? {
            if !is_owned_secret_ref(&entry.server_id, &entry.credential_ref)
                || !mcp_servers::cleanup_target_is_stale(store.conn(), &entry)
                    .map_err(mcp_read_error)?
            {
                return Err(crate::credential_errors::state_error(
                    CredentialReadStage::Mcp,
                ));
            }
            match vega_store::keystore::delete_key(&self.config_root, &entry.credential_ref) {
                Ok(()) | Err(vega_store::keystore::Error::Missing) => {}
                Err(error) => {
                    return Err(crate::credential_errors::credential_error(
                        CredentialReadStage::Mcp,
                        error,
                    ));
                }
            }
            mcp_servers::acknowledge_cleanup(store.conn(), &entry).map_err(mcp_read_error)?;
        }
        for row in mcp_servers::list(store.conn())
            .map_err(mcp_read_error)?
            .into_iter()
            .filter(|row| row.deleting)
        {
            let _ = mcp_servers::finish_remove(store.conn(), &row.id).map_err(mcp_read_error)?;
        }
        Ok(())
    }

    async fn ensure_ready(&self, row: &McpServerRow) -> Result<McpReadyServer, McpSettingsError> {
        let (epoch, connecting) = {
            let mut state = self.lock()?;
            if state.suspended.contains(&row.id) {
                return Err(McpSettingsError::Conflict);
            }
            state
                .leases
                .entry(row.id.clone())
                .or_default()
                .retain(|(_, lease)| lease.is_live());
            (state.epoch(&row.id), state.connection_token(&row.id))
        };
        // The sink is not armed until the exact connection passes the DB CAS
        // and its revocation lease is registered. Thereafter a 403 from an
        // older, cancelled or detached run cannot propose a scope upgrade.
        let bound_lease = Arc::new(OnceLock::<McpRevocationLease>::new());
        let scope_sink: Option<ScopeChallengeSink> =
            (row.remote_auth_mode.as_deref() == Some("oauth")).then(|| {
                let weak_registry = Arc::downgrade(&self.registry);
                let bound_lease = bound_lease.clone();
                let id = row.id.clone();
                let revision = row.config_revision;
                Arc::new(move |challenge: vega_mcp::ScopeChallenge| {
                    if let Some(bound) = bound_lease.get().filter(|lease| lease.is_live())
                        && let Some(registry) = weak_registry.upgrade()
                        && let Ok(mut state) = registry.lock()
                        && state.epoch(&id) == epoch
                        && !state.suspended.contains(&id)
                        && state.leases.get(&id).is_some_and(|leases| {
                            leases.iter().any(|(current_revision, lease)| {
                                *current_revision == revision
                                    && lease.same_connection(bound)
                                    && lease.is_live()
                            })
                        })
                    {
                        state.step_up_challenges.insert(
                            id.clone(),
                            BoundStepUpChallenge {
                                revision,
                                epoch,
                                challenge: Arc::new(challenge),
                            },
                        );
                    }
                }) as ScopeChallengeSink
            });
        let attempt = tokio::select! {
            biased;
            _ = connecting.cancelled() => return Err(McpSettingsError::Conflict),
            ready = connect_row_with_timeout(&self.config_root, row, scope_sink, MCP_CONNECT_DISCOVER_TIMEOUT) => ready,
        };
        let connected = match attempt {
            Ok(ready) => ready,
            Err(error) => {
                let store = self.store()?;
                let _ = mcp_servers::set_last_error(
                    store.conn(),
                    &row.id,
                    row.config_revision,
                    Some(error.code()),
                );
                return Err(error);
            }
        };
        let store = self.store()?;
        let mut state = self.lock()?;
        let current =
            mcp_servers::find(store.conn(), &row.id)?.ok_or(McpSettingsError::NotFound)?;
        if state.epoch(&row.id) != epoch
            || state.suspended.contains(&row.id)
            || !current.enabled
            || current.deleting
            || current.config_revision != row.config_revision
        {
            connected.ready.revoke();
            return Err(McpSettingsError::Conflict);
        }
        if let Some((reference, envelope)) = connected.refreshed_oauth.as_ref() {
            if current.remote_credential_ref.as_deref() != Some(reference.as_str()) {
                connected.ready.revoke();
                return Err(McpSettingsError::Conflict);
            }
            if vega_store::keystore::set_key(&self.config_root, reference, envelope).is_err() {
                connected.ready.revoke();
                return Err(McpSettingsError::Credential);
            }
        }
        // A concurrent external metadata revision must not become executable
        // after the validation read but before registry publication.
        mcp_servers::set_last_error(store.conn(), &row.id, row.config_revision, None)?;
        let lease = connected.ready.revocation_lease();
        state
            .leases
            .entry(row.id.clone())
            .or_default()
            .push((row.config_revision, lease.clone()));
        drop(state);
        let _ = bound_lease.set(lease);
        let mut owner = self.clone();
        owner.configuration_budget = None;
        let id = row.id.clone();
        let revision = row.config_revision;
        Ok(connected.ready.with_admission_check(Arc::new(move || {
            let store = owner.store().map_err(|_| ())?;
            vega_store::mcp_config::admitted(
                store.conn(),
                &owner.config_root.join("config.yaml"),
                &id,
                revision,
            )
            .map_err(|_| ())
        })))
    }
}

fn oauth_endpoint(
    row: &McpServerRow,
    expected_revision: u64,
) -> Result<(&str, bool), McpSettingsError> {
    if row.config_revision != expected_revision
        || row.deleting
        || row.transport != "remote"
        || row.remote_auth_mode.as_deref() != Some("oauth")
    {
        return Err(McpSettingsError::Conflict);
    }
    Ok((
        row.remote_endpoint
            .as_deref()
            .ok_or(McpSettingsError::Invalid)?,
        row.remote_allow_loopback_http,
    ))
}

fn map_oauth_error(error: vega_mcp::McpError) -> McpSettingsError {
    match error {
        vega_mcp::McpError::UnsupportedTransport => McpSettingsError::UnsupportedTransport,
        vega_mcp::McpError::CimdUnavailable => McpSettingsError::CimdUnavailable,
        vega_mcp::McpError::ConsentRequired => McpSettingsError::ConfirmationRequired,
        vega_mcp::McpError::AuthRequired
        | vega_mcp::McpError::AuthSecurity
        | vega_mcp::McpError::CredentialBinding
        | vega_mcp::McpError::ScopeEscalation => McpSettingsError::AuthorizationFailed,
        _ => McpSettingsError::Connection,
    }
}

fn map_remote_connection_error(error: vega_mcp::McpError) -> McpSettingsError {
    match error {
        vega_mcp::McpError::UnsupportedTransport => McpSettingsError::UnsupportedTransport,
        _ => McpSettingsError::Connection,
    }
}

async fn receive_oauth_callback(
    listener: StdTcpListener,
    remaining: Duration,
) -> Result<(String, OAuthCallbackStream), McpSettingsError> {
    let listener = tokio::net::TcpListener::from_std(listener)
        .map_err(|_| McpSettingsError::AuthorizationFailed)?;
    let port = listener
        .local_addr()
        .map_err(|_| McpSettingsError::AuthorizationFailed)?
        .port();
    let (mut browser, peer) = tokio::time::timeout(remaining, listener.accept())
        .await
        .map_err(|_| McpSettingsError::AuthorizationFailed)?
        .map_err(|_| McpSettingsError::AuthorizationFailed)?;
    if !peer.ip().is_loopback() {
        return Err(McpSettingsError::AuthorizationFailed);
    }
    let mut request = Vec::with_capacity(1024);
    loop {
        if request.len() >= 8192 {
            return Err(McpSettingsError::AuthorizationFailed);
        }
        let mut chunk = [0u8; 1024];
        let read = tokio::time::timeout(Duration::from_secs(10), browser.read(&mut chunk))
            .await
            .map_err(|_| McpSettingsError::AuthorizationFailed)?
            .map_err(|_| McpSettingsError::AuthorizationFailed)?;
        if read == 0 {
            return Err(McpSettingsError::AuthorizationFailed);
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request =
        std::str::from_utf8(&request).map_err(|_| McpSettingsError::AuthorizationFailed)?;
    let mut lines = request.split("\r\n");
    let first = lines.next().ok_or(McpSettingsError::AuthorizationFailed)?;
    let mut words = first.split_ascii_whitespace();
    let method = words.next().ok_or(McpSettingsError::AuthorizationFailed)?;
    let target = words.next().ok_or(McpSettingsError::AuthorizationFailed)?;
    let version = words.next().ok_or(McpSettingsError::AuthorizationFailed)?;
    if method != "GET"
        || !target.starts_with("/callback?")
        || !matches!(version, "HTTP/1.1" | "HTTP/1.0")
        || words.next().is_some()
        || !lines.any(|line| line.eq_ignore_ascii_case(&format!("host: 127.0.0.1:{port}")))
    {
        return Err(McpSettingsError::AuthorizationFailed);
    }
    Ok((format!("http://127.0.0.1:{port}{target}"), browser))
}

fn validate_yaml_row(row: &McpServerRow) -> Result<(), McpStoreError> {
    let form = form_from_row(row).map_err(|_| McpStoreError::Invalid)?;
    draft_from_form(&form).map_err(|_| McpStoreError::Invalid)?;
    Ok(())
}

fn draft_from_form(form: &McpServerForm) -> Result<McpServerDraft, McpSettingsError> {
    if form.display_name.trim().is_empty()
        || form.display_name.len() > 128
        || form.display_name.chars().any(char::is_control)
    {
        return Err(McpSettingsError::Invalid);
    }
    let mut draft = McpServerDraft {
        display_name: form.display_name.clone(),
        transport: String::new(),
        local_executable: None,
        local_args_json: None,
        local_working_directory: None,
        local_env_refs_json: None,
        remote_endpoint: None,
        remote_allow_loopback_http: false,
        remote_auth_mode: None,
        remote_credential_ref: None,
        remote_oauth_issuer: None,
        remote_oauth_client_id: None,
    };
    match &form.transport {
        McpServerTransport::Local {
            executable,
            args,
            working_directory,
            environment,
        } => {
            if !executable.is_absolute()
                || args.len() > 64
                || args
                    .iter()
                    .any(|arg| arg.len() > 4096 || arg.contains('\0') || credential_shaped_arg(arg))
                || working_directory
                    .as_ref()
                    .is_some_and(|path| !path.is_absolute())
                || environment.len() > 32
            {
                return Err(McpSettingsError::Invalid);
            }
            let mut names = HashSet::new();
            for binding in environment {
                if !valid_env_binding(binding) || !names.insert(&binding.variable) {
                    return Err(McpSettingsError::Invalid);
                }
            }
            draft.transport = "local".into();
            draft.local_executable = Some(executable.to_string_lossy().into_owned());
            draft.local_args_json =
                Some(serde_json::to_string(args).map_err(|_| McpSettingsError::Invalid)?);
            draft.local_working_directory = working_directory
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            let names: Vec<_> = environment
                .iter()
                .map(|binding| binding.variable.as_str())
                .collect();
            draft.local_env_refs_json =
                Some(serde_json::to_string(&names).map_err(|_| McpSettingsError::Invalid)?);
        }
        McpServerTransport::Remote {
            endpoint,
            allow_loopback_http,
            authorization,
        } => {
            vega_mcp::validate_endpoint_url(endpoint, *allow_loopback_http)
                .map_err(|_| McpSettingsError::Invalid)?;
            draft.transport = "remote".into();
            draft.remote_endpoint = Some(endpoint.clone());
            draft.remote_allow_loopback_http = *allow_loopback_http;
            match authorization {
                McpRemoteAuthorization::None => draft.remote_auth_mode = Some("none".into()),
                McpRemoteAuthorization::Bearer => {
                    draft.remote_auth_mode = Some("bearer".into());
                }
                McpRemoteAuthorization::OAuth { client_id } => {
                    if client_id.as_ref().is_some_and(|id| {
                        id.is_empty() || id.len() > 512 || id.chars().any(char::is_control)
                    }) {
                        return Err(McpSettingsError::Invalid);
                    }
                    draft.remote_auth_mode = Some("oauth".into());
                    draft.remote_oauth_client_id = client_id.clone();
                }
            }
        }
    }
    Ok(draft)
}

fn valid_env_binding(binding: &McpEnvironmentVariable) -> bool {
    !binding.variable.is_empty()
        && binding.variable.len() <= 128
        && binding
            .variable
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Arbitrary argv cannot be classified perfectly, so block the common direct
/// credential forms before any SQLite write. The Settings UI points users to
/// owner-only env slots; it must not promise every custom argument is secret.
fn credential_shaped_arg(argument: &str) -> bool {
    let lower = argument.to_ascii_lowercase();
    let normalized = lower.trim_start_matches('-');
    let key = normalized.split(['=', ':']).next().unwrap_or_default();
    matches!(
        key,
        "token"
            | "access-token"
            | "refresh-token"
            | "api-key"
            | "apikey"
            | "password"
            | "secret"
            | "authorization"
    ) || lower.contains("authorization: bearer ")
        || lower.contains("api_key=")
        || lower.contains("access_token=")
        || lower.contains("refresh_token=")
}

fn draft_from_row(row: &McpServerRow) -> McpServerDraft {
    McpServerDraft {
        display_name: row.display_name.clone(),
        transport: row.transport.clone(),
        local_executable: row.local_executable.clone(),
        local_args_json: row.local_args_json.clone(),
        local_working_directory: row.local_working_directory.clone(),
        local_env_refs_json: row.local_env_refs_json.clone(),
        remote_endpoint: row.remote_endpoint.clone(),
        remote_allow_loopback_http: row.remote_allow_loopback_http,
        remote_auth_mode: row.remote_auth_mode.clone(),
        remote_credential_ref: None,
        remote_oauth_issuer: row.remote_oauth_issuer.clone(),
        remote_oauth_client_id: row.remote_oauth_client_id.clone(),
    }
}

fn form_from_row(row: &McpServerRow) -> Result<McpServerForm, McpSettingsError> {
    let transport = match row.transport.as_str() {
        "local" => McpServerTransport::Local {
            executable: PathBuf::from(
                row.local_executable
                    .as_ref()
                    .ok_or(McpSettingsError::Store)?,
            ),
            args: serde_json::from_str(
                row.local_args_json
                    .as_deref()
                    .ok_or(McpSettingsError::Store)?,
            )
            .map_err(|_| McpSettingsError::Store)?,
            working_directory: row.local_working_directory.as_ref().map(PathBuf::from),
            environment: env_slots_from_row(row)?
                .into_iter()
                .map(|slot| McpEnvironmentVariable {
                    variable: slot.variable,
                })
                .collect(),
        },
        "remote" => {
            let authorization = match row.remote_auth_mode.as_deref() {
                Some("none") => McpRemoteAuthorization::None,
                Some("bearer") => McpRemoteAuthorization::Bearer,
                Some("oauth") => McpRemoteAuthorization::OAuth {
                    client_id: row.remote_oauth_client_id.clone(),
                },
                _ => return Err(McpSettingsError::Store),
            };
            McpServerTransport::Remote {
                endpoint: row.remote_endpoint.clone().ok_or(McpSettingsError::Store)?,
                allow_loopback_http: row.remote_allow_loopback_http,
                authorization,
            }
        }
        _ => return Err(McpSettingsError::Store),
    };
    let form = McpServerForm {
        display_name: row.display_name.clone(),
        transport,
    };
    Ok(form)
}

fn env_slots_from_row(row: &McpServerRow) -> Result<Vec<McpEnvSlot>, McpSettingsError> {
    let json = row
        .local_env_refs_json
        .as_deref()
        .ok_or(McpSettingsError::Store)?;
    let slots: Vec<McpEnvSlot> = serde_json::from_str(json).map_err(|_| McpSettingsError::Store)?;
    if slots.len() > 32 {
        return Err(McpSettingsError::Store);
    }
    let mut names = HashSet::new();
    for slot in &slots {
        if !valid_env_binding(&McpEnvironmentVariable {
            variable: slot.variable.clone(),
        }) || !names.insert(&slot.variable)
            || !is_owned_env_ref(
                &row.id,
                row.config_revision,
                &slot.variable,
                &slot.credential_ref,
            )
        {
            return Err(McpSettingsError::Store);
        }
    }
    Ok(slots)
}

fn oauth_token_values(envelope: &str) -> Result<Vec<String>, McpSettingsError> {
    let value: serde_json::Value =
        serde_json::from_str(envelope).map_err(|_| McpSettingsError::Credential)?;
    let access = value
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(McpSettingsError::Credential)?;
    let mut values = vec![access.to_owned()];
    if let Some(refresh) = value.get("refresh_token") {
        match refresh {
            serde_json::Value::Null => {}
            serde_json::Value::String(refresh) if !refresh.is_empty() => {
                values.push(refresh.clone());
            }
            _ => return Err(McpSettingsError::Credential),
        }
    }
    Ok(values)
}

pub(crate) fn all_owner_credentials(
    config_root: &std::path::Path,
    store: &Store,
) -> Result<Vec<String>, CredentialReadError> {
    // A reference name is not a credential type: a Provider may legitimately
    // be named `mcp-demo-oauth`. Only validated MCP records can identify an
    // owner-only OAuth token envelope; every other key is opaque.
    let mut oauth_refs = HashSet::new();
    for row in mcp_servers::list(store.conn()).map_err(mcp_read_error)? {
        if row.transport == "remote"
            && row.remote_auth_mode.as_deref() == Some("oauth")
            && let Some(reference) = row.remote_credential_ref
        {
            if !is_owned_secret_ref(&row.id, &reference) || !reference.ends_with("-oauth") {
                return Err(CredentialReadError::new(
                    CredentialReadStage::Mcp,
                    CredentialReadReason::Malformed,
                ));
            }
            oauth_refs.insert(reference);
        }
    }
    for pending in mcp_servers::pending_cleanup(store.conn()).map_err(mcp_read_error)? {
        if pending.credential_ref.ends_with("-oauth")
            && is_owned_secret_ref(&pending.server_id, &pending.credential_ref)
            && mcp_servers::cleanup_target_is_stale(store.conn(), &pending)
                .map_err(mcp_read_error)?
        {
            oauth_refs.insert(pending.credential_ref);
        }
    }
    let references = match vega_store::keystore::available_refs(config_root) {
        Ok(references) => references,
        Err(vega_store::keystore::Error::Missing) => return Ok(Vec::new()),
        Err(error) => {
            return Err(crate::credential_errors::credential_error(
                CredentialReadStage::Mcp,
                error,
            ));
        }
    };
    let mut known = Vec::new();
    for reference in references {
        let value = match vega_store::keystore::get_key(config_root, &reference) {
            Ok(value) => value,
            Err(vega_store::keystore::Error::Missing) => continue,
            Err(error) => {
                return Err(crate::credential_errors::credential_error(
                    CredentialReadStage::Mcp,
                    error,
                ));
            }
        };
        if oauth_refs.contains(&reference) {
            known.extend(oauth_token_values(&value).map_err(|_| {
                CredentialReadError::new(CredentialReadStage::Mcp, CredentialReadReason::Malformed)
            })?);
        } else {
            known.push(value);
        }
    }
    Ok(known)
}

fn known_credentials_from_row(
    config_root: &std::path::Path,
    row: &McpServerRow,
) -> Result<Vec<String>, CredentialReadError> {
    let references = match row.transport.as_str() {
        "local" => env_slots_from_row(row)
            .map_err(|_| {
                CredentialReadError::new(
                    CredentialReadStage::Mcp,
                    CredentialReadReason::Configuration,
                )
            })?
            .into_iter()
            .map(|slot| slot.credential_ref)
            .collect::<Vec<_>>(),
        "remote" if matches!(row.remote_auth_mode.as_deref(), Some("bearer" | "oauth")) => {
            let expected_suffix = if row.remote_auth_mode.as_deref() == Some("oauth") {
                "-oauth"
            } else {
                "-bearer"
            };
            match row.remote_credential_ref.as_ref() {
                Some(reference)
                    if is_owned_secret_ref(&row.id, reference)
                        && reference.ends_with(expected_suffix) =>
                {
                    vec![reference.clone()]
                }
                Some(_) => {
                    return Err(CredentialReadError::new(
                        CredentialReadStage::Mcp,
                        CredentialReadReason::Malformed,
                    ));
                }
                None => Vec::new(),
            }
        }
        "remote" => Vec::new(),
        _ => {
            return Err(CredentialReadError::new(
                CredentialReadStage::Mcp,
                CredentialReadReason::Configuration,
            ));
        }
    };
    let mut known = Vec::new();
    for reference in references {
        let value = match vega_store::keystore::get_key(config_root, &reference) {
            Ok(value) => value,
            // A missing reference is not an owner-held value; ensure_ready
            // already reports that server as unavailable.
            Err(vega_store::keystore::Error::Missing) => continue,
            Err(error) => {
                return Err(crate::credential_errors::credential_error(
                    CredentialReadStage::Mcp,
                    error,
                ));
            }
        };
        if row.remote_auth_mode.as_deref() == Some("oauth") {
            known.extend(oauth_token_values(&value).map_err(|_| {
                CredentialReadError::new(CredentialReadStage::Mcp, CredentialReadReason::Malformed)
            })?);
        } else {
            known.push(value);
        }
    }
    Ok(known)
}

fn is_owned_secret_ref(id: &str, reference: &str) -> bool {
    if !id
        .parse::<ulid::Ulid>()
        .is_ok_and(|parsed| parsed.to_string() == id)
    {
        return false;
    }
    let prefix = format!("mcp-{id}-");
    let Some(suffix) = reference.strip_prefix(&prefix) else {
        return false;
    };
    let Some(rest) = suffix.strip_prefix('r') else {
        return false;
    };
    let Some((revision, kind)) = rest.split_once('-') else {
        return false;
    };
    if !revision
        .parse::<u64>()
        .is_ok_and(|parsed| parsed > 0 && parsed.to_string() == revision)
    {
        return false;
    }
    if matches!(kind, "bearer" | "oauth") {
        return true;
    }
    kind.strip_prefix("env-").is_some_and(|variable| {
        valid_env_binding(&McpEnvironmentVariable {
            variable: variable.to_owned(),
        })
    })
}

fn is_owned_env_ref(id: &str, current_revision: u64, variable: &str, reference: &str) -> bool {
    if !is_owned_secret_ref(id, reference) {
        return false;
    }
    let prefix = format!("mcp-{id}-r");
    reference
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&format!("-env-{variable}")))
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|revision| revision > 0 && revision <= current_revision)
}

fn view(
    row: &McpServerRow,
    _state: &RegistryState,
    config_root: &std::path::Path,
) -> Result<McpServerView, McpSettingsError> {
    let form = form_from_row(row)?;
    let references: Vec<String> = match &form.transport {
        McpServerTransport::Local { .. } => env_slots_from_row(row)?
            .into_iter()
            .map(|slot| slot.credential_ref)
            .collect(),
        McpServerTransport::Remote {
            authorization: McpRemoteAuthorization::Bearer | McpRemoteAuthorization::OAuth { .. },
            ..
        } => row.remote_credential_ref.iter().cloned().collect(),
        _ => Vec::new(),
    };
    let mut credential_error = false;
    let credential_configured = if row.deleting {
        false
    } else if references.is_empty() {
        !matches!(
            &form.transport,
            McpServerTransport::Remote {
                authorization: McpRemoteAuthorization::OAuth { .. }
                    | McpRemoteAuthorization::Bearer,
                ..
            }
        )
    } else {
        let available = match vega_store::keystore::available_refs(config_root) {
            Ok(available) => available,
            Err(vega_store::keystore::Error::Missing) => Vec::new(),
            Err(_) => {
                credential_error = true;
                Vec::new()
            }
        };
        references
            .iter()
            .all(|reference| available.iter().any(|name| name == reference))
    };
    let health = if row.deleting {
        McpServerHealth::PendingRemoval
    } else if !row.enabled {
        McpServerHealth::Disabled
    } else if credential_error {
        McpServerHealth::Error("credential_unavailable".into())
    } else if let Some(code) = &row.last_error_code {
        match code.as_str() {
            "credential_missing" => McpServerHealth::NeedsCredential,
            "authorization_required" => McpServerHealth::NeedsAuthorization,
            _ => McpServerHealth::Error(code.clone()),
        }
    } else {
        McpServerHealth::Disconnected
    };
    Ok(McpServerView {
        id: row.id.clone(),
        config_revision: row.config_revision,
        enabled: row.enabled,
        deleting: row.deleting,
        form,
        credential_configured,
        health,
        tool_names: Vec::new(),
        rejected_tools: Vec::new(),
    })
}

async fn connect_row_with_timeout(
    config_root: &std::path::Path,
    row: &McpServerRow,
    scope_sink: Option<ScopeChallengeSink>,
    timeout: Duration,
) -> Result<ConnectedRow, McpSettingsError> {
    // A single deadline covers transport negotiation, OAuth refresh where
    // applicable, and every tools/list page. The transport's own per-phase
    // ceilings may be stricter, but may never extend this overall budget.
    tokio::time::timeout(timeout, connect_row(config_root, row, scope_sink))
        .await
        .map_err(|_| McpSettingsError::Connection)?
}

async fn connect_row(
    config_root: &std::path::Path,
    row: &McpServerRow,
    scope_sink: Option<ScopeChallengeSink>,
) -> Result<ConnectedRow, McpSettingsError> {
    let form = form_from_row(row)?;
    match form.transport {
        McpServerTransport::Local {
            executable,
            args,
            working_directory,
            environment: _,
        } => {
            let slots = env_slots_from_row(row)?;
            let config_path = config_root.join("config.yaml");
            let config = if slots.is_empty() {
                vega_store::config::AppConfig::default()
            } else {
                vega_store::config::read_from(&config_path)
                    .map_err(|_| McpSettingsError::CredentialBindingRequired)?
            };
            let bindings = slots
                .iter()
                .map(|slot| {
                    config
                        .credential_bindings
                        .iter()
                        .find(|binding| {
                            binding.env == slot.variable && binding.key_ref == slot.credential_ref
                        })
                        .cloned()
                        .ok_or(McpSettingsError::CredentialBindingRequired)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let root = config_root.to_path_buf();
            let approved = config.credential_bindings.clone();
            let reader: vega_credentials::SecretReader = Arc::new(move |key_ref| {
                let current = vega_store::config::read_from(&config_path)
                    .map_err(|_| vega_credentials::CredentialError::Unavailable)?;
                if current.credential_bindings != approved {
                    return Err(vega_credentials::CredentialError::Unavailable);
                }
                vega_store::keystore::get_key(&root, key_ref)
                    .map_err(|_| vega_credentials::CredentialError::Unavailable)
            });
            let policy = Some({
                Arc::new(vega_credentials::ProcessPolicy {
                    root: config_root.to_path_buf(),
                    bindings,
                    reader,
                })
            });
            let ready = McpReadyServer::connect_local(
                row.id.clone(),
                row.config_revision,
                vega_mcp::LocalServer {
                    credential_policy: policy,
                    executable,
                    args,
                    working_directory: working_directory
                        .unwrap_or_else(|| config_root.to_path_buf()),
                    environment: Vec::new(),
                },
            )
            .await
            .map_err(|_| McpSettingsError::Connection)?
            .with_server_display_name(row.display_name.clone())
            .map_err(|_| McpSettingsError::Connection)?;
            Ok(ConnectedRow {
                ready,
                refreshed_oauth: None,
            })
        }
        McpServerTransport::Remote {
            endpoint,
            allow_loopback_http,
            authorization,
        } => {
            let mut refreshed_oauth = None;
            let mut known_credentials = Vec::new();
            let client = match authorization {
                McpRemoteAuthorization::None => {
                    vega_mcp::HttpClient::connect(&endpoint, allow_loopback_http)
                        .await
                        .map_err(map_remote_connection_error)?
                }
                McpRemoteAuthorization::Bearer => {
                    let reference = row
                        .remote_credential_ref
                        .as_deref()
                        .ok_or(McpSettingsError::Credential)?;
                    if !is_owned_secret_ref(&row.id, reference) || !reference.ends_with("-bearer") {
                        return Err(McpSettingsError::Credential);
                    }
                    let secret = vega_store::keystore::get_key(config_root, reference)
                        .map_err(|_| McpSettingsError::Credential)?;
                    known_credentials.push(secret.clone());
                    let bearer =
                        vega_mcp::BearerCredential::manual(&endpoint, allow_loopback_http, secret)
                            .map_err(|_| McpSettingsError::Credential)?;
                    vega_mcp::HttpClient::connect_with_bearer(
                        &endpoint,
                        allow_loopback_http,
                        bearer,
                    )
                    .await
                    .map_err(map_remote_connection_error)?
                }
                McpRemoteAuthorization::OAuth { .. } => {
                    let reference = row
                        .remote_credential_ref
                        .as_deref()
                        .ok_or(McpSettingsError::AuthorizationRequired)?;
                    if !is_owned_secret_ref(&row.id, reference) || !reference.ends_with("-oauth") {
                        return Err(McpSettingsError::Credential);
                    }
                    let generation = reference
                        .strip_prefix(&format!("mcp-{}-r", row.id))
                        .and_then(|suffix| suffix.strip_suffix("-oauth"))
                        .and_then(|generation| generation.parse::<u64>().ok())
                        .ok_or(McpSettingsError::Credential)?;
                    let issuer = row
                        .remote_oauth_issuer
                        .as_deref()
                        .ok_or(McpSettingsError::AuthorizationRequired)?;
                    let client_id = row
                        .remote_oauth_client_id
                        .as_deref()
                        .ok_or(McpSettingsError::AuthorizationRequired)?;
                    let resource =
                        vega_mcp::ResourceAuthorization::discover(&endpoint, allow_loopback_http)
                            .await
                            .map_err(map_oauth_error)?;
                    let server = resource
                        .discover_server(issuer)
                        .await
                        .map_err(map_oauth_error)?;
                    // Refresh uses no browser redirect; this exact loopback
                    // URI is only a local OAuthClient identity placeholder.
                    let oauth_client = server
                        .pre_registered(client_id, issuer, "http://127.0.0.1:1/callback")
                        .map_err(map_oauth_error)?;
                    let envelope = vega_store::keystore::get_key(config_root, reference)
                        .map_err(|_| McpSettingsError::Credential)?;
                    known_credentials.extend(oauth_token_values(&envelope)?);
                    let tokens = oauth_client
                        .restore_owner_only_tokens(&envelope, &resource, &row.id, generation)
                        .map_err(map_oauth_error)?;
                    let tokens = if tokens.is_expired() {
                        let refreshed = oauth_client
                            .refresh(&tokens)
                            .await
                            .map_err(map_oauth_error)?;
                        let envelope = refreshed
                            .to_owner_only_envelope(&row.id, generation)
                            .map_err(map_oauth_error)?;
                        known_credentials.extend(oauth_token_values(&envelope)?);
                        refreshed_oauth = Some((reference.to_owned(), envelope));
                        refreshed
                    } else {
                        tokens
                    };
                    let bearer = tokens
                        .bearer_credential(&resource)
                        .map_err(map_oauth_error)?;
                    vega_mcp::HttpClient::connect_with_bearer(
                        &endpoint,
                        allow_loopback_http,
                        bearer,
                    )
                    .await
                    .map_err(map_oauth_error)?
                }
            };
            let ready = McpReadyServer::connect_http_with_scope_challenge_sink(
                row.id.clone(),
                row.config_revision,
                client,
                scope_sink,
            )
            .await
            .map_err(|_| McpSettingsError::Connection)?
            .with_server_display_name(row.display_name.clone())
            .map_err(|_| McpSettingsError::Connection)?
            .with_known_credentials(known_credentials);
            Ok(ConnectedRow {
                ready,
                refreshed_oauth,
            })
        }
    }
}
