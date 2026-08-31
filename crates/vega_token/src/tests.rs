use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;

use crate::{
    CatalogLoad, ModelPricingSpec, PricingCatalog, PricingError, PricingProfile, RateSpec,
    UsageCounts, catalog, load_catalog, load_catalog_snapshot, load_or_seed_catalog, persistence,
    save_catalog_atomic,
};

fn static_spec(
    model: &str,
    input: &str,
    output: &str,
    read: &str,
    write: &str,
) -> ModelPricingSpec {
    ModelPricingSpec {
        model: model.to_string(),
        rates: RateSpec {
            input_usd_per_million: input.to_string(),
            output_usd_per_million: output.to_string(),
            cache_read_usd_per_million: read.to_string(),
            cache_write_usd_per_million: write.to_string(),
        },
        max_standard_input_tokens: None,
        schedule: None,
    }
}

fn distinct_old_catalog() -> PricingCatalog {
    PricingCatalog::from_specs(vec![static_spec(
        "old-custom-model",
        "0.11",
        "0.22",
        "0.03",
        "0.44",
    )])
    .unwrap()
}

fn valid_model_json(model: &str) -> Value {
    json!({
        "model": model,
        "input_usd_per_million": "1",
        "output_usd_per_million": "2",
        "cache_read_usd_per_million": "0.1",
        "cache_write_usd_per_million": "3"
    })
}

fn valid_schedule_json() -> Value {
    json!({
        "kind": "utc_weekly_v1",
        "windows": [{"weekdays":[1],"start_minute":60,"end_minute":120}],
        "peak": {
            "input_usd_per_million":"2",
            "output_usd_per_million":"2",
            "cache_read_usd_per_million":"2",
            "cache_write_usd_per_million":"2"
        }
    })
}

fn catalog_json(models: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": "pricing_v1",
        "currency": "USD",
        "models": models
    }))
    .unwrap()
}

fn rate_values(rates: &RateSpec) -> [&str; 4] {
    [
        &rates.input_usd_per_million,
        &rates.cache_read_usd_per_million,
        &rates.output_usd_per_million,
        &rates.cache_write_usd_per_million,
    ]
}

#[test]
fn builtins_are_exact_five_and_cover_three_families() {
    let catalog = PricingCatalog::built_in().unwrap();
    let specs: Vec<_> = catalog.specs().collect();
    assert_eq!(specs.len(), 5);
    assert_eq!(
        specs
            .iter()
            .map(|spec| spec.model.as_str())
            .collect::<Vec<_>>(),
        [
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "deepseek-v4-flash",
            "deepseek-v4-pro",
            "claude-sonnet-5"
        ]
    );
    assert_eq!(specs[0].rates.input_usd_per_million, "2.00");
    assert_eq!(specs[0].rates.cache_write_usd_per_million, "2.50");
    assert_eq!(specs[1].rates.output_usd_per_million, "1.20");
    assert_eq!(specs[2].rates.cache_read_usd_per_million, "0.007");
    assert_eq!(specs[3].rates.output_usd_per_million, "1.98");
    assert_eq!(specs[4].rates.cache_write_usd_per_million, "2.50");
}

#[test]
fn builtins_exhaustively_match_every_base_and_peak_source_rate() {
    let catalog = PricingCatalog::built_in().unwrap();
    let expected = [
        ("gpt-5.6-terra", ["2.00", "0.20", "12.00", "2.50"], None),
        ("gpt-5.6-luna", ["0.20", "0.02", "1.20", "0.25"], None),
        (
            "deepseek-v4-flash",
            ["0.22", "0.007", "0.66", "0"],
            Some(["0.44", "0.014", "1.32", "0"]),
        ),
        (
            "deepseek-v4-pro",
            ["0.66", "0.022", "1.98", "0"],
            Some(["1.32", "0.044", "3.96", "0"]),
        ),
        ("claude-sonnet-5", ["2.00", "0.20", "10.00", "2.50"], None),
    ];
    for (model, base, peak) in expected {
        let spec = catalog.specs().find(|spec| spec.model == model).unwrap();
        assert_eq!(rate_values(&spec.rates), base);
        assert_eq!(
            spec.schedule
                .as_ref()
                .map(|schedule| rate_values(&schedule.peak)),
            peak
        );
    }
}

#[test]
fn missing_file_seeds_and_reopens_without_touching_global_data() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("pricing.json");
    let first = load_or_seed_catalog(&path).unwrap();
    assert!(first.seeded);
    assert!(!first.durability_unknown);
    assert_eq!(first.catalog.specs().len(), 5);
    let first_bytes = fs::read(&path).unwrap();

    let second = load_or_seed_catalog(&path).unwrap();
    assert!(!second.seeded);
    assert!(!second.durability_unknown);
    assert_eq!(first.catalog, second.catalog);
    assert_eq!(fs::read(path).unwrap(), first_bytes);
}

