use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use vega_store::config::ProviderConfig;

const KEY: &str = "synthetic-r14-loopback-only";
const PI_KEY: &str = "fake-pi-agent-key-for-tests-only";

struct Fixture {
    _root: tempfile::TempDir,
    service: ProviderSettingsService,
    provider: ProviderConfig,
}
impl Fixture {
    fn new(base_url: String) -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("config");
        vega_store::keystore::set_key(&directory, "synthetic", KEY).unwrap();
        let service = ProviderSettingsService::new(directory.join("config.toml"));
        let provider = ProviderConfig {
            enabled: true,
            name: "owned".into(),
            base_url,
            key_ref: "synthetic".into(),
            models: vec!["configured-model".into()],
        };
        AppConfig {
            providers: vec![provider.clone()],
            ..Default::default()
        }
        .save_to(&service.config_path)
        .unwrap();
        Self {
            _root: root,
            service,
            provider,
        }
    }
    fn request(&self, action: ProviderNetworkAction) -> ProviderNetworkRequest {
        ProviderNetworkRequest {
            operation_id: 42,
            generation: 7,
            provider: self.provider.clone(),
            action,
        }
    }
}

/// Owned real HTTP boundary; no mock transport or override of service behavior.
fn server(
    status: u16,
    body: String,
    extra_headers: &str,
    delay: Duration,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let response = if extra_headers.contains("Transfer-Encoding: chunked") {
        format!(
            "HTTP/1.1 {status} Fixture\r\nConnection: close\r\n{extra_headers}\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
            body.len()
        )
    } else {
        format!(
            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
            body.len()
        )
    };
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let start = Instant::now();
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && start.elapsed() < Duration::from_secs(5) =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("owned loopback accept: {error}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut bytes = [0; 4096];
            let count = socket.read(&mut bytes).unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&bytes[..count]);
            let text = std::str::from_utf8(&request).unwrap();
            if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|length| length.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if body.len() >= length {
                    break;
                }
            }
        }
        thread::sleep(delay);
        let _ = socket.write_all(response.as_bytes());
        drop(socket);
        thread::sleep(Duration::from_millis(50));
        assert!(
            listener.accept().is_err(),
            "check must make exactly one request"
        );
        String::from_utf8(request).unwrap()
    });
    (base, handle)
}

/// Let the Tokio connection driver finish while joining the bounded fixture thread.
async fn finish_server(http: thread::JoinHandle<String>) -> String {
    tokio::task::spawn_blocking(move || http.join())
        .await
        .unwrap()
        .unwrap()
}

fn valid_stream() -> String {
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".into()
}

fn import_fixture() -> (
    tempfile::TempDir,
    ProviderSettingsService,
    ProviderConfig,
    std::path::PathBuf,
) {
    let root = tempfile::tempdir().unwrap();
    let config_path = root.path().join("vega").join("config.toml");
    let pi_path = root.path().join("pi").join("models.json");
    std::fs::create_dir_all(pi_path.parent().unwrap()).unwrap();
    let provider = ProviderConfig {
        enabled: false,
        name: "cpa".into(),
        base_url: "https://cpa.example.test/v1".into(),
        key_ref: "cpa".into(),
        models: vec!["glm-5.3-flash".into()],
    };
    AppConfig {
        providers: vec![
            provider.clone(),
            ProviderConfig {
                enabled: true,
                name: "unrelated".into(),
                base_url: "https://other.example.test/v1".into(),
                key_ref: "unrelated".into(),
                models: vec!["other-model".into()],
            },
        ],
        ..Default::default()
    }
    .save_to(&config_path)
    .unwrap();
    vega_store::keystore::set_key(
        root.path().join("vega").as_path(),
        "unrelated",
        "old-unrelated-fake-key",
    )
    .unwrap();
    write_pi_source(
        &pi_path,
        serde_json::json!({
            "providers": {
                "cpa": {
                    "api": "openai-completions",
                    "baseUrl": provider.base_url.clone(),
                    "apiKey": PI_KEY,
                    "models": [{"id": "glm-5.3-flash"}]
                },
                "unrelated": {
                    "api": "other-api",
                    "baseUrl": "https://other.example.test/v1",
                    "apiKey": "other-pi-fake-key",
                    "models": [{"id": "other-model"}]
                }
            }
        }),
    );
    let service = ProviderSettingsService::with_pi_models_path(config_path, pi_path.clone());
    (root, service, provider, pi_path)
}

