use super::*;
use serde_json::{Value, json};
use sha2::Digest;
use std::fs;
use tempfile::TempDir;

fn prepared_run() -> (TempDir, SkillRun, SkillCatalog, RunBinding) {
    let root = tempfile::tempdir().unwrap();
    let skills = root.path().join(".agents/skills");
    for name in ["first", "later"] {
        let directory = skills.join(name);
        fs::create_dir_all(directory.join("references")).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Guide for {name}.\n---\n{name} ORIGINAL BODY"),
        )
        .unwrap();
    }
    fs::write(
        skills.join("first/references/guide.md"),
        "ORIGINAL REFERENCE",
    )
    .unwrap();
    let candidates = SkillSource::project_approved(root.path())
        .unwrap()
        .unwrap()
        .discover()
        .unwrap()
        .candidates;
    let approvals: Vec<_> = candidates
        .iter()
        .map(|candidate| SkillApproval::reviewed(candidate, "project-one", true, true).unwrap())
        .collect();
    let catalog = SkillCatalog::freeze(candidates, &approvals, true).unwrap();
    let binding = RunBinding::for_catalog("run-one", "thread-one", 7, 11, &catalog).unwrap();
    let run = SkillRun::new_bound(catalog.clone(), true, binding.clone()).unwrap();
    (root, run, catalog, binding)
}

#[test]
fn s12_snapshot_round_trip_keeps_activated_bytes_and_rechecks_unactivated_source() {
    let (root, mut run, _catalog, binding) = prepared_run();
    assert_eq!(run.load_model("first", |_| true).receipt.status, "loaded");
    let first = run
        .read_reference("first", "references/guide.md", |_| true)
        .unwrap();
    let snapshot = run.export_snapshot().unwrap();
    assert!(snapshot.bytes().len() <= MAX_RUN_SNAPSHOT_BYTES);

    fs::write(
        root.path().join(".agents/skills/first/SKILL.md"),
        "---\nname: first\ndescription: Guide for first.\n---\nCHANGED BODY",
    )
    .unwrap();
    fs::write(
        root.path().join(".agents/skills/first/references/guide.md"),
        "CHANGED REFERENCE",
    )
    .unwrap();
    fs::write(
        root.path().join(".agents/skills/later/SKILL.md"),
        "---\nname: later\ndescription: Guide for later.\n---\nCHANGED BODY",
    )
    .unwrap();

    let mut restored =
        SkillRun::restore_snapshot(snapshot.bytes(), &binding, snapshot.sha256()).unwrap();
    let envelope = restored.render_skill_envelope().unwrap();
    assert!(envelope.contains("first ORIGINAL BODY"));
    assert!(!envelope.contains("CHANGED BODY"));
    assert_eq!(
        restored
            .read_reference("first", "references/guide.md", |_| true)
            .unwrap(),
        first
    );
    assert_eq!(
        restored.load_model("later", |_| true).receipt.status,
        "stale"
    );
}

#[test]
fn s12_snapshot_restores_activation_order_without_live_root() {
    let (root, mut run, _catalog, binding) = prepared_run();
    assert_eq!(run.load_model("later", |_| true).receipt.status, "loaded");
    assert_eq!(run.load_model("first", |_| true).receipt.status, "loaded");
    let snapshot = run.export_snapshot().unwrap();
    fs::remove_dir_all(root.path().join(".agents/skills")).unwrap();

    let restored =
        SkillRun::restore_snapshot(snapshot.bytes(), &binding, snapshot.sha256()).unwrap();
    let envelope = restored.render_skill_envelope().unwrap();
    let later = envelope.find("later ORIGINAL BODY").expect("later body");
    let first = envelope.find("first ORIGINAL BODY").expect("first body");
    assert!(later < first);
}

#[test]
fn s12_snapshot_rejects_wrong_trusted_binding_and_unbound_export() {
    let (_root, mut run, catalog, binding) = prepared_run();
    assert_eq!(run.load_model("first", |_| true).receipt.status, "loaded");
    let bytes = run.export_snapshot().unwrap();
    let alternate_run =
        RunBinding::for_catalog("run-other", "thread-one", 7, 11, &catalog).unwrap();
    let revoked = RunBinding::for_catalog("run-one", "thread-one", 7, 12, &catalog).unwrap();
    let consent_changed =
        RunBinding::for_catalog("run-one", "thread-one", 8, 11, &catalog).unwrap();
    assert!(matches!(
        SkillRun::restore_snapshot(bytes.bytes(), &alternate_run, bytes.sha256()),
        Err(SkillError::Stale)
    ));
    assert!(matches!(
        SkillRun::restore_snapshot(bytes.bytes(), &revoked, bytes.sha256()),
        Err(SkillError::Stale)
    ));
    assert!(matches!(
        SkillRun::restore_snapshot(bytes.bytes(), &consent_changed, bytes.sha256()),
        Err(SkillError::Stale)
    ));
    assert!(SkillRun::restore_snapshot(bytes.bytes(), &binding, bytes.sha256()).is_ok());

    let unbound = SkillRun::new(catalog, true);
    assert!(matches!(
        unbound.export_snapshot(),
        Err(SkillError::InvalidFormat)
    ));
}

