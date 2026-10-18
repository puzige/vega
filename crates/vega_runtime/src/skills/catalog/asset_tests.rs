use super::*;
use crate::skills::asset_probe;
use serde_json::{Value, json};

use std::fs;

use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, symlink};

use tempfile::TempDir;

fn prepared(body: &str, bound: bool) -> (TempDir, SkillRun, SkillSource, RunBinding) {
    let root = tempfile::tempdir().unwrap();
    let skill = root.path().join(".agents/skills/reviewer");
    fs::create_dir_all(skill.join("assets")).unwrap();
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: reviewer\ndescription: Review code changes.\n---\n{body}"),
    )
    .unwrap();
    let source = SkillSource::project_approved(root.path()).unwrap().unwrap();
    let candidate = source.discover().unwrap().candidates.remove(0);
    let approval = SkillApproval::reviewed(&candidate, "project-one", true, true).unwrap();
    let catalog = SkillCatalog::freeze(vec![candidate], &[approval], true).unwrap();
    let binding = RunBinding::for_catalog("run-one", "thread-one", 7, 11, &catalog).unwrap();
    let run = if bound {
        SkillRun::new_bound(catalog, true, binding.clone()).unwrap()
    } else {
        SkillRun::new(catalog, true)
    };
    (root, run, source, binding)
}

fn returned(output: &str) -> Value {
    serde_json::from_str(
        output
            .strip_prefix("[Lower-trust Skill asset metadata]\n")
            .unwrap(),
    )
    .unwrap()
}
