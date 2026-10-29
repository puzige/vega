use super::*;
use vega_store::threads;

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn original_details_are_current_thread_bound_paginated_and_never_follow_output_files() -> TestResult
{
    let directory = Directory(
        std::env::temp_dir().join(format!("vega-history-store-{}", ulid::Ulid::generate())),
    );
    std::fs::create_dir(&directory.0)?;
    let path = directory.0.join("test.sqlite");
    let store = Store::open(&path)?;
    store.migrate()?;
    for thread in ["current", "other"] {
        threads::create_standalone(
            store.conn(),
            threads::NewThread {
                id: thread,
                project_id: "",
                title: "Synthetic",
                mode: "execute",
                permission_mode: crate::types::PermissionMode::ReadOnly.as_str(),
                permission_revision: 0,
                full_access_grant: None,
                reasoning_preferences: None,
                model: "model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )?;
    }
    for (id, thread, seq, role) in [
        ("user", "current", 1, "user"),
        ("answer", "current", 2, "assistant"),
        ("foreign", "other", 1, "user"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: thread.into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: format!("original-{id}"),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
    }
    store.conn().execute(
        "INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('user',0,x'010203')",
        [],
    )?;
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: "old-bash",
            thread_id: "current",
            message_id: "answer",
            seq: 1,
            tool: "bash",
            input_json: "{\"command\":\"touch must-not-execute\"}",
            description: None,
            status: "success",
            created_at: 1,
        },
    )?;
    let file = directory.0.join("output.txt");
    std::fs::write(&file, "unreachable-file-private-content")?;
    store.conn().execute("UPDATE tool_calls SET output_text='persisted-output',output_full_path=?1 WHERE id='old-bash'",[file.to_string_lossy().as_ref()])?;
    let reader = reader(path, "current".into());
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: "oversized-tool",
            thread_id: "current",
            message_id: "answer",
            seq: 2,
            tool: &"x".repeat(1048577),
            input_json: "{}",
            description: None,
            status: "success",
            created_at: 1,
        },
    )?;
    let oversized_id = "y".repeat(1048577);
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: oversized_id.clone(),
            thread_id: "current".into(),
            seq: 3,
            role: "user".into(),
            kind: "text".into(),
            content: "oversized identity".into(),
            status: "done".into(),
            created_at: 3,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )?;
    let mut request = ConversationReadRequest {
        kind: "messages".into(),
        source_id: None,
        after_sequence: 0,
        limit: 1,
        content_offset: 0,
    };
    let first = reader(&request)?;
    assert_eq!(first["records"][0]["source_id"], "user");
    assert_eq!(first["records"][0]["attachments"]["count"], 1);
    assert_eq!(first["records"][0]["attachments"]["encoded_bytes"], 3);
    assert_eq!(first["has_more"], true);
    request.after_sequence = first["next_after_sequence"].as_u64().ok_or("cursor")?;
    let next = reader(&request)?;
    assert_eq!(next["records"][0]["source_id"], "answer");
    assert_eq!(next["has_more"], false);
    request.after_sequence = 0;
    request.source_id = Some("foreign".into());
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("records")?
            .is_empty()
    );
    request.source_id = Some("user".into());
    store.conn().execute(
        "UPDATE messages SET content='current-edit' WHERE id='user'",
        [],
    )?;
    assert_eq!(reader(&request)?["records"][0]["content"], "current-edit");
    store.conn().execute(
        "UPDATE messages SET content=?1 WHERE id='user'",
        ["x".repeat(1048577)],
    )?;
    assert!(reader(&request)?["records"][0]["content"].is_null());
    store
        .conn()
        .execute("DELETE FROM messages WHERE id='user'", [])?;
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("deleted")?
            .is_empty()
    );
    request.kind = "tools".into();
    request.source_id = Some("old-bash".into());
    let tool = reader(&request)?;
    assert_eq!(tool["records"][0]["output"], "persisted-output");
    let mut listed = request.clone();
    listed.source_id = None;
    listed.limit = 8;
    let bounded = reader(&listed)?;
    assert_eq!(
        bounded["records"]
            .as_array()
            .ok_or("bounded records")?
            .len(),
        1
    );
    assert!(!bounded.to_string().contains("oversized-tool"));
    listed.kind = "messages".into();
    let bounded_messages = reader(&listed)?;
    assert_eq!(
        bounded_messages["records"]
            .as_array()
            .ok_or("bounded messages")?
            .len(),
        1
    );
    assert!(!bounded_messages.to_string().contains("oversized identity"));
    assert!(
        !tool
            .to_string()
            .contains("unreachable-file-private-content")
    );
    assert!(!directory.0.join("must-not-execute").exists());
    assert_eq!(
        std::fs::read_to_string(file)?,
        "unreachable-file-private-content"
    );
    store
        .conn()
        .execute("DELETE FROM tool_calls WHERE id='old-bash'", [])?;
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("deleted call")?
            .is_empty()
    );
    Ok(())
}

