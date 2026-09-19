//! MCP Settings authority: DB metadata, explicit credential references, and
//! revocable ready handles. UI code never reads SQLite or launches MCP itself.
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::net::TcpListener as StdTcpListener;
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
    #[error("MCP configuration is invalid")]
    Invalid,
    #[error("MCP configuration changed; reconnect this server before retrying")]
    Conflict,
    #[error("MCP server not found")]
    NotFound,
    #[error("at most eight MCP servers may be enabled")]
    Capacity,
    #[error("MCP configuration storage failed")]
    Store,
    #[error("MCP credential storage is unavailable")]
    Credential,
    #[error("MCP authorization is required")]
    AuthorizationRequired,
    #[error("this authorization server requires a public HTTPS Client ID Metadata identity")]
    CimdUnavailable,
    #[error("MCP browser authorization failed or expired")]
    AuthorizationFailed,
    #[error("MCP connection failed")]
    Connection,
    #[error("confirm this MCP server's command or endpoint before activation")]
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
            Self::AuthorizationRequired => "authorization_required",
            Self::CimdUnavailable => "cimd_unavailable",
            Self::AuthorizationFailed => "authorization_failed",
            Self::Connection => "connection_failed",
            Self::ConfirmationRequired => "confirmation_required",
        }
    }
}

impl From<McpStoreError> for McpSettingsError {
    fn from(error: McpStoreError) -> Self {
        match error {
            McpStoreError::Invalid => Self::Invalid,
            McpStoreError::Conflict => Self::Conflict,
            McpStoreError::NotFound => Self::NotFound,
            McpStoreError::Capacity => Self::Capacity,
            McpStoreError::Sql(_) => Self::Store,
        }
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
        self.suspended.insert(id.to_owned());
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
}

impl McpServerSettingsService {
    pub fn new(database_path: PathBuf, config_root: PathBuf) -> Self {
        Self {
            database_path,
            config_root,
            registry: Arc::new(Mutex::new(RegistryState::default())),
        }
    }

