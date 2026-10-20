use super::*;
use futures::StreamExt;
use vega_runtime::{ChatMessage, ChatRequest, ChatRole, ProviderEvent};
use vega_store::model_capabilities::{ModelCapabilityConfig, ModelCapabilityRecord};

#[path = "../../vega_runtime/src/openai/compatibility_tests/http.rs"]
mod http;
use http::{HttpFixture, Reply};

fn owned(
    root: &std::path::Path,
    endpoint: &str,
    api: vega_store::config::ProviderApi,
) -> (Store, Thread, std::path::PathBuf) {
    let path = root.join("config.toml");
    vega_store::config::AppConfig {
        providers: vec![vega_store::config::ProviderConfig {
            name: "fixture".into(),
            base_url: endpoint.into(),
            models: vec!["future".into()],
            key_ref: "fixture-key".into(),
            api,
            enabled: true,
        }],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    vega_store::keystore::set_key(root, "fixture-key", "owned-fixture-key").unwrap();
    let store = Store::open(root.join("store.db")).unwrap();
    store.migrate().unwrap();
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread",
            project_id: "",
            title: "Fixture",
            mode: "execute",
            permission_mode: "readonly",
            model: "future",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    let thread = vega_conversation::threads::get_thread(&store, "thread").unwrap();
    (store, thread, path)
}

fn capability(root: &std::path::Path, image: Option<bool>, efforts: Option<Vec<String>>) {
    std::fs::write(
        root.join("model-capabilities.toml"),
        vega_store::model_capabilities::encode(&ModelCapabilityConfig {
            version: 1,
            models: vec![ModelCapabilityRecord {
                provider: "fixture".into(),
                model: "future".into(),
                allowed_efforts: efforts,
                image_input: image,
            }],
        })
        .unwrap(),
    )
    .unwrap();
}

fn completed(api: vega_store::config::ProviderApi) -> Vec<serde_json::Value> {
    if api == vega_store::config::ProviderApi::Responses {
        vec![
            serde_json::json!({"type":"response.completed","response":{"id":"r","status":"completed","output":[{"type":"message","id":"m","role":"assistant","content":[{"type":"output_text","text":"Complete"}]}]}}),
        ]
    } else {
        vec![
            serde_json::json!({"choices":[{"delta":{"content":"Complete"},"finish_reason":""}]}),
            serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
        ]
    }
}

#[tokio::test]
async fn commit_provider_factory_binds_actual_model_overrides_for_default_requests() {
    for api in [
        vega_store::config::ProviderApi::ChatCompletions,
        vega_store::config::ProviderApi::Responses,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut server = HttpFixture::new(vec![Reply::events(&completed(api))]).await;
        let (_, thread, path) = owned(dir.path(), &server.base, api);
        capability(dir.path(), Some(true), Some(vec!["low".into()]));
        let provider = commit_provider(&thread, Some(&path)).unwrap();
        let mut user = ChatMessage::new(ChatRole::User, "Describe the pixel");
        user.images.push(
            vega_runtime::ImageAttachment::from_bytes(
                include_bytes!(
                    "../../vega_runtime/src/openai/compatibility_tests/fixtures/red.png"
                )
                .to_vec(),
            )
            .unwrap(),
        );
        let mut stream = provider
            .chat_stream_once(
                ChatRequest {
                    model: "future".into(),
                    messages: vec![user],
                    ..Default::default()
                },
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            if let ProviderEvent::TextDelta(delta) = event.unwrap() {
                text.push_str(&delta);
            }
        }
        assert_eq!(text, "Complete");
        server.finish().await;
        assert_eq!(server.received.lock().unwrap().len(), 1);
        assert_eq!(
            server.received.lock().unwrap()[0].path,
            if api == vega_store::config::ProviderApi::Responses {
                "/v1/responses"
            } else {
                "/v1/chat/completions"
            }
        );
        assert!(
            server.received.lock().unwrap()[0]
                .body
                .to_string()
                .contains("data:image/png;base64,")
        );
        assert!(
            server.received.lock().unwrap()[0]
                .body
                .get("reasoning_effort")
                .is_none()
        );
        assert!(
            server.received.lock().unwrap()[0]
                .body
                .get("reasoning")
                .is_none()
        );
    }
}

#[tokio::test]
async fn commit_provider_factory_rejects_unsupported_images_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let server = HttpFixture::new(Vec::new()).await;
    let (_, thread, path) = owned(
        dir.path(),
        &server.base,
        vega_store::config::ProviderApi::ChatCompletions,
    );
    capability(dir.path(), Some(false), None);
    let provider = commit_provider(&thread, Some(&path)).unwrap();
    let mut user = ChatMessage::new(ChatRole::User, "Image");
    user.images.push(
        vega_runtime::ImageAttachment::from_bytes(
            include_bytes!("../../vega_runtime/src/openai/compatibility_tests/fixtures/red.png")
                .to_vec(),
        )
        .unwrap(),
    );
    assert!(
        provider
            .chat_stream_once(
                ChatRequest {
                    model: "future".into(),
                    messages: vec![user],
                    ..Default::default()
                },
                tokio_util::sync::CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert!(server.received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn real_agent_worker_resolves_saved_capabilities_and_default_selection_before_sending() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = HttpFixture::new(vec![Reply::events(&completed(
        vega_store::config::ProviderApi::ChatCompletions,
    ))])
    .await;
    let (store, thread, path) = owned(
        dir.path(),
        &server.base,
        vega_store::config::ProviderApi::ChatCompletions,
    );
    capability(dir.path(), Some(true), Some(vec!["low".into()]));
    let database = dir.path().join("store.db");
    let project = dir.path().to_path_buf();
    let (sender, receiver) = mpsc::sync_channel(4096);
    let image = vega_conversation::types::ImageAttachment::from_bytes(
        include_bytes!("../../vega_runtime/src/openai/compatibility_tests/fixtures/red.png")
            .to_vec(),
    )
    .unwrap();
    let worker = tokio::task::spawn_blocking(move || {
        run_agent_worker_with_mcp(
            database,
            project,
            thread,
            PendingAgentRun::UserMessage(UserSubmission {
                content: "Describe image".into(),
                images: vec![image],
            }),
            vega_conversation::agent::PermissionQueue::new(),
            vega_conversation::agent::UserQuestionQueue::new(),
            tokio_util::sync::CancellationToken::new(),
            sender,
            None,
            Some(path),
            None,
            None,
            None,
            vega_tools::ReadState::default(),
            None,
            Vec::new(),
            None,
        )
    });
    tokio::time::timeout(Duration::from_secs(8), worker)
        .await
        .unwrap()
        .unwrap();
    let updates: Vec<_> = receiver.try_iter().collect();
    assert!(
        updates
            .iter()
            .any(|update| matches!(update, AgentUpdate::Finished { success: true, .. }))
    );
    server.finish().await;
    assert_eq!(server.received.lock().unwrap().len(), 1);
    assert!(
        server.received.lock().unwrap()[0]
            .body
            .to_string()
            .contains("data:image/png;base64,")
    );
    let visible: String = store
        .conn()
        .query_row(
            "SELECT content FROM messages WHERE role='assistant' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(visible, "Complete");
}

#[::core::prelude::v1::test]
fn real_reasoning_loader_rejects_invalid_saved_choice_without_clamping_and_accepts_default_repair()
{
    let dir = tempfile::tempdir().unwrap();
    let (_, _, path) = owned(
        dir.path(),
        "https://unknown.invalid/v1",
        vega_store::config::ProviderApi::ChatCompletions,
    );
    capability(dir.path(), None, Some(Vec::new()));
    let pref = path.with_file_name("reasoning-preferences.toml");
    std::fs::write(
        &pref,
        "version=1\n[[preferences]]\nprovider='fixture'\nmodel='future'\npreference='high'\n",
    )
    .unwrap();
    assert!(
        reasoning_for_provider_model(
            "fixture",
            vega_store::config::ProviderApi::ChatCompletions,
            "https://unknown.invalid/v1",
            "future",
            Some(&pref)
        )
        .is_err()
    );
    let (base, snapshot) = vega_store::reasoning::read_authority_from(&pref).unwrap();
    let cap_before = std::fs::read(dir.path().join("model-capabilities.toml")).unwrap();
    vega_store::reasoning::save_profile_patch(
        &pref,
        &snapshot,
        &base,
        &vega_store::reasoning::ReasoningProfilePatch {
            provider: "fixture".into(),
            model: "future".into(),
            preference: Some("provider_default".into()),
        },
    )
    .unwrap();
    assert_eq!(
        reasoning_for_provider_model(
            "fixture",
            vega_store::config::ProviderApi::ChatCompletions,
            "https://unknown.invalid/v1",
            "future",
            Some(&pref)
        )
        .unwrap()
        .choice,
        vega_runtime::ReasoningChoice::ProviderDefault
    );
    assert_eq!(
        std::fs::read(dir.path().join("model-capabilities.toml")).unwrap(),
        cap_before
    );
}