fn write_pi_source(path: &Path, document: serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn assert_config_unchanged(service: &ProviderSettingsService, before: &[u8]) {
    assert_eq!(std::fs::read(&service.config_path).unwrap(), before);
    assert!(!format!("{service:?}").contains(PI_KEY));
}

#[test]
fn production_pi_import_stores_only_selected_credential_enables_provider_and_preserves_others() {
    let (root, service, provider, pi_path) = import_fixture();
    let source_before = std::fs::read(&pi_path).unwrap();
    let saved = service.import_pi_credential(provider.clone()).unwrap();
    assert!(saved.providers[0].enabled);
    assert_eq!(saved.providers[0].models, provider.models);
    assert_eq!(saved.providers[1].name, "unrelated");
    assert!(saved.providers[1].enabled);
    assert_eq!(saved.providers[1].models, ["other-model"]);
    assert_eq!(saved.providers[1].key_ref, "unrelated");
    assert_eq!(
        vega_store::keystore::get_key(root.path().join("vega").as_path(), "cpa").unwrap(),
        PI_KEY
    );
    assert_eq!(
        vega_store::keystore::get_key(root.path().join("vega").as_path(), "unrelated").unwrap(),
        "old-unrelated-fake-key"
    );
    assert!(
        !std::fs::read_to_string(&service.config_path)
            .unwrap()
            .contains(PI_KEY)
    );
    assert_eq!(std::fs::read(&pi_path).unwrap(), source_before);
    assert!(!format!("{saved:?}").contains(PI_KEY));
}

#[test]
fn production_pi_import_rejects_stale_snapshot_without_mutation() {
    let (root, service, provider, _pi_path) = import_fixture();
    let before = std::fs::read(&service.config_path).unwrap();
    let mut changed = service.load().unwrap();
    changed.providers[0].models = vec!["changed-model".into()];
    changed.save_to(&service.config_path).unwrap();
    let current = std::fs::read(&service.config_path).unwrap();
    assert_eq!(
        service.import_pi_credential(provider),
        Err(ProviderSettingsError::Conflict)
    );
    assert_eq!(std::fs::read(&service.config_path).unwrap(), current);
    assert_eq!(
        vega_store::keystore::get_key(root.path().join("vega").as_path(), "cpa"),
        Err(vega_store::keystore::Error::Missing)
    );
    assert_ne!(before, current);
}

#[test]
fn production_pi_import_rejects_selected_entry_validation_failures_without_mutation() {
    let cases = [
        (
            "missing-key",
            serde_json::json!({
                "api": "openai-completions",
                "baseUrl": "https://cpa.example.test/v1",
                "models": [{"id": "glm-5.3-flash"}]
            }),
        ),
        (
            "wrong-api",
            serde_json::json!({
                "api": "anthropic-messages",
                "baseUrl": "https://cpa.example.test/v1",
                "apiKey": PI_KEY,
                "models": [{"id": "glm-5.3-flash"}]
            }),
        ),
        (
            "wrong-url",
            serde_json::json!({
                "api": "openai-completions",
                "baseUrl": "https://wrong.example.test/v1",
                "apiKey": PI_KEY,
                "models": [{"id": "glm-5.3-flash"}]
            }),
        ),
        (
            "no-shared-model",
            serde_json::json!({
                "api": "openai-completions",
                "baseUrl": "https://cpa.example.test/v1",
                "apiKey": PI_KEY,
                "models": [{"id": "different-model"}]
            }),
        ),
    ];
    for (label, selected) in cases {
        let (_root, service, provider, pi_path) = import_fixture();
        let before = std::fs::read(&service.config_path).unwrap();
        write_pi_source(
            &pi_path,
            serde_json::json!({"providers": {"cpa": selected}}),
        );
        let error = service.import_pi_credential(provider).unwrap_err();
        assert_eq!(error, ProviderSettingsError::Invalid, "{label}");
        assert_config_unchanged(&service, &before);
        assert!(!format!("{error:?} {error}").contains(PI_KEY));
    }
}

#[cfg(unix)]
#[test]
fn production_pi_import_rejects_missing_oversized_symlink_nonregular_and_insecure_sources() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let (_root, service, provider, pi_path) = import_fixture();
    let before = std::fs::read(&service.config_path).unwrap();
    std::fs::remove_file(&pi_path).unwrap();
    assert_eq!(
        service.import_pi_credential(provider.clone()),
        Err(ProviderSettingsError::PiSource)
    );
    assert_config_unchanged(&service, &before);

    let target = pi_path.with_extension("target");
    write_pi_source(&target, serde_json::json!({"providers": {}}));
    symlink(&target, &pi_path).unwrap();
    assert_eq!(
        service.import_pi_credential(provider.clone()),
        Err(ProviderSettingsError::PiSource)
    );
    std::fs::remove_file(&pi_path).unwrap();

    std::fs::create_dir(&pi_path).unwrap();
    assert_eq!(
        service.import_pi_credential(provider.clone()),
        Err(ProviderSettingsError::PiSource)
    );
    std::fs::remove_dir(&pi_path).unwrap();

    std::fs::write(&pi_path, vec![b'x'; PI_MODELS_MAX_BYTES as usize + 1]).unwrap();
    std::fs::set_permissions(&pi_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        service.import_pi_credential(provider.clone()),
        Err(ProviderSettingsError::PiSource)
    );

    write_pi_source(
        &pi_path,
        serde_json::json!({"providers": {"cpa": {
            "api": "openai-completions",
            "baseUrl": "https://cpa.example.test/v1",
            "apiKey": PI_KEY,
            "models": [{"id": "glm-5.3-flash"}]
        }}}),
    );
    std::fs::set_permissions(&pi_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        service.import_pi_credential(provider),
        Err(ProviderSettingsError::PiSource)
    );
    assert_config_unchanged(&service, &before);
}

