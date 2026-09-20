//! Bounded MCP OAuth discovery and public-client code flow. No credential is
//! persisted or logged here; the UI owns consent and owner-only storage.

use std::fs::File;
use std::io::Read;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures::StreamExt;
use reqwest::header::{CONTENT_TYPE, WWW_AUTHENTICATE};
use reqwest::{Client, Response, StatusCode, Url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::time::timeout;

use crate::McpError;
use crate::http::validate_endpoint;
use crate::wire::{empty_params, modern_request};

const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const AUTH_BODY_LIMIT: usize = 64 * 1024;
const AUTH_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_SCOPES: usize = 32;
const MAX_TOKEN_BYTES: usize = 8192;

/// Validated protected-resource metadata and the AS choices shown by Settings.
/// Discovering an AS is a separate, explicit operation: a server cannot force
/// an arbitrary advertised origin to be fetched before the UI selects it.
pub struct ResourceAuthorization {
    client: Client,
    endpoint: Url,
    metadata_url: Url,
    resource: String,
    authorization_servers: Vec<String>,
    requested_scopes: Vec<String>,
    allow_loopback_http: bool,
}

impl ResourceAuthorization {
    /// Probe the exact MCP endpoint, then use the 401 PRM link or ordered
    /// well-known fallbacks on that same origin.
    pub async fn discover(endpoint: &str, allow_loopback_http: bool) -> Result<Self, McpError> {
        timeout(
            AUTH_TIMEOUT,
            Self::discover_inner(endpoint, allow_loopback_http),
        )
        .await
        .map_err(|_| McpError::Timeout)?
    }

    async fn discover_inner(endpoint: &str, allow_loopback_http: bool) -> Result<Self, McpError> {
        let endpoint = Url::parse(endpoint).map_err(|_| McpError::InvalidConfig)?;
        validate_endpoint(&endpoint, allow_loopback_http)?;
        let client = auth_client()?;
        let probe = modern_request(1, "server/discover", empty_params());
        let response = client
            .post(endpoint.clone())
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", crate::MODERN_VERSION)
            .header("Mcp-Method", "server/discover")
            .json(&probe)
            .timeout(AUTH_TIMEOUT)
            .send()
            .await
            .map_err(|_| McpError::Transport)?;
        if response.status().is_redirection() {
            return Err(McpError::AuthSecurity);
        }
        if !matches!(
            response.status(),
            StatusCode::UNAUTHORIZED
                | StatusCode::BAD_REQUEST
                | StatusCode::NOT_FOUND
                | StatusCode::METHOD_NOT_ALLOWED
        ) {
            return Err(McpError::AuthDiscovery);
        }
        let mut challenge = None;
        for header in response.headers().get_all(WWW_AUTHENTICATE) {
            let header = header.to_str().map_err(|_| McpError::AuthSecurity)?;
            if let Some(found) = parse_bearer_challenge(header)?
                && challenge.replace(found).is_some()
            {
                return Err(McpError::AuthSecurity);
            }
        }
        let (metadata_url, challenge_scopes) = match challenge {
            Some(challenge) => (challenge.resource_metadata, challenge.scopes),
            None => (None, Vec::new()),
        };
        let candidates = match metadata_url {
            Some(raw) => {
                let url = secure_url(&raw, allow_loopback_http)?;
                if url.origin() != endpoint.origin() {
                    return Err(McpError::AuthSecurity);
                }
                vec![url]
            }
            None => resource_metadata_candidates(&endpoint)?,
        };
        for url in candidates {
            let response = client
                .get(url.clone())
                .timeout(AUTH_TIMEOUT)
                .send()
                .await
                .map_err(|_| McpError::AuthDiscovery)?;
            if response.status() == StatusCode::NOT_FOUND {
                continue;
            }
            if response.status() != StatusCode::OK {
                return Err(McpError::AuthDiscovery);
            }
            let document = bounded_json(response).await?;
            let resource = required_string(&document, "resource", 2048)?;
            let resource_url = secure_url(resource, allow_loopback_http)?;
            if resource_url.origin() != endpoint.origin()
                || !resource_path_covers_endpoint(resource_url.path(), endpoint.path())
                || resource_url.query().is_some()
            {
                return Err(McpError::AuthSecurity);
            }
            let issuers = required_string_list(&document, "authorization_servers", 8, 2048)?;
            for issuer in &issuers {
                let url = secure_url(issuer, allow_loopback_http)?;
                if url.query().is_some() {
                    return Err(McpError::AuthSecurity);
                }
            }
            let requested_scopes = if !challenge_scopes.is_empty() {
                challenge_scopes.clone()
            } else {
                optional_string_list(&document, "scopes_supported", MAX_SCOPES, 128)?
            };
            return Ok(Self {
                client,
                endpoint,
                metadata_url: url,
                resource: resource.to_owned(),
                authorization_servers: issuers,
                requested_scopes,
                allow_loopback_http,
            });
        }
        Err(McpError::AuthDiscovery)
    }

    /// Canonical target resource that every authorize/token request must carry.
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Validated AS issuer identifiers; the UI must choose one before fetching it.
    pub fn authorization_servers(&self) -> &[String] {
        &self.authorization_servers
    }

    /// Initial least-privilege scope set from the 401 challenge or PRM.
    pub fn requested_scopes(&self) -> &[String] {
        &self.requested_scopes
    }

    /// Discover only an issuer explicitly selected from protected-resource
    /// metadata; OAuth AS metadata is tried before OIDC in normative order.
    pub async fn discover_server(
        &self,
        selected_issuer: &str,
    ) -> Result<AuthorizationServer, McpError> {
        timeout(AUTH_TIMEOUT, self.discover_server_inner(selected_issuer))
            .await
            .map_err(|_| McpError::Timeout)?
    }

    async fn discover_server_inner(
        &self,
        selected_issuer: &str,
    ) -> Result<AuthorizationServer, McpError> {
        if !self
            .authorization_servers
            .iter()
            .any(|issuer| issuer == selected_issuer)
        {
            return Err(McpError::AuthSecurity);
        }
        let issuer = secure_url(selected_issuer, self.allow_loopback_http)?;
        for candidate in authorization_metadata_candidates(&issuer)? {
            let response = self
                .client
                .get(candidate)
                .timeout(AUTH_TIMEOUT)
                .send()
                .await
                .map_err(|_| McpError::AuthDiscovery)?;
            if response.status() == StatusCode::NOT_FOUND {
                continue;
            }
            if response.status() != StatusCode::OK {
                return Err(McpError::AuthDiscovery);
            }
            let document = bounded_json(response).await?;
            if required_string(&document, "issuer", 2048)? != selected_issuer {
                return Err(McpError::AuthSecurity);
            }
            let methods =
                required_string_list(&document, "code_challenge_methods_supported", 16, 32)?;
            if !methods.iter().any(|method| method == "S256") {
                return Err(McpError::AuthSecurity);
            }
            let authorization_endpoint = secure_url(
                required_string(&document, "authorization_endpoint", 2048)?,
                self.allow_loopback_http,
            )?;
            let token_endpoint = secure_url(
                required_string(&document, "token_endpoint", 2048)?,
                self.allow_loopback_http,
            )?;
            reject_reserved_query(
                &authorization_endpoint,
                &[
                    "response_type",
                    "client_id",
                    "redirect_uri",
                    "state",
                    "code_challenge",
                    "code_challenge_method",
                    "resource",
                    "scope",
                ],
            )?;
            reject_reserved_query(
                &token_endpoint,
                &[
                    "grant_type",
                    "code",
                    "refresh_token",
                    "client_id",
                    "redirect_uri",
                    "code_verifier",
                    "resource",
                ],
            )?;
            let registration_endpoint = document
                .get("registration_endpoint")
                .map(|value| {
                    value
                        .as_str()
                        .ok_or(McpError::AuthDiscovery)
                        .and_then(|raw| secure_url(raw, self.allow_loopback_http))
                })
                .transpose()?;
            return Ok(AuthorizationServer {
                endpoint: self.endpoint.clone(),
                metadata_url: self.metadata_url.clone(),
                resource: self.resource.clone(),
                issuer: selected_issuer.to_owned(),
                authorization_endpoint,
                token_endpoint,
                registration_endpoint,
                cimd_advertised: optional_bool(&document, "client_id_metadata_document_supported")?,
                iss_required: optional_bool(
                    &document,
                    "authorization_response_iss_parameter_supported",
                )?,
                scopes: self.requested_scopes.clone(),
            });
        }
        Err(McpError::AuthDiscovery)
    }
}

/// Issuer-validated OAuth endpoints and registration capabilities.
#[derive(Clone)]
pub struct AuthorizationServer {
    endpoint: Url,
    metadata_url: Url,
    resource: String,
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
    registration_endpoint: Option<Url>,
    cimd_advertised: bool,
    iss_required: bool,
    scopes: Vec<String>,
}

impl AuthorizationServer {
    /// Exact issuer identity used to bind all client registrations and tokens.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// DCR is a deprecated compatibility route and requires UI confirmation.
    pub fn supports_dcr(&self) -> bool {
        self.registration_endpoint.is_some()
    }

    /// Validated DCR URL for display before distinct user confirmation.
    pub fn registration_endpoint(&self) -> Option<&str> {
        self.registration_endpoint.as_ref().map(Url::as_str)
    }

    /// Whether the AS advertises CIMD, which Vega cannot yet originate.
    pub fn advertises_cimd(&self) -> bool {
        self.cimd_advertised
    }

    /// Bind a user-supplied pre-registered public client ID to this exact AS.
    pub fn pre_registered(
        &self,
        client_id: &str,
        registered_issuer: &str,
        redirect_uri: &str,
    ) -> Result<OAuthClient, McpError> {
        if registered_issuer != self.issuer || !valid_client_id(client_id) {
            return Err(McpError::AuthSecurity);
        }
        let redirect_uri = validate_redirect(redirect_uri)?;
        Ok(OAuthClient {
            server: self.clone(),
            client_id: client_id.to_owned(),
            redirect_uri,
        })
    }

    /// Register a native public client only after a distinct UI confirmation.
    /// A missing registration endpoint never falls back to a fabricated CIMD.
    pub async fn register_dcr(
        &self,
        redirect_uri: &str,
        explicit_user_confirmation: bool,
    ) -> Result<OAuthClient, McpError> {
        if !explicit_user_confirmation {
            return Err(McpError::ConsentRequired);
        }
        let registration_endpoint = match &self.registration_endpoint {
            Some(endpoint) => endpoint,
            None if self.cimd_advertised => return Err(McpError::CimdUnavailable),
            None => return Err(McpError::Registration),
        };
        let redirect_uri = validate_redirect(redirect_uri)?;
        let request = json!({
            "client_name":"Vega",
            "redirect_uris":[redirect_uri.as_str()],
            "grant_types":["authorization_code", "refresh_token"],
            "response_types":["code"],
            "token_endpoint_auth_method":"none",
            "application_type":"native"
        });
        // The discovery runtime may already be gone when Settings confirms
        // DCR. Never reuse its pooled client/reactor across UI operations.
        let response = auth_client()?
            .post(registration_endpoint.clone())
            .json(&request)
            .timeout(AUTH_TIMEOUT)
            .send()
            .await
            .map_err(|_| McpError::Registration)?;
        if !matches!(response.status(), StatusCode::OK | StatusCode::CREATED) {
            return Err(McpError::Registration);
        }
        let document = bounded_json(response)
            .await
            .map_err(|_| McpError::Registration)?;
        let client_id =
            required_string(&document, "client_id", 2048).map_err(|_| McpError::Registration)?;
        if !valid_client_id(client_id)
            || document.get("client_secret").is_some()
            || document
                .get("token_endpoint_auth_method")
                .is_some_and(|method| method.as_str() != Some("none"))
        {
            return Err(McpError::Registration);
        }
        if let Some(redirects) = document.get("redirect_uris") {
            let returned = redirects.as_array().ok_or(McpError::Registration)?;
            if !returned
                .iter()
                .any(|value| value.as_str() == Some(redirect_uri.as_str()))
            {
                return Err(McpError::Registration);
            }
        }
        Ok(OAuthClient {
            server: self.clone(),
            client_id: client_id.to_owned(),
            redirect_uri,
        })
    }
}

/// Public OAuth client bound to one issuer, resource, endpoint and redirect.
pub struct OAuthClient {
    server: AuthorizationServer,
    client_id: String,
    redirect_uri: Url,
}

impl OAuthClient {
    /// Public registration identity, never a client secret.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn server_issuer(&self) -> &str {
        self.server.issuer()
    }

    /// Restore only from an owner-only secret slot after a fresh protected-
    /// resource and issuer discovery. The envelope is untrusted until every
    /// identity component and scope has been revalidated.
    pub fn restore_owner_only_tokens(
        &self,
        envelope: &str,
        current_resource: &ResourceAuthorization,
        server_id: &str,
        revision: u64,
    ) -> Result<OAuthTokens, McpError> {
        if envelope.len() > 32 * 1024 || !canonical_server_id(server_id) || revision == 0 {
            return Err(McpError::CredentialBinding);
        }
        let value: Value =
            serde_json::from_str(envelope).map_err(|_| McpError::CredentialBinding)?;
        let object = value.as_object().ok_or(McpError::CredentialBinding)?;
        if object.len() != 13
            || object.get("version").and_then(Value::as_u64) != Some(1)
            || object.get("server_id").and_then(Value::as_str) != Some(server_id)
            || object.get("revision").and_then(Value::as_u64) != Some(revision)
            || object.get("endpoint").and_then(Value::as_str) != Some(self.server.endpoint.as_str())
            || object.get("metadata_url").and_then(Value::as_str)
                != Some(self.server.metadata_url.as_str())
            || object.get("resource").and_then(Value::as_str) != Some(self.server.resource.as_str())
            || object.get("issuer").and_then(Value::as_str) != Some(self.server.issuer.as_str())
            || object.get("client_id").and_then(Value::as_str) != Some(self.client_id.as_str())
            || current_resource.endpoint != self.server.endpoint
            || current_resource.metadata_url != self.server.metadata_url
            || current_resource.resource != self.server.resource
            || !current_resource
                .authorization_servers
                .iter()
                .any(|issuer| issuer == &self.server.issuer)
        {
            return Err(McpError::CredentialBinding);
        }
        let access_token = required_string(&value, "access_token", MAX_TOKEN_BYTES)
            .map_err(|_| McpError::CredentialBinding)?;
        validate_token(access_token).map_err(|_| McpError::CredentialBinding)?;
        let refresh_token = match object.get("refresh_token") {
            Some(Value::String(token)) => {
                validate_token(token).map_err(|_| McpError::CredentialBinding)?;
                Some(token.clone())
            }
            Some(Value::Null) => None,
            _ => return Err(McpError::CredentialBinding),
        };
        let scopes = envelope_scopes(&value, "scopes")?;
        let requested_scopes = envelope_scopes(&value, "requested_scopes")?;
        if scopes.iter().any(|scope| !requested_scopes.contains(scope)) {
            return Err(McpError::ScopeEscalation);
        }
        let expires_at = match object.get("expires_at_unix") {
            Some(Value::Null) => None,
            Some(value) => {
                let expiry = value.as_u64().ok_or(McpError::CredentialBinding)?;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| McpError::CredentialBinding)?
                    .as_secs();
                // Expired access tokens may still carry a valid refresh
                // token. Rehydrate them as expired; bearer_credential() will
                // reject until OAuthClient::refresh() succeeds.
                let remaining = expiry.saturating_sub(now);
                Some(
                    Instant::now()
                        .checked_add(Duration::from_secs(remaining))
                        .ok_or(McpError::CredentialBinding)?,
                )
            }
            None => return Err(McpError::CredentialBinding),
        };
        Ok(OAuthTokens {
            endpoint: self.server.endpoint.clone(),
            metadata_url: self.server.metadata_url.clone(),
            resource: self.server.resource.clone(),
            issuer: self.server.issuer.clone(),
            client_id: self.client_id.clone(),
            access_token: access_token.to_owned(),
            refresh_token,
            expires_at,
            scopes,
            requested_scopes,
        })
    }

    /// Create a one-shot authorization URL with fresh OS-random state and
    /// PKCE S256. The caller opens this URL only after showing the AS/scope UI.
    pub fn begin_authorization(&self) -> Result<AuthorizationRequest, McpError> {
        self.begin_with_scopes(&self.server.scopes)
    }

    /// Start an explicit step-up round with the union of previous and newly
    /// challenged scopes. No request is retried by this method.
    pub fn begin_step_up(
        &self,
        previous_scopes: &[String],
        challenge_scopes: &[String],
        explicit_user_confirmation: bool,
    ) -> Result<AuthorizationRequest, McpError> {
        if !explicit_user_confirmation {
            return Err(McpError::ConsentRequired);
        }
        let mut scopes = previous_scopes.to_vec();
        for scope in challenge_scopes {
            if !scopes.contains(scope) {
                scopes.push(scope.clone());
            }
        }
        validate_scopes(&scopes)?;
        self.begin_with_scopes(&scopes)
    }

    /// Begin a new, consented authorization only for a challenge proven to
    /// belong to this OAuth client, endpoint and discovered resource.
    pub fn begin_verified_step_up(
        &self,
        tokens: &OAuthTokens,
        challenge: &ScopeChallenge,
        explicit_user_confirmation: bool,
    ) -> Result<AuthorizationRequest, McpError> {
        let scopes = self.verified_step_up_scopes(tokens, challenge)?;
        if !explicit_user_confirmation {
            return Err(McpError::ConsentRequired);
        }
        self.begin_with_scopes(&scopes)
    }

    /// Validate the exact credential/challenge binding and return the bounded
    /// union for a Settings preview, without minting state or opening a URL.
    pub fn verified_step_up_scopes(
        &self,
        tokens: &OAuthTokens,
        challenge: &ScopeChallenge,
    ) -> Result<Vec<String>, McpError> {
        if tokens.endpoint != self.server.endpoint
            || tokens.resource != self.server.resource
            || tokens.issuer != self.server.issuer
            || tokens.client_id != self.client_id
            || tokens.metadata_url != self.server.metadata_url
            || challenge.endpoint != tokens.endpoint
            || challenge.resource != tokens.resource
            || challenge.issuer != tokens.issuer
            || challenge.client_id != tokens.client_id
            || challenge.metadata_url != tokens.metadata_url
        {
            return Err(McpError::CredentialBinding);
        }
        let mut scopes = tokens.requested_scopes.clone();
        for scope in &challenge.scopes {
            if !scopes.contains(scope) {
                scopes.push(scope.clone());
            }
        }
        validate_scopes(&scopes)?;
        Ok(scopes)
    }

    fn begin_with_scopes(&self, scopes: &[String]) -> Result<AuthorizationRequest, McpError> {
        validate_scopes(scopes)?;
        let state = random_urlsafe(32)?;
        let verifier = random_urlsafe(32)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorization_url = self.server.authorization_endpoint.clone();
        {
            let mut query = authorization_url.query_pairs_mut();
            query.append_pair("response_type", "code");
            query.append_pair("client_id", &self.client_id);
            query.append_pair("redirect_uri", self.redirect_uri.as_str());
            query.append_pair("state", &state);
            query.append_pair("code_challenge", &challenge);
            query.append_pair("code_challenge_method", "S256");
            query.append_pair("resource", &self.server.resource);
            if !scopes.is_empty() {
                query.append_pair("scope", &scopes.join(" "));
            }
        }
        Ok(AuthorizationRequest {
            authorization_url,
            state,
            verifier,
            issuer: self.server.issuer.clone(),
            resource: self.server.resource.clone(),
            endpoint: self.server.endpoint.clone(),
            client_id: self.client_id.clone(),
            redirect_uri: self.redirect_uri.clone(),
            scopes: scopes.to_vec(),
        })
    }

    /// Validate the exact redirect, state and issuer before sending the code
    /// to the prevalidated token endpoint. The pending request is consumed.
    pub async fn finish_authorization(
        &self,
        pending: AuthorizationRequest,
        callback_url: &str,
    ) -> Result<OAuthTokens, McpError> {
        if pending.issuer != self.server.issuer
            || pending.resource != self.server.resource
            || pending.endpoint != self.server.endpoint
            || pending.client_id != self.client_id
            || pending.redirect_uri != self.redirect_uri
        {
            return Err(McpError::AuthSecurity);
        }
        let callback = Url::parse(callback_url).map_err(|_| McpError::AuthSecurity)?;
        if callback.scheme() != self.redirect_uri.scheme()
            || callback.host_str() != self.redirect_uri.host_str()
            || callback.port() != self.redirect_uri.port()
            || callback.path() != self.redirect_uri.path()
            || callback.fragment().is_some()
        {
            return Err(McpError::AuthSecurity);
        }
        let values = callback_fields(&callback)?;
        if values.error.is_some()
            || !constant_time_equal(values.state.as_deref().unwrap_or(""), &pending.state)
        {
            return Err(McpError::AuthSecurity);
        }
        match values.issuer {
            Some(issuer) if issuer != pending.issuer => return Err(McpError::AuthSecurity),
            None if self.server.iss_required => return Err(McpError::AuthSecurity),
            _ => {}
        }
        let code = values.code.ok_or(McpError::AuthSecurity)?;
        if code.is_empty() || code.len() > 4096 {
            return Err(McpError::AuthSecurity);
        }
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code_verifier", pending.verifier.as_str()),
            ("resource", self.server.resource.as_str()),
        ];
        let document = self.token_request(&form).await?;
        parse_tokens(
            document,
            &self.server,
            &self.client_id,
            None,
            &pending.scopes,
            &pending.scopes,
        )
    }

    /// Refresh only with a token originally issued to this exact binding.
    /// The returned token replaces the old one; failure never falls back to
    /// anonymous requests or a different issuer.
    pub async fn refresh(&self, tokens: &OAuthTokens) -> Result<OAuthTokens, McpError> {
        if tokens.endpoint != self.server.endpoint
            || tokens.resource != self.server.resource
            || tokens.issuer != self.server.issuer
            || tokens.client_id != self.client_id
        {
            return Err(McpError::CredentialBinding);
        }
        let refresh = tokens
            .refresh_token
            .as_deref()
            .ok_or(McpError::AuthRequired)?;
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", self.client_id.as_str()),
            ("resource", self.server.resource.as_str()),
        ];
        let document = self.token_request(&form).await?;
        parse_tokens(
            document,
            &self.server,
            &self.client_id,
            Some(refresh),
            &tokens.requested_scopes,
            &tokens.scopes,
        )
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<Value, McpError> {
        // Code exchange and refresh can run on different short-lived Tokio
        // runtimes from metadata discovery and from each other. A fresh
        // policy-constrained client owns only the current runtime's sockets.
        let response = auth_client()?
            .post(self.server.token_endpoint.clone())
            .form(form)
            .timeout(AUTH_TIMEOUT)
            .send()
            .await
            .map_err(|_| McpError::AuthRequired)?;
        if response.status() != StatusCode::OK {
            return Err(McpError::AuthRequired);
        }
        bounded_json(response)
            .await
            .map_err(|_| McpError::AuthRequired)
    }
}

