use super::*;

fn payload() -> Vec<u8> {
    br#"{"official":{"id":"official","name":"Official","models":{"model-a":{"id":"model-a","name":"Model A","canonical_model_id":"lab/model-a","limit":{"context":200000,"input":150000,"output":50000},"modalities":{"input":["text"],"output":["text"]},"reasoning":true,"tool_call":true,"reasoning_options":[{"type":"effort","values":["low","high"]}],"cost":{"input":1,"output":2,"cache_read":0.1,"cache_write":0.2}}}}}"#.to_vec()
}
fn owner(provider: &str) -> CatalogIdentity {
    CatalogIdentity {
        provider: provider.into(),
        model: "model-a".into(),
    }
}
fn request(provider: &str, action: CatalogAction) -> ModelCatalogRequested {
    ModelCatalogRequested {
        generation: 1,
        owner: owner(provider),
        action,
    }
}

#[test]
fn issue296_catalog_exact_explicit_mapping_never_uses_display_name() {
    let root = tempfile::tempdir().unwrap();
    let catalog = refresh_with(root.path(), &owner("official"), 1, || Ok(payload()));
    assert!(catalog.mapped.is_none());
    let mapped = load(
        root.path(),
        &request("relay", CatalogAction::Map(owner("official"))),
        1,
    );
    assert_eq!(mapped.mapped, Some(owner("official")));
    assert_eq!(mapped.matched_model().unwrap().input, Some(150000));
    assert!(
        load(root.path(), &request("Relay", CatalogAction::Load), 1)
            .mapped
            .is_none()
    );
    assert!(
        load(
            root.path(),
            &request("relay", CatalogAction::Map(owner("Official"))),
            1
        )
        .error
        .is_some()
    );
}

#[test]
fn issue296_catalog_same_model_two_providers_keep_independent_mappings() {
    let root = tempfile::tempdir().unwrap();
    refresh_with(root.path(), &owner("relay-a"), 1, || Ok(payload()));
    for local in ["relay-a", "relay-b"] {
        assert_eq!(
            load(
                root.path(),
                &request(local, CatalogAction::Map(owner("official"))),
                1
            )
            .mapped,
            Some(owner("official"))
        );
    }
    assert!(
        load(root.path(), &request("relay-a", CatalogAction::Unmap), 1)
            .mapped
            .is_none()
    );
    assert_eq!(
        load(root.path(), &request("relay-b", CatalogAction::Load), 1).mapped,
        Some(owner("official"))
    );
}

#[test]
fn issue296_catalog_cache_expiry_and_failed_refresh_keep_last_valid_bytes() {
    let root = tempfile::tempdir().unwrap();
    refresh_with(root.path(), &owner("relay"), 10, || Ok(payload()));
    let original = std::fs::read(root.path().join("catalog.json")).unwrap();
    for failure in [
        Err("timeout".into()),
        Ok(b"malformed".to_vec()),
        Ok(vec![b' '; MAX_BYTES + 1]),
    ] {
        let result = refresh_with(root.path(), &owner("relay"), TTL_MS + 10, || failure);
        assert!(result.stale && result.error.is_some());
        assert_eq!(result.providers[0].models[0].context, 200000);
        assert_eq!(
            std::fs::read(root.path().join("catalog.json")).unwrap(),
            original
        );
    }
    assert!(
        !load(
            root.path(),
            &request("relay", CatalogAction::Load),
            TTL_MS + 9
        )
        .stale
    );
    assert!(
        load(
            root.path(),
            &request("relay", CatalogAction::Load),
            TTL_MS + 10
        )
        .stale
    );
    std::fs::write(root.path().join("catalog.json"), b"{}").unwrap();
    let corrupt = load(
        root.path(),
        &request("relay", CatalogAction::Load),
        TTL_MS + 10,
    );
    assert!(corrupt.providers.is_empty() && corrupt.error.is_some());
}

#[test]
fn issue296_catalog_schema_distinguishes_absence_zero_null_and_complex_prices() {
    let mut value: serde_json::Value = serde_json::from_slice(&payload()).unwrap();
    let model = &mut value["official"]["models"]["model-a"];
    model["limit"]["context"] = 0.into();
    model["limit"].as_object_mut().unwrap().remove("input");
    model["reasoning_options"] = serde_json::json!([{"type":"effort","values":[null,"default","none","high"]},{"type":"budget_tokens","min":-1,"max":5000}]);
    model["cost"]["tiers"] = serde_json::json!([]);
    model["future_field"] = true.into();
    let parsed = parse::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    let model = &parsed[0].models[0];
    assert_eq!((model.context, model.input, model.output), (0, None, 50000));
    assert!(model.cost.as_ref().unwrap().complex);
    assert!(
        matches!(&model.reasoning_options[0], CatalogReasoningOption::Effort(values) if values[0].is_none() && values[1].as_deref() == Some("default"))
    );
    value["official"]["models"]["model-a"]
        .as_object_mut()
        .unwrap()
        .remove("cost");
    assert!(
        parse::parse(&serde_json::to_vec(&value).unwrap()).unwrap()[0].models[0]
            .cost
            .is_none()
    );
    value["official"]["models"]["model-a"]["id"] = "different".into();
    assert!(parse::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}