#[test]
fn production_pi_import_rolls_back_key_when_config_save_fails() {
    let (root, service, provider, _pi_path) = import_fixture();
    let config_before = std::fs::read(&service.config_path).unwrap();
    vega_store::keystore::set_key(
        root.path().join("vega").as_path(),
        "cpa",
        "old-cpa-fake-key",
    )
    .unwrap();
    std::fs::create_dir(service.config_path.with_extension("toml.tmp")).unwrap();
    assert_eq!(
        service.import_pi_credential(provider),
        Err(ProviderSettingsError::Config)
    );
    assert_eq!(std::fs::read(&service.config_path).unwrap(), config_before);
    assert_eq!(
        vega_store::keystore::get_key(root.path().join("vega").as_path(), "cpa").unwrap(),
        "old-cpa-fake-key"
    );
    std::fs::remove_dir(service.config_path.with_extension("toml.tmp")).unwrap();
    assert!(
        !std::fs::read_to_string(&service.config_path)
            .unwrap()
            .contains(PI_KEY)
    );
}

#[tokio::test]
async fn production_discovery_explicit_import_probe_and_persistent_patch() {
    let (base, http) = server(
        200,
        r#"{"data":[{"id":"new-model"},{"id":"configured-model"},{"id":"new-model"}]}"#.into(),
        "",
        Duration::ZERO,
    );
    let fixture = Fixture::new(base);
    let before = std::fs::read(&fixture.service.config_path).unwrap();
    let request = fixture.request(ProviderNetworkAction::DiscoverModels);
    let result = fixture
        .service
        .network(request.clone(), CancellationToken::new())
        .await;
    assert_eq!(result.request, request);
    assert_eq!(
        result.outcome,
        Ok(ProviderNetworkOutcome::Models(vec![
            "new-model".into(),
            "configured-model".into()
        ]))
    );
    assert_eq!(
        std::fs::read(&fixture.service.config_path).unwrap(),
        before,
        "network must never mutate config"
    );
    let received = finish_server(http).await;
    assert!(received.starts_with("GET /v1/models HTTP/1.1"));
    assert!(
        received
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {KEY}"))
    );
    let config = fixture
        .service
        .patch(ProviderPatchRequest {
            provider: fixture.provider.clone(),
            action: ProviderPatchAction::ImportModels(vec![
                "new-model".into(),
                "configured-model".into(),
            ]),
        })
        .unwrap();
    assert_eq!(
        config.providers[0].models,
        ["configured-model", "new-model"]
    );
    assert_eq!(fixture.service.load().unwrap(), config);

    let (base, http) = server(
        200,
        valid_stream(),
        "Content-Type: text/event-stream\r\n",
        Duration::ZERO,
    );
    let fixture = Fixture::new(base);
    let result = fixture
        .service
        .network(
            fixture.request(ProviderNetworkAction::TestModel {
                model: "configured-model".into(),
            }),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        result.outcome,
        Ok(ProviderNetworkOutcome::ModelTestSucceeded)
    );
    let received = finish_server(http).await;
    assert!(received.starts_with("POST /v1/chat/completions HTTP/1.1"));
    let (_, body) = received.split_once("\r\n\r\n").unwrap();
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        body["messages"],
        serde_json::json!([{"role":"user","content":"Reply with OK."}])
    );
    assert_eq!(body["model"], "configured-model");
    assert_eq!(body["max_tokens"], 128);
    assert_eq!(body["stream"], true);
    assert!(body.get("tools").is_none());
    assert!(!body.to_string().contains(KEY));
}