    fn store(&self) -> Result<Store, McpSettingsError> {
        let store = Store::open(&self.database_path).map_err(|_| McpSettingsError::Store)?;
        store.migrate().map_err(|_| McpSettingsError::Store)?;
        Ok(store)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, RegistryState>, McpSettingsError> {
        self.registry.lock().map_err(|_| McpSettingsError::Store)
    }

    fn enabled_known_credentials(&self) -> Result<Vec<String>, McpSettingsError> {
        let store = self.store()?;
        let mut known = mcp_servers::list(store.conn())?
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
    pub fn current_owner_credential_values(&self) -> Result<Vec<String>, McpSettingsError> {
        self.enabled_known_credentials()
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
        self.reconcile_pending()?;
        let store = self.store()?;
        let rows: Vec<_> = mcp_servers::list(store.conn())?
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
        let credential_reader: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync> =
            Arc::new(move || credential_owner.enabled_known_credentials().map_err(|_| ()));
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
        let mut state = self.lock()?;
        let store = self.store()?;
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
        Ok(connected.ready)
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
        vega_mcp::McpError::CimdUnavailable => McpSettingsError::CimdUnavailable,
        vega_mcp::McpError::ConsentRequired => McpSettingsError::ConfirmationRequired,
        vega_mcp::McpError::AuthRequired
        | vega_mcp::McpError::AuthSecurity
        | vega_mcp::McpError::CredentialBinding
        | vega_mcp::McpError::ScopeEscalation => McpSettingsError::AuthorizationFailed,
        _ => McpSettingsError::Connection,
    }
}

async fn receive_oauth_callback(
    listener: StdTcpListener,
    remaining: Duration,
) -> Result<(String, tokio::net::TcpStream), McpSettingsError> {
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

fn all_owner_credentials(
    config_root: &std::path::Path,
    store: &Store,
) -> Result<Vec<String>, McpSettingsError> {
    // A reference name is not a credential type: a Provider may legitimately
    // be named `mcp-demo-oauth`. Only validated MCP records can identify an
    // owner-only OAuth token envelope; every other key is opaque.
    let mut oauth_refs = HashSet::new();
    for row in mcp_servers::list(store.conn())? {
        if row.transport == "remote"
            && row.remote_auth_mode.as_deref() == Some("oauth")
            && let Some(reference) = row.remote_credential_ref
        {
            if !is_owned_secret_ref(&row.id, &reference) || !reference.ends_with("-oauth") {
                return Err(McpSettingsError::Credential);
            }
            oauth_refs.insert(reference);
        }
    }
    for pending in mcp_servers::pending_cleanup(store.conn())? {
        if pending.credential_ref.ends_with("-oauth")
            && is_owned_secret_ref(&pending.server_id, &pending.credential_ref)
            && mcp_servers::cleanup_target_is_stale(store.conn(), &pending)?
        {
            oauth_refs.insert(pending.credential_ref);
        }
    }
    let references = match vega_store::keystore::available_refs(config_root) {
        Ok(references) => references,
        Err(vega_store::keystore::Error::Missing) => return Ok(Vec::new()),
        Err(_) => return Err(McpSettingsError::Credential),
    };
    let mut known = Vec::new();
    for reference in references {
        let value = match vega_store::keystore::get_key(config_root, &reference) {
            Ok(value) => value,
            Err(vega_store::keystore::Error::Missing) => continue,
            Err(_) => return Err(McpSettingsError::Credential),
        };
        if oauth_refs.contains(&reference) {
            known.extend(oauth_token_values(&value)?);
        } else {
            known.push(value);
        }
    }
    Ok(known)
}

fn known_credentials_from_row(
    config_root: &std::path::Path,
    row: &McpServerRow,
) -> Result<Vec<String>, McpSettingsError> {
    let references = match row.transport.as_str() {
        "local" => env_slots_from_row(row)?
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
                Some(_) => return Err(McpSettingsError::Credential),
                None => Vec::new(),
            }
        }
        "remote" => Vec::new(),
        _ => return Err(McpSettingsError::Store),
    };
    let mut known = Vec::new();
    for reference in references {
        let value = match vega_store::keystore::get_key(config_root, &reference) {
            Ok(value) => value,
            // A missing reference is not an owner-held value; ensure_ready
            // already reports that server as unavailable.
            Err(vega_store::keystore::Error::Missing) => continue,
            Err(_) => return Err(McpSettingsError::Credential),
        };
        if row.remote_auth_mode.as_deref() == Some("oauth") {
            known.extend(oauth_token_values(&value)?);
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
            let mut values = Vec::with_capacity(slots.len());
            for slot in slots {
                let secret = vega_store::keystore::get_key(config_root, &slot.credential_ref)
                    .map_err(|_| McpSettingsError::Credential)?;
                values.push((OsString::from(slot.variable), OsString::from(secret)));
            }
            let ready = McpReadyServer::connect_local(
                row.id.clone(),
                row.config_revision,
                vega_mcp::LocalServer {
                    executable,
                    args,
                    working_directory: working_directory
                        .unwrap_or_else(|| config_root.to_path_buf()),
                    environment: values,
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
                        .map_err(|_| McpSettingsError::Connection)?
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
                    .map_err(|_| McpSettingsError::Connection)?
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

#[cfg(test)]
#[path = "mcp_settings_oauth_tests.rs"]
mod oauth_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn issue73_owner_secret_sources_cover_local_bearer_and_both_oauth_tokens() {
        let config = tempfile::tempdir().unwrap();
        let id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
        let mut row = McpServerRow {
            id: id.into(),
            display_name: "owned fixture".into(),
            transport: "local".into(),
            local_executable: Some("/bin/sh".into()),
            local_args_json: Some("[]".into()),
            local_working_directory: None,
            local_env_refs_json: None,
            remote_endpoint: None,
            remote_allow_loopback_http: false,
            remote_auth_mode: None,
            remote_credential_ref: None,
            remote_oauth_issuer: None,
            remote_oauth_client_id: None,
            enabled: true,
            deleting: false,
            config_revision: 1,
            last_error_code: None,
            created_at: 0,
            updated_at: 0,
        };
        let local_ref = format!("mcp-{id}-r1-env-API_KEY");
        row.local_env_refs_json = Some(
            serde_json::json!([{
                "variable": "API_KEY",
                "credential_ref": local_ref,
            }])
            .to_string(),
        );
        vega_store::keystore::set_key(config.path(), &local_ref, "fake-local-owner-value-73")
            .unwrap();
        assert_eq!(
            known_credentials_from_row(config.path(), &row).unwrap(),
            vec!["fake-local-owner-value-73"]
        );

        row.transport = "remote".into();
        row.local_env_refs_json = None;
        row.remote_auth_mode = Some("bearer".into());
        let bearer_ref = format!("mcp-{id}-r1-bearer");
        row.remote_credential_ref = Some(bearer_ref.clone());
        vega_store::keystore::set_key(config.path(), &bearer_ref, "fake-bearer-owner-value-73")
            .unwrap();
        assert_eq!(
            known_credentials_from_row(config.path(), &row).unwrap(),
            vec!["fake-bearer-owner-value-73"]
        );

        row.remote_auth_mode = Some("oauth".into());
        let oauth_ref = format!("mcp-{id}-r1-oauth");
        row.remote_credential_ref = Some(oauth_ref.clone());
        vega_store::keystore::set_key(
            config.path(),
            &oauth_ref,
            &serde_json::json!({
                "access_token": "fake-oauth-access-73",
                "refresh_token": "fake-oauth-refresh-73",
                "issuer": "https://fixture.invalid",
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            known_credentials_from_row(config.path(), &row).unwrap(),
            vec!["fake-oauth-access-73", "fake-oauth-refresh-73"]
        );

        row.remote_credential_ref = Some("provider-api-key".into());
        assert!(matches!(
            known_credentials_from_row(config.path(), &row),
            Err(McpSettingsError::Credential)
        ));
    }

    #[test]
    fn issue73_run_secret_snapshot_includes_other_provider_and_disabled_mcp_values() {
        const OTHER_PROVIDER: &str = "fake-second-provider-secret-73";
        const DISABLED_MCP: &str = "fake-disabled-mcp-secret-73";
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        vega_store::keystore::set_key(config.path(), "provider-primary", "fake-primary-73")
            .unwrap();
        vega_store::keystore::set_key(config.path(), "provider-secondary", OTHER_PROVIDER).unwrap();
        let service =
            McpServerSettingsService::new(data.path().join("vega.db"), config.path().into());
        let disabled = service
            .create(McpServerForm {
                display_name: "Disabled owner".into(),
                transport: McpServerTransport::Local {
                    executable: "/bin/sh".into(),
                    args: vec![],
                    working_directory: Some(data.path().into()),
                    environment: vec![McpEnvironmentVariable {
                        variable: "MCP_TOKEN".into(),
                    }],
                },
            })
            .unwrap();
        service
            .set_local_env_secret(
                &disabled.id,
                disabled.config_revision,
                "MCP_TOKEN",
                DISABLED_MCP.into(),
            )
            .unwrap();
        let known = service.enabled_known_credentials().unwrap();
        assert!(known.iter().any(|value| value == OTHER_PROVIDER));
        assert!(known.iter().any(|value| value == DISABLED_MCP));
        assert!(!service.list().unwrap()[0].enabled);
    }

    #[test]
    fn issue73_provider_name_ending_mcp_oauth_is_an_opaque_secret() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        const SECRET: &str = "fake-provider-with-oauth-like-name-73";
        vega_store::keystore::set_key(config.path(), "mcp-demo-oauth", SECRET).unwrap();
        let service =
            McpServerSettingsService::new(data.path().join("vega.db"), config.path().into());

        let known = service.enabled_known_credentials().unwrap();
        assert_eq!(known, vec![SECRET]);
    }

    #[tokio::test]
    async fn issue73_settings_test_never_returns_secret_bearing_tool_name() {
        const SECRET: &str = "fake-provider-key-name-73";
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        vega_store::keystore::set_key(config.path(), "provider-fixture", SECRET).unwrap();
        let script = data.path().join("echo-provider-key-name.sh");
        fs::write(
            &script,
            format!(
                r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{{"tools":{{}}}}}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{{"name":"{SECRET}","inputSchema":{{"type":"object"}}}}]}}}}'
      ;;
  esac
done
"##
            ),
        )
        .unwrap();
        let service =
            McpServerSettingsService::new(data.path().join("vega.db"), config.path().to_path_buf());
        let saved = service
            .create(McpServerForm {
                display_name: "Owned catalog echo".into(),
                transport: McpServerTransport::Local {
                    executable: "/bin/sh".into(),
                    args: vec![script.to_string_lossy().to_string()],
                    working_directory: Some(data.path().to_path_buf()),
                    environment: Vec::new(),
                },
            })
            .unwrap();
        let preview = service
            .test_connection(&saved.id, saved.config_revision, true)
            .await;
        assert!(matches!(preview, Err(McpSettingsError::Connection)));
    }

