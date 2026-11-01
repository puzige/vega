use vega_conversation::display_history as display;
use vega_conversation::history::{self, AssistantStatus, HistoryEntry, HistoryPage};
use vega_conversation::types::{
    Approval, HistoricalHistoryEntry, ToolCallStatus, ToolCardInputProjection,
    ToolCardResultProjection,
};
use vega_store::Store;
use vega_store::messages::PageCursor;

const SECRET: &str = concat!("ghp_", "syntheticFixtureCredential0123456789");

struct Fixture {
    store: Store,
    _root: tempfile::TempDir,
}

impl std::ops::Deref for Fixture {
    type Target = Store;

    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute_batch(
        "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/fixture','Fixture',1,1);
         INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('fork','project','Fork','model',1,1);
         INSERT INTO threads(id,project_id,title,model,parent_thread_id,workspace_owner_thread_id,created_at,updated_at) VALUES('child','project','Current child','model','fork','fork',1,1);
         INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
           ('user','fork',1,'user','Question','done',1),
           ('archived','fork',2,'assistant','Archived reply','done',2),
           ('next-user','fork',3,'user','Continue','done',3),
           ('current','fork',4,'assistant','Current reply','done',4),
           ('child-message','child',1,'assistant','Child result','done',4);
         INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES
           ('archived','archived-final',0,14,'final'),('current','current-final',0,13,'final');
         INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,approval,created_at,finished_at,text_offset_bytes) VALUES
           ('archive-call','fork','archived',1,'spawn_agent','{}','placeholder','success','once',2,2,0),
           ('current-call','fork','current',2,'spawn_agent','{}','placeholder','success','once',4,4,0);
         INSERT INTO historical_tool_receipts(call_id,thread_id,kind,source_thread_id,source_message_id,source_call_id,input_json,output_text,references_json)
           VALUES('archive-call','fork','delegation','source','source-message','source-call','{}','placeholder','[]');
         INSERT INTO delegated_executions(child_thread_id,child_message_id,parent_thread_id,parent_message_id,request_call_id,ordinal,kind,name,task,state,result,created_at)
           VALUES('child','child-message','fork','current','current-call',1,'spawn','Current child','Current task','completed','Current result',4);"
    ).unwrap();
    let archive = serde_json::json!({
        "agent": {
            "agent_id": "historical-child", "message_id": "historical-message",
            "name": "Historical child", "state": "completed", "result": "Historical result",
            "checkpoint": {"evidence": SECRET}, "release_state": "released"
        }
    });
    let current = serde_json::json!({
        "agent": {
            "agent_id": "child", "parent_thread_id": "fork", "parent_message_id": "current",
            "message_id": "child-message", "name": "Current child", "state": "completed",
            "result": "Current result", "truncated": false
        }
    });
    let references = serde_json::json!([
        {"field": "agent_id", "source_id": "historical-child", "destination_id": null}
    ]);
    let escaped = archive.to_string().replace("ghp_", "\\u0067hp_");
    store.conn().execute(
        "UPDATE historical_tool_receipts SET input_json=?1,output_text=?2,references_json=?3 WHERE call_id='archive-call'",
        (serde_json::json!({"task": SECRET}).to_string(), &escaped, references.to_string()),
    ).unwrap();
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET output_text=?1 WHERE id='archive-call'",
            [&escaped],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET output_text=?1 WHERE id='current-call'",
            [current.to_string()],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE projects SET path=?1 WHERE id='project'",
            [root.path().canonicalize().unwrap().to_str().unwrap()],
        )
        .unwrap();
    Fixture { store, _root: root }
}

fn tool<'a>(page: &'a HistoryPage, wanted: &str) -> &'a HistoryEntry {
    page.entries
        .iter()
        .find(|entry| matches!(entry, HistoryEntry::Tool { call_id, .. } if call_id == wanted))
        .unwrap()
}

