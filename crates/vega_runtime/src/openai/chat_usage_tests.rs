use super::{SseAssembler, VegaError};
use crate::{ProviderEvent, ProviderFailureKind, StopReason};
use serde_json::{Value, json};

#[path = "chat_usage_http.rs"]
mod http;
#[path = "chat_usage_runtime.rs"]
mod runtime;

type Counts = (u64, u64, Option<u64>, Option<u64>);

const CALLS: [(&str, &str); 2] = [
    ("read-alpha", r#"{"path":"alpha.txt"}"#),
    ("read-beta", r#"{"path":"beta.txt"}"#),
];

fn chunk(delta: Value, finish: Value) -> Value {
    json!({"id":"synthetic-response","choices":[{"delta":delta,"finish_reason":finish}]})
}

fn alias_usage() -> Value {
    json!({
        "prompt_tokens":100,
        "completion_tokens":9,
        "prompt_tokens_details":{"cached_tokens":70},
        "prompt_cache_hit_tokens":70,
        "cached_tokens":0,
        "prompt_cache_miss_tokens":30
    })
}

fn equal_usage() -> Value {
    json!({
        "prompt_tokens":100,
        "completion_tokens":9,
        "prompt_tokens_details":{"cached_tokens":70},
        "prompt_cache_hit_tokens":70,
        "cached_tokens":70,
        "prompt_cache_miss_tokens":30
    })
}

fn two_reads(usage: Value) -> Vec<Value> {
    let mut terminal = chunk(
        json!({"content":"files. ","tool_calls":[
            {"index":1,"function":{"arguments":"}"}},
            {"index":0,"function":{"arguments":"}"}}
        ]}),
        json!("tool_calls"),
    );
    terminal["usage"] = usage;
    vec![
        chunk(
            json!({"content":"Inspecting ","reasoning_content":"plan ","tool_calls":[
                {"index":1,"id":"read-beta","type":"function","function":{"name":"read","arguments":"{\"path\":\"beta"}},
                {"index":0,"id":"read-alpha","type":"function","function":{"name":"read","arguments":"{\"path\":\"alpha"}}
            ]}),
            json!(""),
        ),
        chunk(
            json!({"reasoning_content":"reads","tool_calls":[
                {"index":0,"function":{"arguments":".txt\""}},
                {"index":1,"function":{"arguments":".txt\""}}
            ]}),
            Value::Null,
        ),
        terminal,
    ]
}

fn decode(chunks: &[Value]) -> Result<Vec<ProviderEvent>, VegaError> {
    let mut assembler = SseAssembler::default();
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(assembler.absorb(&chunk.to_string())?);
    }
    events.extend(assembler.finalize()?);
    Ok(events)
}

fn usages(events: &[ProviderEvent]) -> Vec<Counts> {
    events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::Usage {
                input,
                output,
                cache_read,
                cache_write,
            } => Some((*input, *output, *cache_read, *cache_write)),
            _ => None,
        })
        .collect()
}

fn assert_protocol_without_done(chunks: &[Value], label: &str) {
    let mut assembler = SseAssembler::default();
    let mut events = Vec::new();
    let mut failure = None;
    for chunk in chunks {
        match assembler.absorb(&chunk.to_string()) {
            Ok(next) => events.extend(next),
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    if failure.is_none() {
        match assembler.finalize() {
            Ok(next) => events.extend(next),
            Err(error) => failure = Some(error),
        }
    }
    assert!(
        matches!(
            failure,
            Some(VegaError::ProviderDiagnostic {
                kind: ProviderFailureKind::Protocol,
                retryable: false,
                ..
            })
        ),
        "{label}: {failure:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ProviderEvent::Done { .. })),
        "{label}"
    );
}

fn usage_stream(usage: Value) -> Vec<Value> {
    let mut terminal = chunk(json!({"content":"Complete"}), json!("stop"));
    terminal["usage"] = usage;
    vec![terminal]
}

#[test]
fn terminal_cache_aliases_preserve_text_reasoning_ordered_calls_and_usage() {
    let events = decode(&two_reads(alias_usage())).unwrap();
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::TextDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let thinking: String = events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::ThinkingDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let calls: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::ToolUse {
                id,
                name,
                input_json,
            } => Some((id.as_str(), name.as_str(), input_json.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Inspecting files. ");
    assert_eq!(thinking, "plan reads");
    assert_eq!(
        calls,
        vec![
            (CALLS[0].0, "read", CALLS[0].1),
            (CALLS[1].0, "read", CALLS[1].1)
        ]
    );
    assert_eq!(usages(&events), vec![(100, 9, Some(70), None)]);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse
                }
            ))
            .count(),
        1
    );
    assert!(matches!(
        events.last(),
        Some(ProviderEvent::Done {
            stop_reason: StopReason::ToolUse
        })
    ));
}