    #[test]
    fn issue73_oauth_install_revoke_preserves_own_callback_but_cancels_old_flows() {
        let mut state = RegistryState::default();
        let committing = CancellationToken::new();
        let obsolete = CancellationToken::new();
        state.active_oauth.insert(
            "committing".into(),
            ActiveOAuth {
                server_id: "server".into(),
                created: Instant::now(),
                cancel: committing.clone(),
            },
        );
        state.active_oauth.insert(
            "obsolete".into(),
            ActiveOAuth {
                server_id: "server".into(),
                created: Instant::now(),
                cancel: obsolete.clone(),
            },
        );
        assert!(
            state
                .revoke_for_oauth_install("other", "committing")
                .is_err()
        );
        assert!(!committing.is_cancelled() && !obsolete.is_cancelled());

        state
            .revoke_for_oauth_install("server", "committing")
            .expect("matching flow may commit");
        assert!(
            !committing.is_cancelled(),
            "self-revoke must not abort HTTP 200"
        );
        assert!(obsolete.is_cancelled(), "older flow remains revoked");
        assert!(state.active_oauth.is_empty());
        assert!(state.suspended.contains("server"));
    }

    #[test]
    fn issue73_local_credential_shaped_argv_is_rejected_before_persistence() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let service =
            McpServerSettingsService::new(data.path().join("vega.db"), config.path().to_path_buf());
        for argument in [
            "--token=PRIVATE_VALUE",
            "--api-key",
            "Authorization: Bearer PRIVATE_VALUE",
            "OPENAI_API_KEY=PRIVATE_VALUE",
        ] {
            let result = service.create(McpServerForm {
                display_name: "No argv secrets".into(),
                transport: McpServerTransport::Local {
                    executable: "/bin/sh".into(),
                    args: vec![argument.into()],
                    working_directory: None,
                    environment: Vec::new(),
                },
            });
            assert!(matches!(result, Err(McpSettingsError::Invalid)));
        }
        assert!(service.list().unwrap().is_empty());
    }