#[tokio::test]
async fn production_failures_are_bounded_content_free_and_never_retry() {
    let cases = [
        (
            401,
            format!("secret echo: {KEY}"),
            ProviderSettingsError::Http(401),
        ),
        (200, String::new(), ProviderSettingsError::Malformed),
        (
            200,
            "not-json-or-sse".into(),
            ProviderSettingsError::Malformed,
        ),
        (
            200,
            "data: [DONE]\n\n".into(),
            ProviderSettingsError::Malformed,
        ),
        (
            200,
            "x".repeat(1024 * 1024 + 1),
            ProviderSettingsError::Limit,
        ),
        (302, String::new(), ProviderSettingsError::Http(302)),
    ];
    for (status, body, expected) in cases {
        let (base, http) = server(
            status,
            body,
            "Location: http://127.0.0.1:1/forbidden\r\n",
            Duration::ZERO,
        );
        let fixture = Fixture::new(base);
        let result = fixture
            .service
            .network(
                fixture.request(ProviderNetworkAction::TestModel {
                    model: "configured-model".into(),
                }),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(result.outcome, Err(expected));
        assert!(!format!("{result:?}").contains(KEY));
        finish_server(http).await;
    }
}

#[tokio::test]
async fn production_discovery_parser_limits_and_empty_catalog() {
    for (body, expected) in [
        (
            r#"{"data":[]}"#.into(),
            Ok(ProviderNetworkOutcome::Models(vec![])),
        ),
        (
            r#"{"data":[{"id":" "}]}"#.into(),
            Err(ProviderSettingsError::Malformed),
        ),
        (
            serde_json::json!({"data": [{"id": "m".repeat(201)}]}).to_string(),
            Err(ProviderSettingsError::Malformed),
        ),
        (
            serde_json::json!({"data": vec![serde_json::json!({"id":"m"}); 1001]}).to_string(),
            Err(ProviderSettingsError::Limit),
        ),
        (
            serde_json::json!({"data": [{"id":KEY}]}).to_string(),
            Err(ProviderSettingsError::Malformed),
        ),
    ] {
        let (base, http) = server(200, body, "", Duration::ZERO);
        let fixture = Fixture::new(base);
        let result = fixture
            .service
            .network(
                fixture.request(ProviderNetworkAction::DiscoverModels),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(result.outcome, expected);
        assert!(!format!("{result:?}").contains(KEY));
        finish_server(http).await;
    }
}

#[tokio::test]
async fn production_cancel_and_total_deadline() {
    let (base, http) = server(200, valid_stream(), "", Duration::from_millis(150));
    let fixture = Fixture::new(base);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(75)).await;
        worker_cancel.cancel();
    });
    let result = fixture
        .service
        .network(
            fixture.request(ProviderNetworkAction::DiscoverModels),
            cancel,
        )
        .await;
    assert_eq!(result.outcome, Err(ProviderSettingsError::Cancelled));
    finish_server(http).await;

    let (base, http) = server(200, valid_stream(), "", Duration::from_millis(15_200));
    let fixture = Fixture::new(base);
    let start = Instant::now();
    let result = fixture
        .service
        .network(
            fixture.request(ProviderNetworkAction::DiscoverModels),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(result.outcome, Err(ProviderSettingsError::Timeout));
    assert!(start.elapsed() < Duration::from_secs(17));
    finish_server(http).await;
}

