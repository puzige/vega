//! Owned, loopback-only OAuth/MCP server for native Vega acceptance.
//! Enable explicitly with `--features e2e-fixtures`; no user credential is read.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const FIXTURE_ACCESS: &str = "owned-fixture-access-not-a-user-secret";
const MAX_REQUEST: usize = 64 * 1024;

#[derive(Default)]
struct Counters {
    protected_requests: AtomicUsize,
    denied_requests: AtomicUsize,
    registrations: AtomicUsize,
    token_exchanges: AtomicUsize,
    tool_calls: AtomicUsize,
    scope_challenges: AtomicUsize,
    code_challenge: Mutex<Option<String>>,
    requested_scopes: Mutex<String>,
    challenge_next: AtomicBool,
}

struct Request {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let challenge_once = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--challenge-once") => true,
        Some(_) => return Err("usage: owned_oauth_server [--challenge-once]".into()),
    };
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let origin = format!("http://{}", listener.local_addr()?);
    let ready = json!({
        "endpoint": format!("{origin}/mcp"),
        "issuer": format!("{origin}/issuer"),
        "pre_registered_client_id": "owned-client",
        "stats": format!("{origin}/fixture/stats")
    });
    println!("{ready}");
    std::io::stdout().flush()?;
    let counters = Arc::new(Counters::default());
    counters
        .challenge_next
        .store(challenge_once, Ordering::SeqCst);
    for stream in listener.incoming() {
        let stream = stream?;
        let counters = counters.clone();
        let origin = origin.clone();
        std::thread::Builder::new()
            .name("owned-mcp-oauth-connection".into())
            .spawn(move || {
                let _ = serve(stream, &origin, &counters);
            })?;
    }
    Ok(())
}

