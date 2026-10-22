use super::{
    RECOVERY_CANCELLED_OUTPUT, RECOVERY_DENIAL_APPROVAL_JSON, RECOVERY_REJECTED_OUTPUT,
    RecoveryCounts, recover_thread,
};
use crate::{Store, messages, projects, threads, tool_calls};
use std::path::PathBuf;

const APPROVAL: &str = r#"{"decision":"once","note":null,"source":"user","danger":null}"#;
const INPUT: &str = r#"{"file_path":"outside.txt"}"#;

struct RecoveryFixture(PathBuf);

impl RecoveryFixture {
    fn new() -> Self {
        let fixture = Self(
            std::env::temp_dir().join(format!("vega-read-recovery-{}", ulid::Ulid::generate())),
        );
        std::fs::create_dir(&fixture.0).unwrap();
        fixture
    }

    fn open(&self) -> Store {
        Store::open(self.0.join("recovery.sqlite")).unwrap()
    }
}

impl Drop for RecoveryFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn startup_recovery_rejects_pending_and_cancels_authorized_reads_idempotently() {
    for name in ["read", "Read"] {
        let fixture = RecoveryFixture::new();
        let store = fixture.open();
        store.migrate().unwrap();
        let conn = store.conn();
        let project = projects::create(
            conn,
            fixture.0.to_str().unwrap(),
            "Owned recovery project",
            None,
        )
        .unwrap();
        let thread_id = ulid::Ulid::generate().to_string();
        threads::create(
            conn,
            threads::NewThread {
                id: &thread_id,
                project_id: &project.id,
                title: "Interrupted reads",
                mode: "execute",
                permission_mode: "confirm",
                model: "fixture-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 10,
                updated_at: 10,
            },
        )
        .unwrap();
        let message_id = ulid::Ulid::generate().to_string();
        messages::insert(
            conn,
            &messages::MessageRow {
                id: message_id.clone(),
                thread_id: thread_id.clone(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: 10,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        let calls: Vec<_> = ["pending_approval", "approved", "running"]
            .into_iter()
            .enumerate()
            .map(|(index, state)| {
                let id = ulid::Ulid::generate().to_string();
                tool_calls::insert_pending(
                    conn,
                    tool_calls::NewToolCall {
                        id: &id,
                        thread_id: &thread_id,
                        message_id: &message_id,
                        seq: index as i64 + 1,
                        tool: name,
                        input_json: INPUT,
                        status: "pending_approval",
                        created_at: 10,
                    },
                )
                .unwrap();
                if state != "pending_approval" {
                    tool_calls::approve(conn, &id, APPROVAL, None, 20).unwrap();
                }
                if state == "running" {
                    tool_calls::mark_running(conn, &id).unwrap();
                }
                let initial = tool_calls::find_state(conn, &id).unwrap().unwrap();
                assert_eq!(initial.status, state);
                assert_eq!(
                    initial.approval.as_deref(),
                    (state != "pending_approval").then_some(APPROVAL)
                );
                assert!(initial.output_text.is_none());
                (id, state)
            })
            .collect();
        drop(store);

        let store = fixture.open();
        let conn = store.conn();
        assert_eq!(
            recover_thread(conn, &thread_id, 100).unwrap(),
            RecoveryCounts {
                messages_interrupted: 1,
                tools_rejected: 1,
                tools_cancelled: 2,
            }
        );
        let terminal: Vec<_> = calls
            .iter()
            .map(|(id, initial)| {
                let pending = *initial == "pending_approval";
                let recovered = tool_calls::find_state(conn, id).unwrap().unwrap();
                assert_eq!(
                    recovered,
                    tool_calls::ToolCallState {
                        thread_id: thread_id.clone(),
                        tool: name.into(),
                        input_json: INPUT.into(),
                        status: if pending { "rejected" } else { "cancelled" }.into(),
                        approval: Some(
                            if pending {
                                RECOVERY_DENIAL_APPROVAL_JSON
                            } else {
                                APPROVAL
                            }
                            .into()
                        ),
                        output_text: Some(
                            if pending {
                                RECOVERY_REJECTED_OUTPUT
                            } else {
                                RECOVERY_CANCELLED_OUTPUT
                            }
                            .into()
                        ),
                        exit_code: None,
                        duration_ms: None,
                        output_full_path: None,
                    }
                );
                let finished_at: i64 = conn
                    .query_row(
                        "SELECT finished_at FROM tool_calls WHERE id = ?1",
                        [id],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(finished_at, 100);
                (id.clone(), recovered)
            })
            .collect();
        drop(store);

        let store = fixture.open();
        let conn = store.conn();
        assert_eq!(
            recover_thread(conn, &thread_id, 200).unwrap(),
            RecoveryCounts {
                messages_interrupted: 0,
                tools_rejected: 0,
                tools_cancelled: 0,
            }
        );
        for (id, expected) in terminal {
            assert_eq!(tool_calls::find_state(conn, &id).unwrap(), Some(expected));
            let finished_at: i64 = conn
                .query_row(
                    "SELECT finished_at FROM tool_calls WHERE id = ?1",
                    [&id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(finished_at, 100);
        }
    }
}