    fn bearer_form(endpoint: &str) -> McpServerForm {
        McpServerForm {
            display_name: "Owned remote".into(),
            transport: McpServerTransport::Remote {
                endpoint: endpoint.into(),
                allow_loopback_http: true,
                authorization: McpRemoteAuthorization::Bearer,
            },
        }
    }

    #[cfg(unix)]
    fn break_keystore(config: &std::path::Path) {
        use std::os::unix::fs::symlink;
        let directory = config.join("credentials");
        fs::rename(
            directory.join("credentials.toml"),
            directory.join("saved.toml"),
        )
        .unwrap();
        symlink("saved.toml", directory.join("credentials.toml")).unwrap();
    }

    #[cfg(unix)]
    fn repair_keystore(config: &std::path::Path) {
        let directory = config.join("credentials");
        fs::remove_file(directory.join("credentials.toml")).unwrap();
        fs::rename(
            directory.join("saved.toml"),
            directory.join("credentials.toml"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn issue73_settings_save_is_inert_and_only_confirmed_test_or_enable_connects() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let script = data.path().join("owned-server.sh");
        let marker = data.path().join("launched");
        fs::write(
            &script,
            r##"#!/bin/sh
printf 'started\n' >> "$1"
while IFS= read -r request; do
  case "$request" in
    *server/discover*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}' ;;
    *tools/list*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}' ;;
  esac
done
"##,
        )
        .unwrap();
        let service =
            McpServerSettingsService::new(data.path().join("vega.db"), config.path().to_path_buf());
        let saved = service
            .create(McpServerForm {
                display_name: "Owned local".into(),
                transport: McpServerTransport::Local {
                    executable: "/bin/sh".into(),
                    args: vec![
                        script.to_string_lossy().to_string(),
                        marker.to_string_lossy().to_string(),
                    ],
                    working_directory: Some(data.path().to_path_buf()),
                    environment: Vec::new(),
                },
            })
            .unwrap();
        assert!(!saved.enabled);
        assert_eq!(saved.health, McpServerHealth::Disabled);
        assert!(!marker.exists());
        assert!(matches!(
            service
                .test_connection(&saved.id, saved.config_revision, false)
                .await,
            Err(McpSettingsError::ConfirmationRequired)
        ));
        assert!(!marker.exists());
        let tested = service
            .test_connection(&saved.id, saved.config_revision, true)
            .await
            .unwrap();
        assert_eq!(tested.tool_names, ["echo"]);
        assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);
        assert!(
            service
                .ready_for_run()
                .await
                .unwrap()
                .ready_servers
                .is_empty()
        );
        let enabled = service
            .set_enabled(&saved.id, saved.config_revision, true, true)
            .await
            .unwrap();
        assert!(enabled.enabled);
        assert_eq!(enabled.health, McpServerHealth::Disconnected);
        assert!(enabled.tool_names.is_empty());
        assert!(
            !service.lock().unwrap().leases.contains_key(&saved.id),
            "Settings validation must not register an executable run lease"
        );
        let first_run_handle = service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .pop()
            .unwrap();
        let old_run_handle = service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .pop()
            .unwrap();
        assert!(!old_run_handle.is_revoked());
        assert!(matches!(
            service
                .set_enabled(&saved.id, saved.config_revision, false, false)
                .await,
            Err(McpSettingsError::Conflict)
        ));
        assert!(old_run_handle.is_revoked());
        assert!(first_run_handle.is_revoked());
        let suspended = service.ready_for_run().await.unwrap();
        assert!(suspended.ready_servers.is_empty());
        assert_eq!(suspended.unavailable[0].code, "configuration_changed");
        let reconnected = service
            .set_enabled(&saved.id, enabled.config_revision, true, true)
            .await
            .unwrap();
        assert_eq!(reconnected.health, McpServerHealth::Disconnected);
        let disabled = service
            .set_enabled(&saved.id, reconnected.config_revision, false, false)
            .await
            .unwrap();
        assert_eq!(disabled.health, McpServerHealth::Disabled);
        assert!(
            service
                .ready_for_run()
                .await
                .unwrap()
                .ready_servers
                .is_empty()
        );
    }

    #[tokio::test]
    async fn issue73_replace_and_remove_revoke_each_run_owned_lease() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let script = data.path().join("owned-lease-server.sh");
        fs::write(
            &script,
            r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}' ;;
    *tools/list*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}' ;;
  esac
