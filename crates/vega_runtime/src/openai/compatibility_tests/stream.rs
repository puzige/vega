use super::*;

#[test]
fn empty_or_null_finish_frames_do_not_terminate_text_reasoning_or_tools() {
    for first in [
        json!({"choices":[{"delta":{}}]}),
        chat_chunk(json!({}), Value::Null),
        chat_chunk(json!({}), json!("")),
    ] {
        let chunks = vec![
            first,
            chat_chunk(
                json!({"reasoning_content":"private calculation"}),
                Value::Null,
            ),
            chat_chunk(json!({"content":"Visible answer"}), json!("")),
            chat_chunk(
                json!({"tool_calls":[{"index":0,"id":"call-a","type":"function","function":{"name":"read","arguments":"{\"path\":"}}]}),
                Value::Null,
            ),
            chat_chunk(
                json!({"tool_calls":[{"index":0,"function":{"arguments":"\"README.md\"}"}}]}),
                json!("tool_calls"),
            ),
        ];
        let events = decode_chat(&chunks, true).unwrap();
        assert_eq!(text(&events), "Visible answer");
        assert_eq!(thinking(&events), "private calculation");
        assert_eq!(
            calls(&events),
            vec![(
                "call-a".into(),
                "read".into(),
                r#"{"path":"README.md"}"#.into()
            )]
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse
                    }
                ))
                .count(),
            1
        );
    }
}

#[test]
fn null_chunks_are_ignored_and_null_or_empty_only_eof_still_fails() {
    let events = decode_chat(
        &[
            Value::Null,
            chat_chunk(json!({"content":"OK"}), Value::Null),
            Value::Null,
            chat_chunk(json!({}), json!("stop")),
        ],
        true,
    )
    .unwrap();
    assert_eq!(text(&events), "OK");
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ProviderEvent::Done { .. }))
            .count(),
        1
    );
    for unfinished in [Value::Null, json!("")] {
        assert!(
            decode_chat(
                &[chat_chunk(json!({"content":"partial"}), unfinished)],
                true
            )
            .is_err()
        );
    }
}

#[test]
fn stable_index_retains_first_nonempty_call_id_and_complete_original_arguments() {
    let chunks = vec![
        chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"functions.read:0","type":"function","function":{"name":"read","arguments":""}}]}),
            Value::Null,
        ),
        chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"chatcmpl-tool-a","type":"function","function":{"name":null,"arguments":"{\"path\":\"README"}}]}),
            Value::Null,
        ),
        chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"chatcmpl-tool-b","type":"function","function":{"name":null,"arguments":".md\"}"}}]}),
            json!("tool_calls"),
        ),
    ];
    let events = decode_chat(&chunks, true).unwrap();
    assert_eq!(
        calls(&events),
        vec![(
            "functions.read:0".into(),
            "read".into(),
            r#"{"path":"README.md"}"#.into()
        )]
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse
                }
            ))
            .count(),
        1
    );
    assert!(!format!("{events:?}").contains("README.md"));
}