#[test]
fn ambiguous_initial_seed_reloads_once_without_blind_retry() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("pricing.json");
    let loaded = persistence::load_or_seed_with_postcommit_failure(&path).unwrap();
    assert!(loaded.seeded);
    assert!(loaded.durability_unknown);
    assert_eq!(loaded.catalog, load_catalog(&path).unwrap());
}

#[test]
fn catalog_snapshot_requires_semantic_and_byte_exact_match() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("pricing.json");
    let catalog = PricingCatalog::built_in().unwrap();
    save_catalog_atomic(&path, &catalog).unwrap();
    let snapshot = load_catalog_snapshot(&path).unwrap();
    assert!(snapshot.exactly_matches(&catalog).unwrap());

    let semantically_equal =
        serde_json::to_vec(&serde_json::from_slice::<Value>(&catalog.encode().unwrap()).unwrap())
            .unwrap();
    fs::write(&path, semantically_equal).unwrap();
    let snapshot = load_catalog_snapshot(&path).unwrap();
    assert_eq!(snapshot.catalog(), &catalog);
    assert!(!snapshot.exactly_matches(&catalog).unwrap());
    assert!(!format!("{snapshot:?}").contains("gpt-5.6"));
}

#[test]
fn strict_codec_rejects_missing_extra_duplicate_and_wrong_types() {
    let cases = [
        br#"{"currency":"USD","models":[]}"#.as_slice(),
        br#"{"schema_version":"pricing_v1","models":[]}"#.as_slice(),
        br#"{"schema_version":"pricing_v1","currency":"USD","models":[],"extra":1}"#.as_slice(),
        br#"{"schema_version":"pricing_v1","schema_version":"pricing_v1","currency":"USD","models":[]}"#.as_slice(),
        br#"{"schema_version":"pricing_v1","currency":"USD","models":"no"}"#.as_slice(),
        br#"{"schema_version":"pricing_v1","currency":"EUR","models":[]}"#.as_slice(),
    ];
    for bytes in cases {
        assert!(PricingCatalog::decode(bytes).is_err(), "accepted {bytes:?}");
    }

    let extra_model = br#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"x","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1","extra":0}]}"#;
    let duplicate_field = br#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"x","model":"y","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"}]}"#;
    let numeric_price = br#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"x","input_usd_per_million":1.0,"output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"}]}"#;
    for bytes in [
        extra_model.as_slice(),
        duplicate_field.as_slice(),
        numeric_price.as_slice(),
    ] {
        assert!(PricingCatalog::decode(bytes).is_err());
    }
}

#[test]
fn strict_codec_rejects_duplicate_and_unknown_fields_at_every_nested_level() {
    let cases = [
        r#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"m","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1","max_standard_input_tokens":1,"max_standard_input_tokens":2}]}"#,
        r#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"m","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1","schedule":{"kind":"utc_weekly_v1","kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":0,"end_minute":1}],"peak":{"input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"}}}]}"#,
        r#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"m","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1","schedule":{"kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":0,"start_minute":0,"end_minute":1}],"peak":{"input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"}}}]}"#,
        r#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"m","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1","schedule":{"kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":0,"end_minute":1}],"peak":{"input_usd_per_million":"1","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"}}}]}"#,
        r#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"m","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1","schedule":{"kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":0,"end_minute":1}],"peak":{"input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"},"extra":0}}]}"#,
        r#"{"schema_version":"pricing_v1","currency":"USD","models":[{"model":"m","input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1","schedule":{"kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":0,"end_minute":1}],"peak":{"input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1","extra":0}}}]}"#,
    ];
    for case in cases {
        assert!(PricingCatalog::decode(case.as_bytes()).is_err());
    }
}