#[test]
fn cache_read_aliases_choose_first_nonnull_number_and_keep_explicit_zero() {
    let rows = [
        (
            "nested",
            json!({"prompt_tokens_details":{"cached_tokens":70},"prompt_cache_hit_tokens":60,"cached_tokens":0}),
            Some(70),
        ),
        (
            "nested-zero",
            json!({"prompt_tokens_details":{"cached_tokens":0},"prompt_cache_hit_tokens":70,"cached_tokens":60}),
            Some(0),
        ),
        (
            "nested-null",
            json!({"prompt_tokens_details":{"cached_tokens":null},"prompt_cache_hit_tokens":70,"cached_tokens":0}),
            Some(70),
        ),
        (
            "hit-null",
            json!({"prompt_cache_hit_tokens":null,"cached_tokens":70}),
            Some(70),
        ),
        (
            "hit-zero",
            json!({"prompt_cache_hit_tokens":0,"cached_tokens":70}),
            Some(0),
        ),
        ("absent", json!({}), None),
        (
            "all-null",
            json!({"prompt_tokens_details":{"cached_tokens":null},"prompt_cache_hit_tokens":null,"cached_tokens":null}),
            None,
        ),
    ];
    for (label, mut usage, expected) in rows {
        usage["prompt_tokens"] = json!(100);
        usage["completion_tokens"] = json!(9);
        usage["prompt_cache_miss_tokens"] = json!(17);
        let events =
            decode(&usage_stream(usage)).unwrap_or_else(|error| panic!("{label}: {error:?}"));
        assert_eq!(usages(&events), vec![(100, 9, expected, None)], "{label}");
        assert!(
            matches!(
                events.last(),
                Some(ProviderEvent::Done {
                    stop_reason: StopReason::End
                })
            ),
            "{label}"
        );
    }
}

#[test]
fn cache_write_aliases_preserve_total_input_output_and_separate_read_write_counts() {
    let rows = [
        (
            "nested",
            json!({"prompt_tokens_details":{"cache_write_tokens":30},"cache_write_tokens":0}),
            Some(30),
        ),
        (
            "nested-zero",
            json!({"prompt_tokens_details":{"cache_write_tokens":0},"cache_write_tokens":30}),
            Some(0),
        ),
        (
            "nested-null",
            json!({"prompt_tokens_details":{"cache_write_tokens":null},"cache_write_tokens":30}),
            Some(30),
        ),
        ("absent", json!({}), None),
        (
            "all-null",
            json!({"prompt_tokens_details":{"cache_write_tokens":null},"cache_write_tokens":null}),
            None,
        ),
    ];
    for (label, mut usage, expected) in rows {
        usage["prompt_tokens"] = json!(100);
        usage["completion_tokens"] = json!(5);
        usage["prompt_cache_hit_tokens"] = json!(50);
        usage["completion_tokens_details"] = json!({"reasoning_tokens":3});
        let events =
            decode(&usage_stream(usage)).unwrap_or_else(|error| panic!("{label}: {error:?}"));
        assert_eq!(
            usages(&events),
            vec![(100, 5, Some(50), expected)],
            "{label}"
        );
    }
}

