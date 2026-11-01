use super::*;
use serde_json::{Value, json};

fn frame(calls: Value, finish: Value) -> String {
    json!({"choices":[{"index":0,"delta":{"tool_calls":calls},"finish_reason":finish}]}).to_string()
}

fn start(index: u64, id: &str, name: &str, args: &str) -> Value {
    json!({"index":index,"id":id,"type":"function","function":{"name":name,"arguments":args}})
}

#[test]
fn interleaved_arguments_keep_all_call_identities_and_index_order() {
    let mut assembler = SseAssembler::default();
    assert!(
        assembler
            .absorb(&frame(
                json!([
                    start(2, "third", "glob", "{\"pattern\":"),
                    start(0, "first", "read", "{\"path\":\"a"),
                    start(3, "fourth", "grep", "{\"pattern\":\"needle\",\"path\":"),
                    start(1, "second", "read", "{\"path\":\"b")
                ]),
                Value::Null
            ))
            .unwrap()
            .is_empty()
    );
    assembler
        .absorb(&frame(
            json!([
                {"index":1,"function":{"arguments":".txt\",\"offset\":2,\"limit\":1}"}},
                {"index":3,"function":{"arguments":"null}"}},
                {"index":0,"function":{"arguments":".txt\",\"offset\":null,\"limit\":null}"}},
                {"index":2,"function":{"arguments":"\"*.txt\"}"}}
            ]),
            Value::Null,
        ))
        .unwrap();
    let events = assembler
        .absorb(&frame(Value::Null, json!("tool_calls")))
        .unwrap();
    let calls: Vec<_> = events
        .into_iter()
        .map(|event| match event {
            ProviderEvent::ToolUse {
                id,
                name,
                input_json,
            } => (
                id,
                name,
                serde_json::from_str::<Value>(&input_json).unwrap(),
            ),
            other => panic!("unexpected event: {other:?}"),
        })
        .collect();
    assert_eq!(
        calls,
        vec![
            (
                "first".into(),
                "read".into(),
                json!({"path":"a.txt","offset":null,"limit":null})
            ),
            (
                "second".into(),
                "read".into(),
                json!({"path":"b.txt","offset":2,"limit":1})
            ),
            ("third".into(), "glob".into(), json!({"pattern":"*.txt"})),
            (
                "fourth".into(),
                "grep".into(),
                json!({"pattern":"needle","path":null})
            )
        ]
    );
    assert!(
        !assembler
            .finalize()
            .unwrap()
            .iter()
            .any(|event| matches!(event, ProviderEvent::ToolUse { .. }))
    );
}

#[test]
fn incomplete_or_nonobject_arguments_do_not_emit_partial_successes() {
    for invalid in [
        start(1, "", "read", "{}"),
        start(1, "bad", "read", "{"),
        start(1, "bad", "read", "[]"),
    ] {
        let mut assembler = SseAssembler::default();
        assembler
            .absorb(&frame(
                json!([start(0, "good", "read", "{}"), invalid]),
                Value::Null,
            ))
            .unwrap();
        let error = assembler
            .absorb(&frame(Value::Null, json!("tool_calls")))
            .unwrap_err();
        assert!(format!("{error:?}").contains("IncompleteTool"), "{error:?}");
    }
}

#[test]
fn delayed_and_repeated_matching_ids_keep_argument_fragments() {
    let mut assembler = SseAssembler::default();
    for call in [
        start(0, "", "read", "{\"path\":"),
        start(0, "stable", "read", "\"a"),
        start(0, "stable", "", ".txt\""),
        json!({"index":0,"function":{"arguments":"}"}}),
        json!({"id":"stable","function":{"arguments":" "}}),
    ] {
        assert!(
            assembler
                .absorb(&frame(json!([call]), Value::Null))
                .unwrap()
                .is_empty()
        );
    }
    let events = assembler
        .absorb(&frame(Value::Null, json!("tool_calls")))
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        ProviderEvent::ToolUse {
            id,
            name,
            input_json,
        } => {
            assert_eq!(id, "stable");
            assert_eq!(name, "read");
            assert_eq!(
                serde_json::from_str::<Value>(input_json).unwrap(),
                json!({"path":"a.txt"})
            );
        }
        event => panic!("unexpected event: {event:?}"),
    }
}

#[test]
fn duplicate_ids_and_conflicting_indices_fail_closed() {
    for invalid in [
        start(1, "one", "read", "{}"),
        start(0, "two", "read", "{}"),
        start(0, "one", "glob", ""),
    ] {
        let mut assembler = SseAssembler::default();
        assembler
            .absorb(&frame(json!([start(0, "one", "read", "{}")]), Value::Null))
            .unwrap();
        let error = assembler
            .absorb(&frame(json!([invalid]), Value::Null))
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("ToolIdentityConflict"),
            "{error:?}"
        );
    }
}