/// One-shot OAuth request. Contains a secret PKCE verifier and must not be
/// serialized, logged, cloned or retained after callback handling.
pub struct AuthorizationRequest {
    authorization_url: Url,
    state: String,
    verifier: String,
    issuer: String,
    resource: String,
    endpoint: Url,
    client_id: String,
    redirect_uri: Url,
    scopes: Vec<String>,
}

impl AuthorizationRequest {
    /// URL to open in the system browser after the visible authorization step.
    pub fn authorization_url(&self) -> &str {
        self.authorization_url.as_str()
    }
}

/// In-memory OAuth token set. Persistence belongs to owner-only Settings code.
/// This type deliberately has no Debug or serialization implementation.
pub struct OAuthTokens {
    endpoint: Url,
    metadata_url: Url,
    resource: String,
    issuer: String,
    client_id: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<Instant>,
    scopes: Vec<String>,
    requested_scopes: Vec<String>,
}

impl OAuthTokens {
    /// Sensitive transfer format for Vega's owner-only keystore only. Never
    /// place this string in SQLite, UI state, logs or conversation history.
    pub fn to_owner_only_envelope(
        &self,
        server_id: &str,
        revision: u64,
    ) -> Result<String, McpError> {
        if !canonical_server_id(server_id) || revision == 0 {
            return Err(McpError::CredentialBinding);
        }
        validate_token(&self.access_token)?;
        if let Some(refresh) = &self.refresh_token {
            validate_token(refresh)?;
        }
        validate_scopes(&self.scopes)?;
        validate_scopes(&self.requested_scopes)?;
        if self
            .scopes
            .iter()
            .any(|scope| !self.requested_scopes.contains(scope))
        {
            return Err(McpError::ScopeEscalation);
        }
        let expires_at_unix = match self.expires_at {
            Some(expiry) => {
                let remaining = expiry.saturating_duration_since(Instant::now()).as_secs();
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| McpError::CredentialBinding)?;
                Some(
                    now.as_secs()
                        .checked_add(remaining)
                        .ok_or(McpError::CredentialBinding)?,
                )
            }
            None => None,
        };
        let envelope = json!({
            "version": 1,
            "server_id": server_id,
            "revision": revision,
            "endpoint": self.endpoint.as_str(),
            "metadata_url": self.metadata_url.as_str(),
            "resource": self.resource,
            "issuer": self.issuer,
            "client_id": self.client_id,
            "access_token": self.access_token,
            "refresh_token": self.refresh_token,
            "expires_at_unix": expires_at_unix,
            "scopes": self.scopes,
            "requested_scopes": self.requested_scopes,
        });
        let serialized =
            serde_json::to_string(&envelope).map_err(|_| McpError::CredentialBinding)?;
        if serialized.len() > 32 * 1024 {
            return Err(McpError::CredentialBinding);
        }
        Ok(serialized)
    }

    /// Create a transport credential only after comparing freshly discovered
    /// resource metadata. A changed issuer/resource cannot reuse this token.
    pub fn bearer_credential(
        &self,
        current_resource: &ResourceAuthorization,
    ) -> Result<BearerCredential, McpError> {
        if self.is_expired() {
            return Err(McpError::AuthRequired);
        }
        if self.endpoint != current_resource.endpoint
            || self.metadata_url != current_resource.metadata_url
            || self.resource != current_resource.resource
            || !current_resource
                .authorization_servers
                .iter()
                .any(|issuer| issuer == &self.issuer)
        {
            return Err(McpError::CredentialBinding);
        }
        Ok(BearerCredential {
            endpoint: self.endpoint.clone(),
            secret: self.access_token.clone(),
            expires_at: self.expires_at,
            oauth: Some(OAuthBinding {
                metadata_url: self.metadata_url.clone(),
                resource: self.resource.clone(),
                issuer: self.issuer.clone(),
                client_id: self.client_id.clone(),
            }),
        })
    }

    /// Whether the token must be refreshed before another server request.
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expiry| Instant::now() >= expiry)
    }

    /// Granted scopes reported by the AS, or the requested set if omitted.
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// Scopes Vega explicitly requested in the previous authorization round.
    /// This is display metadata only; access/refresh token values stay private.
    pub fn requested_scopes(&self) -> &[String] {
        &self.requested_scopes
    }
}

