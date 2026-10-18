use std::collections::HashMap;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use vega_mcp::{BearerCredential, HttpClient, McpError, ProtocolVersion, ResourceAuthorization};

#[derive(Clone, Copy)]
enum Fixture {
    PreRegistered,
    Dynamic,
    DynamicRejected,
    DynamicInvalid,
    CimdOnly,
    Redirect,
    MalformedRedirect,
    IssuerMismatch,
    PkceMissing,
    MetadataRedirect,
    ExpiredToken,
    RefreshRejected,
    LegacyProtected,
    InsufficientScope,
    InsufficientScopeWrongMetadata,
    TokenExcessScope,
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn on_new_runtime<T: Send + 'static>(
    operation: impl std::future::Future<Output = T> + Send + 'static,
) -> T {
    tokio::task::spawn_blocking(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("owned short-lived runtime")
            .block_on(operation)
    })
    .await
    .expect("owned worker")
}

#[tokio::test]
async fn m06_dcr_token_and_refresh_survive_separate_destroyed_tokio_runtimes() {
    let (endpoint, issuer, mut requests) = fixture(Fixture::Dynamic).await;
    let redirect = "http://127.0.0.1:54321/callback";
    for _ in 0..8 {
        let discovery_endpoint = endpoint.clone();
        let discovery_issuer = issuer.clone();
        let (resource, server) = on_new_runtime(async move {
            let resource = ResourceAuthorization::discover(&discovery_endpoint, true).await?;
            let server = resource.discover_server(&discovery_issuer).await?;
            Ok::<_, McpError>((resource, server))
        })
        .await
        .expect("discover in first runtime");
        let client = on_new_runtime(async move { server.register_dcr(redirect, true).await })
            .await
            .expect("DCR in second runtime");
        let pending = client.begin_authorization().expect("fresh PKCE request");
        let browser = reqwest::Url::parse(pending.authorization_url()).expect("browser URL");
        let query: HashMap<_, _> = browser.query_pairs().into_owned().collect();
        let callback = format!(
            "{redirect}?code=owned-code&state={}&iss={issuer}",
            query["state"]
        );
        let (client, tokens) = on_new_runtime(async move {
            let tokens = client.finish_authorization(pending, &callback).await?;
            Ok::<_, McpError>((client, tokens))
        })
        .await
        .expect("token exchange in third runtime");
        assert!(tokens.bearer_credential(&resource).is_ok());
        let renewed = on_new_runtime(async move { client.refresh(&tokens).await })
            .await
            .expect("refresh in fourth runtime");
        assert!(renewed.bearer_credential(&resource).is_ok());
    }
    let mut seen = Vec::new();
    while let Ok(request) = requests.try_recv() {
        seen.push(request);
    }
    assert_eq!(
        seen.iter()
            .filter(|request| request.path == "/register")
            .count(),
        8
    );
    assert_eq!(
        seen.iter()
            .filter(|request| request.path == "/token")
            .count(),
        16
    );
}

