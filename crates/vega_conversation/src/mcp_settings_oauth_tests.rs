use super::*;
use crate::agent::{
    PermissionHook, PersistenceActorConfig, run_thread_task_with_images_reasoning_and_mcp,
};
use crate::types::{McpCallIdentity, PermissionDecision, PermissionRequest};
use futures::future::BoxFuture;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason, VegaError};

struct ApproveOwnedMcp;

impl PermissionHook for ApproveOwnedMcp {
    fn request(
        &self,
        _request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        Box::pin(async { Ok(PermissionDecision::Once) })
    }
}

#[derive(Default)]
struct FixtureCounts {
    token: AtomicUsize,
    registration: AtomicUsize,
    authenticated: AtomicUsize,
    block_registration: AtomicBool,
    registration_started: Notify,
    registration_release: Notify,
    force_step_up: AtomicBool,
    challenged_calls: AtomicUsize,
    resource_scope: Mutex<Option<String>>,
    challenge_scope: Mutex<Option<String>>,
}

struct FixtureRequest {
    path: String,
    auth: bool,
    body: Vec<u8>,
}

async fn owned_oauth_fixture() -> (String, String, Arc<FixtureCounts>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned fixture");
    let origin = format!(
        "http://{}",
        listener.local_addr().expect("listener address")
    );
    let endpoint = format!("{origin}/mcp");
    let issuer = format!("{origin}/issuer");
    let counts = Arc::new(FixtureCounts::default());
    let server_counts = counts.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let origin = origin.clone();
            let counts = server_counts.clone();
            tokio::spawn(async move {
                serve_one(stream, &origin, &counts).await;
            });
        }
    });
    (endpoint, issuer, counts)
}

async fn serve_one(mut stream: TcpStream, origin: &str, counts: &FixtureCounts) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    let endpoint = format!("{origin}/mcp");
    let issuer = format!("{origin}/issuer");
    if request.path == "/mcp"
        && request.auth
        && counts.force_step_up.load(Ordering::SeqCst)
        && serde_json::from_slice::<serde_json::Value>(&request.body)
            .ok()
            .and_then(|body| body["method"].as_str().map(str::to_owned))
            .as_deref()
            == Some("tools/call")
    {
        counts.challenged_calls.fetch_add(1, Ordering::SeqCst);
        let challenge_scope = counts
            .challenge_scope
            .lock()
            .expect("fixture scope")
            .clone()
            .unwrap_or_else(|| "tools:write".into());
        let response = format!(
            "HTTP/1.1 403 Forbidden\r\nWWW-Authenticate: Bearer error=\"insufficient_scope\", scope=\"{challenge_scope}\", resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }
    let (status, kind, body, extra) = if request.path == "/mcp" {
        if !request.auth {
            let resource_scope = counts
                .resource_scope
                .lock()
                .expect("fixture scope")
                .clone()
                .unwrap_or_else(|| "tools:read".into());
            (
                401,
                "text/plain",
                String::new(),
                Some(format!(
                    "WWW-Authenticate: Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\", scope=\"{resource_scope}\"\r\n"
                )),
            )
        } else {
            counts.authenticated.fetch_add(1, Ordering::SeqCst);
            let body: serde_json::Value =
                serde_json::from_slice(&request.body).expect("fixture request JSON");
            let result = match body["method"].as_str().expect("method") {
                "server/discover" => serde_json::json!({
                    "resultType":"complete", "supportedVersions":["2026-07-28"],
                    "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private"
                }),
                "tools/list" => serde_json::json!({
                    "resultType":"complete", "tools":[{"name":"echo", "inputSchema":{"type":"object"}}],
                    "ttlMs":0, "cacheScope":"private"
                }),
                "tools/call" => serde_json::json!({
                    "resultType":"complete", "content":[{"type":"text", "text":"authorized"}],
                    "isError":false
                }),
                _ => return,
            };
            (
                200,
                "application/json",
                serde_json::json!({"jsonrpc":"2.0", "id":body["id"], "result":result}).to_string(),
                None,
            )
        }
    } else if request.path == "/.well-known/oauth-protected-resource/mcp" {
        let resource_scope = counts
            .resource_scope
            .lock()
            .expect("fixture scope")
            .clone()
            .unwrap_or_else(|| "tools:read".into());
        (
            200,
            "application/json",
            serde_json::json!({
                "resource":endpoint, "authorization_servers":[issuer],
                "scopes_supported":[resource_scope]
            })
            .to_string(),
            None,
        )
    } else if request.path == "/.well-known/oauth-authorization-server/issuer" {
        (
            200,
            "application/json",
            serde_json::json!({
                "issuer":issuer,
                "authorization_endpoint":format!("{origin}/authorize"),
                "token_endpoint":format!("{origin}/token"),
                "registration_endpoint":format!("{origin}/register"),
                "code_challenge_methods_supported":["S256"],
                "authorization_response_iss_parameter_supported":true
            })
            .to_string(),
            None,
        )
    } else if request.path == "/register" {
        counts.registration.fetch_add(1, Ordering::SeqCst);
        if counts.block_registration.load(Ordering::SeqCst) {
            counts.registration_started.notify_one();
            counts.registration_release.notified().await;
        }
        let body: serde_json::Value = serde_json::from_slice(&request.body).expect("DCR JSON");
        (
            201,
            "application/json",
            serde_json::json!({
                "client_id":"dynamic-client",
                "token_endpoint_auth_method":"none",
                "redirect_uris":body["redirect_uris"]
            })
            .to_string(),
            None,
        )
    } else if request.path == "/token" {
        counts.token.fetch_add(1, Ordering::SeqCst);
        (
            200,
            "application/json",
            serde_json::json!({
                "access_token":"owned-access", "refresh_token":"owned-refresh",
                "token_type":"Bearer", "expires_in":3600, "scope":"tools:read"
            })
            .to_string(),
            None,
        )
    } else {
        (404, "text/plain", String::new(), None)
    };
    let reason = match status {
        200 => "OK",
        201 => "Created",
        401 => "Unauthorized",
        _ => "Not Found",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
        body.len(),
        extra.unwrap_or_default(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn read_request(stream: &mut TcpStream) -> Option<FixtureRequest> {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).await.ok()?;
        if count == 0 || bytes.len() + count > 1024 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let head = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let mut lines = head.split("\r\n");
    let path = lines.next()?.split_ascii_whitespace().nth(1)?.to_owned();
    let mut length = 0usize;
    let mut auth = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().ok()?;
            }
            if name.eq_ignore_ascii_case("authorization") {
                auth = value.trim() == "Bearer owned-access";
            }
        }
    }
    if length > 1024 * 1024 {
        return None;
    }
    while bytes.len() - header_end < length {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).await.ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Some(FixtureRequest {
        path,
        auth,
        body: bytes[header_end..header_end + length].to_vec(),
    })
}