#[test]
fn skill_resource_history_filters_identity_and_refreshes_persisted_details() -> TestResult {
    let directory = Directory(
        std::env::temp_dir().join(format!("vega-resource-history-{}", ulid::Ulid::generate())),
    );
    std::fs::create_dir(&directory.0)?;
    let path = directory.0.join("history.sqlite");
    let store = Store::open(&path)?;
    store.migrate()?;
    for thread in ["current", "other"] {
        threads::create_standalone(
            store.conn(),
            threads::NewThread {
                id: thread,
                project_id: "",
                title: "Synthetic",
                mode: "execute",
                permission_mode: "readonly",
                permission_revision: 0,
                full_access_grant: None,
                reasoning_preferences: None,
                model: "model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )?;
    }
    for (id, thread, role) in [
        ("assistant", "current", "assistant"),
        ("foreign", "other", "assistant"),
        ("human", "current", "user"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: thread.into(),
                seq: if id == "human" { 2 } else { 1 },
                role: role.into(),
                kind: "text".into(),
                content: "history".into(),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
    }
    for (id, thread, message, seq, tool, status) in [
        (
            "reference",
            "current",
            "assistant",
            1,
            "read_skill_resource",
            "success",
        ),
        ("ordinary", "current", "assistant", 2, "read", "success"),
        (
            "failed",
            "current",
            "assistant",
            3,
            "read_skill_resource",
            "failed",
        ),
        (
            "asset",
            "current",
            "assistant",
            4,
            "read_skill_resource",
            "success",
        ),
        (
            "foreign-resource",
            "other",
            "foreign",
            1,
            "read_skill_resource",
            "success",
        ),
        (
            "human-resource",
            "current",
            "assistant",
            5,
            "read_skill_resource",
            "success",
        ),
    ] {
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id,
                thread_id: thread,
                message_id: message,
                seq,
                tool,
                input_json: "{\"name\":\"guide\",\"path\":\"references/missing.md\"}",
                description: None,
                status,
                created_at: 1,
            },
        )?;
    }
    assert_eq!(
        store.conn().execute(
            "UPDATE tool_calls SET message_id='human' WHERE id='human-resource'",
            [],
        )?,
        1
    );
    let output_path = directory.0.join("must-not-read.txt");
    std::fs::write(&output_path, "current-file-is-not-historical-resource")?;
    store.conn().execute("UPDATE tool_calls SET output_text='[Lower-trust Skill reference] saved-reference',output_full_path=?1 WHERE id='reference'",[output_path.to_string_lossy().as_ref()])?;
    store.conn().execute(
        "UPDATE tool_calls SET output_text='asset metadata only' WHERE id='asset'",
        [],
    )?;
    let reader = reader(path, "current".into());
    let mut request = ConversationReadRequest {
        kind: "skill_resources".into(),
        source_id: None,
        after_sequence: 0,
        limit: 1,
        content_offset: 0,
    };
    let first = reader(&request)?;
    assert_eq!(first["records"][0]["source_id"], "reference");
    assert_eq!(
        first["records"][0]["output"],
        "[Lower-trust Skill reference] saved-reference"
    );
    assert_eq!(first["has_more"], true);
    request.after_sequence = first["next_after_sequence"].as_u64().ok_or("cursor")?;
    let second = reader(&request)?;
    assert_eq!(second["records"][0]["source_id"], "asset");
    assert_eq!(second["has_more"], false);
    request.after_sequence = 0;
    for id in ["ordinary", "failed", "foreign-resource", "human-resource"] {
        request.source_id = Some(id.into());
        assert!(
            reader(&request)?["records"]
                .as_array()
                .ok_or("records")?
                .is_empty()
        );
    }
    request.source_id = Some("reference".into());
    store.conn().execute(
        "UPDATE tool_calls SET output_text='edited-reference' WHERE id='reference'",
        [],
    )?;
    assert_eq!(
        reader(&request)?["records"][0]["output"],
        "edited-reference"
    );
    store
        .conn()
        .execute("DELETE FROM tool_calls WHERE id='reference'", [])?;
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("deleted")?
            .is_empty()
    );
    assert_eq!(
        std::fs::read_to_string(output_path)?,
        "current-file-is-not-historical-resource"
    );
    Ok(())
}