#[tokio::test]
async fn m06_preregistered_oauth_pkce_issuer_resource_and_authenticated_tool_call() {
    let (endpoint, issuer, mut requests) = fixture(Fixture::PreRegistered).await;
    let resource = ResourceAuthorization::discover(&endpoint, true)
        .await
        .expect("protected resource");
    assert_eq!(resource.resource(), &*endpoint);
    assert_eq!(resource.authorization_servers(), [issuer.as_str()]);
    assert_eq!(resource.requested_scopes(), ["tools:read"]);
    let server = resource
        .discover_server(&issuer)
        .await
        .expect("AS metadata");
    let redirect = "http://127.0.0.1:54321/callback";
    let client = server
        .pre_registered("owned-client", &issuer, redirect)
        .expect("bound preregistration");
    let pending = client.begin_authorization().expect("PKCE request");
    let url = reqwest::Url::parse(pending.authorization_url()).expect("authorization URL");
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["client_id"], "owned-client");
    assert_eq!(query["resource"], &*endpoint);
    assert_eq!(query["scope"], "tools:read");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["code_challenge"].len(), 43);
    assert!(query["state"].len() >= 32);
    let callback = format!(
        "{redirect}?code=owned-code&state={}&iss={issuer}",
        query["state"]
    );
    let tokens = client
        .finish_authorization(pending, &callback)
        .await
        .expect("code exchanged only after callback validation");
    let mut mcp = HttpClient::connect_with_bearer(
        &endpoint,
        true,
        tokens.bearer_credential(&resource).expect("bound bearer"),
    )
    .await
    .expect("authenticated discovery");
    let tools = mcp.list_tools().await.expect("authenticated tools/list");
    let result = mcp
        .call_tool(&tools.tools[0], json!({"echo":"authorized"}))
        .await
        .expect("authenticated tools/call");
    assert_eq!(result.text, ["authorized"]);
    let renewed = client.refresh(&tokens).await.expect("refresh bound to AS");
    assert!(!renewed.is_expired());

    let mut seen = Vec::new();
    while let Ok(request) = requests.try_recv() {
        seen.push(request);
    }
    assert!(seen.iter().any(|request| request.path == "/mcp"
        && request.headers.get("authorization") == Some(&"Bearer owned-access".into())));
    let token_requests: Vec<_> = seen
        .iter()
        .filter(|request| request.path == "/token")
        .collect();
    assert_eq!(token_requests.len(), 2);
    let exchange = form(&token_requests[0].body);
    assert_eq!(exchange["resource"], &*endpoint);
    assert_eq!(exchange["client_id"], "owned-client");
    assert_eq!(exchange["code"], "owned-code");
    assert!(exchange["code_verifier"].len() >= 43);
    let refresh = form(&token_requests[1].body);
    assert_eq!(refresh["resource"], &*endpoint);
    assert_eq!(refresh["refresh_token"], "owned-refresh");
}