/// In-memory explicit bearer value. The endpoint binding is checked on every
/// HTTP request and redirects are disabled. Never derive Debug/Serialize.
pub struct BearerCredential {
    endpoint: Url,
    secret: String,
    expires_at: Option<Instant>,
    oauth: Option<OAuthBinding>,
}

struct OAuthBinding {
    metadata_url: Url,
    resource: String,
    issuer: String,
    client_id: String,
}

/// Bounded 403 scope step-up requirement bound to the original OAuth identity.
/// Debug intentionally omits untrusted scope strings and resource paths.
pub struct ScopeChallenge {
    endpoint: Url,
    metadata_url: Url,
    resource: String,
    issuer: String,
    client_id: String,
    scopes: Vec<String>,
}

impl std::fmt::Debug for ScopeChallenge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ScopeChallenge(..)")
    }
}

impl ScopeChallenge {
    /// Validated new scope tokens to show in the explicit consent UI.
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

impl BearerCredential {
    /// Bind an explicitly entered non-OAuth bearer to exactly one URL.
    pub fn manual(
        endpoint: &str,
        allow_loopback_http: bool,
        secret: String,
    ) -> Result<Self, McpError> {
        let endpoint = Url::parse(endpoint).map_err(|_| McpError::InvalidConfig)?;
        validate_endpoint(&endpoint, allow_loopback_http)?;
        validate_token(&secret)?;
        Ok(Self {
            endpoint,
            secret,
            expires_at: None,
            oauth: None,
        })
    }

