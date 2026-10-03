use super::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const BODY: &str = "---\nname: example\ndescription: Use for examples.\n---\n\nExample body.\n";

fn write_skill(root: &Path, name: &str, body: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("skill directory");
    fs::write(
        directory.join("SKILL.md"),
        body.replace("name: example", &format!("name: {name}")),
    )
    .expect("skill body");
}

#[test]
fn s01_missing_project_or_global_root_is_an_empty_source() {
    let project = tempdir().expect("project");
    let config = tempdir().expect("config");
    assert_eq!(SkillSource::project_approved(project.path()), Ok(None));
    assert_eq!(SkillSource::vega_global(config.path()), Ok(None));
}

#[test]
fn s06_unapproved_project_and_global_root_symlinks_are_rejected() {
    let project = tempdir().expect("project");
    let config = tempdir().expect("config");
    let outside = tempdir().expect("outside");
    std::os::unix::fs::symlink(outside.path(), project.path().join(".agents"))
        .expect("project root link");
    std::os::unix::fs::symlink(outside.path(), config.path().join("skills"))
        .expect("global root link");
    assert_eq!(
        SkillSource::project_approved(project.path()),
        Err(SkillError::UnsafePath)
    );
    assert_eq!(
        SkillSource::vega_global(config.path()),
        Err(SkillError::UnsafePath)
    );
}

#[test]
fn s01_s03_discovery_is_one_level_and_only_explicit_roots() {
    let project = tempdir().expect("project");
    let global_config = tempdir().expect("config");
    let unrelated = tempdir().expect("unrelated");
    let project_skills = project.path().join(".agents/skills");
    let global_skills = global_config.path().join("skills");
    write_skill(&project_skills, "project-one", BODY);
    write_skill(&global_skills, "global-one", BODY);
    write_skill(&project_skills.join("nested"), "too-deep", BODY);
    write_skill(
        &unrelated.path().join(".codex/skills"),
        "not-imported",
        BODY,
    );

    let project_source = SkillSource::project_approved(project.path())
        .expect("bind project")
        .expect("project root");
    let global_source = SkillSource::vega_global(global_config.path())
        .expect("bind global")
        .expect("global root");
    let project_scan = project_source.discover().expect("project scan");
    let global_scan = global_source.discover().expect("global scan");
    assert_eq!(project_scan.candidates.len(), 1);
    assert_eq!(project_scan.candidates[0].name, "project-one");
    assert_eq!(global_scan.candidates.len(), 1);
    assert_eq!(global_scan.candidates[0].name, "global-one");
    assert!(
        !project_scan
            .candidates
            .iter()
            .any(|item| item.name == "too-deep")
    );
    assert!(
        !global_scan
            .candidates
            .iter()
            .any(|item| item.name == "not-imported")
    );
}

#[test]
fn s04_precedence_and_canonical_import_deduplication() {
    let project = tempdir().expect("project");
    let config = tempdir().expect("config");
    let imported = tempdir().expect("imported");
    let imported_later = tempdir().expect("later imported");
    write_skill(&project.path().join(".agents/skills"), "same", BODY);
    write_skill(&config.path().join("skills"), "same", BODY);
    write_skill(imported.path(), "same", BODY);
    write_skill(imported_later.path(), "same", BODY);

    let project = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let global = SkillSource::vega_global(config.path()).unwrap().unwrap();
    let imported_first = SkillSource::imported_approved(imported.path(), 0).unwrap();
    let imported_repeat = SkillSource::imported_approved(imported.path(), 1).unwrap();
    let imported_second = SkillSource::imported_approved(imported_later.path(), 2).unwrap();
    let unique_sources = deduplicate_sources(vec![
        imported_repeat.clone(),
        imported_second.clone(),
        imported_first.clone(),
        global.clone(),
        project.clone(),
    ]);
    assert_eq!(unique_sources.len(), 4);
    assert_eq!(unique_sources[2].kind(), SourceKind::Imported { order: 0 });
    let all = [
        imported_repeat.discover().unwrap().candidates.remove(0),
        global.discover().unwrap().candidates.remove(0),
        imported_first.discover().unwrap().candidates.remove(0),
        project.discover().unwrap().candidates.remove(0),
    ];
    let winners = resolve_precedence(all.to_vec());
    assert_eq!(winners.len(), 1);
    assert_eq!(winners[0].source.kind(), SourceKind::Project);

    let approved_without_project = resolve_precedence(all[..3].to_vec());
    assert_eq!(
        approved_without_project[0].source.kind(),
        SourceKind::VegaGlobal
    );
    let imported_only = resolve_precedence(vec![all[0].clone(), all[2].clone()]);
    assert_eq!(imported_only.len(), 1);
    assert_eq!(
        imported_only[0].source.kind(),
        SourceKind::Imported { order: 0 }
    );
    let ordered_imports = resolve_precedence(vec![
        imported_second.discover().unwrap().candidates.remove(0),
        all[2].clone(),
    ]);
    assert_eq!(
        ordered_imports[0].source.kind(),
        SourceKind::Imported { order: 0 }
    );
}