done
"##,
        )
        .unwrap();
        let service =
            McpServerSettingsService::new(data.path().join("vega.db"), config.path().into());
        let saved = service
            .create(McpServerForm {
                display_name: "lease-owner".into(),
                transport: McpServerTransport::Local {
                    executable: "/bin/sh".into(),
                    args: vec![script.to_string_lossy().into_owned()],
                    working_directory: Some(data.path().to_path_buf()),
                    environment: Vec::new(),
                },
            })
            .unwrap();
        let enabled = service
            .set_enabled(&saved.id, saved.config_revision, true, true)
            .await
            .unwrap();
        let discarded = service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .pop()
            .unwrap();
        let discarded_lease = discarded.revocation_lease();
        assert!(discarded_lease.is_live());
        drop(discarded);
        assert!(
            !discarded_lease.is_live(),
            "Settings must not retain a strong run transport after its task ends"
        );
        let first = service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .pop()
            .unwrap();
        let second = service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .pop()
            .unwrap();
        assert!(!first.is_revoked() && !second.is_revoked());
        let changed = service
            .replace(&saved.id, enabled.config_revision, enabled.form.clone())
            .unwrap();
        assert!(first.is_revoked() && second.is_revoked());
        assert!(!changed.enabled);
        let enabled_again = service
            .set_enabled(&saved.id, changed.config_revision, true, true)
            .await
            .unwrap();
        let third = service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .pop()
            .unwrap();
        assert!(!third.is_revoked());
        service
            .remove(&saved.id, enabled_again.config_revision, true)
            .unwrap();
        assert!(third.is_revoked());
        assert!(service.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn issue73_enabled_servers_discover_concurrently_before_first_message() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let script = data.path().join("owned-barrier-server.sh");
        fs::write(
            &script,
            r##"#!/bin/sh
printf 'started' > "$2/$1.started"
while [ ! -f "$2/first.started" ] || [ ! -f "$2/second.started" ]; do
  sleep 0.02
done
while IFS= read -r request; do
  case "$request" in
    *server/discover*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}' ;;
    *tools/list*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}' ;;
  esac
done
"##,
        )
        .unwrap();
        let path = data.path().join("vega.db");
        let service = McpServerSettingsService::new(path.clone(), config.path().to_path_buf());
        let mut saved = Vec::new();
        for name in ["first", "second"] {
            let row = service
                .create(McpServerForm {
                    display_name: name.into(),
                    transport: McpServerTransport::Local {
                        executable: "/bin/sh".into(),
                        args: vec![
                            script.to_string_lossy().to_string(),
                            name.into(),
                            data.path().to_string_lossy().to_string(),
                        ],
                        working_directory: Some(data.path().to_path_buf()),
                        environment: Vec::new(),
                    },
                })
                .unwrap();
            saved.push(row);
        }
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        for row in &saved {
            mcp_servers::set_enabled(store.conn(), &row.id, row.config_revision, true).unwrap();
        }
        let result = tokio::time::timeout(Duration::from_secs(3), service.ready_for_run())
            .await
            .expect("two owned servers must start together")
            .unwrap();
        assert_eq!(result.ready_servers.len(), 2);
        assert!(result.unavailable.is_empty());
        assert!(data.path().join("first.started").exists());
        assert!(data.path().join("second.started").exists());
    }

    #[tokio::test]
    async fn issue73_remove_cancels_concurrent_slow_connection_test() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let marker = data.path().join("slow-started");
        let script = data.path().join("slow-server.sh");
        fs::write(
            &script,
            r##"#!/bin/sh
printf 'started' > "$1"
while IFS= read -r request; do
  :
done
"##,
        )
        .unwrap();
        let service =
            McpServerSettingsService::new(data.path().join("vega.db"), config.path().to_path_buf());
        let saved = service
            .create(McpServerForm {
                display_name: "Slow owned local".into(),
                transport: McpServerTransport::Local {
                    executable: "/bin/sh".into(),
                    args: vec![
                        script.to_string_lossy().to_string(),
                        marker.to_string_lossy().to_string(),
                    ],
                    working_directory: Some(data.path().to_path_buf()),
                    environment: Vec::new(),
                },
            })
            .unwrap();
        let testing = service.clone();
        let id = saved.id.clone();
        let revision = saved.config_revision;
        let attempt =
            tokio::spawn(async move { testing.test_connection(&id, revision, true).await });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !marker.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        service
            .remove(&saved.id, saved.config_revision, true)
            .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), attempt)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(McpSettingsError::Conflict)));
    }

    #[tokio::test]
    async fn issue73_connection_deadline_covers_probe_and_catalog_together() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let script = data.path().join("two-slow-phases.sh");
        let probed = data.path().join("probe-completed");
        fs::write(
            &script,
            r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      sleep 0.18
      printf 'yes' > "$1"
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}'
      ;;
    *tools/list*)
      sleep 0.18
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}'
      ;;
  esac