    pub(crate) fn bound_to(&self, endpoint: &Url) -> bool {
        &self.endpoint == endpoint
    }

    pub(crate) fn secret(&self) -> &str {
        &self.secret
    }

    pub(crate) fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expiry| Instant::now() >= expiry)
    }

    pub(crate) fn scope_challenge(
        &self,
        endpoint: &Url,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<Option<ScopeChallenge>, McpError> {
        let Some(binding) = &self.oauth else {
            return Ok(None);
        };
        if &self.endpoint != endpoint {
            return Err(McpError::CredentialBinding);
        }
        let mut parsed = None;
        for raw in headers.get_all(WWW_AUTHENTICATE) {
            let raw = raw.to_str().map_err(|_| McpError::AuthSecurity)?;
            if let Some(challenge) = parse_bearer_challenge(raw)?
                && parsed.replace(challenge).is_some()
            {
                return Err(McpError::AuthSecurity);
            }
        }
        let Some(challenge) = parsed else {
            return Ok(None);
        };
        if challenge.error.as_deref() != Some("insufficient_scope") {
            return Ok(None);
        }
        if challenge.scopes.is_empty() {
            return Err(McpError::AuthSecurity);
        }
        if let Some(metadata) = challenge.resource_metadata {
            let metadata = secure_url(&metadata, endpoint.scheme() == "http")?;
            if metadata != binding.metadata_url {
                return Err(McpError::AuthSecurity);
            }
        }
        Ok(Some(ScopeChallenge {
            endpoint: endpoint.clone(),
            metadata_url: binding.metadata_url.clone(),
            resource: binding.resource.clone(),
            issuer: binding.issuer.clone(),
            client_id: binding.client_id.clone(),
            scopes: challenge.scopes,
        }))
    }
}