fn oauth_form(endpoint: String, client_id: Option<&str>) -> McpServerForm {
    McpServerForm {
        display_name: "Owned OAuth fixture".into(),
        transport: McpServerTransport::Remote {
            endpoint,
            allow_loopback_http: true,
            authorization: McpRemoteAuthorization::OAuth {
                client_id: client_id.map(str::to_owned),
            },
        },
    }
}

async fn browser_callback(redirect_uri: &str, state: &str, issuer: &str) {
    let host_port = redirect_uri
        .strip_prefix("http://")
        .and_then(|url| url.split_once('/'))
        .map(|(host, _)| host)
        .expect("loopback redirect");
    let mut stream = TcpStream::connect(host_port)
        .await
        .expect("callback connect");
    let request = format!(
        "GET /callback?code=owned-code&state={state}&iss={issuer} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("callback send");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("callback reply");
    assert!(response.starts_with(b"HTTP/1.1 200 OK"));
}

#[tokio::test]
async fn issue73_oauth_metadata_scope_matching_short_owner_secret_never_reaches_ui() {
    let (endpoint, issuer, counts) = owned_oauth_fixture().await;
    let data = tempfile::tempdir().expect("database");
    let config = tempfile::tempdir().expect("config");
    let service = McpServerSettingsService::new(data.path().join("vega.db"), config.path().into());
    let row = service
        .create(oauth_form(endpoint, Some("owned-client")))
        .expect("OAuth row");
    vega_store::keystore::set_key(config.path(), "provider-short-token", "q7")
        .expect("fake owner secret");
    *counts.resource_scope.lock().expect("fixture scope") = Some("q7".into());

    assert!(matches!(
        service
            .discover_oauth(&row.id, row.config_revision, true)
            .await,
        Err(McpSettingsError::AuthorizationFailed)
    ));
    assert!(matches!(
        service
            .prepare_oauth(&row.id, row.config_revision, &issuer)
            .await,
        Err(McpSettingsError::AuthorizationFailed)
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let credential_dir = config.path().join("credentials");
        std::fs::set_permissions(&credential_dir, std::fs::Permissions::from_mode(0o755))
            .expect("make owner credential store unsafe");
        assert!(matches!(
            service
                .discover_oauth(&row.id, row.config_revision, true)
                .await,
            Err(McpSettingsError::Credential)
        ));
    }
}

#[tokio::test]
async fn issue73_oauth_start_rechecks_secrets_configured_after_preview() {
    let (endpoint, issuer, _) = owned_oauth_fixture().await;
    let data = tempfile::tempdir().expect("database");
    let config = tempfile::tempdir().expect("config");
    let service = McpServerSettingsService::new(data.path().join("vega.db"), config.path().into());
    let row = service
        .create(oauth_form(endpoint, Some("owned-client")))
        .expect("OAuth row");
    let prepared = service
        .prepare_oauth(&row.id, row.config_revision, &issuer)
        .await
        .expect("safe preview");
    vega_store::keystore::set_key(config.path(), "provider-new-key", "tools:read")
        .expect("fake newly configured owner secret");

    assert!(matches!(
        service.begin_oauth(&prepared.flow_id, true).await,
        Err(McpSettingsError::AuthorizationFailed)
    ));
}

#[tokio::test]
async fn issue73_settings_oauth_preregistered_callback_restart_and_authenticated_connect() {
    let (endpoint, issuer, counts) = owned_oauth_fixture().await;
    let database = tempfile::tempdir().expect("database");
    let config = tempfile::tempdir().expect("config");
    let path = database.path().join("vega.db");
    let service = McpServerSettingsService::new(path.clone(), config.path().into());
    let created = service
        .create(oauth_form(endpoint, Some("owned-client")))
        .expect("disabled OAuth row");
    let discovery = service
        .discover_oauth(&created.id, created.config_revision, true)
        .await
        .expect("explicit resource discovery");
    assert_eq!(discovery.issuers, [issuer.as_str()]);
    let prepared = service
        .prepare_oauth(&created.id, created.config_revision, &issuer)
        .await
        .expect("exact preconsent details");
    assert_eq!(prepared.registration, McpOAuthRegistration::PreRegistered);
    assert!(prepared.redirect_uri.starts_with("http://127.0.0.1:"));
    assert_eq!(prepared.requested_scopes, ["tools:read"]);
    assert!(matches!(
        service.begin_oauth(&prepared.flow_id, false).await,
        Err(McpSettingsError::ConfirmationRequired)
    ));
    assert_eq!(counts.registration.load(Ordering::SeqCst), 0);
    let start = service
        .begin_oauth(&prepared.flow_id, true)
        .await
        .expect("explicit browser start");
    let state = start
        .authorization_url
        .split("state=")
        .nth(1)
        .and_then(|suffix| suffix.split('&').next())
        .expect("fresh state");
    let finish_service = service.clone();
    let flow_id = start.flow_id.clone();
    let completion = tokio::spawn(async move { finish_service.finish_oauth(&flow_id).await });
    browser_callback(&prepared.redirect_uri, state, &issuer).await;
    let connected = completion
        .await
        .expect("finish worker")
        .expect("OAuth complete");
    assert!(!connected.enabled);
    assert!(connected.credential_configured);
    assert_eq!(counts.token.load(Ordering::SeqCst), 1);
    let enabled = service
        .set_enabled(&connected.id, connected.config_revision, true, true)
        .await
        .expect("explicit enable");
    assert!(enabled.enabled);
    assert!(matches!(enabled.health, McpServerHealth::Disconnected));
    assert_eq!(
        service.ready_for_run().await.unwrap().ready_servers.len(),
        1
    );
    drop(service);
    let restarted = McpServerSettingsService::new(path, config.path().into());
    assert_eq!(
        restarted.ready_for_run().await.unwrap().ready_servers.len(),
        1
    );
    assert!(counts.authenticated.load(Ordering::SeqCst) >= 4);
}

#[tokio::test]
async fn issue73_settings_dcr_requires_exact_preview_and_fresh_consent() {
    let (endpoint, issuer, counts) = owned_oauth_fixture().await;
    let database = tempfile::tempdir().expect("database");
    let config = tempfile::tempdir().expect("config");
    let service =
        McpServerSettingsService::new(database.path().join("vega.db"), config.path().into());
    let row = service.create(oauth_form(endpoint, None)).expect("DCR row");
    let prepared = service
        .prepare_oauth(&row.id, row.config_revision, &issuer)
        .await
        .expect("preconsent preview");
    assert_eq!(
        prepared.registration,
        McpOAuthRegistration::DynamicRegistration
    );
    assert_eq!(
        prepared.registration_endpoint.as_deref(),
        Some(format!("{}register", issuer.trim_end_matches("issuer")).as_str())
    );
    assert_eq!(counts.registration.load(Ordering::SeqCst), 0);
    assert!(matches!(
        service.begin_oauth(&prepared.flow_id, false).await,
        Err(McpSettingsError::ConfirmationRequired)
    ));
    assert_eq!(counts.registration.load(Ordering::SeqCst), 0);
    let _ = service
        .begin_oauth(&prepared.flow_id, true)
        .await
        .expect("consented DCR");
    assert_eq!(counts.registration.load(Ordering::SeqCst), 1);
    service
        .cancel_oauth(&prepared.flow_id)
        .expect("cancel flow");
    assert!(matches!(
        service.finish_oauth(&prepared.flow_id).await,
        Err(McpSettingsError::Conflict)
    ));
}

#[tokio::test]
async fn issue73_settings_slow_dcr_cancel_cannot_resurrect_flow_or_block_next_prepare() {
    let (endpoint, issuer, counts) = owned_oauth_fixture().await;
    let database = tempfile::tempdir().expect("database");
    let config = tempfile::tempdir().expect("config");
    let service =
        McpServerSettingsService::new(database.path().join("vega.db"), config.path().into());
    let row = service.create(oauth_form(endpoint, None)).expect("DCR row");
    let prepared = service
        .prepare_oauth(&row.id, row.config_revision, &issuer)
        .await
        .expect("first preview");
    assert!(matches!(
        service
            .prepare_oauth(&row.id, row.config_revision, &issuer)
            .await,
        Err(McpSettingsError::Capacity)
    ));

    counts.block_registration.store(true, Ordering::SeqCst);
    let flow_id = prepared.flow_id.clone();
    let begin_service = service.clone();
    let begin_flow_id = flow_id.clone();
    let begin = tokio::spawn(async move { begin_service.begin_oauth(&begin_flow_id, true).await });
    tokio::time::timeout(
        Duration::from_secs(3),
        counts.registration_started.notified(),
    )
    .await
    .expect("DCR began before cancellation");
    assert_eq!(counts.registration.load(Ordering::SeqCst), 1);
    let current = service
        .list()
        .expect("current disabled row")
        .into_iter()
        .find(|candidate| candidate.id == row.id)
        .expect("current server");
    assert!(!current.enabled);
    assert!(matches!(
        service
            .prepare_oauth(&row.id, current.config_revision, &issuer)
            .await,
        Err(McpSettingsError::Capacity)
    ));

    service.cancel_oauth(&flow_id).expect("cancel active DCR");
    let second = service
        .prepare_oauth(&row.id, current.config_revision, &issuer)
        .await
        .expect("cancelled flow no longer reserves server");
    counts.registration_release.notify_one();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), begin)
            .await
            .expect("old begin completed")
            .expect("old begin worker"),
        Err(McpSettingsError::Conflict)
    ));
    assert!(matches!(
        service.finish_oauth(&flow_id).await,
        Err(McpSettingsError::Conflict)
    ));
    assert!(matches!(
        service.begin_oauth(&flow_id, true).await,
        Err(McpSettingsError::Conflict)
    ));
    service
        .cancel_oauth(&second.flow_id)
        .expect("cancel new flow");
}