#[test]
fn strict_codec_rejects_explicit_null_at_optional_and_required_fields() {
    for optional in ["max_standard_input_tokens", "schedule"] {
        let mut model = valid_model_json("custom");
        model
            .as_object_mut()
            .unwrap()
            .insert(optional.to_string(), Value::Null);
        assert!(PricingCatalog::decode(&catalog_json(vec![model])).is_err());
    }

    for field in ["schema_version", "currency", "models"] {
        let mut document = json!({
            "schema_version":"pricing_v1",
            "currency":"USD",
            "models":[valid_model_json("custom")]
        });
        document
            .as_object_mut()
            .unwrap()
            .insert(field.to_string(), Value::Null);
        assert!(PricingCatalog::decode(&serde_json::to_vec(&document).unwrap()).is_err());
    }

    for field in [
        "model",
        "input_usd_per_million",
        "output_usd_per_million",
        "cache_read_usd_per_million",
        "cache_write_usd_per_million",
    ] {
        let mut model = valid_model_json("custom");
        model
            .as_object_mut()
            .unwrap()
            .insert(field.to_string(), Value::Null);
        assert!(PricingCatalog::decode(&catalog_json(vec![model])).is_err());
    }

    for field in ["kind", "windows", "peak"] {
        let mut schedule = valid_schedule_json();
        schedule
            .as_object_mut()
            .unwrap()
            .insert(field.to_string(), Value::Null);
        let mut model = valid_model_json("scheduled-custom");
        model
            .as_object_mut()
            .unwrap()
            .insert("schedule".to_string(), schedule);
        assert!(PricingCatalog::decode(&catalog_json(vec![model])).is_err());
    }

    for field in ["weekdays", "start_minute", "end_minute"] {
        let mut schedule = valid_schedule_json();
        schedule["windows"][0]
            .as_object_mut()
            .unwrap()
            .insert(field.to_string(), Value::Null);
        let mut model = valid_model_json("scheduled-custom");
        model
            .as_object_mut()
            .unwrap()
            .insert("schedule".to_string(), schedule);
        assert!(PricingCatalog::decode(&catalog_json(vec![model])).is_err());
    }

    for field in [
        "input_usd_per_million",
        "output_usd_per_million",
        "cache_read_usd_per_million",
        "cache_write_usd_per_million",
    ] {
        let mut schedule = valid_schedule_json();
        schedule["peak"]
            .as_object_mut()
            .unwrap()
            .insert(field.to_string(), Value::Null);
        let mut model = valid_model_json("scheduled-custom");
        model
            .as_object_mut()
            .unwrap()
            .insert("schedule".to_string(), schedule);
        assert!(PricingCatalog::decode(&catalog_json(vec![model])).is_err());
    }

    assert!(PricingCatalog::decode(&catalog_json(vec![valid_model_json("static")])).is_ok());
    let mut capped = valid_model_json("capped");
    capped
        .as_object_mut()
        .unwrap()
        .insert("max_standard_input_tokens".to_string(), json!(1));
    assert!(PricingCatalog::decode(&catalog_json(vec![capped])).is_ok());
    let mut scheduled = valid_model_json("scheduled");
    scheduled
        .as_object_mut()
        .unwrap()
        .insert("schedule".to_string(), valid_schedule_json());
    assert!(PricingCatalog::decode(&catalog_json(vec![scheduled])).is_ok());
}

#[test]
fn duplicate_model_ids_and_case_sensitive_lookup_fail_closed() {
    let duplicate = catalog_json(vec![valid_model_json("exact"), valid_model_json("exact")]);
    assert_eq!(
        PricingCatalog::decode(&duplicate),
        Err(PricingError::DuplicateModel {
            model: "exact".to_string()
        })
    );
    let catalog = PricingCatalog::decode(&catalog_json(vec![valid_model_json("Exact")])).unwrap();
    assert!(matches!(
        catalog.quote("exact", UsageCounts::default(), 0),
        Err(PricingError::ModelNotFound { .. })
    ));
}

#[test]
fn decimal_shape_boundaries_are_exact() {
    for invalid in ["-1", "+1", "1e2", ".1", "1.", "1.0000000", "1.2.3", ""] {
        assert!(
            PricingCatalog::from_specs(vec![static_spec("m", invalid, "0", "0", "0")]).is_err()
        );
    }

    let maximum = "18446744073709.551615";
    assert!(PricingCatalog::from_specs(vec![static_spec("m", maximum, "0", "0", "0")]).is_ok());
    assert!(
        PricingCatalog::from_specs(vec![static_spec(
            "m",
            "18446744073709.551616",
            "0",
            "0",
            "0"
        )])
        .is_err()
    );
    assert!(PricingCatalog::from_specs(vec![static_spec("m", "0.000001", "0", "0", "0")]).is_ok());
    let leading =
        PricingCatalog::from_specs(vec![static_spec("m", "0001.20", "0", "0", "0")]).unwrap();
    assert_eq!(
        leading.specs().next().unwrap().rates.input_usd_per_million,
        "0001.20"
    );
}