#[test]
fn parallel_slots_content_and_reasoning_accumulate_independently() {
    let chunks = vec![
        chat_chunk(
            json!({"content":"Before ","reasoning":"plan ","tool_calls":[
                {"index":1,"id":"b","function":{"name":"read","arguments":"{\"path\":\"b"}},
                {"index":0,"id":"a","function":{"name":"read","arguments":"{\"path\":\"a"}},
                {"id":"c","function":{"name":"list","arguments":"{\"path\":\"packages"}},
                {"id":"d","function":{"name":"write","arguments":"{\"path\":\"out.txt\","}}
            ]}),
            Value::Null,
        ),
        chat_chunk(
            json!({"content":"after","reasoning":"execute","tool_calls":[
                {"index":0,"function":{"arguments":".txt\"}"}},
                {"index":1,"function":{"arguments":".txt\"}"}},
                {"id":"c","function":{"arguments":"/ai\"}"}},
                {"id":"d","function":{"arguments":"\"content\":\"ok\"}"}}
            ]}),
            json!("tool_calls"),
        ),
    ];
    let events = decode_chat(&chunks, true).unwrap();
    assert_eq!(text(&events), "Before after");
    assert_eq!(thinking(&events), "plan execute");
    assert_eq!(
        calls(&events),
        vec![
            ("a".into(), "read".into(), r#"{"path":"a.txt"}"#.into()),
            ("b".into(), "read".into(), r#"{"path":"b.txt"}"#.into()),
            (
                "c".into(),
                "list".into(),
                r#"{"path":"packages/ai"}"#.into()
            ),
            (
                "d".into(),
                "write".into(),
                r#"{"path":"out.txt","content":"ok"}"#.into()
            )
        ]
    );
}

#[test]
fn empty_custom_objects_do_not_replace_function_calls() {
    let events=decode_chat(&[
        chat_chunk(json!({"tool_calls":[{"index":0,"id":"a","type":"function","custom":{},"function":{"name":"read","arguments":"{\"path\":\"README.md\"}"}}]}),json!("tool_calls"))
    ],true).unwrap();
    assert_eq!(
        calls(&events),
        vec![("a".into(), "read".into(), r#"{"path":"README.md"}"#.into())]
    );
}

#[test]
fn conflicting_slots_names_and_unrepaired_final_json_fail_before_call_release() {
    let cases = vec![
        vec![chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"same","function":{"name":"read","arguments":"{}"}},{"index":1,"id":"same","function":{"name":"read","arguments":"{}"}}]}),
            json!("tool_calls"),
        )],
        vec![
            chat_chunk(
                json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"read","arguments":"{"}}]}),
                Value::Null,
            ),
            chat_chunk(
                json!({"tool_calls":[{"index":0,"function":{"name":"write","arguments":"}"}}]}),
                json!("tool_calls"),
            ),
        ],
        vec![chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"read","arguments":"{\"path\":\"README.md\""}}]}),
            json!("tool_calls"),
        )],
        vec![chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"read","arguments":"{}"}}]}),
            json!("stop"),
        )],
        vec![chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"read","arguments":"{}"}}]}),
            Value::Null,
        )],
    ];
    for chunks in cases {
        let mut assembler = super::super::sse::SseAssembler::default();
        let mut events = Vec::new();
        let mut error = false;
        for chunk in chunks {
            match assembler.absorb(&chunk.to_string()) {
                Ok(next) => events.extend(next),
                Err(_) => {
                    error = true;
                    break;
                }
            }
        }
        if !error {
            match assembler.finalize() {
                Ok(next) => events.extend(next),
                Err(_) => error = true,
            }
        }
        assert!(error);
        assert!(calls(&events).is_empty());
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ProviderEvent::Done { .. }))
        );
    }
}

#[test]
fn unknown_nonempty_finish_and_mutation_after_terminal_are_rejected() {
    for reason in ["provider-custom", "content_filter", "error", "tool_result"] {
        assert!(
            decode_chat(
                &[chat_chunk(json!({"content":"partial"}), json!(reason))],
                true
            )
            .is_err(),
            "{reason}"
        );
    }
    assert!(
        decode_chat(
            &[
                chat_chunk(json!({"content":"done"}), json!("stop")),
                chat_chunk(json!({"content":"changed"}), Value::Null)
            ],
            true
        )
        .is_err()
    );
}

#[test]
fn original_reasoning_channel_presence_is_preserved_without_duplicate_alias_text() {
    for field in ["reasoning_content", "reasoning", "reasoning_text"] {
        let mut delta = json!({});
        delta[field] = json!("raw-source");
        let events = decode_chat(
            &[
                chat_chunk(delta, Value::Null),
                chat_chunk(json!({"content":"answer"}), json!("stop")),
            ],
            true,
        )
        .unwrap();
        assert_eq!(thinking(&events), "raw-source");
        assert_eq!(text(&events), "answer");
        if field == "reasoning_content" {
            assert!(events.iter().any(|e|matches!(e,ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::NonEmpty(s)) if s=="raw-source")));
        } else {
            assert!(events.iter().any(|e|matches!(e,ProviderEvent::ChatReasoningState(raw) if raw.text.as_ref().is_some_and(|(source,s)|source.field()==field&&s=="raw-source"))));
        }
        assert!(!format!("{events:?}").contains("raw-source"));
    }
    let absent = decode_chat(&[chat_chunk(json!({}), json!("stop"))], true).unwrap();
    let empty = decode_chat(
        &[chat_chunk(json!({"reasoning_content":""}), json!("stop"))],
        true,
    )
    .unwrap();
    assert!(absent.iter().any(|e| matches!(
        e,
        ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::Absent)
    )));
    assert!(empty.iter().any(|e| matches!(
        e,
        ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::Empty)
    )));
    let equal = decode_chat(
        &[chat_chunk(
            json!({"reasoning":"one","reasoning_content":"one"}),
            json!("stop"),
        )],
        true,
    )
    .unwrap();
    assert_eq!(thinking(&equal), "one");
    assert!(
        decode_chat(
            &[chat_chunk(
                json!({"reasoning":"one","reasoning_content":"different"}),
                json!("stop")
            )],
            true
        )
        .is_err()
    );
    for malformed in [json!(false), json!({}), json!([]), json!(3)] {
        assert!(
            decode_chat(
                &[chat_chunk(
                    json!({"reasoning_content":malformed}),
                    json!("stop")
                )],
                true
            )
            .is_err()
        );
    }
}