struct BearerChallenge {
    resource_metadata: Option<String>,
    scopes: Vec<String>,
    error: Option<String>,
}

fn parse_bearer_challenge(raw: &str) -> Result<Option<BearerChallenge>, McpError> {
    let raw = raw.trim();
    if raw.len() > 8192 {
        return Err(McpError::LimitExceeded);
    }
    let entries = split_auth_parameters(raw)?;
    let mut resource_metadata = None;
    let mut scopes = Vec::new();
    let mut error = None;
    let mut seen_scope = false;
    let mut in_bearer = false;
    for entry in entries {
        let entry = entry.trim();
        let entry = if !in_bearer {
            let Some(prefix) = entry.get(..6) else {
                continue;
            };
            if !prefix.eq_ignore_ascii_case("bearer") {
                continue;
            }
            let rest = entry.get(6..).ok_or(McpError::AuthSecurity)?;
            if !rest.starts_with(char::is_whitespace) {
                return Err(McpError::AuthSecurity);
            }
            in_bearer = true;
            rest.trim()
        } else {
            entry
        };
        if entry.is_empty() {
            continue;
        }
        let (key, value) = entry.split_once('=').ok_or(McpError::AuthSecurity)?;
        if key.trim().chars().any(char::is_whitespace) {
            break;
        }
        let value = value.trim();
        let value = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            &value[1..value.len() - 1]
        } else if !value.contains('"') {
            value
        } else {
            return Err(McpError::AuthSecurity);
        };
        if value.contains('"') || value.contains('\\') || value.chars().any(char::is_control) {
            return Err(McpError::AuthSecurity);
        }
        match key.trim() {
            "resource_metadata" => {
                if resource_metadata.replace(value.to_owned()).is_some() {
                    return Err(McpError::AuthSecurity);
                }
            }
            "scope" => {
                if seen_scope {
                    return Err(McpError::AuthSecurity);
                }
                seen_scope = true;
                if value.is_empty() {
                    return Err(McpError::AuthSecurity);
                }
                scopes = value.split_whitespace().map(str::to_owned).collect();
                validate_scopes(&scopes)?;
            }
            "error" if error.is_some() || value.is_empty() => return Err(McpError::AuthSecurity),
            "error" => error = Some(value.to_owned()),
            _ => {}
        }
    }
    Ok(in_bearer.then_some(BearerChallenge {
        resource_metadata,
        scopes,
        error,
    }))
}