#[tokio::test]
async fn m06_403_scope_step_up_is_typed_and_bound_to_original_oauth_resource() {
    for scenario in [
        Fixture::InsufficientScope,
        Fixture::InsufficientScopeWrongMetadata,
    ] {
        let (endpoint, issuer, mut requests) = fixture(scenario).await;
        let resource = ResourceAuthorization::discover(&endpoint, true)
            .await
            .expect("PRM");
        let server = resource.discover_server(&issuer).await.expect("AS");
        let redirect = "http://127.0.0.1:54321/callback";
        let client = server
            .pre_registered("owned-client", &issuer, redirect)
            .expect("client");
        let pending = client.begin_authorization().expect("auth start");
        let url = reqwest::Url::parse(pending.authorization_url()).expect("URL");
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .expect("state")
            .1;
        let callback = format!("{redirect}?code=owned-code&state={state}&iss={issuer}");
        let tokens = client
            .finish_authorization(pending, &callback)
            .await
            .expect("token");
        let mut mcp = HttpClient::connect_with_bearer(
            &endpoint,
            true,
            tokens.bearer_credential(&resource).expect("bound bearer"),
        )
        .await
        .expect("MCP");
        let tools = mcp.list_tools().await.expect("tools");
        let result = mcp
            .call_tool(&tools.tools[0], json!({"echo":"needs write"}))
            .await;
        if matches!(scenario, Fixture::InsufficientScope) {
            let challenge = match result {
                Err(McpError::InsufficientScope(challenge)) => challenge,
                other => panic!("expected typed step-up challenge, got {other:?}"),
            };
            assert_eq!(challenge.scopes(), ["tools:write"]);
            assert!(matches!(
                client.begin_verified_step_up(&tokens, &challenge, false),
                Err(McpError::ConsentRequired)
            ));
            let other_client = server
                .pre_registered("other-client", &issuer, redirect)
                .expect("other client");
            assert!(matches!(
                other_client.begin_verified_step_up(&tokens, &challenge, true),
                Err(McpError::CredentialBinding)
            ));
            let step_up = client
                .begin_verified_step_up(&tokens, &challenge, true)
                .expect("confirmed bound step-up");
            let url = reqwest::Url::parse(step_up.authorization_url()).expect("step-up URL");
            let scope = url
                .query_pairs()
                .find(|(key, _)| key == "scope")
                .expect("scope")
                .1;
            assert_eq!(scope, "fallback:read tools:write");
        } else {
            assert!(matches!(result, Err(McpError::AuthSecurity)));
        }
        let seen = drain(&mut requests);
        assert_eq!(
            seen.iter()
                .filter(|request| request.path == "/mcp"
                    && serde_json::from_slice::<Value>(&request.body)
                        .ok()
                        .is_some_and(|body| body["method"] == "tools/call"))
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn m06_token_response_cannot_silently_expand_requested_scope() {
    let (endpoint, issuer, mut requests) = fixture(Fixture::TokenExcessScope).await;
    let resource = ResourceAuthorization::discover(&endpoint, true)
        .await
        .expect("PRM");
    let server = resource.discover_server(&issuer).await.expect("AS");
    let redirect = "http://127.0.0.1:54321/callback";
    let client = server
        .pre_registered("owned-client", &issuer, redirect)
        .expect("client");
    let pending = client.begin_authorization().expect("auth start");
    let url = reqwest::Url::parse(pending.authorization_url()).expect("URL");
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .expect("state")
        .1;
    let callback = format!("{redirect}?code=owned-code&state={state}&iss={issuer}");
    assert!(matches!(
        client.finish_authorization(pending, &callback).await,
        Err(McpError::ScopeEscalation)
    ));
    assert!(
        !drain(&mut requests)
            .iter()
            .any(|request| request.path == "/mcp" && request.headers.contains_key("authorization"))
    );
}

#[tokio::test]
async fn m06_callback_state_and_issuer_fail_before_any_token_request() {
    let (endpoint, issuer, mut requests) = fixture(Fixture::PreRegistered).await;
    let resource = ResourceAuthorization::discover(&endpoint, true)
        .await
        .expect("PRM");
    let server = resource.discover_server(&issuer).await.expect("AS");
    let redirect = "http://127.0.0.1:54321/callback";
    let client = server
        .pre_registered("owned-client", &issuer, redirect)
        .expect("pre-reg");
    let pending = client.begin_authorization().expect("start");
    let bad_state = format!("{redirect}?code=stolen&state=wrong&iss={issuer}");
    assert!(matches!(
        client.finish_authorization(pending, &bad_state).await,
        Err(McpError::AuthSecurity)
    ));
    let pending = client.begin_authorization().expect("new start");
    let url = reqwest::Url::parse(pending.authorization_url()).expect("URL");
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .expect("state")
        .1;
    let missing_iss = format!("{redirect}?code=stolen&state={state}");
    assert!(matches!(
        client.finish_authorization(pending, &missing_iss).await,
        Err(McpError::AuthSecurity)
    ));
    let pending = client.begin_authorization().expect("new start");
    let url = reqwest::Url::parse(pending.authorization_url()).expect("URL");
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .expect("state")
        .1;
    let bad_iss = format!("{redirect}?code=stolen&state={state}&iss=http://127.0.0.1:1");
    assert!(matches!(
        client.finish_authorization(pending, &bad_iss).await,
        Err(McpError::AuthSecurity)
    ));
    assert!(
        !drain(&mut requests)
            .iter()
            .any(|request| request.path == "/token")
    );
}

#[tokio::test]
async fn m06_dcr_requires_explicit_confirmation_and_is_issuer_bound() {
    let (endpoint, issuer, mut requests) = fixture(Fixture::Dynamic).await;
    let resource = ResourceAuthorization::discover(&endpoint, true)
        .await
        .expect("PRM fallback");
    let server = resource
        .discover_server(&issuer)
        .await
        .expect("OIDC fallback");
    let redirect = "http://127.0.0.1:54321/callback";
    assert!(matches!(
        server.register_dcr(redirect, false).await,
        Err(McpError::ConsentRequired)
    ));
    assert!(
        !drain(&mut requests)
            .iter()
            .any(|request| request.path == "/register")
    );
    let client = server
        .register_dcr(redirect, true)
        .await
        .expect("confirmed DCR");
    let pending = client.begin_authorization().expect("DCR client ID");
    let url = reqwest::Url::parse(pending.authorization_url()).expect("URL");
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], "dynamic-client");
    let registration = drain(&mut requests)
        .into_iter()
        .find(|request| request.path == "/register")
        .expect("registration POST");
    let document: Value = serde_json::from_slice(&registration.body).expect("registration JSON");
    assert_eq!(document["application_type"], "native");
    assert_eq!(document["token_endpoint_auth_method"], "none");
    assert_eq!(document["redirect_uris"][0], redirect);
    assert_eq!(registration.method, "POST");
    let callback = format!(
        "{redirect}?code=owned-code&state={}&iss={issuer}",
        query["state"]
    );
    let tokens = client
        .finish_authorization(pending, &callback)
        .await
        .expect("DCR code exchange");
    let mut mcp = HttpClient::connect_with_bearer(
        &endpoint,
        true,
        tokens
            .bearer_credential(&resource)
            .expect("DCR token bound"),
    )
    .await
    .expect("DCR authenticated connection");
    let tools = mcp.list_tools().await.expect("DCR tools/list");
    let result = mcp
        .call_tool(&tools.tools[0], json!({"echo":"after DCR"}))
        .await
        .expect("DCR tools/call");
    assert_eq!(result.text, ["after DCR"]);
    assert!(drain(&mut requests).iter().any(|request| {
        request.path == "/mcp"
            && request.headers.get("authorization").map(String::as_str)
                == Some("Bearer owned-access")
            && serde_json::from_slice::<Value>(&request.body)
                .ok()
                .is_some_and(|body| body["method"] == "tools/call")
    }));
}

#[tokio::test]
async fn m06_dcr_rejection_and_cimd_only_fail_closed() {
    let (endpoint, issuer, mut requests) = fixture(Fixture::DynamicRejected).await;
    let resource = ResourceAuthorization::discover(&endpoint, true)
        .await
        .expect("PRM");
    let server = resource.discover_server(&issuer).await.expect("AS");
    assert!(matches!(
        server
            .register_dcr("http://127.0.0.1:54321/callback", true)
            .await,
        Err(McpError::Registration)
    ));
    assert!(
        !drain(&mut requests)
            .iter()
            .any(|request| request.path == "/token")
    );

    let (endpoint, issuer, mut requests) = fixture(Fixture::DynamicInvalid).await;
    let resource = ResourceAuthorization::discover(&endpoint, true)
        .await
        .expect("PRM");
    let server = resource.discover_server(&issuer).await.expect("AS");
    assert!(matches!(
        server
            .register_dcr("http://127.0.0.1:54321/callback", true)
            .await,
        Err(McpError::Registration)
    ));
    assert!(
        !drain(&mut requests)
            .iter()
            .any(|request| request.path == "/token")
    );

    let (endpoint, issuer, mut requests) = fixture(Fixture::CimdOnly).await;
    let resource = ResourceAuthorization::discover(&endpoint, true)
        .await
        .expect("PRM");
    let server = resource.discover_server(&issuer).await.expect("AS");
    assert!(matches!(
        server
            .register_dcr("http://127.0.0.1:54321/callback", true)
            .await,
        Err(McpError::CimdUnavailable)
    ));
    assert!(
        !drain(&mut requests)
            .iter()
            .any(|request| request.path == "/register" || request.path == "/token")
    );
}

#[tokio::test]
async fn m06_hostile_metadata_and_missing_pkce_never_reach_token_endpoint() {
    for scenario in [
        Fixture::IssuerMismatch,
        Fixture::PkceMissing,
        Fixture::MetadataRedirect,
    ] {
        let (endpoint, issuer, mut requests) = fixture(scenario).await;
        let resource = ResourceAuthorization::discover(&endpoint, true)
            .await
            .expect("PRM");
        assert!(resource.discover_server(&issuer).await.is_err());
        assert!(
            !drain(&mut requests)
                .iter()
                .any(|request| request.path == "/token" || request.path == "/authorize")
        );
    }
}

#[tokio::test]
async fn m06_expiry_refresh_failure_and_step_up_are_closed() {
    for scenario in [Fixture::ExpiredToken, Fixture::RefreshRejected] {
        let (endpoint, issuer, mut requests) = fixture(scenario).await;
        let resource = ResourceAuthorization::discover(&endpoint, true)
            .await
            .expect("PRM");
        let server = resource.discover_server(&issuer).await.expect("AS");
        let redirect = "http://127.0.0.1:54321/callback";
        let client = server
            .pre_registered("owned-client", &issuer, redirect)
            .expect("pre-reg");
        assert!(matches!(
            client.begin_step_up(&["tools:read".into()], &["tools:write".into()], false),
            Err(McpError::ConsentRequired)
        ));
        let step_up = client
            .begin_step_up(&["tools:read".into()], &["tools:write".into()], true)
            .expect("confirmed step-up");
        let url = reqwest::Url::parse(step_up.authorization_url()).expect("URL");
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["scope"], "tools:read tools:write");
        let callback = format!(
            "{redirect}?code=owned-code&state={}&iss={issuer}",
            query["state"]
        );
        let tokens = client
            .finish_authorization(step_up, &callback)
            .await
            .expect("token");
        match scenario {
            Fixture::ExpiredToken => {
                assert!(tokens.is_expired());
                assert!(matches!(
                    tokens.bearer_credential(&resource),
                    Err(McpError::AuthRequired)
                ));
            }
            Fixture::RefreshRejected => {
                assert!(matches!(
                    client.refresh(&tokens).await,
                    Err(McpError::AuthRequired)
                ));
            }
            _ => unreachable!("only expiry scenarios"),
        }
        assert!(
            !drain(&mut requests)
                .iter()
                .any(|request| request.path == "/mcp"
                    && request.headers.contains_key("authorization"))
        );
    }
}