#[test]
fn file_model_and_id_caps_accept_exact_and_reject_plus_one() {
    let exact_models: Vec<_> = (0..1_000)
        .map(|index| valid_model_json(&format!("m{index}")))
        .collect();
    catalog::reset_raw_construction_counts();
    assert_eq!(
        PricingCatalog::decode(&catalog_json(exact_models))
            .unwrap()
            .specs()
            .len(),
        1_000
    );
    assert_eq!(catalog::raw_construction_counts(), (1_000, 0));

    let over_models: Vec<_> = (0..1_001)
        .map(|index| valid_model_json(&format!("m{index}")))
        .collect();
    catalog::reset_raw_construction_counts();
    assert!(PricingCatalog::decode(&catalog_json(over_models)).is_err());
    assert_eq!(catalog::raw_construction_counts(), (1_000, 0));

    let id_200 = format!("m{}", "a".repeat(199));
    let id_201 = format!("m{}", "a".repeat(200));
    assert!(PricingCatalog::decode(&catalog_json(vec![valid_model_json(&id_200)])).is_ok());
    assert_eq!(
        PricingCatalog::decode(&catalog_json(vec![valid_model_json(&id_201)])),
        Err(PricingError::InvalidModelId)
    );
    for invalid in ["", "é", "m\0x", "m\nx", "/absolute", "m..x", "m//x", "m/"] {
        assert_eq!(
            PricingCatalog::decode(&catalog_json(vec![valid_model_json(invalid)])),
            Err(PricingError::InvalidModelId)
        );
    }

    let mut exact_file = catalog_json(Vec::new());
    exact_file.resize(1024 * 1024, b' ');
    assert!(PricingCatalog::decode(&exact_file).is_ok());
    exact_file.push(b' ');
    assert_eq!(
        PricingCatalog::decode(&exact_file),
        Err(PricingError::FileTooLarge)
    );

    let directory = tempdir().unwrap();
    let path = directory.path().join("pricing.json");
    exact_file.pop();
    fs::write(&path, &exact_file).unwrap();
    assert!(load_catalog(&path).is_ok());
    fs::write(&path, [exact_file.as_slice(), b" "].concat()).unwrap();
    assert_eq!(load_catalog(&path), Err(PricingError::FileTooLarge));
}

