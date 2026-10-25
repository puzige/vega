use std::error::Error;
use std::io::{Cursor, Error as IoError};
use std::path::PathBuf;

use image::{DynamicImage, ImageFormat};
use serde_json::{Value, json};
use vega_runtime::{ChatRole, RuntimeToolStatus};
use vega_store::{Store, context_compaction, messages, thread_fork, tool_calls};

use crate::history::{HistoryEntry, latest_history_page};
use crate::threads::{ForkThreadError, create_thread, fork_thread};
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, PermissionMode, ToolCallStatus,
    ToolCardResultProjection,
};

use super::{PersistenceActorConfig, prepare_run_with_images_and_reasoning};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    store: Store,
    thread: String,
    image: Vec<u8>,
}

impl Fixture {
    fn new() -> TestResult<Self> {
        let root =
            std::env::temp_dir().join(format!("vega-fork-screenshot-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let database = root.join("history.sqlite");
        let store = Store::open(&database)?;
        store.migrate()?;
        let project = vega_store::projects::create(
            store.conn(),
            &root.to_string_lossy(),
            "screenshots",
            None,
        )?;
        let thread = create_thread(&store, &project.id, "test-model", "full_access")?.id;
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(2, 2).write_to(&mut encoded, ImageFormat::Png)?;
        let fixture = Self {
            root,
            database,
            store,
            thread,
            image: encoded.into_inner(),
        };
        fixture.message("request", "user", "done", "Capture the window")?;
        fixture.message("interrupted", "assistant", "streaming", "")?;
        fixture.call(
            "capture",
            "interrupted",
            "cancelled",
            "capture",
            false,
            true,
        )?;
        fixture.call("alias", "interrupted", "success", "capture", true, false)?;
        messages::finish(
            fixture.store.conn(),
            "interrupted",
            "Stopped after delivery",
            "interrupted",
        )?;
        fixture.message("followup", "user", "done", "Explain the existing image")?;
        fixture.message("final", "assistant", "streaming", "The image is attached.")?;
        assert!(messages::record_assistant_response(
            fixture.store.conn(),
            "final",
            "The image is attached.",
            &messages::AssistantResponseRow {
                message_id: "final".into(),
                response_id: "final-response".into(),
                text_start_bytes: 0,
                text_end_bytes: "The image is attached.".len(),
                kind: "final".into(),
            }
        )?);
        messages::finish(
            fixture.store.conn(),
            "final",
            "The image is attached.",
            "done",
        )?;
        Ok(fixture)
    }

    fn message(&self, id: &str, role: &str, status: &str, content: &str) -> TestResult {
        messages::insert(
            self.store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: self.thread.clone(),
                seq: messages::next_seq(self.store.conn(), &self.thread)?,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: status.into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
        Ok(())
    }

    fn call(
        &self,
        id: &str,
        message: &str,
        status: &str,
        capture: &str,
        unchanged: bool,
        attach: bool,
    ) -> TestResult {
        let input = json!({"kind":"computer_use","version":1,"action":"screenshot","argument_bytes":2,"argument_digest":"a".repeat(64)}).to_string();
        tool_calls::insert_pending(
            self.store.conn(),
            tool_calls::NewToolCall {
                id,
                thread_id: &self.thread,
                message_id: message,
                seq: tool_calls::next_seq(self.store.conn(), &self.thread)?,
                tool: "ComputerScreenshot",
                input_json: &input,
                status: "pending_approval",
                created_at: 1,
            },
        )?;
        let approval = ApprovalAudit {
            decision: Approval::Once,
            note: None,
            source: ApprovalSource::ComputerUse,
            danger: None,
        }
        .to_json()?;
        tool_calls::approve(self.store.conn(), id, &approval, None, 1)?;
        tool_calls::mark_running(self.store.conn(), id)?;
        if attach {
            vega_store::computer_use_delivery::insert_screenshot(
                self.store.conn(),
                &self.thread,
                message,
                id,
                &self.image,
            )?;
        }
        tool_calls::finish(
            self.store.conn(),
            tool_calls::FinishToolCall {
                id,
                status,
                output_text: &receipt(capture, unchanged).to_string(),
                exit_code: None,
                duration_ms: None,
                finished_at: 2,
            },
        )?;
        Ok(())
    }

    fn output(&self, call: &str, output: Value) -> TestResult {
        self.store.conn().execute(
            "UPDATE tool_calls SET output_text=?1 WHERE id=?2",
            [output.to_string().as_str(), call],
        )?;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn receipt(capture: &str, unchanged: bool) -> Value {
    json!({"status":"delivered","artifact_id":format!("screenshot:{capture}"),"delivery":"conversation_image_attachment","saved_in_history":true,"read_only":true,"unchanged":unchanged,"next_step":"continue_remaining_requested_work_or_finish"})
}

fn counts(store: &Store) -> TestResult<Vec<i64>> {
    [
        "threads",
        "messages",
        "tool_calls",
        "image_attachments",
        "computer_screenshot_artifacts",
        "historical_tool_receipts",
        "historical_message_delivery",
    ]
    .into_iter()
    .map(|table| {
        Ok(store
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?)
    })
    .collect()
}

fn assert_no_authority(store: &Store) -> TestResult {
    for table in [
        "pending_messages",
        "token_usage",
        "context_checkpoints",
        "context_compaction_status",
        "credential_continuations",
        "credential_privacy_exceptions",
        "delegated_runs",
        "delegated_executions",
        "delegated_stage_checkpoints",
        "delegated_execution_recovery",
        "delegated_read_receipts",
        "skill_run_snapshots",
        "thread_skill_pins",
        "permissions",
    ] {
        let count: i64 =
            store
                .conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?;
        assert_eq!(count, 0, "{table}");
    }
    Ok(())
}

#[test]
fn cancelled_delivered_capture_and_alias_survive_fork_reload_and_preparation() -> TestResult {
    for projection in [
        None,
        Some(
            json!({"code":"observation_budget_exceeded","executed":true,"deferred":true,"retryable":true,"next_step":"use_current_results_then_request_remaining_windows"}),
        ),
        Some(
            json!({"code":"observation_budget_exceeded","executed":true,"deferred":true,"retryable":true,"next_step":"use_current_results_then_request_remaining_windows","capability":"desktop","stage":"screenshot","execution_state":"failed","field":null,"diagnostic":"本批观察已达到容量限制，请先使用已有结果，再分批读取其余窗口。"}),
        ),
    ] {
        let fixture = Fixture::new()?;
        if let Some(projection) = projection {
            let mut output = receipt("capture", false);
            output["model_projection"] = projection;
            fixture.output("capture", output)?;
        }
        let original = context_compaction::load_source(fixture.store.conn(), &fixture.thread)?;
        let fork = fork_thread(&fixture.store, &fixture.thread, "final")?;
        assert_eq!(fork.permission_mode, PermissionMode::Confirm);
        assert!(fork.parent_thread_id.is_none());
        assert_eq!(
            context_compaction::load_source(fixture.store.conn(), &fixture.thread)?,
            original
        );
        let reopened = Store::open(&fixture.database)?;
        let fork = crate::threads::open_thread(&reopened, &fork.id)?;
        let source = context_compaction::load_source(reopened.conn(), &fork.id)?;
        assert_eq!(source.images.len(), 1);
        assert_eq!(source.images[0].encoded, fixture.image);
        let capture = source
            .tool_calls
            .iter()
            .find(|call| call.seq == 1)
            .ok_or_else(|| IoError::other("missing capture"))?;
        let alias = source
            .tool_calls
            .iter()
            .find(|call| call.seq == 2)
            .ok_or_else(|| IoError::other("missing alias"))?;
        assert_ne!(capture.id, "capture");
        assert_ne!(alias.id, "alias");
        assert_eq!(capture.status, "cancelled");
        assert_eq!(alias.status, "success");
        assert_eq!(
            source.images[0].artifact_call_id.as_deref(),
            Some(capture.id.as_str())
        );
        for call in [capture, alias] {
            let value: Value = serde_json::from_str(
                call.output_text
                    .as_deref()
                    .ok_or_else(|| IoError::other("missing output"))?,
            )?;
            assert_eq!(value["artifact_id"], format!("screenshot:{}", capture.id));
            assert_eq!(value["unchanged"], call.id == alias.id);
            let old = original
                .tool_calls
                .iter()
                .find(|old| old.seq == call.seq)
                .ok_or_else(|| IoError::other("missing original"))?;
            assert_eq!(call.input_json, old.input_json);
            assert_eq!(call.approval, old.approval);
        }
        let archives = thread_fork::historical_receipts(reopened.conn(), &fork.id)?;
        assert_eq!(archives.len(), 2);
        for archive in &archives {
            let original_call = original
                .tool_calls
                .iter()
                .find(|call| call.id == archive.source_call_id)
                .ok_or_else(|| IoError::other("missing archived source"))?;
            assert_eq!(archive.output_text, original_call.output_text);
            assert!(
                archive
                    .references
                    .iter()
                    .any(|reference| reference.field == "artifact_id"
                        && reference.source_id == "capture"
                        && reference.destination_id.as_deref() == Some(capture.id.as_str()))
            );
        }
        let page = latest_history_page(&reopened, &fork.id, 20)?;
        assert!(page.entries.iter().any(|entry| matches!(entry, HistoryEntry::AssistantScreenshot { call_id, image, .. } if call_id == &capture.id && image.bytes() == fixture.image.as_slice())));
        assert!(page.entries.iter().any(|entry| matches!(entry, HistoryEntry::Tool { call_id, status: ToolCallStatus::Cancelled, input: Some(_), result: Some(result), .. } if call_id == &capture.id && !matches!(result, ToolCardResultProjection::Corrupt))));
        assert!(page.entries.iter().any(|entry| matches!(entry, HistoryEntry::Tool { call_id, status: ToolCallStatus::Success, input: Some(_), result: Some(result), .. } if call_id == &alias.id && !matches!(result, ToolCardResultProjection::Corrupt))));
        assert_no_authority(&reopened)?;
        let cutoff = source
            .messages
            .last()
            .ok_or_else(|| IoError::other("missing cutoff"))?;
        let second = fork_thread(&reopened, &fork.id, &cutoff.id)?;
        let second_source = context_compaction::load_source(reopened.conn(), &second.id)?;
        assert_eq!(second_source.tool_calls[0].status, "cancelled");
        assert_eq!(second_source.images[0].encoded, fixture.image);
        for call in &second_source.tool_calls {
            let output: Value = serde_json::from_str(
                call.output_text
                    .as_deref()
                    .ok_or_else(|| IoError::other("missing second-fork receipt"))?,
            )?;
            assert_eq!(
                output["artifact_id"],
                format!("screenshot:{}", second_source.tool_calls[0].id)
            );
        }
        let prepared = prepare_run_with_images_and_reasoning(
            fixture.database.clone(),
            fork.id.clone(),
            "Continue without computer control".into(),
            "Test".into(),
            "next-user".into(),
            "next-assistant".into(),
            PersistenceActorConfig::default(),
            false,
            None,
            None,
            Vec::new(),
        )?;
        let restored = prepared
            .request
            .completed_tool_results
            .get(&capture.id)
            .ok_or_else(|| IoError::other("missing recovered result"))?;
        assert_eq!(restored.result.status, RuntimeToolStatus::Cancelled);
        assert!(restored.result.reused);
        assert_eq!(restored.input_json, capture.input_json);
        assert!(
            prepared
                .request
                .history
                .iter()
                .any(|message| message.role == ChatRole::Tool
                    && message.tool_call_id.as_deref() == Some(capture.id.as_str())
                    && message.content == restored.result.output)
        );
        assert!(
            prepared
                .request
                .history
                .iter()
                .all(|message| message.images.is_empty())
        );
        assert!(prepared.request.tool_config.exact_rules.is_empty());
        assert_no_authority(&reopened)?;
    }
    Ok(())
}

#[test]
fn invalid_cancelled_screenshot_proof_or_reference_refuses_fork_atomically() -> TestResult {
    for case in [
        "missing_attachment",
        "approval_source",
        "approval_note",
        "approval_danger",
        "input_action",
        "input_duplicate",
        "approval_missing_field",
        "oversized_receipt",
        "forged_receipt",
        "wrong_capture",
        "extra_receipt_field",
        "forged_alias",
        "alias_approval",
        "forged_projection",
        "cross_thread",
        "outside_prefix",
        "future_capture",
    ] {
        let fixture = Fixture::new()?;
        match case {
            "missing_attachment" => {
                fixture.store.conn().execute(
                    "DELETE FROM image_attachments WHERE message_id='interrupted'",
                    [],
                )?;
            }
            "approval_source" | "approval_note" | "approval_danger" | "alias_approval" => {
                let mut audit =
                    json!({"decision":"once","note":null,"source":"computer_use","danger":null});
                match case {
                    "approval_note" => audit["note"] = "not a delivery audit".into(),
                    "approval_danger" => {
                        audit["danger"] = json!({"rule_id":"fake","decision":"once","note":null})
                    }
                    _ => audit["source"] = "user".into(),
                }
                let id = if case == "alias_approval" {
                    "alias"
                } else {
                    "capture"
                };
                fixture.store.conn().execute(
                    "UPDATE tool_calls SET approval=?1 WHERE id=?2",
                    [audit.to_string().as_str(), id],
                )?;
            }
            "input_action" => {
                fixture.store.conn().execute("UPDATE tool_calls SET input_json=replace(input_json,'screenshot','observe') WHERE id='capture'", [])?;
            }
            "input_duplicate" => {
                fixture.store.conn().execute("UPDATE tool_calls SET input_json=replace(input_json,'\"action\":\"screenshot\"','\"action\":\"observe\",\"action\":\"screenshot\"') WHERE id='capture'", [])?;
            }
            "approval_missing_field" => {
                fixture.store.conn().execute(
                    "UPDATE tool_calls SET approval=?1 WHERE id='capture'",
                    [r#"{"decision":"once","source":"computer_use","note":null}"#],
                )?;
            }
            "oversized_receipt" => {
                let output = format!("{}{}", receipt("capture", false), " ".repeat(2049));
                fixture.store.conn().execute(
                    "UPDATE tool_calls SET output_text=?1 WHERE id='capture'",
                    [&output],
                )?;
            }
            "forged_receipt" | "wrong_capture" | "extra_receipt_field" | "forged_projection" => {
                let mut output = receipt("capture", false);
                match case {
                    "forged_receipt" => output["saved_in_history"] = false.into(),
                    "wrong_capture" => output["artifact_id"] = "screenshot:alias".into(),
                    "extra_receipt_field" => output["authority"] = true.into(),
                    _ => output["model_projection"] = json!({"code":"observation_budget_exceeded"}),
                }
                fixture.output("capture", output)?;
            }
            "forged_alias" => {
                let mut output = receipt("capture", true);
                output["read_only"] = false.into();
                fixture.output("alias", output)?;
            }
            "cross_thread" => {
                let source = crate::threads::get_thread(&fixture.store, &fixture.thread)?;
                let other =
                    create_thread(&fixture.store, &source.project_id, "test-model", "confirm")?;
                fixture.store.conn().execute(
                    "UPDATE tool_calls SET thread_id=?1 WHERE id='capture'",
                    [&other.id],
                )?;
            }
            "outside_prefix" => {
                fixture.message("suffix", "assistant", "streaming", "")?;
                fixture.call(
                    "suffix-capture",
                    "suffix",
                    "cancelled",
                    "suffix-capture",
                    false,
                    true,
                )?;
                messages::finish(
                    fixture.store.conn(),
                    "suffix",
                    "Later capture",
                    "interrupted",
                )?;
                fixture.output("alias", receipt("suffix-capture", true))?;
            }
            "future_capture" => {
                fixture
                    .store
                    .conn()
                    .execute("UPDATE tool_calls SET seq=3 WHERE id='capture'", [])?;
                fixture
                    .store
                    .conn()
                    .execute("UPDATE tool_calls SET seq=1 WHERE id='alias'", [])?;
            }
            _ => return Err(IoError::other("unknown fixture case").into()),
        }
        let before = counts(&fixture.store)?;
        let original = context_compaction::load_source(fixture.store.conn(), &fixture.thread)?;
        assert!(
            matches!(
                fork_thread(&fixture.store, &fixture.thread, "final"),
                Err(ForkThreadError::Refused(
                    thread_fork::ForkError::CorruptHistory(_)
                ))
            ),
            "{case}"
        );
        assert_eq!(counts(&fixture.store)?, before, "{case}");
        assert_eq!(
            context_compaction::load_source(fixture.store.conn(), &fixture.thread)?,
            original,
            "{case}"
        );
    }
    Ok(())
}

#[test]
fn successful_capture_keeps_first_line_receipt_fork_compatibility() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute(
        "UPDATE tool_calls SET status='success' WHERE id='capture'",
        [],
    )?;
    let output = format!(
        "{}\nLegacy observation remainder",
        receipt("capture", false)
    );
    fixture.store.conn().execute(
        "UPDATE tool_calls SET output_text=?1 WHERE id='capture'",
        [&output],
    )?;
    let fork = fork_thread(&fixture.store, &fixture.thread, "final")?;
    let source = context_compaction::load_source(fixture.store.conn(), &fork.id)?;
    assert_eq!(source.tool_calls[0].status, "success");
    assert!(
        source.tool_calls[0]
            .output_text
            .as_deref()
            .is_some_and(|output| output.ends_with("\nLegacy observation remainder"))
    );
    assert_eq!(source.images[0].encoded, fixture.image);
    Ok(())
}