#[tokio::test]
async fn m06_oauth_can_authenticate_legacy_streamable_http_after_well_known_fallback() {
    let (endpoint, issuer, mut requests) = fixture(Fixture::LegacyProtected).await;
    let resource = ResourceAuthorization::discover(&endpoint, true)
        .await
        .expect("legacy PRM fallback");
    let server = resource.discover_server(&issuer).await.expect("legacy AS");
    let redirect = "http://127.0.0.1:54321/callback";
    let client = server
        .pre_registered("owned-client", &issuer, redirect)
        .expect("pre-reg");
    let pending = client.begin_authorization().expect("authorize");
    let url = reqwest::Url::parse(pending.authorization_url()).expect("URL");
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .expect("state")
        .1;
    let callback = format!("{redirect}?code=owned-code&state={state}&iss={issuer}");
    let tokens = client
        .finish_authorization(pending, &callback)
        .await
        .expect("legacy token");
    let mut mcp = HttpClient::connect_with_bearer(
        &endpoint,
        true,
        tokens.bearer_credential(&resource).expect("binding"),
    )
    .await
    .expect("legacy authenticated connect");
    assert_eq!(mcp.version(), ProtocolVersion::Legacy);
    let tools = mcp.list_tools().await.expect("legacy tools/list");
    let result = mcp
        .call_tool(&tools.tools[0], json!({"echo":"legacy auth"}))
        .await
        .expect("legacy tools/call");
    assert_eq!(result.text, ["legacy auth"]);
    let seen = drain(&mut requests);
    assert!(seen.iter().any(|request| request.path == "/mcp" && !request.headers.contains_key("authorization")));
    assert!(seen.iter().any(|request| {
        request.path == "/mcp"
            && request.headers.get("authorization").map(String::as_str)
                == Some("Bearer owned-access")
            && serde_json::from_slice::<Value>(&request.body)
                .ok()
                .is_some_and(|body| body["method"] == "tools/call")
    }));
}

