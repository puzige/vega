use super::*;
use crate::history::{HistoryEntry, latest_history_page};
use serde_json::{Value, json};
use vega_store::{computer_use_delivery, context_compaction, image_attachments, projects, threads};

struct RestoreLanguage(&'static str);

impl Drop for RestoreLanguage {
    fn drop(&mut self) {
        vega_i18n::set_language(self.0);
    }
}

fn audit(tool: &str) -> String {
    json!({
        "kind": "computer_use",
        "version": 1,
        "action": if tool == "ComputerScreenshot" { "screenshot" } else { "observe" },
        "argument_bytes": 2,
        "argument_digest": format!("{:x}", Sha256::digest(b"{}")),
    })
    .to_string()
}

fn approval() -> ApprovalAudit {
    ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ComputerUse,
        danger: None,
    }
}

fn message(store: &Store, id: &str, seq: i64, role: &str, content: &str, status: &str) {
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: id.into(),
            thread_id: "source".into(),
            seq,
            role: role.into(),
            kind: "text".into(),
            content: content.into(),
            status: status.into(),
            created_at: seq,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
}

fn running_call(store: &Store, id: &str, seq: i64, tool: &str, input: &str) {
    tool_calls::insert_pending(
        store.conn(),
        tool_calls::NewToolCall {
            id,
            thread_id: "source",
            message_id: "interrupted",
            seq,
            tool,
            input_json: input,
            status: "pending_approval",
            created_at: seq,
        },
    )
    .unwrap();
    tool_calls::approve(store.conn(), id, &approval().to_json().unwrap(), None, seq).unwrap();
    tool_calls::mark_running(store.conn(), id).unwrap();
}

fn finish_call(store: &Store, id: &str, status: RuntimeToolStatus, output: &str) {
    let state = tool_calls::find_state(store.conn(), id).unwrap().unwrap();
    let result = vega_runtime::RuntimeToolResult {
        call_id: id.into(),
        output: output.into(),
        status,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: Some(approval_audit_to_runtime(&approval())),
        remember_rule: None,
    };
    validate_runtime_terminal("", "source", id, &state, &result, &approval()).unwrap();
    tool_calls::finish(
        store.conn(),
        tool_calls::FinishToolCall {
            id,
            status: if status == RuntimeToolStatus::Cancelled {
                "cancelled"
            } else {
                "failed"
            },
            output_text: output,
            exit_code: None,
            duration_ms: None,
            finished_at: 10,
        },
    )
    .unwrap();
}

fn budget() -> Value {
    serde_json::from_str(r#"{"code":"observation_budget_exceeded","executed":true,"deferred":true,"retryable":true,"next_step":"use_current_results_then_request_remaining_windows","field":null,"capability":"desktop","stage":"screenshot","execution_state":"failed","diagnostic":"\u672c\u6279\u89c2\u5bdf\u5df2\u8fbe\u5230\u5bb9\u91cf\u9650\u5236\uff0c\u8bf7\u5148\u4f7f\u7528\u5df2\u6709\u7ed3\u679c\uff0c\u518d\u5206\u6279\u8bfb\u53d6\u5176\u4f59\u7a97\u53e3\u3002"}"#).unwrap()
}

fn receipt(unchanged: bool) -> String {
    json!({
        "status": "delivered",
        "artifact_id": "screenshot:capture",
        "delivery": "conversation_image_attachment",
        "saved_in_history": true,
        "read_only": true,
        "unchanged": unchanged,
        "next_step": "continue_remaining_requested_work_or_finish",
        "model_projection": budget(),
    })
    .to_string()
}

fn inspect_history(store: &Store, thread: &str, bytes: &[u8]) {
    let source = context_compaction::load_source(store.conn(), thread).unwrap();
    assert!(source.images.is_empty());
    let attachments =
        image_attachments::for_messages(store.conn(), thread, &source.messages).unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].encoded, bytes);
    let captures = source
        .tool_calls
        .iter()
        .filter(|call| call.tool == "ComputerScreenshot")
        .collect::<Vec<_>>();
    assert_eq!(captures.len(), 2);
    let capture = captures
        .iter()
        .find(|call| {
            let output: Value = serde_json::from_str(call.output_text.as_deref().unwrap()).unwrap();
            output["unchanged"] == false
        })
        .unwrap();
    assert_eq!(capture.status, "cancelled");
    assert_eq!(
        attachments[0].artifact_call_id.as_deref(),
        Some(capture.id.as_str())
    );
    assert_eq!(attachments[0].message_id, capture.message_id);
    for call in &source.tool_calls {
        let status = match call.status.as_str() {
            "failed" => RuntimeToolStatus::Failed,
            "cancelled" => RuntimeToolStatus::Cancelled,
            _ => panic!("unexpected persisted status"),
        };
        validate_recovered_projection(
            "",
            thread,
            &call.id,
            &call.tool,
            &call.input_json,
            call.output_text.as_deref().unwrap(),
            status,
            &approval(),
            None,
            None,
        )
        .unwrap();
        if call.tool == "ComputerScreenshot" {
            let output: Value = serde_json::from_str(call.output_text.as_deref().unwrap()).unwrap();
            assert_eq!(output["artifact_id"], format!("screenshot:{}", capture.id));
            assert_eq!(output["model_projection"], budget());
        }
    }
    let history = history_from_context_source(&source, "", None).unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == vega_runtime::ChatRole::Tool)
            .count(),
        4
    );
    assert!(
        history
            .iter()
            .all(|message| message.images.is_empty() && message.replay_frame().is_none())
    );
    let page = latest_history_page(store, thread, 100).unwrap();
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| matches!(entry, HistoryEntry::Tool { .. }))
            .count(),
        4
    );
    for entry in &page.entries {
        if let HistoryEntry::Tool {
            call_id,
            status,
            input,
            result,
            ..
        } = entry
        {
            let call = source
                .tool_calls
                .iter()
                .find(|call| call.id == *call_id)
                .unwrap();
            let expected_status = if call.status == "cancelled" {
                crate::types::ToolCallStatus::Cancelled
            } else {
                crate::types::ToolCallStatus::Failed
            };
            assert_eq!(*status, expected_status);
            assert!(matches!(
                input,
                Some(crate::types::ToolCardInputProjection::ComputerUse { .. })
            ));
            match result {
                Some(crate::types::ToolCardResultProjection::ComputerUse {
                    status,
                    output,
                    reused,
                }) => {
                    assert_eq!(*status, expected_status);
                    assert_eq!(Some(output.as_str()), call.output_text.as_deref());
                    assert!(*reused);
                }
                _ => panic!("expected audited computer-use result"),
            }
        }
    }
    let screenshots = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::AssistantScreenshot {
                message_id,
                call_id,
                image,
                ..
            } => Some((message_id, call_id, image)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(screenshots.len(), 1);
    assert_eq!(screenshots[0].0, &capture.message_id);
    assert_eq!(screenshots[0].1, &capture.id);
    assert_eq!(screenshots[0].2.bytes(), bytes);
}

