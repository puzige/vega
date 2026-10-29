use super::*;
use vega_runtime::skills::{
    RunBinding, SkillApproval, SkillCatalog, SkillRun, SkillSelection, SkillSource,
};

#[test]
fn historical_skill_reader_checks_real_snapshot_thread_digest_and_current_generations() -> TestResult
{
    let (directory, path, store) = source_fixture()?;
    let skill_path = directory.0.join(".agents/skills/bounded");
    std::fs::create_dir_all(&skill_path)?;
    std::fs::write(
        skill_path.join("SKILL.md"),
        "---\nname: bounded\ndescription: Synthetic bounded skill\n---\nFrozen historical body",
    )?;
    let source = SkillSource::project_approved(&directory.0)
        .map_err(|error| format!("{error:?}"))?
        .ok_or("source")?;
    let discovery = source.discover().map_err(|error| format!("{error:?}"))?;
    assert_eq!(discovery.candidates.len(), 1);
    let candidate = &discovery.candidates[0];
    let selection = SkillSelection::from_candidate(candidate);
    let approval = SkillApproval::reviewed(candidate, "development", true, true)
        .map_err(|error| format!("{error:?}"))?;
    let catalog = SkillCatalog::freeze(discovery.candidates, &[approval], true)
        .map_err(|error| format!("{error:?}"))?;
    let settings = vega_store::skills::read_settings(store.conn())?;
    let binding = RunBinding::for_catalog(
        "owner",
        "thread",
        settings.consent_generation,
        settings.revocation_generation,
        &catalog,
    )
    .map_err(|error| format!("{error:?}"))?;
    let mut run = SkillRun::new_bound(catalog, true, binding.clone())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        run.load_explicit(&selection, |_| true).audit.status,
        "loaded"
    );
    let snapshot = run
        .export_snapshot()
        .map_err(|error| format!("{error:?}"))?;
    vega_store::skills::save_snapshot(
        store.conn(),
        vega_store::skills::NewSkillSnapshot {
            run_id: "owner",
            thread_id: "thread",
            consent_generation: binding.consent_generation(),
            revocation_generation: binding.revocation_generation(),
            catalog_sha256: binding.catalog_sha256(),
            snapshot_sha256: &snapshot.sha256,
            bytes: &snapshot.bytes,
            updated_at: 1,
        },
    )?;
    let reader = crate::agent::skills::context_skill_reader(path.clone(), "thread".into());
    let bodies = reader("owner");
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].0.name, "bounded");
    assert!(bodies[0].1.contains("Frozen historical body"));
    std::fs::write(skill_path.join("SKILL.md"), "changed external content")?;
    assert_eq!(reader("owner"), bodies);
    assert!(reader("missing").is_empty());
    assert!(
        crate::agent::skills::context_skill_reader(path, "other-thread".into())("owner").is_empty()
    );
    store.conn().execute(
        "UPDATE skill_run_snapshots SET bytes=x'00' WHERE run_id='owner'",
        [],
    )?;
    assert!(reader("owner").is_empty());
    store.conn().execute(
        "UPDATE skill_run_snapshots SET bytes=?1 WHERE run_id='owner'",
        [&snapshot.bytes],
    )?;
    assert_eq!(reader("owner"), bodies);
    for generation in ["consent_generation", "revocation_generation"] {
        store.conn().execute(
            &format!("UPDATE skill_settings SET {generation}={generation}+1 WHERE id=1"),
            [],
        )?;
        assert!(reader("owner").is_empty());
        store.conn().execute(
            &format!("UPDATE skill_settings SET {generation}={generation}-1 WHERE id=1"),
            [],
        )?;
    }
    assert_eq!(reader("owner"), bodies);
    assert_eq!(
        vega_store::skills::load_recoverable_snapshot(store.conn(), "owner")?
            .ok_or("snapshot")?
            .bytes,
        snapshot.bytes
    );
    assert_eq!(checkpoints(&store)?, 0);
    Ok(())
}