#[tokio::test]
async fn m07_manual_bearer_exact_endpoint_and_redirect_never_forward() {
    let (endpoint, _issuer, mut requests) = fixture(Fixture::Redirect).await;
    let credential =
        BearerCredential::manual(&endpoint, true, "manual-secret".into()).expect("manual binding");
    let different = endpoint.replace("/mcp", "/other");
    assert!(matches!(
        HttpClient::connect_with_bearer(&different, true, credential).await,
        Err(McpError::CredentialBinding)
    ));
    let credential =
        BearerCredential::manual(&endpoint, true, "manual-secret".into()).expect("manual binding");
    assert!(matches!(
        HttpClient::connect_with_bearer(&endpoint, true, credential).await,
        Err(McpError::InvalidConfig)
    ));
    let first = requests.recv().await.expect("redirecting request");
    assert_eq!(
        first.headers.get("authorization").map(String::as_str),
        Some("Bearer manual-secret")
    );
    assert!(
        requests.try_recv().is_err(),
        "redirect target was never contacted"
    );
}

#[tokio::test]
async fn m07_malformed_redirect_never_forwards_bearer() {
    let (endpoint, _issuer, mut requests) = fixture(Fixture::MalformedRedirect).await;
    let credential =
        BearerCredential::manual(&endpoint, true, "manual-secret".into()).expect("manual binding");
    assert!(matches!(
        HttpClient::connect_with_bearer(&endpoint, true, credential).await,
        Err(McpError::InvalidConfig)
    ));
    let first = requests.recv().await.expect("redirecting request");
    assert_eq!(first.path, "/mcp");
    assert_eq!(
        first.headers.get("authorization").map(String::as_str),
        Some("Bearer manual-secret")
    );
    assert!(
        requests.try_recv().is_err(),
        "malformed redirect target was never contacted"
    );
}