#[test]
fn s06_changed_file_and_unsafe_file_types_fail_closed() {
    let project = tempdir().expect("project");
    let outside = tempdir().expect("outside");
    let root = project.path().join(".agents/skills");
    write_skill(&root, "safe", BODY);
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let candidate = source.discover().unwrap().candidates.remove(0);

    fs::write(
        root.join("safe/SKILL.md"),
        BODY.replace("Example body", "Changed body"),
    )
    .expect("change body");
    assert_eq!(source.load_candidate(&candidate), Err(SkillError::Stale));

    let outside_file = outside.path().join("outside.md");
    fs::write(&outside_file, BODY).expect("outside file");
    fs::remove_file(root.join("safe/SKILL.md")).expect("remove fixture");
    std::os::unix::fs::symlink(&outside_file, root.join("safe/SKILL.md")).expect("symlink fixture");
    assert_eq!(source.read_skill_md("safe"), Err(SkillError::UnsafePath));

    fs::remove_file(root.join("safe/SKILL.md")).expect("remove link");
    fs::hard_link(&outside_file, root.join("safe/SKILL.md")).expect("hardlink fixture");
    assert_eq!(source.read_skill_md("safe"), Err(SkillError::Hardlink));
}

#[test]
fn s06_reference_paths_and_import_root_retarget_are_fenced() {
    let first = tempdir().expect("first root");
    let second = tempdir().expect("second root");
    let link_holder = tempdir().expect("link holder");
    write_skill(first.path(), "safe", BODY);
    write_skill(second.path(), "safe", BODY);
    let refs = first.path().join("safe/references");
    fs::create_dir_all(&refs).expect("references");
    fs::write(refs.join("note.md"), "reference text").expect("reference");
    let link = link_holder.path().join("imported");
    std::os::unix::fs::symlink(first.path(), &link).expect("source link");
    let source = SkillSource::imported_approved(&link, 0).expect("bind imported root");
    assert_eq!(
        source.read_reference("safe", "references/note.md").unwrap(),
        "reference text"
    );
    assert_eq!(
        source.read_reference("safe", "../outside.md"),
        Err(SkillError::UnsafePath)
    );
    assert_eq!(
        source.read_reference("safe", "references/./note.md"),
        Err(SkillError::UnsafePath)
    );
    assert_eq!(
        source.read_reference("safe", "/etc/passwd"),
        Err(SkillError::UnsafePath)
    );

    std::os::unix::fs::symlink(second.path().join("safe/SKILL.md"), refs.join("link.md"))
        .expect("reference link");
    assert_eq!(
        source.read_reference("safe", "references/link.md"),
        Err(SkillError::UnsafePath)
    );
    std::os::unix::fs::symlink(second.path().join("safe"), refs.join("nested"))
        .expect("nested reference link");
    assert_eq!(
        source.read_reference("safe", "references/nested/SKILL.md"),
        Err(SkillError::UnsafePath)
    );

    fs::remove_file(&link).expect("unlink import");
    std::os::unix::fs::symlink(second.path(), &link).expect("retarget import");
    assert_eq!(source.read_skill_md("safe"), Err(SkillError::RootChanged));
}

#[test]
fn s15_hardlinked_reference_file_is_rejected() {
    let project = tempdir().expect("project");
    let outside = tempdir().expect("outside");
    let skills = project.path().join(".agents/skills");
    write_skill(&skills, "safe", BODY);
    let references = skills.join("safe/references");
    fs::create_dir_all(&references).expect("references");
    let outside_file = outside.path().join("outside.md");
    fs::write(&outside_file, "PRIVATE OUTSIDE REFERENCE").expect("outside reference");
    fs::hard_link(&outside_file, references.join("shared.md")).expect("hardlink reference");
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();

    assert_eq!(
        source.read_reference("safe", "references/shared.md"),
        Err(SkillError::Hardlink)
    );
}

#[test]
fn s06_oversize_and_nonregular_skill_files_are_rejected() {
    let project = tempdir().expect("project");
    let root = project.path().join(".agents/skills");
    write_skill(&root, "large", BODY);
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    fs::write(root.join("large/SKILL.md"), vec![b'a'; MAX_SKILL_BYTES + 1]).expect("large skill");
    assert_eq!(source.read_skill_md("large"), Err(SkillError::TooLarge));

    fs::remove_file(root.join("large/SKILL.md")).expect("remove skill");
    fs::create_dir(root.join("large/SKILL.md")).expect("replace with directory");
    assert_eq!(source.read_skill_md("large"), Err(SkillError::NotRegular));
}