#[test]
fn schedule_codec_rejects_unknown_kind_bad_windows_and_nested_extra() {
    let base_schedule = json!({
        "kind": "utc_weekly_v1",
        "windows": [{"weekdays":[1],"start_minute":60,"end_minute":120}],
        "peak": {
            "input_usd_per_million":"2",
            "output_usd_per_million":"2",
            "cache_read_usd_per_million":"2",
            "cache_write_usd_per_million":"2"
        }
    });
    let mut cases = Vec::new();
    for schedule in [
        json!({"kind":"future","windows":[{"weekdays":[1],"start_minute":60,"end_minute":120}],"peak":{"input_usd_per_million":"2","output_usd_per_million":"2","cache_read_usd_per_million":"2","cache_write_usd_per_million":"2"}}),
        json!({"kind":"utc_weekly_v1","windows":[],"peak":{"input_usd_per_million":"2","output_usd_per_million":"2","cache_read_usd_per_million":"2","cache_write_usd_per_million":"2"}}),
        json!({"kind":"utc_weekly_v1","windows":[{"weekdays":[1,1],"start_minute":60,"end_minute":120}],"peak":{"input_usd_per_million":"2","output_usd_per_million":"2","cache_read_usd_per_million":"2","cache_write_usd_per_million":"2"}}),
        json!({"kind":"utc_weekly_v1","windows":[{"weekdays":[0],"start_minute":60,"end_minute":120}],"peak":{"input_usd_per_million":"2","output_usd_per_million":"2","cache_read_usd_per_million":"2","cache_write_usd_per_million":"2"}}),
        json!({"kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":120,"end_minute":120}],"peak":{"input_usd_per_million":"2","output_usd_per_million":"2","cache_read_usd_per_million":"2","cache_write_usd_per_million":"2"}}),
        json!({"kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":0,"end_minute":1441}],"peak":{"input_usd_per_million":"2","output_usd_per_million":"2","cache_read_usd_per_million":"2","cache_write_usd_per_million":"2"}}),
        json!({"kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":60,"end_minute":120},{"weekdays":[1],"start_minute":119,"end_minute":180}],"peak":{"input_usd_per_million":"2","output_usd_per_million":"2","cache_read_usd_per_million":"2","cache_write_usd_per_million":"2"}}),
        json!({"kind":"utc_weekly_v1","windows":[{"weekdays":[1],"start_minute":60,"end_minute":120,"extra":0}],"peak":{"input_usd_per_million":"2","output_usd_per_million":"2","cache_read_usd_per_million":"2","cache_write_usd_per_million":"2"}}),
    ] {
        let mut model = valid_model_json("scheduled");
        model
            .as_object_mut()
            .unwrap()
            .insert("schedule".to_string(), schedule);
        cases.push(catalog_json(vec![model]));
    }
    for bytes in cases {
        assert!(PricingCatalog::decode(&bytes).is_err());
    }

    let mut valid = valid_model_json("scheduled");
    valid
        .as_object_mut()
        .unwrap()
        .insert("schedule".to_string(), base_schedule.clone());
    assert!(PricingCatalog::decode(&catalog_json(vec![valid])).is_ok());

    let mut cap_and_schedule = valid_model_json("scheduled");
    cap_and_schedule
        .as_object_mut()
        .unwrap()
        .insert("max_standard_input_tokens".to_string(), json!(1));
    cap_and_schedule
        .as_object_mut()
        .unwrap()
        .insert("schedule".to_string(), base_schedule);
    assert!(PricingCatalog::decode(&catalog_json(vec![cap_and_schedule])).is_err());

    let windows: Vec<_> = (0..32)
        .map(|minute| json!({"weekdays":[1],"start_minute":minute,"end_minute":minute + 1}))
        .collect();
    let mut exact_windows = valid_model_json("scheduled");
    exact_windows.as_object_mut().unwrap().insert(
        "schedule".to_string(),
        json!({"kind":"utc_weekly_v1","windows":windows,"peak":{"input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"}}),
    );
    catalog::reset_raw_construction_counts();
    assert!(PricingCatalog::decode(&catalog_json(vec![exact_windows])).is_ok());
    assert_eq!(catalog::raw_construction_counts(), (1, 32));

    let windows: Vec<_> = (0..33)
        .map(|minute| json!({"weekdays":[1],"start_minute":minute,"end_minute":minute + 1}))
        .collect();
    let mut too_many_windows = valid_model_json("scheduled");
    too_many_windows.as_object_mut().unwrap().insert(
        "schedule".to_string(),
        json!({"kind":"utc_weekly_v1","windows":windows,"peak":{"input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"}}),
    );
    catalog::reset_raw_construction_counts();
    assert!(PricingCatalog::decode(&catalog_json(vec![too_many_windows])).is_err());
    assert_eq!(catalog::raw_construction_counts(), (0, 32));

    let mut too_many_weekdays = valid_model_json("scheduled");
    too_many_weekdays.as_object_mut().unwrap().insert(
        "schedule".to_string(),
        json!({"kind":"utc_weekly_v1","windows":[{"weekdays":[1,2,3,4,5,6,7,1],"start_minute":0,"end_minute":1}],"peak":{"input_usd_per_million":"1","output_usd_per_million":"1","cache_read_usd_per_million":"1","cache_write_usd_per_million":"1"}}),
    );
    assert!(PricingCatalog::decode(&catalog_json(vec![too_many_weekdays])).is_err());
}

#[test]
fn custom_exact_override_roundtrips_and_does_not_alias() {
    let mut catalog = PricingCatalog::built_in().unwrap();
    let mut custom = static_spec("gpt-5.6-terra", "9", "8", "7", "6");
    custom.max_standard_input_tokens = Some(272_000);
    catalog.upsert(custom).unwrap();
    let quote = catalog
        .quote(
            "gpt-5.6-terra",
            UsageCounts {
                input: 272_000,
                ..UsageCounts::default()
            },
            0,
        )
        .unwrap();
    assert_eq!(quote.cost_microcents, 2_448_000);
    assert!(matches!(
        catalog.quote("gpt-5.6", UsageCounts::default(), 0),
        Err(PricingError::ModelNotFound { .. })
    ));
    let reopened = PricingCatalog::decode(&catalog.encode().unwrap()).unwrap();
    assert_eq!(reopened, catalog);
}