fn serve(
    mut stream: TcpStream,
    origin: &str,
    counters: &Counters,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let request = read_request(&mut stream)?;
    let target = reqwest::Url::parse(&format!("{origin}{}", request.target))?;
    match (request.method.as_str(), target.path()) {
        ("GET", "/fixture/stats") => respond_json(
            &mut stream,
            200,
            json!({
                "protected_requests": counters.protected_requests.load(Ordering::SeqCst),
                "denied_requests": counters.denied_requests.load(Ordering::SeqCst),
                "registrations": counters.registrations.load(Ordering::SeqCst),
                "token_exchanges": counters.token_exchanges.load(Ordering::SeqCst),
                "tool_calls": counters.tool_calls.load(Ordering::SeqCst),
                "scope_challenges": counters.scope_challenges.load(Ordering::SeqCst),
            }),
        )?,
        ("GET", "/.well-known/oauth-protected-resource/mcp") => respond_json(
            &mut stream,
            200,
            json!({
                "resource": format!("{origin}/mcp"),
                "authorization_servers": [format!("{origin}/issuer")],
                "scopes_supported": ["tools:read", "tools:write"]
            }),
        )?,
        ("GET", "/.well-known/oauth-authorization-server/issuer") => respond_json(
            &mut stream,
            200,
            json!({
                "issuer": format!("{origin}/issuer"),
                "authorization_endpoint": format!("{origin}/authorize"),
                "token_endpoint": format!("{origin}/token"),
                "registration_endpoint": format!("{origin}/register"),
                "code_challenge_methods_supported": ["S256"],
                "authorization_response_iss_parameter_supported": true
            }),
        )?,
        ("POST", "/register") => {
            let body: Value = serde_json::from_slice(&request.body)?;
            let redirect = body["redirect_uris"]
                .as_array()
                .and_then(|uris| uris.first())
                .and_then(Value::as_str)
                .ok_or("missing redirect URI")?;
            validate_redirect(redirect)?;
            counters.registrations.fetch_add(1, Ordering::SeqCst);
            respond_json(
                &mut stream,
                201,
                json!({
                    "client_id": "owned-dcr-client",
                    "token_endpoint_auth_method": "none",
                    "redirect_uris": [redirect]
                }),
            )?;
        }
        ("GET", "/authorize") => {
            let query: HashMap<String, String> = target.query_pairs().into_owned().collect();
            let redirect = query.get("redirect_uri").ok_or("missing redirect")?;
            validate_redirect(redirect)?;
            let state = query.get("state").ok_or("missing state")?;
            let challenge = query
                .get("code_challenge")
                .ok_or("missing PKCE challenge")?;
            if query.get("code_challenge_method").map(String::as_str) != Some("S256")
                || query.get("resource").map(String::as_str) != Some(&format!("{origin}/mcp"))
            {
                return Err("invalid OAuth audience or PKCE method".into());
            }
            *counters.code_challenge.lock().map_err(|_| "fixture lock")? = Some(challenge.clone());
            let scopes = query.get("scope").cloned().unwrap_or_default();
            if scopes.len() > 256
                || scopes
                    .split_ascii_whitespace()
                    .any(|scope| !matches!(scope, "tools:read" | "tools:write"))
            {
                return Err("unsupported fixture scope".into());
            }
            *counters
                .requested_scopes
                .lock()
                .map_err(|_| "fixture lock")? = scopes;
            let mut callback = reqwest::Url::parse(redirect)?;
            callback
                .query_pairs_mut()
                .append_pair("code", "owned-code")
                .append_pair("state", state)
                .append_pair("iss", &format!("{origin}/issuer"));
            respond(
                &mut stream,
                302,
                "text/plain",
                b"",
                Some(("Location", callback.as_str())),
            )?;
        }
        ("POST", "/token") => {
            let body = std::str::from_utf8(&request.body)?;
            let url = reqwest::Url::parse(&format!("{origin}/?{body}"))?;
            let form: HashMap<String, String> = url.query_pairs().into_owned().collect();
            let valid_client = matches!(
                form.get("client_id").map(String::as_str),
                Some("owned-client" | "owned-dcr-client")
            );
            let grant = form.get("grant_type").map(String::as_str);
            let valid_grant = if grant == Some("authorization_code") {
                let verifier = form.get("code_verifier").ok_or("missing verifier")?;
                let digest = Sha256::digest(verifier.as_bytes());
                let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
                let expected = counters.code_challenge.lock().map_err(|_| "fixture lock")?;
                form.get("code").map(String::as_str) == Some("owned-code")
                    && expected.as_deref() == Some(encoded.as_str())
            } else {
                grant == Some("refresh_token")
                    && form.get("refresh_token").map(String::as_str)
                        == Some("owned-fixture-refresh")
            };
            if !valid_client || !valid_grant {
                respond_json(&mut stream, 400, json!({"error":"invalid_grant"}))?;
            } else {
                counters.token_exchanges.fetch_add(1, Ordering::SeqCst);
                let scopes = counters
                    .requested_scopes
                    .lock()
                    .map_err(|_| "fixture lock")?
                    .clone();
                respond_json(
                    &mut stream,
                    200,
                    json!({
                        "access_token": FIXTURE_ACCESS,
                        "refresh_token": "owned-fixture-refresh",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "scope": scopes
                    }),
                )?;
            }
        }
        ("POST", "/mcp") => {
            counters.protected_requests.fetch_add(1, Ordering::SeqCst);
            let authorized = request
                .headers
                .get("authorization")
                .is_some_and(|value| value == &format!("Bearer {FIXTURE_ACCESS}"));
            if !authorized {
                counters.denied_requests.fetch_add(1, Ordering::SeqCst);
                let challenge = format!(
                    "Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\", scope=\"tools:read\""
                );
                respond(
                    &mut stream,
                    401,
                    "text/plain",
                    b"",
                    Some(("WWW-Authenticate", &challenge)),
                )?;
                return Ok(());
            }
            let body: Value = serde_json::from_slice(&request.body)?;
            let result = match body["method"].as_str() {
                Some("server/discover") => json!({
                    "resultType":"complete", "supportedVersions":["2026-07-28"],
                    "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private"
                }),
                Some("tools/list") => json!({
                    "resultType":"complete", "ttlMs":0, "cacheScope":"private",
                    "tools":[{"name":"echo","description":"Owned E2E echo",
                        "inputSchema":{"type":"object","properties":{"echo":{"type":"string"}}}}]
                }),
                Some("tools/call") => {
                    if counters.challenge_next.swap(false, Ordering::SeqCst) {
                        counters.scope_challenges.fetch_add(1, Ordering::SeqCst);
                        let challenge = format!(
                            "Bearer error=\"insufficient_scope\", scope=\"tools:write\", resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\""
                        );
                        respond(
                            &mut stream,
                            403,
                            "text/plain",
                            b"",
                            Some(("WWW-Authenticate", &challenge)),
                        )?;
                        return Ok(());
                    }
                    counters.tool_calls.fetch_add(1, Ordering::SeqCst);
                    json!({"resultType":"complete", "isError":false,
                        "content":[{"type":"text","text":"owned-oauth-tool-ok"}]})
                }
                _ => {
                    respond_json(&mut stream, 404, json!({"error":"unknown_method"}))?;
                    return Ok(());
                }
            };
            respond_json(
                &mut stream,
                200,
                json!({"jsonrpc":"2.0", "id":body["id"], "result":result}),
            )?;
        }
        _ => respond_json(&mut stream, 404, json!({"error":"not_found"}))?,
    }
    Ok(())
}

fn validate_redirect(value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let url = reqwest::Url::parse(value)?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || url.path() != "/callback"
        || url.fragment().is_some()
    {
        return Err("only exact owned loopback callbacks are accepted".into());
    }
    Ok(())
}

fn read_request(stream: &mut TcpStream) -> Result<Request, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk)?;
        if count == 0 || bytes.len() + count > MAX_REQUEST {
            return Err("request truncated or too large".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end])?;
    let mut lines = header.split("\r\n");
    let mut first = lines
        .next()
        .ok_or("missing request line")?
        .split_ascii_whitespace();
    let method = first.next().ok_or("missing method")?.to_owned();
    let target = first.next().ok_or("missing target")?.to_owned();
    if first.next() != Some("HTTP/1.1") || first.next().is_some() || !target.starts_with('/') {
        return Err("invalid request line".into());
    }
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or_default();
    if content_length > MAX_REQUEST - header_end {
        return Err("body too large".into());
    }
    while bytes.len() - header_end < content_length {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk)?;
        if count == 0 || bytes.len() + count > MAX_REQUEST {
            return Err("body truncated or too large".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(Request {
        method,
        target,
        headers,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn respond_json(
    stream: &mut TcpStream,
    status: u16,
    value: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    respond(
        stream,
        status,
        "application/json",
        value.to_string().as_bytes(),
        None,
    )
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    extra: Option<(&str, &str)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let extra = extra
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .unwrap_or_default();
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}