fn assert_archive(
    page: &HistoryPage,
    expected_status: ToolCallStatus,
    expected_approval: Approval,
) {
    let HistoryEntry::Tool {
        message_id,
        status,
        approval,
        input,
        result,
        ..
    } = tool(page, "archive-call")
    else {
        panic!("archive tool missing")
    };
    assert_eq!(message_id, "archived");
    assert_eq!(*status, expected_status);
    assert_eq!(*approval, Some(expected_approval));
    assert!(
        matches!(input, Some(ToolCardInputProjection::Subagent { tool, .. }) if tool == "historical_delegation")
    );
    let Some(ToolCardResultProjection::Subagent {
        status,
        agent,
        output,
        reused,
    }) = result
    else {
        panic!("archive result missing")
    };
    assert_eq!(*status, expected_status);
    assert!(agent.is_none());
    assert!(!reused);
    assert!(!output.contains(SECRET));
    let details: serde_json::Value = serde_json::from_str(output).unwrap();
    assert_eq!(
        details["agents"][0]["historical_agent_id"],
        "historical-child"
    );
    assert_eq!(
        details["agents"][0]["historical_message_id"],
        "historical-message"
    );
    assert_eq!(details["agents"][0]["name"], "Historical child");
    assert_eq!(details["agents"][0]["state"], "completed");
    assert_eq!(details["agents"][0]["result"], "Historical result");
    assert_eq!(details["agents"][0]["release_state"], "released");
    let checkpoint: serde_json::Value =
        serde_json::from_str(details["agents"][0]["checkpoint_json"].as_str().unwrap()).unwrap();
    assert_ne!(checkpoint["evidence"], SECRET);
    assert!(
        checkpoint["evidence"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
    );
    let input: serde_json::Value =
        serde_json::from_str(details["input_json"].as_str().unwrap()).unwrap();
    assert_ne!(input["task"], SECRET);
    assert_eq!(details["references"][0]["field"], "agent_id");
    assert_eq!(details["references"][0]["source_id"], "historical-child");
    assert!(details["references"][0]["destination_id"].is_null());
    let receipt: serde_json::Value =
        serde_json::from_str(details["receipt"].as_str().unwrap()).unwrap();
    assert_eq!(receipt["agent"]["result"], "Historical result");
    assert_ne!(receipt["agent"]["checkpoint"]["evidence"], SECRET);
    assert!(
        page.subagents
            .iter()
            .all(|agent| agent.agent_id != "historical-child")
    );
}

fn assert_current(page: &HistoryPage) {
    let HistoryEntry::Tool {
        message_id,
        status,
        input,
        result,
        ..
    } = tool(page, "current-call")
    else {
        panic!("current tool missing")
    };
    assert_eq!(message_id, "current");
    assert_eq!(*status, ToolCallStatus::Success);
    assert!(
        matches!(input, Some(ToolCardInputProjection::Subagent { tool, .. }) if tool == "spawn_agent")
    );
    let Some(ToolCardResultProjection::Subagent {
        agent: Some(agent), ..
    }) = result
    else {
        panic!("current executable identity missing")
    };
    assert_eq!(agent.agent_id, "child");
    assert_eq!(agent.message_id, "child-message");
    assert_eq!(agent.parent_thread_id, "fork");
    assert_eq!(agent.parent_message_id, "current");
    assert_eq!(page.subagents.len(), 1);
    assert_eq!(page.subagents[0].agent_id, "child");
    assert_eq!(page.subagents[0].parent_message_id, "current");
}

fn message_order(page: &HistoryPage) -> Vec<&str> {
    page.entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::UserText { message_id, .. }
            | HistoryEntry::AssistantText { message_id, .. } => Some(message_id.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn mixed_delegations_keep_identity_through_restart_and_privacy_refresh() {
    let store = fixture();
    let latest = display::latest_history_page(&store, "fork", 20).unwrap();
    assert_archive(&latest, ToolCallStatus::Success, Approval::Once);
    assert_current(&latest);
    assert_eq!(
        message_order(&latest),
        ["user", "archived", "next-user", "current"]
    );
    assert_eq!(
        (latest.older_cursor, latest.newer_cursor, latest.newest_seq),
        (None, None, Some(4))
    );
    assert_eq!(
        display::restart_history_page(&store, "fork", 20).unwrap(),
        latest
    );
    assert_eq!(
        display::latest_history_page(&store, "fork", 20).unwrap(),
        latest
    );
    let stored: String = store
        .conn()
        .query_row(
            "SELECT input_json FROM historical_tool_receipts WHERE call_id='archive-call'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored.contains(SECRET));
    let agents: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM delegated_executions WHERE parent_thread_id='fork'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(agents, 1);
}

#[test]
fn mixed_delegations_preserve_paging_location_and_cursor_edges() {
    let store = fixture();
    let latest = display::latest_history_page(&store, "fork", 2).unwrap();
    assert_current(&latest);
    assert_eq!(message_order(&latest), ["next-user", "current"]);
    assert_eq!(
        (latest.older_cursor, latest.newer_cursor, latest.newest_seq),
        (Some(3), None, Some(4))
    );
    let older = display::history_page_before(&store, "fork", PageCursor::Before(3), 2).unwrap();
    assert_archive(&older, ToolCallStatus::Success, Approval::Once);
    assert!(older.subagents.is_empty());
    assert_eq!(message_order(&older), ["user", "archived"]);
    assert_eq!(older.older_cursor, Some(1));
    assert_eq!(older.newer_cursor, Some(2));
    assert_eq!(older.newest_seq, Some(2));
    let newer = display::history_page_after(&store, "fork", 2, 2).unwrap();
    assert_current(&newer);
    assert_eq!(message_order(&newer), ["next-user", "current"]);
    assert_eq!(
        (newer.older_cursor, newer.newer_cursor, newer.newest_seq),
        (Some(3), Some(4), Some(4))
    );
    let located = display::history_page_containing_message(&store, "fork", "archived", 4)
        .unwrap()
        .unwrap();
    assert_archive(&located, ToolCallStatus::Success, Approval::Once);
    assert_current(&located);
    assert_eq!(
        message_order(&located),
        ["user", "archived", "next-user", "current"]
    );
    assert_eq!(
        (
            located.older_cursor,
            located.newer_cursor,
            located.newest_seq
        ),
        (None, None, Some(4))
    );
    assert!(
        display::history_page_containing_message(&store, "fork", "missing", 4)
            .unwrap()
            .is_none()
    );
    assert!(
        display::history_page_containing_message(&store, "fork", "child-message", 4)
            .unwrap()
            .is_none()
    );
}

#[test]
fn archive_cards_preserve_terminal_status_and_approval_without_actions() {
    let store = fixture();
    for (status, expected_status, approval, expected_approval) in [
        ("success", ToolCallStatus::Success, "once", Approval::Once),
        ("failed", ToolCallStatus::Failed, "always", Approval::Always),
        (
            "cancelled",
            ToolCallStatus::Cancelled,
            "once",
            Approval::Once,
        ),
        ("rejected", ToolCallStatus::Rejected, "deny", Approval::Deny),
    ] {
        store
            .conn()
            .execute(
                "UPDATE tool_calls SET status=?1,approval=?2 WHERE id='archive-call'",
                (status, approval),
            )
            .unwrap();
        let page = display::latest_history_page(&store, "fork", 20).unwrap();
        assert_archive(&page, expected_status, expected_approval);
        assert_current(&page);
    }
}

#[test]
fn ordinary_display_readers_match_legacy_and_headless_keeps_historical_semantics() {
    let store = fixture();
    assert!(history::latest_history_page(&store, "fork", 20).is_err());
    let headless = history::latest_historical_history_page(&store, "fork", 20).unwrap();
    let calls: Vec<_> = headless
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoricalHistoryEntry::Delegation { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["archive-call", "current-call"]);
    store
        .conn()
        .execute("DELETE FROM historical_tool_receipts", [])
        .unwrap();
    assert_eq!(
        display::latest_history_page(&store, "fork", 20).unwrap(),
        history::latest_history_page(&store, "fork", 20).unwrap()
    );
    assert_eq!(
        display::restart_history_page(&store, "fork", 20).unwrap(),
        history::restart_history_page(&store, "fork", 20).unwrap()
    );
    assert_eq!(
        display::history_page_before(&store, "fork", PageCursor::Before(3), 2).unwrap(),
        history::history_page_before(&store, "fork", PageCursor::Before(3), 2).unwrap()
    );
    assert_eq!(
        display::history_page_after(&store, "fork", 2, 2).unwrap(),
        history::history_page_after(&store, "fork", 2, 2).unwrap()
    );
    assert_eq!(
        display::history_page_containing_message(&store, "fork", "current", 2).unwrap(),
        history::history_page_containing_message(&store, "fork", "current", 2).unwrap()
    );
}

#[test]
fn restart_recovery_respects_admission_and_retains_inert_history() {
    let store = fixture();
    store
        .conn()
        .execute(
            "UPDATE messages SET status='streaming' WHERE id='current'",
            [],
        )
        .unwrap();
    let admission = vega_store::thread_admission::try_acquire(store.database_identity(), "fork")
        .unwrap()
        .unwrap();
    let guarded = display::restart_history_page(&store, "fork", 20).unwrap();
    assert_archive(&guarded, ToolCallStatus::Success, Approval::Once);
    let status: String = store
        .conn()
        .query_row(
            "SELECT status FROM messages WHERE id='current'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "streaming");
    assert!(guarded.interrupted_tail.is_none());
    let thread = vega_store::threads::find(store.conn(), "fork")
        .unwrap()
        .unwrap();
    let fence = vega_conversation::types::PermissionFence {
        schema_version: 1,
        execution_policy_version: 2,
        authority_epoch: vega_store::permission_contracts::load_state(store.conn())
            .unwrap()
            .authority_epoch,
        database_identity: store.database_identity().permission_fence_key(),
        thread_id: thread.id,
        project_id: thread.project_id,
        workspace: vega_conversation::threads::task_workspace_root(&store, "fork")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned(),
        run_mode: thread.mode,
        permission_mode: thread.permission_mode,
        permission_revision: thread.permission_revision,
        exact_rules: Vec::new(),
        full_access_grant: None,
        delegation: None,
    };
    assert!(fence.is_valid());
    let tx = store.immediate_transaction().unwrap();
    vega_store::permission_contracts::record_run_fence(
        &tx,
        "current",
        "fork",
        &serde_json::to_string(&fence).unwrap(),
    )
    .unwrap();
    tx.commit().unwrap();
    drop(admission);
    let recovered = display::restart_history_page(&store, "fork", 20).unwrap();
    assert_archive(&recovered, ToolCallStatus::Success, Approval::Once);
    assert_current(&recovered);
    assert_eq!(
        recovered.interrupted_tail.as_ref().unwrap().message_id,
        "current"
    );
    assert!(recovered.entries.iter().any(|entry| matches!(entry, HistoryEntry::AssistantText { message_id, status: AssistantStatus::Interrupted, .. } if message_id == "current")));
}