#[test]
fn cache_semantics_and_single_half_up_are_exact() {
    let catalog = PricingCatalog::from_specs(vec![static_spec("m", "1", "2", "0.1", "3")]).unwrap();
    let quote = catalog
        .quote(
            "m",
            UsageCounts {
                input: 1_000_000,
                output: 1_000_000,
                cache_read: 400_000,
                cache_write: 1_000_000,
            },
            0,
        )
        .unwrap();
    assert_eq!(quote.cost_microcents, 5_640_000);
    assert_eq!(
        catalog.quote(
            "m",
            UsageCounts {
                input: 1,
                cache_read: 2,
                ..UsageCounts::default()
            },
            0
        ),
        Err(PricingError::InvalidCacheUsage)
    );

    let tiny =
        PricingCatalog::from_specs(vec![static_spec("tiny", "0.000001", "0", "0", "0")]).unwrap();
    assert_eq!(
        tiny.quote(
            "tiny",
            UsageCounts {
                input: 499_999,
                ..UsageCounts::default()
            },
            0
        )
        .unwrap()
        .cost_microcents,
        0
    );
    assert_eq!(
        tiny.quote(
            "tiny",
            UsageCounts {
                input: 500_000,
                ..UsageCounts::default()
            },
            0
        )
        .unwrap()
        .cost_microcents,
        1
    );

    let combined = PricingCatalog::from_specs(vec![static_spec(
        "combined", "0.000001", "0.000001", "0", "0",
    )])
    .unwrap();
    assert_eq!(
        combined
            .quote(
                "combined",
                UsageCounts {
                    input: 250_000,
                    output: 250_000,
                    ..UsageCounts::default()
                },
                0,
            )
            .unwrap()
            .cost_microcents,
        1
    );
}

#[test]
fn checked_cost_rejects_u128_and_i64_overflow() {
    let max_rate = "18446744073709.551615";
    let huge = PricingCatalog::from_specs(vec![static_spec(
        "huge", max_rate, max_rate, max_rate, max_rate,
    )])
    .unwrap();
    assert_eq!(
        huge.quote(
            "huge",
            UsageCounts {
                input: u64::MAX,
                output: u64::MAX,
                cache_read: 0,
                cache_write: u64::MAX,
            },
            0
        ),
        Err(PricingError::Overflow)
    );

    let one = PricingCatalog::from_specs(vec![static_spec("one", "1", "0", "0", "0")]).unwrap();
    assert_eq!(
        one.quote(
            "one",
            UsageCounts {
                input: u64::MAX,
                ..UsageCounts::default()
            },
            0
        ),
        Err(PricingError::Overflow)
    );
}

#[test]
fn openai_standard_limit_accepts_exact_and_rejects_plus_one() {
    let catalog = PricingCatalog::built_in().unwrap();
    assert!(
        catalog
            .quote(
                "gpt-5.6-terra",
                UsageCounts {
                    input: 272_000,
                    ..UsageCounts::default()
                },
                0
            )
            .is_ok()
    );
    assert_eq!(
        catalog.quote(
            "gpt-5.6-terra",
            UsageCounts {
                input: 272_001,
                ..UsageCounts::default()
            },
            0
        ),
        Err(PricingError::UnsupportedInputLimit {
            model: "gpt-5.6-terra".to_string(),
            max_tokens: 272_000
        })
    );
    assert_eq!(
        catalog.quote(
            "gpt-5.6-terra",
            UsageCounts {
                input: 272_001,
                cache_read: 272_002,
                ..UsageCounts::default()
            },
            0
        ),
        Err(PricingError::InvalidCacheUsage)
    );
}

#[test]
fn deepseek_utc_weekday_and_half_open_edges_are_exact() {
    let catalog = PricingCatalog::built_in().unwrap();
    let monday = 4 * 86_400;
    let saturday = 9 * 86_400;
    let usage = UsageCounts {
        input: 1_000_000,
        ..UsageCounts::default()
    };
    let cases = [
        (monday + 59 * 60, PricingProfile::Base, 220_000),
        (monday + 60 * 60, PricingProfile::PeakUtcWeekly, 440_000),
        (monday + 239 * 60, PricingProfile::PeakUtcWeekly, 440_000),
        (monday + 240 * 60, PricingProfile::Base, 220_000),
        (monday + 360 * 60, PricingProfile::PeakUtcWeekly, 440_000),
        (monday + 599 * 60, PricingProfile::PeakUtcWeekly, 440_000),
        (monday + 600 * 60, PricingProfile::Base, 220_000),
        (saturday + 60 * 60, PricingProfile::Base, 220_000),
    ];
    for (timestamp, profile, cost) in cases {
        let quote = catalog
            .quote("deepseek-v4-flash", usage, timestamp)
            .unwrap();
        assert_eq!((quote.profile, quote.cost_microcents), (profile, cost));
    }
    let before_epoch_monday = -3 * 86_400 + 60 * 60;
    assert_eq!(
        catalog
            .quote("deepseek-v4-flash", usage, before_epoch_monday)
            .unwrap()
            .profile,
        PricingProfile::PeakUtcWeekly
    );
    for timestamp in [i64::MIN, i64::MAX] {
        assert!(catalog.quote("deepseek-v4-flash", usage, timestamp).is_ok());
    }
    for (timestamp, expected) in [
        (1_788_137_999, PricingProfile::Base),
        (1_788_138_000, PricingProfile::PeakUtcWeekly),
        (1_788_148_799, PricingProfile::PeakUtcWeekly),
        (1_788_148_800, PricingProfile::Base),
        (1_788_155_999, PricingProfile::Base),
        (1_788_156_000, PricingProfile::PeakUtcWeekly),
        (1_788_170_399, PricingProfile::PeakUtcWeekly),
        (1_788_170_400, PricingProfile::Base),
        (1_788_573_600, PricingProfile::Base),
        (1_788_678_000, PricingProfile::Base),
    ] {
        assert_eq!(
            catalog
                .quote("deepseek-v4-flash", usage, timestamp)
                .unwrap()
                .profile,
            expected
        );
    }
}