#[test]
fn s05_frontmatter_accepts_yaml_quoting_blocks_and_optional_fields() {
    let input = b"---\nname: example\ndescription: >-\n  Reviews YAML and Markdown\n  when the user asks.\nlicense: MIT\ncompatibility: 'Requires git: 2.x'\nmetadata:\n  author: team\n  version: '1.0'\nallowed-tools: Bash(*)\nunknown-display-only: [1, 2]\n---\n# Instructions\nUse the normal permission flow.\n";
    let parsed = parse_skill_md(input, "example").expect("valid skill");
    assert_eq!(parsed.metadata.name, "example");
    assert_eq!(
        parsed.metadata.description,
        "Reviews YAML and Markdown when the user asks."
    );
    assert_eq!(parsed.metadata.license.as_deref(), Some("MIT"));
    assert_eq!(
        parsed.metadata.compatibility.as_deref(),
        Some("Requires git: 2.x")
    );
    assert_eq!(
        parsed.metadata.metadata.get("author").map(String::as_str),
        Some("team")
    );
    assert_eq!(parsed.metadata.allowed_tools.as_deref(), Some("Bash(*)"));
    assert_eq!(
        parsed.body,
        "# Instructions\nUse the normal permission flow.\n"
    );
}

#[test]
fn s05_frontmatter_rejects_missing_malformed_duplicate_and_multiple_documents() {
    for input in [
        "---\ndescription: useful\n---\nbody",
        "---\nname: example\n---\nbody",
        "---\nname: example\ndescription: [broken\n---\nbody",
        "---\nname: example\nname: other\ndescription: useful\n---\nbody",
        "---\nname: example\ndescription: useful\nunknown: one\nunknown: two\n---\nbody",
        "---\nname: example\ndescription: useful\n...\n---\nbody",
        "---\n- name: example\n- description: useful\n---\nbody",
    ] {
        assert!(
            parse_skill_md(input.as_bytes(), "example").is_err(),
            "accepted: {input}"
        );
    }
}

#[test]
fn s05_frontmatter_rejects_invalid_names_lengths_and_custom_tags() {
    for (input, directory) in [
        ("---\nname: other\ndescription: useful\n---\n", "example"),
        (
            "---\nname: bad--name\ndescription: useful\n---\n",
            "bad--name",
        ),
        ("---\nname: example\ndescription: '   '\n---\n", "example"),
        (
            "---\nname: example\ndescription: !include /tmp/sentinel\n---\n",
            "example",
        ),
        (
            "---\nname: example\ndescription: !env SOME_SECRET\n---\n",
            "example",
        ),
        (
            "---\nname: example\ndescription: !<tag:yaml.org,2002:include> /tmp/sentinel\n---\n",
            "example",
        ),
    ] {
        assert!(
            parse_skill_md(input.as_bytes(), directory).is_err(),
            "accepted: {input}"
        );
    }
    let too_long = format!(
        "---\nname: example\ndescription: {}\n---\n",
        "a".repeat(1025)
    );
    assert!(parse_skill_md(too_long.as_bytes(), "example").is_err());
    let too_large_frontmatter = format!(
        "---\nname: example\ndescription: useful\nextra: {}\n---\n",
        "a".repeat(MAX_FRONTMATTER_BYTES)
    );
    assert_eq!(
        parse_skill_md(too_large_frontmatter.as_bytes(), "example"),
        Err(SkillError::TooLarge)
    );
    assert_eq!(
        parse_skill_md(b"---\nname: example\0\n---\n", "example"),
        Err(SkillError::InvalidUtf8)
    );
}

#[test]
fn s05_environment_syntax_is_literal_and_include_never_reads_a_file() {
    let secret = tempdir().expect("owned sentinel directory");
    let sentinel = secret.path().join("sentinel.txt");
    fs::write(&sentinel, "owned secret sentinel").expect("sentinel");
    let include = format!(
        "---\nname: example\ndescription: !include {}\n---\n",
        sentinel.display()
    );
    assert_eq!(
        parse_skill_md(include.as_bytes(), "example"),
        Err(SkillError::ForbiddenTag)
    );
    let literal =
        b"---\nname: example\ndescription: 'Use ${VEGA_FAKE_ENV_SENTINEL} literally.'\n---\n";
    let parsed = parse_skill_md(literal, "example").expect("literal syntax");
    assert_eq!(
        parsed.metadata.description,
        "Use ${VEGA_FAKE_ENV_SENTINEL} literally."
    );
}

#[test]
fn s05_invalid_candidate_is_diagnostic_not_model_catalog_entry() {
    let project = tempdir().expect("project");
    let root = project.path().join(".agents/skills");
    write_skill(&root, "valid", BODY);
    write_skill(
        &root,
        "invalid",
        "---\nname: example\ndescription: !include x\n---\n",
    );
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let scan = source.discover().expect("scan");
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(scan.candidates[0].name, "valid");
    assert_eq!(scan.diagnostics.len(), 1);
    assert_eq!(scan.diagnostics[0].name, "invalid");
    assert_eq!(scan.diagnostics[0].error, SkillError::ForbiddenTag);
}