#[tokio::test]
async fn issue73_real_403_scope_challenge_reaches_settings_without_replaying_failed_call() {
    let (endpoint, issuer, counts) = owned_oauth_fixture().await;
    let database = tempfile::tempdir().expect("database");
    let config = tempfile::tempdir().expect("config");
    let project = tempfile::tempdir().expect("project");
    let path = database.path().join("vega.db");
    let service = McpServerSettingsService::new(path.clone(), config.path().into());
    let created = service
        .create(oauth_form(endpoint, Some("owned-client")))
        .expect("OAuth row");
    let prepared = service
        .prepare_oauth(&created.id, created.config_revision, &issuer)
        .await
        .expect("first grant preview");
    let start = service
        .begin_oauth(&prepared.flow_id, true)
        .await
        .expect("first grant start");
    let state = start
        .authorization_url
        .split("state=")
        .nth(1)
        .and_then(|suffix| suffix.split('&').next())
        .expect("state");
    let finish_service = service.clone();
    let flow_id = start.flow_id.clone();
    let completion = tokio::spawn(async move { finish_service.finish_oauth(&flow_id).await });
    browser_callback(&prepared.redirect_uri, state, &issuer).await;
    let connected = completion
        .await
        .expect("finish worker")
        .expect("first grant");
    service
        .set_enabled(&connected.id, connected.config_revision, true, true)
        .await
        .expect("explicit enable");
    let readiness = service.ready_for_run().await.expect("OAuth ready");
    assert_eq!(readiness.ready_servers.len(), 1);

    let store = Store::open(&path).expect("conversation store");
    store.migrate().expect("migration");
    let project_row = vega_store::projects::create(
        store.conn(),
        project.path().to_str().expect("project path"),
        "scope-fixture",
        Some("master"),
    )
    .expect("project");
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "scope-thread",
            project_id: &project_row.id,
            title: "",
            mode: "execute",
            permission_mode: "confirm",
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .expect("thread");
    let identity = McpCallIdentity {
        server_id: created.id.clone(),
        config_revision: service
            .list()
            .expect("enabled row")
            .into_iter()
            .find(|row| row.id == created.id)
            .expect("row")
            .config_revision,
        exact_tool_name: "echo".into(),
        arguments_bytes: 0,
        arguments_sha256: "0".repeat(64),
        argument_preview: String::new(),
    };
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "scope-call".into(),
                name: identity.alias(),
                input_json: r#"{"echo":"do not replay"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    *counts.challenge_scope.lock().expect("fixture scope") = Some("q7".into());
    counts.force_step_up.store(true, Ordering::SeqCst);
    let tools = vega_tools::Tools::new(project.path()).expect("tools");
    let challenged_run = readiness.ready_servers[0].clone();
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "scope-thread",
        "Try the protected tool once",
        "System",
        CancellationToken::new(),
        &ApproveOwnedMcp,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
        readiness.ready_servers,
    )
    .await
    .expect("conversation completes with failed tool");
    assert!(!run.failed);
    assert_eq!(counts.challenged_calls.load(Ordering::SeqCst), 1);
    vega_store::keystore::set_key(config.path(), "provider-short-token", "q7")
        .expect("fake owner secret");
    assert!(matches!(
        service.step_up_offers(),
        Err(McpSettingsError::AuthorizationFailed)
    ));
    assert!(matches!(
        service
            .prepare_verified_step_up(&created.id, identity.config_revision)
            .await,
        Err(McpSettingsError::AuthorizationFailed)
    ));
    vega_store::keystore::delete_key(config.path(), "provider-short-token")
        .expect("remove fake owner secret");
    let offers = service.step_up_offers().expect("bound challenge offer");
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].server_id, created.id);
    assert_eq!(offers[0].added_scopes, ["q7"]);
    let restarted = McpServerSettingsService::new(path.clone(), config.path().into());
    assert!(
        restarted
            .step_up_offers()
            .expect("restart offers")
            .is_empty(),
        "restart must not resurrect an unconsented external scope request"
    );
    let preview = service
        .prepare_verified_step_up(&created.id, offers[0].config_revision)
        .await
        .expect("verified scope preview");
    assert_eq!(preview.step_up_added_scopes, ["q7"]);
    assert_eq!(preview.requested_scopes, ["tools:read", "q7"]);
    assert!(matches!(
        service.begin_oauth(&preview.flow_id, false).await,
        Err(McpSettingsError::ConfirmationRequired)
    ));
    let start = service
        .begin_oauth(&preview.flow_id, true)
        .await
        .expect("explicit step-up start");
    assert!(
        challenged_run.is_revoked(),
        "consented scope upgrade invalidates the old run's executable lease"
    );
    assert_eq!(start.requested_scopes, preview.requested_scopes);
    assert_eq!(counts.challenged_calls.load(Ordering::SeqCst), 1);
    service
        .cancel_oauth(&start.flow_id)
        .expect("cancel step-up");
}