#[test]
fn every_builtin_one_million_rate_is_quoted_as_integer_microcents() {
    let catalog = PricingCatalog::built_in().unwrap();
    let base = 4 * 86_400;
    let peak = base + 60 * 60;
    let input = UsageCounts {
        input: 1_000_000,
        ..UsageCounts::default()
    };
    let output = UsageCounts {
        output: 1_000_000,
        ..UsageCounts::default()
    };
    for (model, timestamp, expected) in [
        ("deepseek-v4-flash", base, 220_000),
        ("deepseek-v4-flash", peak, 440_000),
        ("deepseek-v4-pro", base, 660_000),
        ("deepseek-v4-pro", peak, 1_320_000),
    ] {
        assert_eq!(
            catalog
                .quote(model, input, timestamp)
                .unwrap()
                .cost_microcents,
            expected
        );
    }
    for (model, expected) in [
        ("gpt-5.6-terra", 12_000_000),
        ("gpt-5.6-luna", 1_200_000),
        ("claude-sonnet-5", 10_000_000),
    ] {
        assert_eq!(
            catalog.quote(model, output, 0).unwrap().cost_microcents,
            expected
        );
    }
    assert_eq!(
        catalog
            .quote(
                "claude-sonnet-5",
                UsageCounts {
                    cache_write: 1_000_000,
                    ..UsageCounts::default()
                },
                0,
            )
            .unwrap()
            .cost_microcents,
        2_500_000
    );
}

#[test]
fn ordinary_save_preserves_malformed_existing_catalog_without_temp() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("pricing.json");
    let malformed = b"malformed existing pricing evidence";
    fs::write(&path, malformed).unwrap();
    let catalog = PricingCatalog::built_in().unwrap();
    assert_eq!(
        save_catalog_atomic(&path, &catalog),
        Err(PricingError::MalformedJson)
    );
    assert_eq!(fs::read(&path).unwrap(), malformed);
    let entries: Vec<_> = fs::read_dir(directory.path()).unwrap().collect();
    assert_eq!(entries.len(), 1);
}

#[test]
fn atomic_precommit_failure_preserves_old_bytes_and_cleans_temp() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("pricing.json");
    let old_catalog = distinct_old_catalog();
    let old = old_catalog.encode().unwrap();
    let catalog = PricingCatalog::built_in().unwrap();
    fs::write(&path, &old).unwrap();
    assert!(persistence::save_with_precommit_failure(&path, &catalog).is_err());
    assert_eq!(fs::read(&path).unwrap(), old);
    let entries: Vec<_> = fs::read_dir(directory.path()).unwrap().collect();
    assert_eq!(entries.len(), 1);
}

#[test]
fn concurrent_target_change_wins_and_is_not_overwritten() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("pricing.json");
    fs::write(&path, distinct_old_catalog().encode().unwrap()).unwrap();
    let catalog = PricingCatalog::built_in().unwrap();
    assert_eq!(
        persistence::save_with_concurrent_target_change(&path, &catalog),
        Err(PricingError::SaveTargetChanged)
    );
    assert_eq!(fs::read(&path).unwrap(), b"concurrent winner");
    let entries: Vec<_> = fs::read_dir(directory.path()).unwrap().collect();
    assert_eq!(entries.len(), 1);
}

#[test]
fn postcommit_durability_unknown_reports_new_bytes_may_be_visible() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("pricing.json");
    let old = distinct_old_catalog().encode().unwrap();
    fs::write(&path, &old).unwrap();
    let catalog = PricingCatalog::built_in().unwrap();
    assert_eq!(
        persistence::save_with_postcommit_failure(&path, &catalog),
        Err(PricingError::CommittedDurabilityUnknown)
    );
    assert_eq!(load_catalog(&path).unwrap(), catalog);
    assert_ne!(fs::read(&path).unwrap(), old);
}

