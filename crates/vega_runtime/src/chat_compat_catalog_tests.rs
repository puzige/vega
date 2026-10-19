use serde_json::json;

#[test]
fn catalog_decoder_accepts_supported_records_and_equal_duplicate_identities() {
    let model = json!({"provider":"fixture","api":"openai-completions","id":"sample","baseUrl":"https://fixture.example/v1","reasoning":true,"thinkingLevelMap":{"off":null,"low":"low","high":"high"},"compat":{"thinkingFormat":"openai","supportsReasoningEffort":true,"maxTokensField":"max_tokens"}});
    assert!(
        super::decode_catalog(
            &serde_json::to_vec(&json!({"version":1,"models":[model.clone()]}))
                .expect("catalog bytes")
        )
        .is_ok()
    );
    let mut duplicate = model.clone();
    duplicate["provider"] = json!("fixture-alias");
    assert!(
        super::decode_catalog(
            &serde_json::to_vec(&json!({"version":1,"models":[model,duplicate]}))
                .expect("duplicate bytes")
        )
        .is_ok()
    );
}

#[test]
fn catalog_decoder_rejects_unsupported_dialects_templates_mappings_and_conflicting_identity() {
    let valid = json!({"provider":"fixture","api":"openai-completions","id":"sample","baseUrl":"https://fixture.example/v1","reasoning":true,"compat":{"thinkingFormat":"openai"}});
    for invalid in [
        json!({"compat":{"thinkingFormat":"unimplemented"}}),
        json!({"compat":{"thinkingFormat":"chat-template"}}),
        json!({"compat":{"thinkingFormat":"baseten","chatTemplateArgs":{"enable_thinking":{"$var":"unknown.variable"}}}}),
        json!({"thinkingLevelMap":{"high":true}}),
        json!({"thinkingLevelMap":{"other":"high"}}),
        json!({"compat":{"maxTokensField":"unknown_cap"}}),
    ] {
        let mut record = valid.clone();
        record
            .as_object_mut()
            .expect("model object")
            .extend(invalid.as_object().expect("invalid object").clone());
        assert!(
            super::decode_catalog(
                &serde_json::to_vec(&json!({"version":1,"models":[record]}))
                    .expect("invalid bytes")
            )
            .is_err()
        );
    }
    let mut conflicting = valid.clone();
    conflicting["compat"]["thinkingFormat"] = json!("qwen");
    assert!(
        super::decode_catalog(
            &serde_json::to_vec(&json!({"version":1,"models":[valid,conflicting]}))
                .expect("conflict bytes")
        )
        .is_err()
    );
    assert!(super::decode_catalog(br#"{"version":2,"models":[]}"#).is_err());
    assert!(super::decode_catalog(b"{malformed").is_err());
}