fn form(body: &[u8]) -> HashMap<String, String> {
    let text = std::str::from_utf8(body).expect("form UTF-8");
    reqwest::Url::parse(&format!("http://127.0.0.1/?{text}"))
        .expect("form URL")
        .query_pairs()
        .into_owned()
        .collect()
}

fn drain(receiver: &mut mpsc::UnboundedReceiver<Request>) -> Vec<Request> {
    let mut requests = Vec::new();
    while let Ok(request) = receiver.try_recv() {
        requests.push(request);
    }
    requests
}

async fn fixture(
    scenario: Fixture,
) -> (
    vega_mcp::mock::Endpoint,
    String,
    mpsc::UnboundedReceiver<Request>,
) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let endpoint = vega_mcp::mock::Endpoint::new("/mcp", |origin| {
        let origin = origin.to_owned();
        std::sync::Arc::new(move |request| {
            let captured = Request {
                method: request.method().to_string(),
                path: request.url().path().to_string(),
                headers: request
                    .headers()
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_string()))
                    .collect(),
                body: request
                    .body()
                    .and_then(|body| body.as_bytes())
                    .unwrap_or_default()
                    .to_vec(),
            };
            let (status, kind, body, headers) = response(scenario, &origin, &captured);
            sender.send(captured).expect("capture receiver");
            Box::pin(async move { Ok(vega_mcp::mock::response(status, kind, body, headers)) })
        })
    });
    let issuer = format!("{}/issuer", endpoint.origin());
    (endpoint, issuer, receiver)
}

