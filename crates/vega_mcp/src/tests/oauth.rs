use std::collections::HashMap;

use serde_json::{Value, json};
use tokio::sync::mpsc;

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
