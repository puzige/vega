use super::*;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;
use vega_runtime::skills::{SkillSource, SourceScope};
use vega_store::skills::{self, NewSkillApproval, NewSkillSource, NewThreadSkillPin};

mod s16_durable;
mod s18_durable_budget;

fn add_skill(root: &Path, name: &str, body: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Review code changes.\n---\n{body}\n"),
    )
    .unwrap();
}

fn approve(
    store: &Store,
    source: &SkillSource,
    source_id: &str,
    project_id: Option<&str>,
    configured_root: &Path,
    automatic: bool,
) -> (String, String) {
    let candidate = source.discover().unwrap().candidates.remove(0);
    let identity = source.identity();
    let scope = match identity.scope() {
        SourceScope::Project => "project",
        SourceScope::VegaGlobal => "vega_global",
        SourceScope::Imported => "imported",
    };
    let root_dev = identity.device().to_string();
    let root_ino = identity.inode().to_string();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::link_source(
        store.conn(),
        settings.consent_generation,
        NewSkillSource {
            id: source_id,
            scope,
            project_id,
            configured_root: configured_root.to_str().unwrap(),
            canonical_root: identity.canonical_root().to_str().unwrap(),
            root_dev: &root_dev,
            root_ino: &root_ino,
            import_order: 0,
            enabled: true,
            automatic,
            created_at: 1,
        },
    )
    .unwrap();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::approve_skill(
        store.conn(),
        settings.consent_generation,
        NewSkillApproval {
            source_id,
            name: &candidate.name,
            approved_sha256: &candidate.sha256,
            source_label: source_id,
            enabled: true,
            automatic,
            reviewed_at: 2,
        },
    )
    .unwrap();
    (candidate.name, candidate.sha256)
}