#[test]
fn signed_reasoning_detail_order_and_fragment_identity_are_lossless() {
    let events=decode_chat(&[
        chat_chunk(json!({"reasoning_details":[{"type":"reasoning.text","id":"r","index":0,"text":"private ","signature":"sig"}]}),Value::Null),
        chat_chunk(json!({"reasoning_details":[{"type":"reasoning.text","id":"r","index":0,"text":"text","signature":"sig"},{"type":"reasoning.summary","id":"s","index":1,"summary":"summary"}]}),Value::Null),
        chat_chunk(json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"e","index":2,"data":"opaque+/=","format":"opaque-v1"}]}),json!("stop")),
    ],true).unwrap();
    let raw = events
        .iter()
        .find_map(|event| match event {
            ProviderEvent::ChatReasoningState(raw) => Some(raw),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        raw.details.as_ref().unwrap(),
        &vec![
            json!({"type":"reasoning.text","id":"r","index":0,"text":"private text","signature":"sig"}),
            json!({"type":"reasoning.summary","id":"s","index":1,"summary":"summary"}),
            json!({"type":"reasoning.encrypted","id":"e","index":2,"data":"opaque+/=","format":"opaque-v1"}),
        ]
    );
    assert!(!format!("{raw:?}").contains("opaque+/="));
    for detail in [
        json!({"type":"reasoning.text","id":"r","index":1,"text":"conflict"}),
        json!({"type":"reasoning.text","id":"other","index":0,"text":"conflict"}),
        json!({"type":"reasoning.unknown","text":"invalid"}),
    ] {
        assert!(decode_chat(&[chat_chunk(json!({"reasoning_details":[{"type":"reasoning.text","id":"r","index":0,"text":"one"},detail]}),json!("stop"))],true).is_err());
    }
}

#[test]
fn usage_tracks_total_input_caches_and_progressive_snapshots_without_double_output() {
    for choice_usage in [false, true] {
        let usage = json!({"prompt_tokens":20,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":2,"cache_write_tokens":3},"completion_tokens_details":{"reasoning_tokens":5}});
        let mut chunk = chat_chunk(json!({"content":"OK"}), json!("stop"));
        if choice_usage {
            chunk["choices"][0]["usage"] = usage;
        } else {
            chunk["usage"] = usage;
        }
        let events = decode_chat(&[chunk], true).unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            ProviderEvent::Usage {
                input: 20,
                output: 7,
                cache_read: Some(2),
                cache_write: Some(3)
            }
        )));
    }
    let events=decode_chat(&[
        json!({"choices":[{"delta":{"content":"O"},"finish_reason":null}],"usage":{"prompt_tokens":10,"completion_tokens":1}}),
        json!({"choices":[{"delta":{"content":"K"},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":7}}),
    ],true).unwrap();
    assert!(events.iter().any(|e| matches!(
        e,
        ProviderEvent::Usage {
            input: 20,
            output: 7,
            ..
        }
    )));
    let conflict = json!({"choices":[{"delta":{},"finish_reason":"stop","usage":{"prompt_tokens":9,"completion_tokens":2}}],"usage":{"prompt_tokens":10,"completion_tokens":2}});
    assert!(decode_chat(&[conflict], true).is_err());
}

#[test]
fn fragmented_unicode_visible_text_is_not_mixed_with_private_reasoning() {
    let events = decode_chat(
        &[
            chat_chunk(
                json!({"content":"你好，","reasoning_content":"私有"}),
                Value::Null,
            ),
            chat_chunk(
                json!({"content":"🌍\n","reasoning_content":"思考"}),
                json!(""),
            ),
            chat_chunk(json!({"content":"```rust\nlet n = 1;\n```"}), json!("stop")),
        ],
        true,
    )
    .unwrap();
    assert_eq!(text(&events), "你好，🌍\n```rust\nlet n = 1;\n```");
    assert_eq!(thinking(&events), "私有思考");
    assert!(!text(&events).contains("私有"));
}