#[test]
fn atomic_fault_matrix_preserves_precommit_and_reports_postcommit_state() {
    let old_catalog = distinct_old_catalog();
    let old = old_catalog.encode().unwrap();
    let catalog = PricingCatalog::built_in().unwrap();
    for fault in [
        persistence::SaveFault::TempCollisionExhaustion,
        persistence::SaveFault::Write,
        persistence::SaveFault::Flush,
        persistence::SaveFault::FileSync,
        persistence::SaveFault::Rename,
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("pricing.json");
        fs::write(&path, &old).unwrap();
        if fault == persistence::SaveFault::TempCollisionExhaustion {
            persistence::reset_temp_collision_attempts();
        }
        assert!(persistence::save_with_fault(&path, &catalog, fault).is_err());
        assert_eq!(fs::read(&path).unwrap(), old);
        assert_eq!(load_catalog(&path).unwrap(), old_catalog);
        if fault == persistence::SaveFault::TempCollisionExhaustion {
            assert_eq!(persistence::temp_collision_attempts(), 16);
        }
        let entries: Vec<_> = fs::read_dir(directory.path()).unwrap().collect();
        assert_eq!(entries.len(), 1, "owned temp leaked for {fault:?}");
    }

    for fault in [
        persistence::SaveFault::DirectoryOpen,
        persistence::SaveFault::DirectorySync,
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("pricing.json");
        fs::write(&path, &old).unwrap();
        assert_eq!(
            persistence::save_with_fault(&path, &catalog, fault),
            Err(PricingError::CommittedDurabilityUnknown)
        );
        assert_eq!(load_catalog(&path).unwrap(), catalog);
        assert_ne!(fs::read(&path).unwrap(), old);
        let entries: Vec<_> = fs::read_dir(directory.path()).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn save_rejects_symlink_nonregular_and_hardlinked_targets() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().unwrap();
    let catalog = PricingCatalog::built_in().unwrap();

    let real = directory.path().join("real");
    fs::write(&real, b"old").unwrap();
    let link = directory.path().join("link");
    symlink(&real, &link).unwrap();
    assert_eq!(
        save_catalog_atomic(&link, &catalog),
        Err(PricingError::UnsafeSaveTarget)
    );
    assert_eq!(fs::read(&real).unwrap(), b"old");

    let hard = directory.path().join("hard");
    fs::hard_link(&real, &hard).unwrap();
    assert_eq!(
        save_catalog_atomic(&real, &catalog),
        Err(PricingError::UnsafeSaveTarget)
    );
    assert_eq!(fs::read(&hard).unwrap(), b"old");

    let subdirectory = directory.path().join("directory-target");
    fs::create_dir(&subdirectory).unwrap();
    assert_eq!(
        save_catalog_atomic(&subdirectory, &catalog),
        Err(PricingError::UnsafeSaveTarget)
    );
}

#[test]
fn public_errors_and_catalog_debug_are_content_and_path_free() {
    let secret = "/Users/person/private/pricing.json sk-secret-body";
    let error = PricingCatalog::decode(secret.as_bytes()).unwrap_err();
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(secret));
    assert!(!rendered.contains("/Users/person"));
    assert!(!rendered.contains("sk-secret"));

    let catalog = PricingCatalog::built_in().unwrap();
    let debug = format!("{catalog:?}");
    assert_eq!(debug, "PricingCatalog { model_count: 5 }");
    assert!(!debug.contains("gpt-5.6"));

    let invalid_decimal = PricingCatalog::from_specs(vec![static_spec(
        "safe-model",
        "999999999999999999999999999999-secret",
        "0",
        "0",
        "0",
    )])
    .unwrap_err();
    let invalid_model = catalog
        .quote("/Users/person sk-secret-model", UsageCounts::default(), 0)
        .unwrap_err();
    let missing = load_catalog(std::path::Path::new(
        "/Users/person/private/sk-secret-pricing.json",
    ))
    .unwrap_err();
    for error in [invalid_decimal, invalid_model, missing] {
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains("999999999999999999999999999999-secret"));
        assert!(!rendered.contains("/Users/person"));
        assert!(!rendered.contains("sk-secret"));
    }

    let load = CatalogLoad {
        catalog: catalog.clone(),
        seeded: true,
        durability_unknown: false,
    };
    let spec = catalog.specs().next().unwrap();
    let public_debug = format!("{load:?} {spec:?} {:?}", spec.rates);
    assert!(!public_debug.contains("gpt-5.6"));
    assert!(!public_debug.contains("2.00"));
    assert!(!public_debug.contains("12.00"));
}
