use super::*;
use std::sync::{Arc, Mutex};
use std::{thread, time::Duration};
use tokio::time::Instant;
use vega_store::config::ProviderConfig;

const KEY: &str = "synthetic-r14-loopback-only";

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
    fn with_transport(transport: provider_check::mock::Transport) -> Self {
        let mut fixture = Self::new("http://fixture.invalid/v1".into());
        fixture.service = fixture.service.with_test_transport(transport);
        fixture
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

type Captured = Arc<Mutex<Vec<String>>>;
fn server(
    status: u16,
    body: String,
    extra_headers: &str,
    delay: Duration,
) -> (provider_check::mock::Transport, Captured) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();
    let headers = extra_headers.to_owned();
    let transport = provider_check::mock::Transport(Arc::new(move |request| {
        let mut encoded = format!("{} {} HTTP/1.1\r\n", request.method(), request.url().path());
        for (name, value) in request.headers() {
            encoded.push_str(&format!("{name}: {}\r\n", value.to_str().unwrap()));
        }
        encoded.push_str("\r\n");
        encoded.push_str(
            std::str::from_utf8(
                request
                    .body()
                    .and_then(|body| body.as_bytes())
                    .unwrap_or_default(),
            )
            .unwrap(),
        );
        requests.lock().unwrap().push(encoded);
        let body = body.clone();
        let headers = headers.clone();
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(provider_check::mock::response(
                status,
                &headers,
                body.into_bytes(),
            ))
        })
    }));
    (transport, captured)
}
async fn finish_server(captured: Captured) -> String {
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1, "check must make exactly one request");
    requests[0].clone()
}
async fn wait_for_request(captured: &Captured) {
    let start = std::time::Instant::now();
    while captured.lock().unwrap().is_empty() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "request must begin"
        );
        tokio::task::yield_now().await;
    }
}
fn valid_stream() -> String {
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".into()
}

#[test]
fn issue81_provider_lifecycle_preserves_manual_credentials_and_enabled_state() {
    let root = tempfile::tempdir().unwrap();
    let config_path = root.path().join("config.toml");
    AppConfig::default().save_to(&config_path).unwrap();
    let service = ProviderSettingsService::new(config_path.clone());
    let provider = ProviderConfig {
        enabled: true,
        name: "manual-provider".into(),
        base_url: "https://manual.example.test/v1".into(),
        key_ref: "untrusted-key-ref".into(),
        models: vec!["manual-model".into()],
    };
    let saved = service
        .save_provider(None, provider.clone(), Some("manual-entry-secret".into()))
        .unwrap();
    assert!(saved.providers[0].enabled);
    assert_eq!(saved.providers[0].key_ref, "manual-provider");
    assert_eq!(
        vega_store::keystore::get_key(root.path(), "manual-provider").unwrap(),
        "manual-entry-secret"
    );
    let before = std::fs::read(&config_path).unwrap();
    let loaded = service.load().unwrap();
    assert_eq!(loaded, saved);
    assert_eq!(std::fs::read(&config_path).unwrap(), before);

    let edited = service
        .save_provider(
            Some(saved.providers[0].clone()),
            saved.providers[0].clone(),
            None,
        )
        .unwrap();
    assert!(edited.providers[0].enabled);
    assert_eq!(edited.providers[0].key_ref, "manual-provider");
    assert_eq!(
        vega_store::keystore::get_key(root.path(), "manual-provider").unwrap(),
        "manual-entry-secret"
    );

    let disabled = service
        .patch(ProviderPatchRequest {
            provider: edited.providers[0].clone(),
            action: ProviderPatchAction::SetEnabled(false),
        })
        .unwrap();
    assert!(!disabled.providers[0].enabled);
    assert_eq!(
        vega_store::keystore::get_key(root.path(), "manual-provider").unwrap(),
        "manual-entry-secret"
    );
    let enabled = service
        .patch(ProviderPatchRequest {
            provider: disabled.providers[0].clone(),
            action: ProviderPatchAction::SetEnabled(true),
        })
        .unwrap();
    assert!(enabled.providers[0].enabled);
    assert_eq!(
        vega_store::keystore::get_key(root.path(), "manual-provider").unwrap(),
        "manual-entry-secret"
    );
}

#[tokio::test(start_paused = true)]
async fn production_discovery_explicit_import_probe_and_persistent_patch() {
    let (base, http) = server(
        200,
        r#"{"data":[{"id":"new-model"},{"id":"configured-model"},{"id":"new-model"}]}"#.into(),
        "",
        Duration::ZERO,
    );
    let fixture = Fixture::with_transport(base);
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
    let fixture = Fixture::with_transport(base);
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

#[tokio::test(start_paused = true)]
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
        let fixture = Fixture::with_transport(base);
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

#[tokio::test(start_paused = true)]
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
        let fixture = Fixture::with_transport(base);
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

#[tokio::test(start_paused = true)]
async fn production_cancel_and_total_deadline() {
    let (base, http) = server(200, valid_stream(), "", Duration::from_millis(150));
    let fixture = Fixture::with_transport(base);
    let cancel = CancellationToken::new();
    let service = fixture.service.clone();
    let request = fixture.request(ProviderNetworkAction::DiscoverModels);
    let worker_cancel = cancel.clone();
    let worker = tokio::spawn(async move { service.network(request, worker_cancel).await });
    wait_for_request(&http).await;
    cancel.cancel();
    let result = worker.await.unwrap();
    assert_eq!(result.outcome, Err(ProviderSettingsError::Cancelled));
    finish_server(http).await;

    let (base, http) = server(200, valid_stream(), "", Duration::from_millis(15_200));
    let fixture = Fixture::with_transport(base);
    let start = Instant::now();
    let result = fixture
        .service
        .network(
            fixture.request(ProviderNetworkAction::DiscoverModels),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(result.outcome, Err(ProviderSettingsError::Timeout));
    assert!(start.elapsed() >= Duration::from_secs(15));
    assert!(start.elapsed() < Duration::from_secs(17));
    finish_server(http).await;
}

#[tokio::test(start_paused = true)]
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

#[tokio::test(start_paused = true)]
async fn production_completion_rechecks_external_provider_changes() {
    let (base, http) = server(200, valid_stream(), "", Duration::from_millis(350));
    let fixture = Fixture::with_transport(base);
    let service = fixture.service.clone();
    let request = fixture.request(ProviderNetworkAction::TestModel {
        model: "configured-model".into(),
    });
    let worker =
        tokio::spawn(async move { service.network(request, CancellationToken::new()).await });
    wait_for_request(&http).await;
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

#[tokio::test(start_paused = true)]
async fn production_probe_decodes_multiline_sse_and_completed_reasoning() {
    let body = "data: {\"choices\":\ndata: [{\"index\":0,\"delta\":{\"reasoning_content\":\"connected\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    let (base, http) = server(200, body.into(), "", Duration::ZERO);
    let fixture = Fixture::with_transport(base);
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

#[tokio::test(start_paused = true)]
async fn production_chunked_body_limit_without_content_length() {
    let (base, http) = server(
        200,
        "x".repeat(1024 * 1024 + 1),
        "Transfer-Encoding: chunked\r\n",
        Duration::ZERO,
    );
    let fixture = Fixture::with_transport(base);
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