fn historical_checkpoint(id: &str, message: &str) -> serde_json::Value {
    serde_json::json!({"id":id,"agent_id":"child","message_id":message,
        "origin":"declared","source_version":7,"source_fingerprint":"fingerprint",
        "previous_checkpoint_id":null,"permission_fence":{"secret":"fence-proof"},
        "authority":"approval-proof","read_receipts":["receipt-proof"],
        "manifest":{"stage":"review","summary":format!("stage-{id}"),
            "artifacts":[{"path":"/nonexistent/history-artifact","read_call_id":"read-proof","sha256":"hash-proof","authority":"artifact-authority"}],
            "evidence_call_ids":["evidence"],"unresolved":[],"next_step":"inspect",
            "ready_for_review":true,"authority":"manifest-authority"},
        "unknown_effects":[{"call_id":"shell","tool":"bash","status":"failed","authority":"effect-authority"}]})
}

#[test]
fn delegated_history_binds_exact_terminal_execution_and_projects_only_safe_stage_data() -> TestResult
{
    let directory = Directory(
        std::env::temp_dir().join(format!("vega-delegated-history-{}", ulid::Ulid::generate())),
    );
    std::fs::create_dir(&directory.0)?;
    let path = directory.0.join("history.sqlite");
    let store = Store::open(&path)?;
    store.migrate()?;
    for thread in ["parent", "other", "child"] {
        threads::create_standalone(
            store.conn(),
            threads::NewThread {
                id: thread,
                project_id: "",
                title: "Synthetic",
                mode: "execute",
                permission_mode: "readonly",
                permission_revision: 0,
                full_access_grant: None,
                reasoning_preferences: None,
                model: "model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )?;
    }
    for (id, thread, seq, role) in [
        ("parent-message", "parent", 1, "assistant"),
        ("other-message", "other", 1, "assistant"),
        ("e1", "child", 1, "assistant"),
        ("instruction", "child", 2, "user"),
        ("e2", "child", 3, "assistant"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: thread.into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: "history".into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
    }
    for (id, seq, tool) in [
        ("spawn", 1, "spawn_agent"),
        ("followup", 2, "followup_agent"),
    ] {
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id,
                thread_id: "parent",
                message_id: "parent-message",
                seq,
                tool,
                input_json: "{}",
                description: None,
                status: "success",
                created_at: 1,
            },
        )?;
    }
    store.conn().execute("INSERT INTO delegated_executions(child_message_id,child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,kind,name,task,state,result,truncated,created_at) VALUES('e1','child','parent','parent-message','spawn',1,'spawn','child','task','completed','E1-result',1,1)",[])?;
    store.conn().execute("INSERT INTO delegated_executions(child_message_id,child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,previous_child_message_id,kind,instruction_message_id,name,task,state,result,created_at) VALUES('e2','child','parent','parent-message','followup',2,'e1','followup','instruction','child','task','failed','E2-result',2)",[])?;
    for (id, message) in [
        ("handoff-e1", "e1"),
        ("latest-e1", "e1"),
        ("latest-e2", "e2"),
    ] {
        store.conn().execute("INSERT INTO delegated_stage_checkpoints(id,child_thread_id,child_message_id,payload,authority,created_at) VALUES(?1,'child',?2,?3,'database-authority',1)",
            (id,message,historical_checkpoint(id,message).to_string()))?;
    }
    store.conn().execute("INSERT INTO delegated_execution_recovery(child_thread_id,child_message_id,release_state,authority,handoff_checkpoint_id) VALUES('child','e1','held','database-authority','handoff-e1')",[])?;
    let output_file = directory.0.join("must-not-read");
    std::fs::write(&output_file, "live-artifact-private")?;
    store.conn().execute(
        "UPDATE tool_calls SET output_full_path=?1 WHERE id='spawn'",
        [output_file.to_string_lossy().as_ref()],
    )?;
    let reader = reader(path.clone(), "parent".into());
    let mut request = ConversationReadRequest {
        kind: "delegated_results".into(),
        source_id: None,
        after_sequence: 0,
        limit: 1,
        content_offset: 0,
    };
    let first = reader(&request)?;
    let row = &first["records"][0];
    assert_eq!(row["source_id"], "spawn");
    assert_eq!(row["output"], "E1-result");
    assert_eq!(row["result_truncated"], true);
    assert_eq!(row["status"], "completed");
    assert_eq!(row["release_state"], "held");
    assert_eq!(row["worker_liveness"], "unknown");
    assert_eq!(row["content"]["id"], "handoff-e1");
    assert_eq!(
        row["content"]["manifest"]["artifacts"],
        serde_json::json!([{"path":"/nonexistent/history-artifact"}])
    );
    assert_eq!(row["content"].as_object().ok_or("checkpoint")?.len(), 9);
    for forbidden in [
        "permission_fence",
        "authority",
        "read_call_id",
        "sha256",
        "read_receipts",
        "proof",
        "live-artifact-private",
    ] {
        assert!(
            !row["content"].to_string().contains(forbidden),
            "{forbidden}"
        );
    }
    assert_eq!(first["has_more"], true);
    request.after_sequence = first["next_after_sequence"].as_u64().ok_or("cursor")?;
    let second = reader(&request)?;
    assert_eq!(second["records"][0]["output"], "E2-result");
    assert_eq!(second["records"][0]["content"]["id"], "latest-e2");
    assert_eq!(second["records"][0]["release_state"], "unknown");
    assert_eq!(second["has_more"], false);
    request.source_id = Some("spawn".into());
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("cursor bound")?
            .is_empty()
    );
    request.after_sequence = 0;
    assert_eq!(reader(&request)?["records"][0]["output"], "E1-result");
    assert!(
        super::reader(path, "other".into())(&request)?["records"]
            .as_array()
            .ok_or("cross parent")?
            .is_empty()
    );
    for tool in ["resume_agent", "followup_agent", "spawn_agent"] {
        store
            .conn()
            .execute("UPDATE tool_calls SET tool=?1 WHERE id='spawn'", [tool])?;
        assert_eq!(reader(&request)?["records"][0]["output"], "E1-result");
    }
    for state in ["completed", "failed", "interrupted"] {
        store.conn().execute(
            "UPDATE delegated_executions SET state=?1 WHERE child_message_id='e1'",
            [state],
        )?;
        assert_eq!(reader(&request)?["records"][0]["status"], state);
    }
    for state in ["running", "cancelling"] {
        store.conn().execute(
            "UPDATE delegated_executions SET state=?1 WHERE child_message_id='e1'",
            [state],
        )?;
        assert!(
            reader(&request)?["records"]
                .as_array()
                .ok_or("nonterminal")?
                .is_empty()
        );
    }
    store.conn().execute(
        "UPDATE delegated_executions SET state='completed' WHERE child_message_id='e1'",
        [],
    )?;
    store.conn().execute("UPDATE delegated_execution_recovery SET handoff_checkpoint_id=NULL WHERE child_message_id='e1'",[])?;
    assert_eq!(
        reader(&request)?["records"][0]["content"]["id"],
        "latest-e1"
    );
    store.conn().execute("UPDATE delegated_execution_recovery SET handoff_checkpoint_id='latest-e2' WHERE child_message_id='e1'",[])?;
    assert!(reader(&request)?["records"][0]["content"].is_null());
    store.conn().execute("UPDATE delegated_execution_recovery SET handoff_checkpoint_id='handoff-e1' WHERE child_message_id='e1'",[])?;
    store.conn().execute(
        "UPDATE delegated_executions SET result=?1 WHERE child_message_id='e1'",
        ["x".repeat(1048577)],
    )?;
    store.conn().execute(
        "UPDATE delegated_stage_checkpoints SET payload=?1 WHERE id='handoff-e1'",
        ["x".repeat(1048577)],
    )?;
    let oversized = reader(&request)?;
    assert!(oversized["records"][0]["output"].is_null());
    assert!(oversized["records"][0]["content"].is_null());
    assert_eq!(oversized["records"][0]["result_truncated"], true);
    for (change, restore) in [
        (
            "UPDATE tool_calls SET status='failed' WHERE id='spawn'",
            "UPDATE tool_calls SET status='success' WHERE id='spawn'",
        ),
        (
            "UPDATE tool_calls SET tool='bash' WHERE id='spawn'",
            "UPDATE tool_calls SET tool='spawn_agent' WHERE id='spawn'",
        ),
        (
            "UPDATE messages SET role='user' WHERE id='parent-message'",
            "UPDATE messages SET role='assistant' WHERE id='parent-message'",
        ),
        (
            "UPDATE messages SET role='user' WHERE id='e1'",
            "UPDATE messages SET role='assistant' WHERE id='e1'",
        ),
        (
            "UPDATE delegated_executions SET parent_message_id='other-message' WHERE child_message_id='e1'",
            "UPDATE delegated_executions SET parent_message_id='parent-message' WHERE child_message_id='e1'",
        ),
        (
            "UPDATE delegated_executions SET child_thread_id='other' WHERE child_message_id='e1'",
            "UPDATE delegated_executions SET child_thread_id='child' WHERE child_message_id='e1'",
        ),
    ] {
        store.conn().execute(change, [])?;
        assert!(
            reader(&request)?["records"]
                .as_array()
                .ok_or("invalid source")?
                .is_empty(),
            "{change}"
        );
        store.conn().execute(restore, [])?;
    }
    let oversized_id = "界".repeat(43);
    store.conn().execute(
        "UPDATE delegated_executions SET child_message_id=?1 WHERE child_message_id='e1'",
        [&oversized_id],
    )?;
    store
        .conn()
        .execute("UPDATE messages SET id=?1 WHERE id='e1'", [&oversized_id])?;
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("byte bounded identity")?
            .is_empty()
    );
    store.conn().execute(
        "UPDATE delegated_executions SET child_message_id='e1' WHERE child_message_id=?1",
        [&oversized_id],
    )?;
    store
        .conn()
        .execute("UPDATE messages SET id='e1' WHERE id=?1", [&oversized_id])?;
    store
        .conn()
        .execute("DELETE FROM messages WHERE id='e1'", [])?;
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("missing child")?
            .is_empty()
    );
    request.source_id = Some("followup".into());
    store
        .conn()
        .execute("DELETE FROM tool_calls WHERE id='followup'", [])?;
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("deleted call")?
            .is_empty()
    );
    request.source_id = Some("absent".into());
    assert!(
        reader(&request)?["records"]
            .as_array()
            .ok_or("missing call")?
            .is_empty()
    );
    assert_eq!(
        std::fs::read_to_string(output_file)?,
        "live-artifact-private"
    );
    Ok(())
}

#[test]
fn delegated_checkpoint_projection_rejects_forged_identity_and_nested_authority() -> TestResult {
    let original = historical_checkpoint("checkpoint", "e1");
    for (field, value) in [
        ("agent_id", serde_json::json!("other")),
        ("message_id", serde_json::json!("e2")),
        ("id", serde_json::json!("other-checkpoint")),
        ("origin", serde_json::json!("界".repeat(43))),
        ("previous_checkpoint_id", serde_json::json!("x".repeat(129))),
    ] {
        let mut forged = original.clone();
        forged[field] = value;
        assert!(checkpoint_projection(&forged.to_string(), "checkpoint", "child", "e1").is_none());
    }
    let mut forged = original;
    forged["manifest"]["summary"] = serde_json::json!({"authority":"hidden-proof"});
    assert!(checkpoint_projection(&forged.to_string(), "checkpoint", "child", "e1").is_none());
    Ok(())
}
