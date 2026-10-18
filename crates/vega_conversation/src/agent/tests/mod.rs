use super::*;
use crate::types::{ConversationEvent, ToolCallStatus};
use std::fs;
use tempfile::tempdir;
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};

mod external_grep;

fn setup_external(permission_mode: &str) -> (Store, tempfile::TempDir, tempfile::TempDir, String) {
    let project_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let store = Store::open(data_dir.path().join("vega.db")).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        project_dir.path().to_str().unwrap(),
        "external-fixture",
        Some("master"),
    )
    .unwrap();
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread-1",
            project_id: &project.id,
            title: "",
            mode: "execute",
            permission_mode,
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    (store, project_dir, data_dir, project.id)
}

fn scripted_provider(call_id: &str, input_json: &str) -> MockProvider {
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Checking. ".into()),
            ProviderEvent::ThinkingDelta("Need grep".into()),
            ProviderEvent::ToolUse {
                id: call_id.into(),
                name: "grep".into(),
                input_json: input_json.into(),
            },
            ProviderEvent::Usage {
                input: 12,
                output: 3,
                cache_read: Some(2),
                cache_write: Some(1),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Found the TODO.".into()),
            ProviderEvent::Usage {
                input: 20,
                output: 5,
                cache_read: Some(0),
                cache_write: Some(0),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ])
}