#[test]
fn canonical_errors_and_delivered_budget_receipts_reopen_and_fork_across_ui_languages() {
    let _restore = RestoreLanguage(vega_i18n::language());
    vega_i18n::set_language("zh-CN");
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let project = projects::create(store.conn(), "/in-memory-project", "fixture", None).unwrap();
    threads::create(
        store.conn(),
        threads::NewThread {
            id: "source",
            project_id: &project.id,
            title: "fixture",
            mode: "execute",
            permission_mode: "confirm",
            model: "fixture",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    message(&store, "user", 1, "user", "Capture the window", "done");
    message(&store, "interrupted", 2, "assistant", "", "streaming");
    let structured: Value = serde_json::from_str(r#"{"code":"target_expired","capability":"desktop","stage":"observe","execution_state":"not_dispatched","retryable":true,"next_step":"refresh_directory","field":"target_id","diagnostic":"\u76ee\u6807\u5df2\u53d8\u5316\u6216\u5173\u95ed\uff0c\u8bf7\u91cd\u65b0\u53d1\u73b0\u6216\u8fde\u63a5"}"#).unwrap();
    for (id, seq, output) in [
        ("structured", 1, structured.to_string()),
        (
            "legacy",
            2,
            "Tool error: computer use: historical failure".into(),
        ),
    ] {
        running_call(
            &store,
            id,
            seq,
            "ComputerObserve",
            &audit("ComputerObserve"),
        );
        finish_call(&store, id, RuntimeToolStatus::Failed, &output);
    }
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    running_call(
        &store,
        "capture",
        3,
        "ComputerScreenshot",
        &audit("ComputerScreenshot"),
    );
    assert_eq!(
        computer_use_delivery::insert_screenshot(
            store.conn(),
            "source",
            "interrupted",
            "capture",
            &bytes
        )
        .unwrap(),
        0
    );
    finish_call(
        &store,
        "capture",
        RuntimeToolStatus::Cancelled,
        &receipt(false),
    );
    running_call(
        &store,
        "alias",
        4,
        "ComputerScreenshot",
        &audit("ComputerScreenshot"),
    );
    finish_call(
        &store,
        "alias",
        RuntimeToolStatus::Cancelled,
        &receipt(true),
    );
    messages::finish(store.conn(), "interrupted", "", "interrupted").unwrap();
    message(&store, "continued", 3, "user", "Explain the result", "done");
    message(
        &store,
        "final",
        4,
        "assistant",
        "The image was delivered before stopping.",
        "streaming",
    );
    assert!(
        messages::record_assistant_response(
            store.conn(),
            "final",
            "The image was delivered before stopping.",
            &messages::AssistantResponseRow {
                message_id: "final".into(),
                response_id: "final-response".into(),
                text_start_bytes: 0,
                text_end_bytes: "The image was delivered before stopping.".len(),
                kind: "final".into(),
            }
        )
        .unwrap()
    );
    messages::finish(
        store.conn(),
        "final",
        "The image was delivered before stopping.",
        "done",
    )
    .unwrap();
    vega_i18n::set_language("en");
    inspect_history(&store, "source", &bytes);
    let child = crate::threads::fork_thread(&store, "source", "final").unwrap();
    assert_eq!(child.forked_from_thread_id.as_deref(), Some("source"));
    inspect_history(&store, &child.id, &bytes);
    vega_i18n::set_language("zh-CN");
    inspect_history(&store, &child.id, &bytes);
    let child_source = context_compaction::load_source(store.conn(), &child.id).unwrap();
    let cutoff = child_source
        .messages
        .iter()
        .find(|message| message.content == "The image was delivered before stopping.")
        .unwrap();
    let grandchild = crate::threads::fork_thread(&store, &child.id, &cutoff.id).unwrap();
    inspect_history(&store, &grandchild.id, &bytes);
    let before: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .unwrap();
    let mut forged: Value = serde_json::from_str(&receipt(false)).unwrap();
    forged["model_projection"]["diagnostic"] = "untrusted diagnostic".into();
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET output_text=?1 WHERE id='capture'",
            [forged.to_string()],
        )
        .unwrap();
    assert!(crate::threads::fork_thread(&store, "source", "final").is_err());
    let after: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(before, after);
    inspect_history(&store, &child.id, &bytes);
}
