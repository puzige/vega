use super::*;
use serde_json::{Value, json};
use vega_store::{messages::MessageRow, threads::NewThread};

fn store() -> Store {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    vega_store::threads::create_standalone(
        store.conn(),
        NewThread {
            id: "thread",
            project_id: "",
            title: "prediction",
            mode: "execute",
            permission_mode: "confirm",
            permission_revision: 0,
            full_access_grant: None,
            reasoning_preferences: None,
            model: "model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
        },
    )
    .unwrap();
    store
}

fn message(store: &Store, seq: i64, role: &str, kind: &str, status: &str, content: &str) {
    vega_store::messages::insert(
        store.conn(),
        &MessageRow {
            id: seq.to_string(),
            thread_id: "thread".into(),
            seq,
            role: role.into(),
            kind: kind.into(),
            content: content.into(),
            status: status.into(),
            created_at: 0,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
}

fn records(store: &Store, terminal: i64) -> Vec<Value> {
    serde_json::from_str(&prediction_context(store, "thread", &terminal.to_string()).unwrap())
        .unwrap()
}

fn retained(record: &Value) -> usize {
    ["content", "content_tail"]
        .into_iter()
        .filter_map(|key| record[key].as_str())
        .map(|text| text.chars().count())
        .sum()
}

#[test]
fn complete_messages_preserve_text_through_eight_thousand_characters() {
    for text in ["short\n报告🙂".to_owned(), "中".repeat(8_000)] {
        let (record, count) = prediction_record("assistant", &text, 16_000).unwrap();
        assert_eq!(
            record,
            json!({"role":"assistant","content":text,"truncated":false})
        );
        assert_eq!(count, text.chars().count());
        assert!(record.get("content_tail").is_none());
    }
}

#[test]
fn oversized_message_preserves_unicode_head_and_ordered_tail_separately() {
    let head = "首🙂".repeat(2_000);
    let tail = "尾🚀".repeat(2_000);
    let text = format!("{head}省{tail}");
    let (record, count) = prediction_record("assistant", &text, 16_000).unwrap();
    assert_eq!(
        record,
        json!({"role":"assistant","content":head,"content_tail":tail,"truncated":true})
    );
    assert_eq!(count, 8_000);
    assert_eq!(retained(&record), 8_000);
}

#[test]
fn remaining_budget_clips_even_messages_below_per_message_limit() {
    for (budget, head, tail) in [(5, "abc", "ij"), (2, "a", "j"), (1, "a", "")] {
        let (record, count) = prediction_record("user", "abcdefghij", budget).unwrap();
        assert_eq!(
            record,
            json!({"role":"user","content":head,"content_tail":tail,"truncated":true})
        );
        assert_eq!(count, budget);
        assert_eq!(retained(&record), budget);
    }
    assert!(prediction_record("user", "text", 0).is_none());
}

#[test]
fn empty_retained_fragments_are_skipped_but_useful_tail_is_kept() {
    assert!(prediction_record("assistant", " \n\t", 8_000).is_none());
    assert!(prediction_record("assistant", "  hidden  ", 4).is_none());
    let (record, count) = prediction_record("assistant", "    尾🙂", 4).unwrap();
    assert_eq!(record["content"], "  ");
    assert_eq!(record["content_tail"], "尾🙂");
    assert_eq!(count, 4);
}

#[test]
fn context_prioritizes_latest_messages_and_restores_chronological_order() {
    let store = store();
    message(&store, 0, "user", "text", "done", "old");
    message(&store, 1, "user", "text", "done", &"较早".repeat(4_500));
    message(
        &store,
        2,
        "assistant",
        "text",
        "done",
        &"最新".repeat(4_500),
    );
    let records = records(&store, 2);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["role"], "user");
    assert_eq!(records[1]["role"], "assistant");
    assert!(records.iter().all(|record| record["truncated"] == true));
    assert_eq!(records.iter().map(retained).sum::<usize>(), 16_000);
    assert_eq!(records[0]["content_tail"], "较早".repeat(2_000));
    assert_eq!(records[1]["content_tail"], "最新".repeat(2_000));
}

#[test]
fn context_marks_clipping_from_remaining_total_budget() {
    let store = store();
    message(&store, 0, "user", "text", "done", &"甲".repeat(7_000));
    message(&store, 1, "assistant", "text", "done", &"乙".repeat(7_000));
    message(&store, 2, "assistant", "text", "done", &"丙".repeat(7_000));
    let records = records(&store, 2);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["truncated"], true);
    assert_eq!(records[0]["content"], "甲".repeat(1_000));
    assert_eq!(records[0]["content_tail"], "甲".repeat(1_000));
    assert_eq!(records[1]["truncated"], false);
    assert_eq!(records[2]["truncated"], false);
    assert_eq!(records.iter().map(retained).sum::<usize>(), 16_000);
}

#[test]
fn context_keeps_existing_filters_and_terminal_fence() {
    let store = store();
    message(&store, 0, "user", "text", "done", "真实请求");
    message(
        &store,
        1,
        "user",
        "text",
        "done",
        "[@file] private file body",
    );
    message(
        &store,
        2,
        "assistant",
        "thinking",
        "done",
        "hidden reasoning",
    );
    message(&store, 3, "assistant", "text", "failed", "failed output");
    message(
        &store,
        4,
        "user",
        "text",
        "done",
        crate::plans::APPROVAL_INSTRUCTION,
    );
    message(&store, 5, "tool", "text", "done", "tool result");
    message(&store, 6, "assistant", "text", "done", "完整结论");
    let records = records(&store, 6);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["content"], "真实请求");
    assert_eq!(records[1]["content"], "完整结论");
    assert!(prediction_context(&store, "thread", "3").is_none());
}

#[test]
fn context_projects_credentials_before_retaining_head_and_tail() {
    let store = store();
    let credential = format!("sk-proj-{}", "a1B2c3D4".repeat(8));
    let text = format!("{credential}\n{}\n{credential}", "报告".repeat(5_000));
    message(&store, 0, "assistant", "text", "done", &text);
    let records = records(&store, 0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["truncated"], true);
    for key in ["content", "content_tail"] {
        let fragment = records[0][key].as_str().unwrap();
        assert!(!fragment.contains(&credential));
        assert!(!fragment.contains("a1B2c3D4"));
        assert!(fragment.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER));
    }
    assert_eq!(retained(&records[0]), 8_000);
}