fn split_auth_parameters(raw: &str) -> Result<Vec<&str>, McpError> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, byte) in raw.bytes().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            b',' if !quoted => {
                let part = raw.get(start..index).ok_or(McpError::AuthSecurity)?;
                if !part.trim().is_empty() {
                    parts.push(part);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted {
        return Err(McpError::AuthSecurity);
    }
    let tail = raw.get(start..).ok_or(McpError::AuthSecurity)?;
    if !tail.trim().is_empty() {
        parts.push(tail);
    }
    if parts.len() > 32 {
        return Err(McpError::LimitExceeded);
    }
    Ok(parts)
}

fn auth_client() -> Result<Client, McpError> {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(AUTH_TIMEOUT)
        .build()
        .map_err(|_| McpError::Transport)
}

fn secure_url(raw: &str, allow_loopback_http: bool) -> Result<Url, McpError> {
    let url = Url::parse(raw).map_err(|_| McpError::AuthSecurity)?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(McpError::AuthSecurity);
    }
    match url.scheme() {
        "https" => Ok(url),
        "http"
            if allow_loopback_http
                && matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1")) =>
        {
            Ok(url)
        }
        _ => Err(McpError::AuthSecurity),
    }
}

fn validate_redirect(raw: &str) -> Result<Url, McpError> {
    let url = secure_url(raw, true)?;
    if url.query().is_some() || url.path().is_empty() {
        return Err(McpError::AuthSecurity);
    }
    Ok(url)
}

fn reject_reserved_query(url: &Url, reserved: &[&str]) -> Result<(), McpError> {
    if url
        .query_pairs()
        .any(|(key, _)| reserved.iter().any(|name| *name == key))
    {
        return Err(McpError::AuthSecurity);
    }
    Ok(())
}

fn resource_path_covers_endpoint(resource_path: &str, endpoint_path: &str) -> bool {
    resource_path == "/"
        || resource_path == endpoint_path
        || endpoint_path
            .strip_prefix(resource_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn resource_metadata_candidates(endpoint: &Url) -> Result<Vec<Url>, McpError> {
    let origin = endpoint.origin().ascii_serialization();
    let path = endpoint.path().trim_start_matches('/');
    let mut candidates = Vec::new();
    if !path.is_empty() {
        candidates.push(
            Url::parse(&format!(
                "{origin}/.well-known/oauth-protected-resource/{path}"
            ))
            .map_err(|_| McpError::AuthDiscovery)?,
        );
    }
    candidates.push(
        Url::parse(&format!("{origin}/.well-known/oauth-protected-resource"))
            .map_err(|_| McpError::AuthDiscovery)?,
    );
    Ok(candidates)
}

fn authorization_metadata_candidates(issuer: &Url) -> Result<Vec<Url>, McpError> {
    let origin = issuer.origin().ascii_serialization();
    let path = issuer.path().trim_matches('/');
    let suffix = if path.is_empty() {
        String::new()
    } else {
        format!("/{path}")
    };
    let mut candidates = vec![
        Url::parse(&format!(
            "{origin}/.well-known/oauth-authorization-server{suffix}"
        ))
        .map_err(|_| McpError::AuthDiscovery)?,
        Url::parse(&format!(
            "{origin}/.well-known/openid-configuration{suffix}"
        ))
        .map_err(|_| McpError::AuthDiscovery)?,
    ];
    if !path.is_empty() {
        candidates.push(
            Url::parse(&format!("{origin}/{path}/.well-known/openid-configuration"))
                .map_err(|_| McpError::AuthDiscovery)?,
        );
    }
    Ok(candidates)
}

async fn bounded_json(response: Response) -> Result<Value, McpError> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !content_type.starts_with("application/json") {
        return Err(McpError::AuthDiscovery);
    }
    let bytes = timeout(AUTH_TIMEOUT, async {
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = timeout(AUTH_IDLE_TIMEOUT, stream.next())
            .await
            .map_err(|_| McpError::Timeout)?
        {
            let chunk = chunk.map_err(|_| McpError::Transport)?;
            if bytes.len().saturating_add(chunk.len()) > AUTH_BODY_LIMIT {
                return Err(McpError::LimitExceeded);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok::<_, McpError>(bytes)
    })
    .await
    .map_err(|_| McpError::Timeout)??;
    serde_json::from_slice(&bytes).map_err(|_| McpError::AuthDiscovery)
}

fn required_string<'a>(
    document: &'a Value,
    key: &str,
    max_len: usize,
) -> Result<&'a str, McpError> {
    let value = document
        .get(key)
        .and_then(Value::as_str)
        .ok_or(McpError::AuthDiscovery)?;
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(McpError::AuthDiscovery);
    }
    Ok(value)
}

fn required_string_list(
    document: &Value,
    key: &str,
    max_count: usize,
    max_len: usize,
) -> Result<Vec<String>, McpError> {
    let values = optional_string_list(document, key, max_count, max_len)?;
    if values.is_empty() {
        return Err(McpError::AuthDiscovery);
    }
    Ok(values)
}

fn optional_string_list(
    document: &Value,
    key: &str,
    max_count: usize,
    max_len: usize,
) -> Result<Vec<String>, McpError> {
    let Some(raw) = document.get(key) else {
        return Ok(Vec::new());
    };
    let values = raw.as_array().ok_or(McpError::AuthDiscovery)?;
    if values.len() > max_count {
        return Err(McpError::LimitExceeded);
    }
    let mut output = Vec::with_capacity(values.len());
    for value in values {
        let text = value.as_str().ok_or(McpError::AuthDiscovery)?;
        if text.is_empty() || text.len() > max_len || text.chars().any(char::is_control) {
            return Err(McpError::AuthDiscovery);
        }
        output.push(text.to_owned());
    }
    Ok(output)
}

fn optional_bool(document: &Value, key: &str) -> Result<bool, McpError> {
    match document.get(key) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(McpError::AuthDiscovery),
    }
}