#[tokio::test]
async fn production_rejects_missing_credentials_disabled_stale_and_unsafe_urls_before_network() {
    let fixture = Fixture::new("http://127.0.0.1:1".into());
    vega_store::keystore::delete_key(fixture.service.config_path.parent().unwrap(), "synthetic")
        .unwrap();
    let result = fixture
        .service
        .network(
            fixture.request(ProviderNetworkAction::DiscoverModels),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(result.outcome, Err(ProviderSettingsError::Credential));
    let mut config = fixture
        .service
        .patch(ProviderPatchRequest {
            provider: fixture.provider.clone(),
            action: ProviderPatchAction::SetEnabled(false),
        })
        .unwrap();
    let mut request = fixture.request(ProviderNetworkAction::DiscoverModels);
    assert_eq!(
        fixture
            .service
            .network(request.clone(), CancellationToken::new())
            .await
            .outcome,
        Err(ProviderSettingsError::Conflict)
    );
    request.provider = config.providers[0].clone();
    assert_eq!(
        fixture
            .service
            .network(request.clone(), CancellationToken::new())
            .await
            .outcome,
        Err(ProviderSettingsError::Disabled)
    );
    for base in [
        "http://user:secret@127.0.0.1",
        "http://@127.0.0.1",
        "http://127.0.0.1?k=secret",
        "http://127.0.0.1/#secret",
        "file:///tmp/not-http",
    ] {
        config.providers[0].enabled = true;
        config.providers[0].base_url = base.into();
        config.save_to(&fixture.service.config_path).unwrap();
        request.provider = config.providers[0].clone();
        assert_eq!(
            fixture
                .service
                .network(request.clone(), CancellationToken::new())
                .await
                .outcome,
            Err(ProviderSettingsError::Invalid)
        );
        assert!(!format!("{request:?}").contains("secret"));
    }
}

#[test]
fn production_patch_preserves_other_fields_rejects_conflicts_and_persists_order() {
    let fixture = Fixture::new("http://127.0.0.1:1".into());
    let mut config = fixture.service.load().unwrap();
    let mut other = fixture.provider.clone();
    other.name = "other".into();
    other.key_ref = "other-ref".into();
    config.providers.push(other.clone());
    config.defaults.model = "unchanged-default".into();
    config.ui.theme = "light".into();
    config.save_to(&fixture.service.config_path).unwrap();
    let order = vec!["owned".into(), "other".into()];
    let reversed = vec!["other".into(), "owned".into()];
    let request = ProviderPatchRequest {
        provider: fixture.provider.clone(),
        action: ProviderPatchAction::Move {
            expected_order: order,
            ordered_names: reversed,
        },
    };
    let saved = fixture.service.patch(request.clone()).unwrap();
    assert_eq!(saved.providers, [other, fixture.provider.clone()]);
    assert_eq!(saved.defaults, config.defaults);
    assert_eq!(saved.ui, config.ui);
    assert_eq!(
        fixture.service.patch(request),
        Err(ProviderSettingsError::Conflict)
    );
    let before = std::fs::read(&fixture.service.config_path).unwrap();
    assert_eq!(
        fixture.service.patch(ProviderPatchRequest {
            provider: fixture.provider.clone(),
            action: ProviderPatchAction::EditModels(vec!["duplicate".into(), "duplicate".into()])
        }),
        Err(ProviderSettingsError::Invalid)
    );
    assert_eq!(std::fs::read(&fixture.service.config_path).unwrap(), before);
    let saved = fixture
        .service
        .patch(ProviderPatchRequest {
            provider: fixture.provider.clone(),
            action: ProviderPatchAction::SetEnabled(false),
        })
        .unwrap();
    assert!(!saved.providers[1].enabled);
    assert_eq!(saved.providers[1].models, fixture.provider.models);
    assert_eq!(saved.providers[1].key_ref, fixture.provider.key_ref);
    assert_eq!(fixture.service.load().unwrap(), saved);
    assert!(ProviderConfig::default().enabled);
    let old = std::fs::read_to_string(&fixture.service.config_path)
        .unwrap()
        .replace("enabled = false\n", "")
        .replace("enabled = true\n", "");
    std::fs::write(&fixture.service.config_path, old).unwrap();
    assert!(
        fixture
            .service
            .load()
            .unwrap()
            .providers
            .iter()
            .all(|p| p.enabled)
    );
}

#[tokio::test]
async fn production_completion_rechecks_external_provider_changes() {
    let (base, http) = server(200, valid_stream(), "", Duration::from_millis(350));
    let fixture = Fixture::new(base);
    let service = fixture.service.clone();
    let request = fixture.request(ProviderNetworkAction::TestModel {
        model: "configured-model".into(),
    });
    let worker =
        tokio::spawn(async move { service.network(request, CancellationToken::new()).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    let mut config = fixture.service.load().unwrap();
    config.providers[0].enabled = false;
    config.save_to(&fixture.service.config_path).unwrap();
    assert_eq!(
        worker.await.unwrap().outcome,
        Err(ProviderSettingsError::Conflict)
    );
    finish_server(http).await;
    assert!(!fixture.service.load().unwrap().providers[0].enabled);
}

#[tokio::test]
async fn production_probe_decodes_multiline_sse_and_completed_reasoning() {
    let body = "data: {\"choices\":\ndata: [{\"index\":0,\"delta\":{\"reasoning_content\":\"connected\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    let (base, http) = server(200, body.into(), "", Duration::ZERO);
    let fixture = Fixture::new(base);
    let result = fixture
        .service
        .network(
            fixture.request(ProviderNetworkAction::TestModel {
                model: "configured-model".into(),
            }),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        result.outcome,
        Ok(ProviderNetworkOutcome::ModelTestSucceeded)
    );
    assert!(!format!("{result:?}").contains("connected"));
    finish_server(http).await;
}

#[test]
fn production_form_edits_share_patch_authority_and_rollback_credentials_on_config_failure() {
    let fixture = Fixture::new("http://127.0.0.1:1".into());
    let mut config = fixture.service.load().unwrap();
    config.providers[0].enabled = false;
    config.ui.theme = "light".into();
    config.save_to(&fixture.service.config_path).unwrap();
    let baseline = config.providers[0].clone();
    let mut edit = baseline.clone();
    edit.enabled = true; // Form code cannot accidentally turn a disabled provider on.
    edit.key_ref = "untrusted-replacement".into();
    edit.name = "renamed".into();
    let saved = fixture
        .service
        .save_provider(Some(baseline.clone()), edit.clone(), None)
        .unwrap();
    assert!(!saved.providers[0].enabled);
    assert_eq!(saved.providers[0].key_ref, "synthetic");
    assert_eq!(saved.ui, config.ui);
    assert_eq!(
        fixture.service.save_provider(Some(baseline), edit, None),
        Err(ProviderSettingsError::Conflict)
    );
    let before = std::fs::read(&fixture.service.config_path).unwrap();
    // A real write failure after credential persistence must restore the former key.
    std::fs::create_dir(fixture.service.config_path.with_extension("toml.tmp")).unwrap();
    let current = saved.providers[0].clone();
    assert_eq!(
        fixture.service.save_provider(
            Some(current.clone()),
            current,
            Some("synthetic-new-key".into())
        ),
        Err(ProviderSettingsError::Config)
    );
    assert_eq!(std::fs::read(&fixture.service.config_path).unwrap(), before);
    assert!(
        vega_store::keystore::get_key(fixture.service.config_path.parent().unwrap(), "synthetic")
            .unwrap()
            == KEY
    );
}

#[tokio::test]
async fn production_chunked_body_limit_without_content_length() {
    let (base, http) = server(
        200,
        "x".repeat(1024 * 1024 + 1),
        "Transfer-Encoding: chunked\r\n",
        Duration::ZERO,
    );
    let fixture = Fixture::new(base);
    let result = fixture
        .service
        .network(
            fixture.request(ProviderNetworkAction::DiscoverModels),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(result.outcome, Err(ProviderSettingsError::Limit));
    finish_server(http).await;
}

#[test]
fn production_concurrent_provider_default_and_sidebar_fields_survive_shared_edit() {
    let fixture = Fixture::new("http://127.0.0.1:1".into());
    let gate = std::sync::Arc::new(std::sync::Barrier::new(4));
    let provider_worker = {
        let gate = gate.clone();
        let service = fixture.service.clone();
        let provider = fixture.provider.clone();
        thread::spawn(move || {
            gate.wait();
            service
                .patch(ProviderPatchRequest {
                    provider,
                    action: ProviderPatchAction::SetEnabled(false),
                })
                .unwrap();
        })
    };
    let default_worker = {
        let gate = gate.clone();
        let path = fixture.service.config_path.clone();
        thread::spawn(move || {
            gate.wait();
            config::update_from(&path, |config| config.defaults.model = "new-default".into())
                .unwrap();
        })
    };
    let sidebar_worker = {
        let gate = gate.clone();
        let path = fixture.service.config_path.clone();
        thread::spawn(move || {
            gate.wait();
            config::update_from(&path, |config| config.ui.sidebar_collapsed = true).unwrap();
        })
    };
    gate.wait();
    provider_worker.join().unwrap();
    default_worker.join().unwrap();
    sidebar_worker.join().unwrap();
    let saved = fixture.service.load().unwrap();
    assert!(!saved.providers[0].enabled);
    assert_eq!(saved.providers[0].models, fixture.provider.models);
    assert_eq!(saved.defaults.model, "new-default");
    assert!(saved.ui.sidebar_collapsed);
    assert!(
        !fixture
            .service
            .config_path
            .with_extension("toml.tmp")
            .exists()
    );
    assert!(
        vega_store::keystore::get_key(fixture.service.config_path.parent().unwrap(), "synthetic")
            .unwrap()
            == KEY
    );
}