#[test]
fn s12_trusted_store_parts_rebuild_binding_without_reading_snapshot_binding() {
    let (_root, mut run, _catalog, binding) = prepared_run();
    assert_eq!(run.load_model("first", |_| true).receipt.status, "loaded");
    let snapshot = run.export_snapshot().unwrap();
    let restored_binding = RunBinding::from_trusted_parts(
        binding.run_id(),
        binding.thread_id(),
        binding.consent_generation(),
        binding.revocation_generation(),
        binding.catalog_sha256(),
    )
    .unwrap();
    assert_eq!(restored_binding, binding);
    assert!(
        SkillRun::restore_snapshot(snapshot.bytes(), &restored_binding, snapshot.sha256()).is_ok()
    );
    assert!(matches!(
        RunBinding::from_trusted_parts("../run", "thread-one", 7, 11, binding.catalog_sha256()),
        Err(SkillError::InvalidFormat)
    ));
    assert!(matches!(
        RunBinding::from_trusted_parts("run-one", "thread-one", 7, 11, "bad-sha"),
        Err(SkillError::InvalidFormat)
    ));
}

#[test]
fn s12_snapshot_bounds_number_of_first_read_references() {
    let (root, mut run, _catalog, binding) = prepared_run();
    assert_eq!(run.load_model("first", |_| true).receipt.status, "loaded");
    let directory = root.path().join(".agents/skills/first/references");
    for index in 0..128 {
        let path = format!("references/empty-{index}.md");
        fs::write(directory.join(format!("empty-{index}.md")), "").unwrap();
        run.read_reference("first", &path, |_| true).unwrap();
    }
    fs::write(directory.join("one-too-many.md"), "").unwrap();
    assert!(matches!(
        run.read_reference("first", "references/one-too-many.md", |_| true),
        Err(SkillError::AggregateLimit)
    ));
    let snapshot = run.export_snapshot().unwrap();
    assert!(SkillRun::restore_snapshot(snapshot.bytes(), &binding, snapshot.sha256()).is_ok());
}

#[test]
fn s12_snapshot_rejects_tampered_catalog_body_reference_path_hash_and_limits() {
    let (_root, mut run, _catalog, binding) = prepared_run();
    assert_eq!(run.load_model("first", |_| true).receipt.status, "loaded");
    run.read_reference("first", "references/guide.md", |_| true)
        .unwrap();
    let bytes = run.export_snapshot().unwrap();
    let original: Value = serde_json::from_slice(bytes.bytes()).unwrap();

    let mutations: Vec<Value> = [("version", json!(99)), ("reference_bytes", json!(0))]
        .into_iter()
        .map(|(key, value)| {
            let mut record = original.clone();
            record[key] = value;
            record
        })
        .chain([
            {
                let mut record = original.clone();
                record["catalog"]["entries"][0]["label"] = json!("other-label");
                record
            },
            {
                let mut record = original.clone();
                record["active"][0]["body"] = json!("TAMPERED BODY");
                record
            },
            {
                let mut record = original.clone();
                let body = "TAMPERED BODY";
                record["active"][0]["body"] = json!(body);
                record["active"][0]["body_sha256"] =
                    json!(format!("{:x}", sha2::Sha256::digest(body.as_bytes())));
                record
            },
            {
                let mut record = original.clone();
                record["references"][0]["text"] = json!("TAMPERED REFERENCE");
                record
            },
            {
                let mut record = original.clone();
                let text = "TAMPERED REFERENCE";
                record["references"][0]["text"] = json!(text);
                record["references"][0]["content_sha256"] =
                    json!(format!("{:x}", sha2::Sha256::digest(text.as_bytes())));
                record
            },
            {
                let mut record = original.clone();
                record["references"][0]["path"] = json!("../escape");
                record
            },
            {
                let mut record = original.clone();
                record["references"][0]["content_sha256"] = json!("0".repeat(64));
                record
            },
        ])
        .collect();
    for record in mutations {
        let changed = serde_json::to_vec(&record).unwrap();
        assert!(
            SkillRun::restore_snapshot(&changed, &binding, bytes.sha256()).is_err(),
            "accepted mutated record: {record:?}"
        );
    }
    assert!(matches!(
        SkillRun::restore_snapshot(
            &vec![b' '; MAX_RUN_SNAPSHOT_BYTES + 1],
            &binding,
            bytes.sha256()
        ),
        Err(SkillError::TooLarge)
    ));
}