done
"##,
        )
        .unwrap();
        let service =
            McpServerSettingsService::new(data.path().join("vega.db"), config.path().into());
        let saved = service
            .create(McpServerForm {
                display_name: "Two slow phases".into(),
                transport: McpServerTransport::Local {
                    executable: "/bin/sh".into(),
                    args: vec![
                        script.to_string_lossy().into_owned(),
                        probed.to_string_lossy().into_owned(),
                    ],
                    working_directory: Some(data.path().into()),
                    environment: Vec::new(),
                },
            })
            .unwrap();
        let store = service.store().unwrap();
        let row = mcp_servers::find(store.conn(), &saved.id).unwrap().unwrap();
        let started = Instant::now();
        let result =
            connect_row_with_timeout(config.path(), &row, None, Duration::from_millis(280)).await;
        assert!(matches!(result, Err(McpSettingsError::Connection)));
        assert!(
            probed.exists(),
            "probe must finish before the shared deadline expires"
        );
        assert!(started.elapsed() < Duration::from_millis(700));
    }

    #[test]
    fn issue73_settings_remote_bearer_secret_never_enters_sqlite_or_view() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let database_path = data.path().join("vega.db");
        let service = McpServerSettingsService::new(database_path.clone(), config.path().into());
        let form = McpServerForm {
            display_name: "Owned remote".into(),
            transport: McpServerTransport::Remote {
                endpoint: "http://127.0.0.1:17777/mcp".into(),
                allow_loopback_http: true,
                authorization: McpRemoteAuthorization::Bearer,
            },
        };
        let saved = service.create(form.clone()).unwrap();
        assert!(!saved.credential_configured);
        assert!(matches!(
            service.create(McpServerForm {
                display_name: "not loopback".into(),
                transport: McpServerTransport::Remote {
                    endpoint: "http://example.com/mcp".into(),
                    allow_loopback_http: true,
                    authorization: McpRemoteAuthorization::None,
                },
            }),
            Err(McpSettingsError::Invalid)
        ));
        let rotated = service
            .set_bearer_secret(
                &saved.id,
                saved.config_revision,
                "PRIVATE_TEST_TOKEN".into(),
            )
            .unwrap();
        assert_eq!(rotated.config_revision, saved.config_revision + 1);
        assert!(!rotated.enabled);
        assert!(rotated.credential_configured);
        assert_eq!(rotated.form, form);
        assert!(
            !fs::read(&database_path)
                .unwrap()
                .windows("PRIVATE_TEST_TOKEN".len())
                .any(|bytes| bytes == b"PRIVATE_TEST_TOKEN")
        );
        let reopened = McpServerSettingsService::new(database_path.clone(), config.path().into());
        assert!(reopened.list().unwrap()[0].credential_configured);
        reopened
            .remove(&saved.id, rotated.config_revision, true)
            .unwrap();
        assert!(reopened.list().unwrap().is_empty());
        assert!(
            vega_store::keystore::available_refs(config.path())
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn issue73_replace_cleanup_failure_survives_restart_without_erasing_new_secret() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let database_path = data.path().join("vega.db");
        let service = McpServerSettingsService::new(database_path.clone(), config.path().into());
        let saved = service
            .create(bearer_form("http://127.0.0.1:17777/mcp"))
            .unwrap();
        let configured = service
            .set_bearer_secret(
                &saved.id,
                saved.config_revision,
                "OLD_PRIVATE_BEARER".into(),
            )
            .unwrap();
        let store = Store::open(&database_path).unwrap();
        store.migrate().unwrap();
        let old_ref = mcp_servers::find(store.conn(), &saved.id)
            .unwrap()
            .unwrap()
            .remote_credential_ref
            .unwrap();
        break_keystore(config.path());
        assert!(matches!(
            service.replace(
                &saved.id,
                configured.config_revision,
                bearer_form("http://127.0.0.1:18888/mcp")
            ),
            Err(McpSettingsError::Credential)
        ));
        let changed = mcp_servers::find(store.conn(), &saved.id).unwrap().unwrap();
        let new_ref = changed.remote_credential_ref.clone().unwrap();
        assert_ne!(old_ref, new_ref);
        assert!(!changed.enabled);
        assert!(matches!(
            mcp_servers::set_enabled(store.conn(), &saved.id, changed.config_revision, true),
            Err(McpStoreError::Conflict)
        ));
        let pending = mcp_servers::pending_cleanup(store.conn()).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].credential_ref, old_ref);
        repair_keystore(config.path());
        vega_store::keystore::set_key(config.path(), &new_ref, "NEW_PRIVATE_BEARER").unwrap();
        drop(service);
        let restarted = McpServerSettingsService::new(database_path, config.path().into());
        restarted.reconcile_pending().unwrap();
        assert!(
            mcp_servers::pending_cleanup(store.conn())
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            vega_store::keystore::get_key(config.path(), &old_ref),
            Err(vega_store::keystore::Error::Missing)
        ));
        assert_eq!(
            vega_store::keystore::get_key(config.path(), &new_ref).unwrap(),
            "NEW_PRIVATE_BEARER"
        );
    }

    #[cfg(unix)]
    #[test]
    fn issue73_remove_failure_leaves_disabled_tombstone_and_restart_finishes() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let database_path = data.path().join("vega.db");
        let service = McpServerSettingsService::new(database_path.clone(), config.path().into());
        let saved = service
            .create(bearer_form("http://127.0.0.1:17777/mcp"))
            .unwrap();
        let configured = service
            .set_bearer_secret(
                &saved.id,
                saved.config_revision,
                "REMOVE_PRIVATE_BEARER".into(),
            )
            .unwrap();
        let store = Store::open(&database_path).unwrap();
        store.migrate().unwrap();
        break_keystore(config.path());
        assert!(matches!(
            service.remove(&saved.id, configured.config_revision, true),
            Err(McpSettingsError::Credential)
        ));
        let tombstone = mcp_servers::find(store.conn(), &saved.id).unwrap().unwrap();
        assert!(tombstone.deleting);
        assert!(!tombstone.enabled);
        assert!(matches!(
            mcp_servers::set_enabled(store.conn(), &saved.id, tombstone.config_revision, true),
            Err(McpStoreError::Conflict)
        ));
        repair_keystore(config.path());
        drop(service);
        let restarted = McpServerSettingsService::new(database_path, config.path().into());
        restarted.reconcile_pending().unwrap();
        assert!(
            mcp_servers::find(store.conn(), &saved.id)
                .unwrap()
                .is_none()
        );
        assert!(
            vega_store::keystore::available_refs(config.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn issue73_tampered_outbox_cannot_delete_provider_or_current_mcp_secret() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let database_path = data.path().join("vega.db");
        let service = McpServerSettingsService::new(database_path.clone(), config.path().into());
        let saved = service
            .create(bearer_form("http://127.0.0.1:17777/mcp"))
            .unwrap();
        let configured = service
            .set_bearer_secret(
                &saved.id,
                saved.config_revision,
                "CURRENT_PRIVATE_BEARER".into(),
            )
            .unwrap();
        let store = Store::open(&database_path).unwrap();
        store.migrate().unwrap();
        let current_ref = mcp_servers::find(store.conn(), &saved.id)
            .unwrap()
            .unwrap()
            .remote_credential_ref
            .unwrap();
        vega_store::keystore::set_key(config.path(), "provider-key", "PROVIDER_PRIVATE").unwrap();
        store.conn()
            .execute(
                "INSERT INTO mcp_secret_cleanup (credential_ref, server_id, created_at) VALUES (?1, ?2, 1)",
                ["provider-key", saved.id.as_str()],
            )
            .unwrap();
        assert!(matches!(
            service.reconcile_pending(),
            Err(McpSettingsError::Store)
        ));
        assert_eq!(
            vega_store::keystore::get_key(config.path(), "provider-key").unwrap(),
            "PROVIDER_PRIVATE"
        );
        store
            .conn()
            .execute("DELETE FROM mcp_secret_cleanup", [])
            .unwrap();
        let other = service
            .create(bearer_form("http://127.0.0.1:18888/mcp"))
            .unwrap();
        service
            .set_bearer_secret(
                &other.id,
                other.config_revision,
                "OTHER_SERVER_PRIVATE".into(),
            )
            .unwrap();
        let other_ref = mcp_servers::find(store.conn(), &other.id)
            .unwrap()
            .unwrap()
            .remote_credential_ref
            .unwrap();
        store.conn()
            .execute(
                "INSERT INTO mcp_secret_cleanup (credential_ref, server_id, created_at) VALUES (?1, ?2, 1)",
                [other_ref.as_str(), saved.id.as_str()],
            )
            .unwrap();
        assert!(matches!(
            service.reconcile_pending(),
            Err(McpSettingsError::Store)
        ));
        assert_eq!(
            vega_store::keystore::get_key(config.path(), &other_ref).unwrap(),
            "OTHER_SERVER_PRIVATE"
        );
        store
            .conn()
            .execute("DELETE FROM mcp_secret_cleanup", [])
            .unwrap();
        store.conn()
            .execute(
                "INSERT INTO mcp_secret_cleanup (credential_ref, server_id, created_at) VALUES (?1, ?2, 1)",
                [current_ref.as_str(), saved.id.as_str()],
            )
            .unwrap();
        assert!(matches!(
            service.reconcile_pending(),
            Err(McpSettingsError::Store)
        ));
        assert_eq!(
            vega_store::keystore::get_key(config.path(), &current_ref).unwrap(),
            "CURRENT_PRIVATE_BEARER"
        );
        assert!(!configured.enabled);
    }

    #[test]
    fn issue73_oauth_outbox_rejects_provider_other_server_and_current_revision() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let database_path = data.path().join("vega.db");
        let service = McpServerSettingsService::new(database_path.clone(), config.path().into());
        let make_form = |port| McpServerForm {
            display_name: format!("OAuth {port}"),
            transport: McpServerTransport::Remote {
                endpoint: format!("http://127.0.0.1:{port}/mcp"),
                allow_loopback_http: true,
                authorization: McpRemoteAuthorization::OAuth { client_id: None },
            },
        };
        let first = service.create(make_form(17777)).unwrap();
        let second = service.create(make_form(18888)).unwrap();
        let store = Store::open(&database_path).unwrap();
        store.migrate().unwrap();
        let (_, first_ref) = mcp_servers::begin_oauth_install(
            store.conn(),
            &first.id,
            first.config_revision,
            "https://issuer.example",
            "first-client",
        )
        .unwrap();
        let (_, second_ref) = mcp_servers::begin_oauth_install(
            store.conn(),
            &second.id,
            second.config_revision,
            "https://issuer.example",
            "second-client",
        )
        .unwrap();
        for (reference, value) in [
            ("provider-key", "PROVIDER_PRIVATE"),
            (first_ref.as_str(), "FIRST_PRIVATE_OAUTH"),
            (second_ref.as_str(), "SECOND_PRIVATE_OAUTH"),
        ] {
            vega_store::keystore::set_key(config.path(), reference, value).unwrap();
        }
        for forged in ["provider-key", first_ref.as_str(), second_ref.as_str()] {
            store.conn()
                .execute(
                    "INSERT INTO mcp_secret_cleanup (credential_ref, server_id, created_at) VALUES (?1, ?2, 1)",
                    [forged, first.id.as_str()],
                )
                .unwrap();
            assert!(matches!(
                service.reconcile_pending(),
                Err(McpSettingsError::Store)
            ));
            store
                .conn()
                .execute("DELETE FROM mcp_secret_cleanup", [])
                .unwrap();
        }
        assert_eq!(
            vega_store::keystore::get_key(config.path(), "provider-key").unwrap(),
            "PROVIDER_PRIVATE"
        );
        assert_eq!(
            vega_store::keystore::get_key(config.path(), &first_ref).unwrap(),
            "FIRST_PRIVATE_OAUTH"
        );
        assert_eq!(
            vega_store::keystore::get_key(config.path(), &second_ref).unwrap(),
            "SECOND_PRIVATE_OAUTH"
        );
    }

    #[test]
    fn issue73_local_slot_rotation_retry_never_deletes_new_revision_secret() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let database_path = data.path().join("vega.db");
        let service = McpServerSettingsService::new(database_path.clone(), config.path().into());
        let saved = service
            .create(McpServerForm {
                display_name: "Owned local".into(),
                transport: McpServerTransport::Local {
                    executable: "/bin/echo".into(),
                    args: Vec::new(),
                    working_directory: None,
                    environment: vec![McpEnvironmentVariable {
                        variable: "API_TOKEN".into(),
                    }],
                },
            })
            .unwrap();
        let store = Store::open(&database_path).unwrap();
        store.migrate().unwrap();
        let old = mcp_servers::find(store.conn(), &saved.id).unwrap().unwrap();
        let old_ref = env_slots_from_row(&old).unwrap()[0].credential_ref.clone();
        vega_store::keystore::set_key(config.path(), &old_ref, "OLD_LOCAL_PRIVATE").unwrap();
        let (changed, new_ref) = mcp_servers::begin_local_secret_rotation(
            store.conn(),
            &saved.id,
            saved.config_revision,
            "API_TOKEN",
        )
        .unwrap();
        assert_ne!(old_ref, new_ref);
        assert!(!changed.enabled);
        assert_eq!(mcp_servers::pending_cleanup(store.conn()).unwrap().len(), 1);
        // Model a crash after the new write but before an old cleanup retry.
        vega_store::keystore::set_key(config.path(), &new_ref, "NEW_LOCAL_PRIVATE").unwrap();
        drop(service);
        let restarted = McpServerSettingsService::new(database_path, config.path().into());
        restarted.reconcile_pending().unwrap();
        assert!(matches!(
            vega_store::keystore::get_key(config.path(), &old_ref),
            Err(vega_store::keystore::Error::Missing)
        ));
        assert_eq!(
            vega_store::keystore::get_key(config.path(), &new_ref).unwrap(),
            "NEW_LOCAL_PRIVATE"
        );
    }
}