fn response(
    scenario: Fixture,
    origin: &str,
    request: &Request,
) -> (u16, &'static str, String, Vec<(&'static str, String)>) {
    let endpoint = format!("{origin}/mcp");
    let issuer = format!("{origin}/issuer");
    if request.path == "/mcp" {
        if matches!(scenario, Fixture::Redirect) {
            return (
                302,
                "text/plain",
                String::new(),
                vec![("Location", format!("{origin}/other"))],
            );
        }
        if matches!(scenario, Fixture::MalformedRedirect) {
            return (
                302,
                "text/plain",
                String::new(),
                vec![("Location", "://invalid-redirect".into())],
            );
        }
        if request.headers.get("authorization").map(String::as_str) != Some("Bearer owned-access") {
            if matches!(scenario, Fixture::LegacyProtected) {
                return (
                    400,
                    "text/plain",
                    "legacy unknown method".into(),
                    Vec::new(),
                );
            }
            let headers = if matches!(scenario, Fixture::PreRegistered | Fixture::TokenExcessScope)
            {
                vec![(
                    "WWW-Authenticate",
                    format!(
                        "Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\", scope=\"tools:read\""
                    ),
                )]
            } else {
                Vec::new()
            };
            return (401, "text/plain", String::new(), headers);
        }
        let body: Value = serde_json::from_slice(&request.body).expect("MCP JSON");
        let id = body["id"].clone();
        if matches!(
            scenario,
            Fixture::InsufficientScope | Fixture::InsufficientScopeWrongMetadata
        ) && body["method"] == "tools/call"
        {
            let metadata = if matches!(scenario, Fixture::InsufficientScope) {
                format!("{origin}/.well-known/oauth-protected-resource/mcp")
            } else {
                format!("{origin}/other-resource-metadata")
            };
            return (
                403,
                "text/plain",
                String::new(),
                vec![(
                    "WWW-Authenticate",
                    format!(
                        "Bearer error=\"insufficient_scope\", scope=\"tools:write\", resource_metadata=\"{metadata}\""
                    ),
                )],
            );
        }
        if matches!(scenario, Fixture::LegacyProtected) {
            match body["method"].as_str().expect("legacy method") {
                "server/discover" => {
                    return (
                        400,
                        "text/plain",
                        "legacy unknown method".into(),
                        Vec::new(),
                    );
                }
                "initialize" => {
                    let result = json!({"protocolVersion":"2025-11-25", "capabilities":{"tools":{}}, "serverInfo":{"name":"owned-auth", "version":"1"}});
                    return (
                        200,
                        "application/json",
                        json!({"jsonrpc":"2.0", "id":id, "result":result}).to_string(),
                        vec![("Mcp-Session-Id", "owned-auth-session".into())],
                    );
                }
                "notifications/initialized" => {
                    return (202, "application/json", String::new(), Vec::new());
                }
                _ => {}
            }
        }
        let result = match body["method"].as_str().expect("method") {
            "server/discover" => {
                json!({"resultType":"complete", "supportedVersions":["2026-07-28"], "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private"})
            }
            "tools/list" => {
                json!({"resultType":"complete", "tools":[{"name":"echo", "inputSchema":{"type":"object"}}], "ttlMs":0, "cacheScope":"private"})
            }
            "tools/call" => {
                json!({"resultType":"complete", "content":[{"type":"text", "text":body["params"]["arguments"]["echo"]}], "isError":false})
            }
            other => panic!("unexpected MCP method: {other}"),
        };
        return (
            200,
            "application/json",
            json!({"jsonrpc":"2.0", "id":id, "result":result}).to_string(),
            Vec::new(),
        );
    }
    if request.path == "/.well-known/oauth-protected-resource/mcp" {
        return (200, "application/json", json!({"resource":endpoint, "authorization_servers":[issuer], "scopes_supported":["fallback:read"]}).to_string(), Vec::new());
    }
    if request.path == "/.well-known/oauth-authorization-server/issuer" {
        if matches!(scenario, Fixture::MetadataRedirect) {
            return (
                302,
                "text/plain",
                String::new(),
                vec![("Location", format!("{origin}/other"))],
            );
        }
        if !matches!(
            scenario,
            Fixture::PreRegistered | Fixture::ExpiredToken | Fixture::RefreshRejected
        ) {
            return (404, "text/plain", String::new(), Vec::new());
        }
        return (
            200,
            "application/json",
            as_metadata(scenario, origin).to_string(),
            Vec::new(),
        );
    }
    if request.path == "/.well-known/openid-configuration/issuer" {
        return (
            200,
            "application/json",
            as_metadata(scenario, origin).to_string(),
            Vec::new(),
        );
    }
    if request.path == "/register" {
        if matches!(scenario, Fixture::DynamicRejected) {
            return (
                400,
                "application/json",
                json!({"error":"invalid_redirect_uri"}).to_string(),
                Vec::new(),
            );
        }
        if matches!(scenario, Fixture::DynamicInvalid) {
            return (201, "application/json", json!({"client_id":"dynamic-client", "client_secret":"should-not-be-kept", "token_endpoint_auth_method":"client_secret_basic", "redirect_uris":["https://wrong.example/callback"]}).to_string(), Vec::new());
        }
        return (201, "application/json", json!({"client_id":"dynamic-client", "token_endpoint_auth_method":"none", "redirect_uris":["http://127.0.0.1:54321/callback"]}).to_string(), Vec::new());
    }
    if request.path == "/token" {
        let request_form = form(&request.body);
        if matches!(scenario, Fixture::RefreshRejected)
            && request_form.get("grant_type").map(String::as_str) == Some("refresh_token")
        {
            return (
                400,
                "application/json",
                json!({"error":"invalid_grant"}).to_string(),
                Vec::new(),
            );
        }
        let expiry = if matches!(scenario, Fixture::ExpiredToken) {
            0
        } else {
            3600
        };
        let scope = if matches!(scenario, Fixture::TokenExcessScope) {
            "tools:read tools:admin"
        } else if matches!(
            scenario,
            Fixture::PreRegistered | Fixture::ExpiredToken | Fixture::RefreshRejected
        ) {
            "tools:read"
        } else {
            "fallback:read"
        };
        return (200, "application/json", json!({"access_token":"owned-access", "refresh_token":"owned-refresh", "token_type":"Bearer", "expires_in":expiry, "scope":scope}).to_string(), Vec::new());
    }
    panic!("unexpected fixture path: {}", request.path);
}

fn as_metadata(scenario: Fixture, origin: &str) -> Value {
    let mut value = json!({
        "issuer":format!("{origin}/issuer"),
        "authorization_endpoint":format!("{origin}/authorize"),
        "token_endpoint":format!("{origin}/token"),
        "code_challenge_methods_supported":["S256"],
        "authorization_response_iss_parameter_supported":true
    });
    if matches!(scenario, Fixture::IssuerMismatch) {
        value["issuer"] = json!("https://attacker.example");
    }
    if matches!(scenario, Fixture::PkceMissing) {
        value
            .as_object_mut()
            .expect("object")
            .remove("code_challenge_methods_supported");
    }
    if matches!(
        scenario,
        Fixture::Dynamic | Fixture::DynamicRejected | Fixture::DynamicInvalid
    ) {
        value["registration_endpoint"] = json!(format!("{origin}/register"));
    }
    if matches!(scenario, Fixture::CimdOnly) {
        value["client_id_metadata_document_supported"] = json!(true);
    }
    value
}
