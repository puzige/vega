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