fn validate_scopes(scopes: &[String]) -> Result<(), McpError> {
    if scopes.len() > MAX_SCOPES
        || scopes.iter().any(|scope| {
            scope.is_empty()
                || scope.len() > 128
                || !scope
                    .bytes()
                    .all(|byte| matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e))
        })
    {
        return Err(McpError::AuthSecurity);
    }
    Ok(())
}

fn envelope_scopes(document: &Value, key: &str) -> Result<Vec<String>, McpError> {
    let values = document
        .get(key)
        .and_then(Value::as_array)
        .ok_or(McpError::CredentialBinding)?;
    let scopes = values
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .ok_or(McpError::CredentialBinding)?;
    validate_scopes(&scopes).map_err(|_| McpError::CredentialBinding)?;
    let unique: std::collections::HashSet<_> = scopes.iter().collect();
    if unique.len() != scopes.len() {
        return Err(McpError::CredentialBinding);
    }
    Ok(scopes)
}

fn canonical_server_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 26
        && bytes
            .first()
            .is_some_and(|byte| (b'0'..=b'7').contains(byte)) && bytes.iter().all(|byte| {
        byte.is_ascii_digit()
            || matches!(byte, b'A'..=b'H' | b'J'..=b'K' | b'M'..=b'N' | b'P'..=b'T' | b'V'..=b'Z')
    })
}

fn valid_client_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 2048
        && value.is_ascii()
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
}

fn validate_token(token: &str) -> Result<(), McpError> {
    if token.is_empty()
        || token.len() > MAX_TOKEN_BYTES
        || !token.is_ascii()
        || token.bytes().any(|byte| !(0x21..=0x7e).contains(&byte))
    {
        return Err(McpError::AuthSecurity);
    }
    Ok(())
}

fn random_urlsafe(length: usize) -> Result<String, McpError> {
    let mut bytes = vec![0u8; length];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|_| McpError::AuthSecurity)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

struct CallbackFields {
    code: Option<String>,
    state: Option<String>,
    issuer: Option<String>,
    error: Option<String>,
}

fn callback_fields(url: &Url) -> Result<CallbackFields, McpError> {
    let mut fields = CallbackFields {
        code: None,
        state: None,
        issuer: None,
        error: None,
    };
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "code" => Some(&mut fields.code),
            "state" => Some(&mut fields.state),
            "iss" => Some(&mut fields.issuer),
            "error" => Some(&mut fields.error),
            _ => None,
        };
        if let Some(slot) = slot
            && slot.replace(value.into_owned()).is_some()
        {
            return Err(McpError::AuthSecurity);
        }
    }
    Ok(fields)
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let difference = left
        .bytes()
        .zip(right.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b));
    difference == 0
}