#[test]
fn every_supported_counter_is_validated_even_when_an_earlier_alias_is_selected() {
    let fields = [
        "/prompt_tokens",
        "/completion_tokens",
        "/prompt_tokens_details/cached_tokens",
        "/prompt_cache_hit_tokens",
        "/cached_tokens",
        "/prompt_tokens_details/cache_write_tokens",
        "/cache_write_tokens",
        "/prompt_cache_miss_tokens",
    ];
    let malformed = [
        json!(-1),
        json!(0.5),
        json!("7"),
        json!(true),
        json!({}),
        json!([]),
        serde_json::from_str::<Value>("18446744073709551616").unwrap(),
    ];
    for field in fields {
        for value in &malformed {
            let mut usage = json!({"prompt_tokens":100,"completion_tokens":5,
                "prompt_tokens_details":{"cached_tokens":20,"cache_write_tokens":10},
                "prompt_cache_hit_tokens":20,"cached_tokens":20,"cache_write_tokens":10,
                "prompt_cache_miss_tokens":80});
            *usage.pointer_mut(field).unwrap() = value.clone();
            assert_protocol_without_done(&two_reads(usage), &format!("{field}={value}"));
        }
    }
    for field in ["prompt_tokens", "completion_tokens"] {
        for missing in [false, true] {
            let mut usage = equal_usage();
            if missing {
                usage.as_object_mut().unwrap().remove(field);
            } else {
                usage[field] = Value::Null;
            }
            assert_protocol_without_done(&two_reads(usage), &format!("{field} missing={missing}"));
        }
    }
}