fn parse_tokens(
    document: Value,
    server: &AuthorizationServer,
    client_id: &str,
    prior_refresh: Option<&str>,
    requested_scopes: &[String],
    allowed_scopes: &[String],
) -> Result<OAuthTokens, McpError> {
    let access_token = required_string(&document, "access_token", MAX_TOKEN_BYTES)?;
    validate_token(access_token)?;
    let token_type = required_string(&document, "token_type", 32)?;
    if !token_type.eq_ignore_ascii_case("Bearer") {
        return Err(McpError::AuthSecurity);
    }
    let refresh_token = document
        .get("refresh_token")
        .map(|value| value.as_str().ok_or(McpError::AuthSecurity))
        .transpose()?
        .or(prior_refresh)
        .map(str::to_owned);
    if let Some(refresh) = &refresh_token {
        validate_token(refresh)?;
    }
    let expires_at = document
        .get("expires_in")
        .map(|value| {
            let seconds = value.as_u64().ok_or(McpError::AuthSecurity)?;
            Instant::now()
                .checked_add(Duration::from_secs(seconds))
                .ok_or(McpError::AuthSecurity)
        })
        .transpose()?;
    let scopes = if let Some(scope) = document.get("scope") {
        let raw = scope.as_str().ok_or(McpError::AuthSecurity)?;
        let scopes: Vec<String> = raw.split_whitespace().map(str::to_owned).collect();
        if scopes.is_empty() {
            return Err(McpError::AuthSecurity);
        }
        validate_scopes(&scopes)?;
        scopes
    } else {
        allowed_scopes.to_vec()
    };
    if scopes.iter().any(|scope| !allowed_scopes.contains(scope)) {
        return Err(McpError::ScopeEscalation);
    }
    Ok(OAuthTokens {
        endpoint: server.endpoint.clone(),
        metadata_url: server.metadata_url.clone(),
        resource: server.resource.clone(),
        issuer: server.issuer.clone(),
        client_id: client_id.to_owned(),
        access_token: access_token.to_owned(),
        refresh_token,
        expires_at,
        scopes,
        requested_scopes: requested_scopes.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_owner_only_envelope_rejects_cross_server_revision_issuer_and_scope_widening() {
        let endpoint = Url::parse("https://mcp.example/mcp").expect("endpoint");
        let metadata_url =
            Url::parse("https://mcp.example/.well-known/oauth-protected-resource/mcp")
                .expect("metadata");
        let resource = ResourceAuthorization {
            client: auth_client().expect("client"),
            endpoint: endpoint.clone(),
            metadata_url: metadata_url.clone(),
            resource: endpoint.to_string(),
            authorization_servers: vec!["https://issuer.example".into()],
            requested_scopes: vec!["read".into()],
            allow_loopback_http: false,
        };
        let server = AuthorizationServer {
            endpoint: endpoint.clone(),
            metadata_url,
            resource: resource.resource.clone(),
            issuer: "https://issuer.example".into(),
            authorization_endpoint: Url::parse("https://issuer.example/authorize")
                .expect("authorization endpoint"),
            token_endpoint: Url::parse("https://issuer.example/token").expect("token endpoint"),
            registration_endpoint: None,
            cimd_advertised: false,
            iss_required: true,
            scopes: vec!["read".into()],
        };
        let client = server
            .pre_registered(
                "owned-client",
                server.issuer(),
                "http://127.0.0.1:48765/callback",
            )
            .expect("pre-registration");
        let tokens = OAuthTokens {
            endpoint,
            metadata_url: resource.metadata_url.clone(),
            resource: resource.resource.clone(),
            issuer: server.issuer().into(),
            client_id: client.client_id().into(),
            access_token: "secret-access".into(),
            refresh_token: Some("secret-refresh".into()),
            expires_at: None,
            scopes: vec!["read".into()],
            requested_scopes: vec!["read".into()],
        };
        let server_a = "01J00000000000000000000000";
        let server_b = "01J00000000000000000000001";
        let envelope = tokens
            .to_owner_only_envelope(server_a, 7)
            .expect("owner-only envelope");
        let restored = client
            .restore_owner_only_tokens(&envelope, &resource, server_a, 7)
            .expect("exact binding");
        assert!(restored.bearer_credential(&resource).is_ok());
        assert!(matches!(
            client.restore_owner_only_tokens(&envelope, &resource, server_b, 7),
            Err(McpError::CredentialBinding)
        ));
        assert!(matches!(
            client.restore_owner_only_tokens(&envelope, &resource, server_a, 8),
            Err(McpError::CredentialBinding)
        ));
        let mut widened: Value = serde_json::from_str(&envelope).expect("envelope JSON");
        widened["scopes"] = json!(["read", "write"]);
        assert!(matches!(
            client.restore_owner_only_tokens(&widened.to_string(), &resource, server_a, 7),
            Err(McpError::ScopeEscalation)
        ));
        let mut changed_issuer: Value = serde_json::from_str(&envelope).expect("envelope JSON");
        changed_issuer["issuer"] = json!("https://other.example");
        assert!(matches!(
            client.restore_owner_only_tokens(&changed_issuer.to_string(), &resource, server_a, 7),
            Err(McpError::CredentialBinding)
        ));
        for (field, replacement) in [
            ("metadata_url", json!("https://other.example/prm")),
            ("resource", json!("https://other.example/mcp")),
            ("client_id", json!("other-client")),
        ] {
            let mut changed: Value = serde_json::from_str(&envelope).expect("envelope JSON");
            changed[field] = replacement;
            assert!(matches!(
                client.restore_owner_only_tokens(&changed.to_string(), &resource, server_a, 7),
                Err(McpError::CredentialBinding)
            ));
        }
        let mut duplicated: Value = serde_json::from_str(&envelope).expect("envelope JSON");
        duplicated["scopes"] = json!(["read", "read"]);
        assert!(matches!(
            client.restore_owner_only_tokens(&duplicated.to_string(), &resource, server_a, 7),
            Err(McpError::CredentialBinding)
        ));
        let mut expired: Value = serde_json::from_str(&envelope).expect("envelope JSON");
        expired["expires_at_unix"] = json!(1);
        let expired = client
            .restore_owner_only_tokens(&expired.to_string(), &resource, server_a, 7)
            .expect("expired token may only be refreshed");
        assert!(expired.is_expired());
        assert!(matches!(
            expired.bearer_credential(&resource),
            Err(McpError::AuthRequired)
        ));
        assert!(matches!(
            client.restore_owner_only_tokens("{", &resource, server_a, 7),
            Err(McpError::CredentialBinding)
        ));
        assert!(matches!(
            client.restore_owner_only_tokens(&"x".repeat(32 * 1024 + 1), &resource, server_a, 7),
            Err(McpError::CredentialBinding)
        ));
    }

    #[test]
    fn bearer_challenge_parser_is_bounded_and_does_not_panic_on_unicode() {
        assert!(parse_bearer_challenge("💥invalid").is_ok());
        assert!(matches!(
            parse_bearer_challenge(
                "Bearer resource_metadata=\"http://a\", resource_metadata=\"http://b\""
            ),
            Err(McpError::AuthSecurity)
        ));
        let parsed = parse_bearer_challenge(
            "Bearer realm=\"owned, fixture\", resource_metadata=\"https://mcp.example/.well-known/oauth-protected-resource\", scope=\"tools:read\"",
        )
        .expect("valid syntax")
        .expect("Bearer");
        assert_eq!(parsed.scopes, ["tools:read"]);
        assert_eq!(
            parsed.resource_metadata.as_deref(),
            Some("https://mcp.example/.well-known/oauth-protected-resource")
        );
        let mixed = parse_bearer_challenge(
            "Basic realm=\"other\", Bearer resource_metadata=\"https://mcp.example/prm\", scope=\"tools:read\"",
        )
        .expect("mixed syntax")
        .expect("Bearer in mixed header");
        assert_eq!(
            mixed.resource_metadata.as_deref(),
            Some("https://mcp.example/prm")
        );
        assert!(matches!(
            parse_bearer_challenge(
                "Bearer error=\"insufficient_scope\", error=\"insufficient_scope\", scope=\"tools:write\""
            ),
            Err(McpError::AuthSecurity)
        ));
        assert!(matches!(
            parse_bearer_challenge(
                "Bearer error=\"insufficient_scope\", scope=\"tools:write\\bad\""
            ),
            Err(McpError::AuthSecurity)
        ));
    }

    #[test]
    fn resource_path_and_reserved_oauth_query_boundaries() {
        assert!(resource_path_covers_endpoint("/", "/mcp"));
        assert!(resource_path_covers_endpoint("/api", "/api/mcp"));
        assert!(!resource_path_covers_endpoint("/api", "/apiv2/mcp"));
        let endpoint = Url::parse("https://issuer.example/authorize?state=fixed").expect("URL");
        assert!(matches!(
            reject_reserved_query(&endpoint, &["state"]),
            Err(McpError::AuthSecurity)
        ));
    }

    #[test]
    fn changed_issuer_cannot_mint_transport_credential() {
        let endpoint = Url::parse("https://mcp.example/mcp").expect("endpoint");
        let resource = ResourceAuthorization {
            client: auth_client().expect("client"),
            endpoint: endpoint.clone(),
            metadata_url: Url::parse(
                "https://mcp.example/.well-known/oauth-protected-resource/mcp",
            )
            .expect("metadata"),
            resource: endpoint.to_string(),
            authorization_servers: vec!["https://new-issuer.example".into()],
            requested_scopes: Vec::new(),
            allow_loopback_http: false,
        };
        let tokens = OAuthTokens {
            endpoint,
            metadata_url: resource.metadata_url.clone(),
            resource: resource.resource.clone(),
            issuer: "https://old-issuer.example".into(),
            client_id: "old-client".into(),
            access_token: "old-secret".into(),
            refresh_token: None,
            expires_at: None,
            scopes: Vec::new(),
            requested_scopes: Vec::new(),
        };
        assert!(matches!(
            tokens.bearer_credential(&resource),
            Err(McpError::CredentialBinding)
        ));
    }

    #[test]
    fn malformed_as_capability_flags_fail_closed() {
        assert!(matches!(
            optional_bool(
                &json!({"authorization_response_iss_parameter_supported":"true"}),
                "authorization_response_iss_parameter_supported"
            ),
            Err(McpError::AuthDiscovery)
        ));
    }
}