#[test]
fn selected_cache_accounting_and_independent_miss_ranges_remain_checked() {
    let cases = [
        (
            "read-over-input",
            json!({"prompt_tokens":100,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":101}}),
        ),
        (
            "write-over-input",
            json!({"prompt_tokens":100,"completion_tokens":5,"cache_write_tokens":101}),
        ),
        (
            "sum-over-input",
            json!({"prompt_tokens":100,"completion_tokens":5,"cached_tokens":60,"cache_write_tokens":50}),
        ),
        (
            "sum-overflow",
            json!({"prompt_tokens":u64::MAX,"completion_tokens":5,"cached_tokens":u64::MAX,"cache_write_tokens":1}),
        ),
        (
            "miss-over-input",
            json!({"prompt_tokens":100,"completion_tokens":5,"prompt_cache_miss_tokens":101}),
        ),
    ];
    for (label, usage) in cases {
        assert_protocol_without_done(&two_reads(usage), label);
    }
}

#[test]
fn usage_locations_and_progressive_snapshots_emit_only_the_final_normalized_usage() {
    for location in ["chunk", "choice", "both"] {
        let mut first = chunk(json!({"content":"Early "}), json!(""));
        first["usage"] = json!({"prompt_tokens":50,"completion_tokens":1,"prompt_tokens_details":{"cached_tokens":20},"cached_tokens":0});
        let mut last = chunk(json!({"content":"complete"}), json!("stop"));
        if location != "choice" {
            last["usage"] = alias_usage();
        }
        if location != "chunk" {
            last["choices"][0]["usage"] = json!({"prompt_tokens":100,"completion_tokens":9,
                "prompt_tokens_details":{"cached_tokens":70},"prompt_cache_hit_tokens":50,"cached_tokens":60,"prompt_cache_miss_tokens":17});
        }
        let events = decode(&[first, last]).unwrap_or_else(|error| panic!("{location}: {error:?}"));
        assert_eq!(
            usages(&events),
            vec![(100, 9, Some(70), None)],
            "{location}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProviderEvent::Done { .. }))
                .count(),
            1,
            "{location}"
        );
    }
}

#[test]
fn separate_usage_objects_still_reject_different_normalized_totals_or_cache_counts() {
    for field in ["prompt_tokens", "completion_tokens", "cached", "write"] {
        let mut chunks = two_reads(equal_usage());
        let mut other = equal_usage();
        match field {
            "prompt_tokens" => other["prompt_tokens"] = json!(101),
            "completion_tokens" => other["completion_tokens"] = json!(10),
            "cached" => {
                other["prompt_tokens_details"]["cached_tokens"] = json!(60);
                other["prompt_cache_hit_tokens"] = json!(60);
                other["cached_tokens"] = json!(60);
                other["prompt_cache_miss_tokens"] = json!(40);
            }
            "write" => other["cache_write_tokens"] = json!(10),
            _ => unreachable!(),
        }
        chunks[2]["choices"][0]["usage"] = other;
        assert_protocol_without_done(&chunks, field);
    }
}

fn invalid_rounds() -> Vec<(&'static str, Vec<Value>)> {
    let mut rows = Vec::new();
    let mut chunks = two_reads(equal_usage());
    chunks[2]["usage"]["cached_tokens"] = json!("not-a-number");
    rows.push(("shadowed-malformed-cache", chunks));
    let mut chunks = two_reads(equal_usage());
    chunks[2]["usage"]["prompt_tokens_details"]["cached_tokens"] = json!(101);
    rows.push(("selected-cache-over-input", chunks));
    let mut chunks = two_reads(equal_usage());
    chunks[2]["usage"]["prompt_cache_miss_tokens"] = json!(101);
    rows.push(("miss-over-input", chunks));
    let mut chunks = two_reads(equal_usage());
    let mut other = equal_usage();
    other["completion_tokens"] = json!(10);
    chunks[2]["choices"][0]["usage"] = other;
    rows.push(("independent-usage-conflict", chunks));
    let mut chunks = two_reads(equal_usage());
    chunks[2]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"] = json!("]");
    rows.push(("malformed-tool-json", chunks));
    let mut chunks = two_reads(equal_usage());
    chunks[0]["choices"][0]["delta"]["tool_calls"][0]["id"] = json!("read-alpha");
    rows.push(("cross-index-identity-conflict", chunks));
    let mut chunks = two_reads(equal_usage());
    chunks[1]["choices"][0]["delta"]["tool_calls"][0]["function"]["name"] = json!("write");
    rows.push(("function-name-conflict", chunks));
    let mut chunks = two_reads(equal_usage());
    chunks[2]["choices"][0]["finish_reason"] = Value::Null;
    rows.push(("missing-terminal", chunks));
    let mut chunks = two_reads(equal_usage());
    chunks[2]["choices"][0]["finish_reason"] = json!("unrecognized-terminal");
    rows.push(("unknown-terminal", chunks));
    let mut chunks = two_reads(equal_usage());
    chunks.push(chunk(json!({"content":"late mutation"}), Value::Null));
    rows.push(("post-terminal-mutation", chunks));
    rows
}

#[test]
fn ordinary_text_and_single_cache_fields_keep_valid_terminals_while_bad_tool_streams_fail() {
    let ordinary = decode(&[chunk(json!({"content":"Complete"}), json!("stop"))]).unwrap();
    assert!(usages(&ordinary).is_empty());
    assert!(matches!(
        ordinary.last(),
        Some(ProviderEvent::Done {
            stop_reason: StopReason::End
        })
    ));
    for usage in [
        json!({"prompt_tokens":100,"completion_tokens":5,"cached_tokens":70}),
        json!({"prompt_tokens":100,"completion_tokens":5,"cache_write_tokens":30}),
    ] {
        let events = decode(&usage_stream(usage)).unwrap();
        assert_eq!(usages(&events).len(), 1);
        assert!(matches!(
            events.last(),
            Some(ProviderEvent::Done {
                stop_reason: StopReason::End
            })
        ));
    }
    for (label, chunks) in invalid_rounds() {
        assert_protocol_without_done(&chunks, label);
    }
}
