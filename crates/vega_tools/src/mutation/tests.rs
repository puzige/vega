use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use tempfile::{TempDir, tempdir};

use super::{FAIL_ATOMIC_REPLACE, PrepareMutationError};
use crate::{
    CheckpointIds, CreatedNewFileMetadata, EditSuccessOutput, InvalidMutation, MutationErrorCode,
    MutationTool, ToolError, Tools, WriteSuccessOutput,
};

struct Fixture {
    project: TempDir,
    checkpoints: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            project: tempdir().unwrap(),
            checkpoints: tempdir().unwrap(),
        }
    }

    fn tools(&self, call: &str) -> Tools {
        Tools::new(self.project.path())
            .unwrap()
            .with_mutation_context(self.checkpoints.path(), "project", "thread", call)
            .unwrap()
    }

    fn call_root(&self, call: &str) -> PathBuf {
        let checkpoint_ref = CheckpointIds::new("project", "thread", call)
            .unwrap()
            .checkpoint_ref();
        checkpoint_ref
            .as_str()
            .split('/')
            .skip(1)
            .fold(self.checkpoints.path().to_path_buf(), |path, component| {
                path.join(component)
            })
    }
}

fn invalid(error: PrepareMutationError) -> crate::InvalidMutation {
    match error {
        PrepareMutationError::Invalid(invalid) => invalid,
        PrepareMutationError::Internal(error) => panic!("unexpected internal error: {error}"),
    }
}

fn mutation_code(error: ToolError) -> MutationErrorCode {
    match error {
        ToolError::Mutation(error) => error.code(),
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn unconfigured_mutation_is_internal_and_never_creates_validation_audit() {
    let fixture = Fixture::new();
    let tools = Tools::new(fixture.project.path()).unwrap();
    let write_raw = r#"{"path":"secret.txt","content":"token=do-not-log"}"#;
    let write_error = tools.prepare_write_json(write_raw).unwrap_err();
    assert!(write_error.invalid().is_none());
    assert!(matches!(
        &write_error,
        PrepareMutationError::Internal(error)
            if error.code() == MutationErrorCode::CheckpointUnavailable
    ));
    assert!(!write_error.to_string().contains("secret.txt"));
    assert!(!format!("{write_error:?}").contains("do-not-log"));

    let edit_raw = r#"{"path":"secret.txt","old_string":"old-secret","new_string":"new-secret"}"#;
    let edit_error = tools.prepare_edit_json(edit_raw).unwrap_err();
    assert!(edit_error.invalid().is_none());
    assert!(matches!(
        &edit_error,
        PrepareMutationError::Internal(error)
            if error.code() == MutationErrorCode::CheckpointUnavailable
    ));
    assert!(!edit_error.to_string().contains("old-secret"));
    assert!(!format!("{edit_error:?}").contains("new-secret"));
    assert!(!fixture.project.path().join("secret.txt").exists());
    assert_eq!(fs::read_dir(fixture.checkpoints.path()).unwrap().count(), 0);

    assert!(
        InvalidMutation::from_raw(
            MutationTool::Write,
            write_raw,
            MutationErrorCode::AtomicWriteFailed,
        )
        .is_err()
    );
}

#[test]
fn audit_only_mutations_match_scoped_audits_without_creating_capabilities() {
    let fixture = Fixture::new();
    fs::write(fixture.project.path().join("target.txt"), "old-secret").unwrap();
    let base = Tools::new(fixture.project.path()).unwrap();
    let write_raw = r#"{"path":"new.txt","content":"write-secret"}"#;
    let edit_raw = r#"{"path":"target.txt","old_string":"old-secret","new_string":"new-secret"}"#;

    let write_audit = base.audit_write_json(write_raw).unwrap();
    let edit_audit = base.audit_edit_json(edit_raw).unwrap();
    let scoped = fixture.tools("audit-call");
    scoped.read("target.txt", None, None).unwrap();
    assert_eq!(
        &write_audit,
        scoped.prepare_write_json(write_raw).unwrap().audit()
    );
    assert_eq!(
        &edit_audit,
        scoped.prepare_edit_json(edit_raw).unwrap().audit()
    );
    let debug = format!("{write_audit:?}{edit_audit:?}");
    assert!(!debug.contains("write-secret"));
    assert!(!debug.contains("old-secret"));
    assert!(!debug.contains("new-secret"));
    assert!(!fixture.project.path().join("new.txt").exists());
    assert_eq!(
        fs::read_to_string(fixture.project.path().join("target.txt")).unwrap(),
        "old-secret"
    );
    assert_eq!(fs::read_dir(fixture.checkpoints.path()).unwrap().count(), 0);

    let invalid_raw = r#"{"path":".git/escape","content":"invalid-secret"}"#;
    let invalid = invalid(base.audit_write_json(invalid_raw).unwrap_err());
    assert_eq!(invalid.code(), MutationErrorCode::PathGit);
    let projection = invalid.audit().to_json().unwrap();
    assert!(!projection.contains("invalid-secret"));
    assert!(!projection.contains("../escape"));
}

#[test]
fn checkpoint_root_equal_to_or_inside_project_is_rejected_without_mutation() {
    let fixture = Fixture::new();
    let target = fixture.project.path().join("target");
    fs::write(&target, "unchanged").unwrap();

    let equal_error = Tools::new(fixture.project.path())
        .unwrap()
        .with_mutation_context(fixture.project.path(), "project", "thread", "equal")
        .unwrap_err();
    assert_eq!(
        mutation_code(equal_error),
        MutationErrorCode::CheckpointUnavailable
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "unchanged");
    assert_eq!(fs::read_dir(fixture.project.path()).unwrap().count(), 1);

    let descendant = fixture.project.path().join("checkpoints");
    fs::create_dir(&descendant).unwrap();
    let descendant_error = Tools::new(fixture.project.path())
        .unwrap()
        .with_mutation_context(&descendant, "project", "thread", "descendant")
        .unwrap_err();
    assert_eq!(
        mutation_code(descendant_error),
        MutationErrorCode::CheckpointUnavailable
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "unchanged");
    assert_eq!(fs::read_dir(&descendant).unwrap().count(), 0);
}

#[test]
fn write_new_file_records_exact_metadata_before_atomic_creation() {
    let fixture = Fixture::new();
    let tools = fixture.tools("new");
    let output = tools.write("metadata.json", "hello\n").unwrap();
    assert_eq!(
        fs::read(fixture.project.path().join("metadata.json")).unwrap(),
        b"hello\n"
    );
    let success = WriteSuccessOutput::from_json(&output.text).unwrap();
    assert_eq!(success.path, "metadata.json");
    assert_eq!(success.bytes_written, 6);

    let call_root = fixture.call_root("new");
    let metadata_text = fs::read_to_string(call_root.join("metadata.json")).unwrap();
    assert_eq!(
        metadata_text,
        r#"{"metadata_version":"preimage_v1","kind":"created_new_file","path":"metadata.json"}"#
    );
    assert_eq!(
        CreatedNewFileMetadata::from_json(&metadata_text)
            .unwrap()
            .path(),
        "metadata.json"
    );
    assert!(!call_root.join("files").exists());
    assert!(
        !output
            .text
            .contains(fixture.checkpoints.path().to_string_lossy().as_ref())
    );
}

#[test]
fn write_overwrite_preserves_preimage_and_permissions_without_metadata() {
    let fixture = Fixture::new();
    let target = fixture.project.path().join("src/data.bin");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"old").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();

    let output = fixture
        .tools("overwrite")
        .write("src/data.bin", "new")
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o640);
    let call_root = fixture.call_root("overwrite");
    assert_eq!(
        fs::read(call_root.join("files/src/data.bin")).unwrap(),
        b"old"
    );
    assert!(!call_root.join("metadata.json").exists());
    assert_eq!(
        WriteSuccessOutput::from_json(&output.text)
            .unwrap()
            .bytes_written,
        3
    );
}

#[test]
fn unsupported_encoding_is_rejected_without_checkpoint_or_mutation() {
    let fixture = Fixture::new();
    let target = fixture.project.path().join("binary.dat");
    fs::write(&target, b"\xffOLD\xfe").unwrap();
    let tools = fixture.tools("binary-edit");
    assert_eq!(
        mutation_code(tools.read("binary.dat", None, None).unwrap_err()),
        MutationErrorCode::UnsupportedEncoding
    );
    assert_eq!(
        invalid(
            tools
                .prepare_edit_json(r#"{"path":"binary.dat","old_string":"OLD","new_string":"NEW"}"#)
                .unwrap_err()
        )
        .code(),
        MutationErrorCode::FileNotRead
    );
    assert_eq!(fs::read(&target).unwrap(), b"\xffOLD\xfe");
    assert!(!fixture.call_root("binary-edit").exists());
}

#[test]
fn edit_zero_and_nonoverlapping_multiple_matches_create_no_checkpoint_or_change() {
    for (call, content, old, expected) in [
        (
            "zero",
            "prefix secret suffix",
            "absent",
            MutationErrorCode::EditNoMatch,
        ),
        (
            "multiple",
            "aaaa secret context",
            "aa",
            MutationErrorCode::EditMultipleMatches,
        ),
    ] {
        let fixture = Fixture::new();
        let target = fixture.project.path().join("file.txt");
        fs::write(&target, content).unwrap();
        let error = fixture
            .tools(call)
            .edit("file.txt", old, "replacement")
            .unwrap_err();
        assert_eq!(mutation_code(error), expected);
        assert_eq!(fs::read_to_string(&target).unwrap(), content);
        assert!(!fixture.call_root(call).exists());
    }
}

#[test]
fn edit_failure_context_is_bounded_and_absent_from_display_and_debug() {
    let fixture = Fixture::new();
    let body = format!("{}secret-old{}", "x".repeat(2_000), "y".repeat(2_000));
    fs::write(fixture.project.path().join("file.txt"), &body).unwrap();
    let error = fixture
        .tools("context")
        .edit("file.txt", "missing-secret", "new-secret")
        .unwrap_err();
    let ToolError::Mutation(error) = error else {
        panic!("expected mutation error");
    };
    let context = error.edit_context().unwrap().expose();
    assert!(context.chars().count() <= 530);
    assert!(!error.to_string().contains(context));
    assert!(!error.to_string().contains("missing-secret"));
    assert!(!format!("{error:?}").contains(context));
    assert!(!format!("{error:?}").contains("new-secret"));
}

#[test]
fn empty_old_string_is_invalid_and_never_checkpoints() {
    let fixture = Fixture::new();
    fs::write(fixture.project.path().join("a"), "body").unwrap();
    let tools = fixture.tools("empty");
    let error = tools.edit("a", "", "x").unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::EditEmptyOldString);
    assert!(!fixture.call_root("empty").exists());
}

#[test]
fn strict_input_validation_codes_are_stable_and_raw_is_absent() {
    let fixture = Fixture::new();
    let tools = fixture.tools("invalids");
    let cases = [
        ("{secret", MutationErrorCode::MalformedJson),
        ("[]", MutationErrorCode::InputNotObject),
        (r#"{"content":"secret"}"#, MutationErrorCode::MissingPath),
        (
            r#"{"path":1,"content":"secret"}"#,
            MutationErrorCode::WrongPathType,
        ),
        (r#"{"path":"a"}"#, MutationErrorCode::MissingContent),
        (
            r#"{"path":"a","content":1}"#,
            MutationErrorCode::WrongContentType,
        ),
        (
            r#"{"path":"a","content":"secret","extra":true}"#,
            MutationErrorCode::UnexpectedField,
        ),
    ];
    for (raw, code) in cases {
        let invalid = invalid(tools.prepare_write_json(raw).unwrap_err());
        assert_eq!(invalid.code(), code);
        let projection = invalid.audit().to_json().unwrap();
        assert!(!projection.contains("secret"));
        assert!(!invalid.tool_result().contains("secret"));
    }
}

#[test]
fn edit_field_validation_codes_cover_every_missing_and_wrong_type() {
    let fixture = Fixture::new();
    fs::write(fixture.project.path().join("a"), "old").unwrap();
    let tools = fixture.tools("edit-invalids");
    let cases = [
        (
            r#"{"old_string":"old","new_string":"new"}"#,
            MutationErrorCode::MissingPath,
        ),
        (
            r#"{"path":1,"old_string":"old","new_string":"new"}"#,
            MutationErrorCode::WrongPathType,
        ),
        (
            r#"{"path":"a","new_string":"new"}"#,
            MutationErrorCode::MissingOldString,
        ),
        (
            r#"{"path":"a","old_string":1,"new_string":"new"}"#,
            MutationErrorCode::WrongOldStringType,
        ),
        (
            r#"{"path":"a","old_string":"old"}"#,
            MutationErrorCode::MissingNewString,
        ),
        (
            r#"{"path":"a","old_string":"old","new_string":1}"#,
            MutationErrorCode::WrongNewStringType,
        ),
        (
            r#"{"path":"a","old_string":"old","new_string":"new","extra":"secret"}"#,
            MutationErrorCode::UnexpectedField,
        ),
    ];
    for (raw, code) in cases {
        let invalid = invalid(tools.prepare_edit_json(raw).unwrap_err());
        assert_eq!(invalid.code(), code);
        assert!(!invalid.audit().to_json().unwrap().contains("secret"));
    }
}

#[test]
fn mutation_fence_rejects_every_escape_and_special_file_shape() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.project.path().join("dir")).unwrap();
    fs::write(fixture.project.path().join("real"), "body").unwrap();
    fs::hard_link(
        fixture.project.path().join("real"),
        fixture.project.path().join("hard"),
    )
    .unwrap();
    symlink("real", fixture.project.path().join("inside-link")).unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("outside"), "outside").unwrap();
    symlink(
        outside.path().join("outside"),
        fixture.project.path().join("outside-link"),
    )
    .unwrap();
    fs::create_dir_all(fixture.project.path().join(".git/hooks")).unwrap();
    fs::write(fixture.project.path().join(".git/hooks/pre-commit"), "hook").unwrap();

    let cases = [
        ("", MutationErrorCode::PathRoot),
        ("inside-link", MutationErrorCode::PathHardlink),
        ("hard", MutationErrorCode::PathHardlink),
        ("dir", MutationErrorCode::PathNotFile),
        (".git/hooks/pre-commit", MutationErrorCode::PathGit),
    ];
    for (index, (path, code)) in cases.into_iter().enumerate() {
        let tools = fixture.tools(&format!("fence-{index}"));
        let raw = serde_json::json!({"path": path, "content": "secret"}).to_string();
        let invalid = invalid(tools.prepare_write_json(&raw).unwrap_err());
        assert_eq!(invalid.code(), code, "{path}");
        let projection: serde_json::Value =
            serde_json::from_str(&invalid.audit().to_json().unwrap()).unwrap();
        assert!(projection.get("path").is_none());
        assert!(projection.get("content").is_none());
    }
    assert_eq!(
        fs::read_to_string(outside.path().join("outside")).unwrap(),
        "outside"
    );
}

#[test]
fn worktree_real_gitdir_and_hooks_are_read_only() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.project.path().join("control/hooks")).unwrap();
    fs::write(
        fixture.project.path().join("control/hooks/pre-commit"),
        "hook",
    )
    .unwrap();
    fs::write(fixture.project.path().join(".git"), "gitdir: control\n").unwrap();
    let tools = Tools::new(fixture.project.path())
        .unwrap()
        .with_mutation_context(fixture.checkpoints.path(), "project", "thread", "gitdir")
        .unwrap();
    let invalid = invalid(
        tools
            .prepare_write_json(r#"{"path":"control/hooks/pre-commit","content":"replace"}"#)
            .unwrap_err(),
    );
    assert_eq!(invalid.code(), MutationErrorCode::PathGit);
    assert_eq!(
        fs::read_to_string(fixture.project.path().join("control/hooks/pre-commit")).unwrap(),
        "hook"
    );
}

#[test]
fn gitdir_added_after_prepare_is_rechecked_before_checkpoint() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.project.path().join("control/hooks")).unwrap();
    let tools = fixture.tools("late-gitdir");
    let prepared = tools
        .prepare_write_json(r#"{"path":"control/hooks/pre-commit","content":"replace"}"#)
        .unwrap();
    fs::write(fixture.project.path().join(".git"), "gitdir: control\n").unwrap();
    let error = tools.execute_write(prepared).unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::PathGit);
    assert!(!fixture.call_root("late-gitdir").exists());
    assert!(
        !fixture
            .project
            .path()
            .join("control/hooks/pre-commit")
            .exists()
    );
}

#[test]
fn checkpoint_root_cannot_be_injected_as_a_symlink_after_configuration() {
    let fixture = Fixture::new();
    let configured_root = fixture.checkpoints.path().join("configured");
    fs::create_dir(&configured_root).unwrap();
    let tools = Tools::new(fixture.project.path())
        .unwrap()
        .with_mutation_context(&configured_root, "project", "thread", "symlink")
        .unwrap();
    let prepared = tools
        .prepare_write_json(r#"{"path":"target","content":"new"}"#)
        .unwrap();
    let moved = fixture.checkpoints.path().join("moved");
    fs::rename(&configured_root, &moved).unwrap();
    let attacker = tempdir().unwrap();
    symlink(attacker.path(), &configured_root).unwrap();
    let error = tools.execute_write(prepared).unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::CheckpointSymlink);
    assert!(!fixture.project.path().join("target").exists());
    assert_eq!(fs::read_dir(attacker.path()).unwrap().count(), 0);
}

#[test]
fn existing_checkpoint_call_data_is_never_overwritten() {
    let fixture = Fixture::new();
    let call_root = fixture.call_root("existing");
    fs::create_dir_all(&call_root).unwrap();
    fs::write(call_root.join("sentinel"), "keep").unwrap();
    let target = fixture.project.path().join("target");
    fs::write(&target, "old").unwrap();
    let error = fixture
        .tools("existing")
        .write("target", "new")
        .unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::CheckpointExists);
    assert_eq!(fs::read_to_string(&target).unwrap(), "old");
    assert_eq!(
        fs::read_to_string(call_root.join("sentinel")).unwrap(),
        "keep"
    );
}

#[test]
fn post_checkpoint_revalidation_prevents_overwriting_a_changed_target() {
    let fixture = Fixture::new();
    let target = fixture.project.path().join("target");
    fs::write(&target, "old").unwrap();
    let tools = fixture.tools("changed");
    tools.read("target", None, None).unwrap();
    let prepared = tools
        .prepare_write_json(r#"{"path":"target","content":"requested"}"#)
        .unwrap();
    let hook_target = target.clone();
    let hook = || fs::write(&hook_target, "concurrent").unwrap();
    let error = tools
        .execute_write_inner(prepared, Some(&hook))
        .unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::TargetChanged);
    assert_eq!(fs::read_to_string(&target).unwrap(), "concurrent");
    assert_eq!(
        fs::read(fixture.call_root("changed").join("files/target")).unwrap(),
        b"old"
    );
}

#[test]
fn post_checkpoint_symlink_swap_cannot_escape_project() {
    let fixture = Fixture::new();
    let target = fixture.project.path().join("target");
    fs::write(&target, "old").unwrap();
    let outside = tempdir().unwrap();
    let outside_target = outside.path().join("outside");
    fs::write(&outside_target, "outside").unwrap();
    let tools = fixture.tools("swap");
    tools.read("target", None, None).unwrap();
    let prepared = tools
        .prepare_write_json(r#"{"path":"target","content":"requested"}"#)
        .unwrap();
    let hook_target = target.clone();
    let hook_outside = outside_target.clone();
    let hook = || {
        fs::remove_file(&hook_target).unwrap();
        symlink(&hook_outside, &hook_target).unwrap();
    };
    let error = tools
        .execute_write_inner(prepared, Some(&hook))
        .unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::TargetChanged);
    assert_eq!(fs::read_to_string(&outside_target).unwrap(), "outside");
}

#[test]
fn atomic_failure_leaves_target_byte_identical_and_no_temp_file() {
    let fixture = Fixture::new();
    let target = fixture.project.path().join("target");
    fs::write(&target, b"old bytes").unwrap();
    let tools = fixture.tools("atomic-fail");
    tools.read("target", None, None).unwrap();
    let prepared = tools
        .prepare_write_json(r#"{"path":"target","content":"new"}"#)
        .unwrap();
    FAIL_ATOMIC_REPLACE.set(true);
    let result = tools.execute_write(prepared);
    FAIL_ATOMIC_REPLACE.set(false);
    assert_eq!(
        mutation_code(result.unwrap_err()),
        MutationErrorCode::AtomicWriteFailed
    );
    assert_eq!(fs::read(&target).unwrap(), b"old bytes");
    let leaked_temp = fs::read_dir(fixture.project.path())
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".vega-write-")
        });
    assert!(!leaked_temp);
}

#[test]
fn new_file_metadata_precedes_target_and_survives_atomic_target_failure() {
    let fixture = Fixture::new();
    let tools = fixture.tools("new-atomic-fail");
    let prepared = tools
        .prepare_write_json(r#"{"path":"new-file","content":"new"}"#)
        .unwrap();
    FAIL_ATOMIC_REPLACE.set(true);
    let result = tools.execute_write(prepared);
    FAIL_ATOMIC_REPLACE.set(false);
    assert_eq!(
        mutation_code(result.unwrap_err()),
        MutationErrorCode::AtomicWriteFailed
    );
    assert!(!fixture.project.path().join("new-file").exists());
    assert_eq!(
        fs::read_to_string(fixture.call_root("new-atomic-fail").join("metadata.json")).unwrap(),
        r#"{"metadata_version":"preimage_v1","kind":"created_new_file","path":"new-file"}"#
    );
}

#[test]
fn prepared_mutation_cannot_cross_tools_or_projects() {
    let first = Fixture::new();
    let second = Fixture::new();
    let first_tools = first.tools("scope");
    let prepared = first_tools
        .prepare_write_json(r#"{"path":"target","content":"secret"}"#)
        .unwrap();
    let error = second.tools("scope").execute_write(prepared).unwrap_err();
    assert_eq!(
        mutation_code(error),
        MutationErrorCode::PreparedScopeMismatch
    );
    assert!(!first.project.path().join("target").exists());
    assert!(!second.project.path().join("target").exists());
}

#[test]
fn configured_checkpoint_root_may_not_be_a_symlink_or_gitdir() {
    let fixture = Fixture::new();
    let real = fixture.checkpoints.path().join("real");
    fs::create_dir(&real).unwrap();
    let link = fixture.checkpoints.path().join("link");
    symlink(&real, &link).unwrap();
    let error = Tools::new(fixture.project.path())
        .unwrap()
        .with_mutation_context(&link, "p", "t", "c")
        .unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::CheckpointSymlink);

    let git_project = tempdir().unwrap();
    fs::create_dir_all(git_project.path().join(".git")).unwrap();
    let error = Tools::new(git_project.path())
        .unwrap()
        .with_mutation_context(git_project.path().join(".git"), "p", "t", "c")
        .unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::PathGit);

    let worktree = tempdir().unwrap();
    let actual_gitdir = tempdir().unwrap();
    fs::write(
        worktree.path().join(".git"),
        format!("gitdir: {}\n", actual_gitdir.path().display()),
    )
    .unwrap();
    let error = Tools::new(worktree.path())
        .unwrap()
        .with_mutation_context(actual_gitdir.path(), "p", "t", "c")
        .unwrap_err();
    assert_eq!(mutation_code(error), MutationErrorCode::PathGit);
}

#[test]
fn normalized_path_and_exact_success_json_are_stable() {
    let fixture = Fixture::new();
    let output = fixture.tools("normalize").write("./nested", "abc").unwrap();
    let success = WriteSuccessOutput::from_json(&output.text).unwrap();
    assert_eq!(success.path, "nested");
    assert_eq!(
        output.text,
        format!(
            r#"{{"path":"nested","bytes_written":3,"checkpoint_ref":"{}"}}"#,
            success.checkpoint_ref.as_str()
        )
    );
}

#[test]
fn checkpoint_user_control_names_remain_under_files_namespace() {
    let fixture = Fixture::new();
    let path = fixture.project.path().join("nested/metadata.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "old").unwrap();
    fixture
        .tools("reserved")
        .write("nested/metadata.json", "new")
        .unwrap();
    let call_root = fixture.call_root("reserved");
    assert_eq!(
        fs::read_to_string(call_root.join("files/nested/metadata.json")).unwrap(),
        "old"
    );
    assert!(!call_root.join("metadata.json").exists());
}

#[test]
fn read_and_write_resolve_symlink_to_same_target() {
    let fixture = Fixture::new();
    fs::write(fixture.project.path().join("real"), "inside\n").unwrap();
    symlink("real", fixture.project.path().join("alias")).unwrap();
    let tools = fixture.tools("read-write-symlink");
    assert_eq!(tools.read("alias", None, None).unwrap().text, "1 | inside");
    let prepared = tools
        .prepare_write_json(r#"{"path":"alias","content":"new"}"#)
        .unwrap();
    assert_eq!(prepared.normalized_path(), "real");
    tools.execute_write(prepared).unwrap();
    assert_eq!(
        fs::read_to_string(fixture.project.path().join("real")).unwrap(),
        "new"
    );
    assert!(
        fs::symlink_metadata(fixture.project.path().join("alias"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn checkpoint_file_is_single_link_and_private() {
    let fixture = Fixture::new();
    fs::write(fixture.project.path().join("target"), "old").unwrap();
    fixture.tools("private").write("target", "new").unwrap();
    let checkpoint = fixture.call_root("private").join("files/target");
    let metadata = fs::metadata(checkpoint).unwrap();
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.mode() & 0o777, 0o600);
}

#[test]
fn missing_parent_is_created_only_during_authorized_execution() {
    let fixture = Fixture::new();
    let tools = fixture.tools("parent");
    let prepared = tools
        .prepare_write_json(r#"{"path":"missing/file","content":"x"}"#)
        .unwrap();
    assert!(!fixture.project.path().join("missing").exists());
    assert!(!fixture.call_root("parent").exists());
    tools.execute_write(prepared).unwrap();
    assert_eq!(
        fs::read_to_string(fixture.project.path().join("missing/file")).unwrap(),
        "x"
    );
}

#[test]
fn checkpoint_ids_cover_one_and_120_byte_boundaries_in_real_layout() {
    for (call, valid) in [("x".to_string(), true), ("x".repeat(120), true)] {
        let fixture = Fixture::new();
        let tools = Tools::new(fixture.project.path())
            .unwrap()
            .with_mutation_context(fixture.checkpoints.path(), "p", "t", &call);
        assert_eq!(tools.is_ok(), valid);
    }
    let fixture = Fixture::new();
    for invalid_id in ["".to_string(), "x".repeat(121)] {
        let error = Tools::new(fixture.project.path())
            .unwrap()
            .with_mutation_context(fixture.checkpoints.path(), "p", "t", &invalid_id)
            .unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::CheckpointIdInvalid);

        let raw = r#"{"path":"secret-path","content":"secret-body"}"#;
        let invalid = InvalidMutation::from_raw(
            MutationTool::Write,
            raw,
            MutationErrorCode::CheckpointIdInvalid,
        )
        .unwrap();
        assert_eq!(invalid.code(), MutationErrorCode::CheckpointIdInvalid);
        assert!(!invalid.audit().to_json().unwrap().contains("secret-path"));
        assert!(!invalid.audit().to_json().unwrap().contains("secret-body"));
    }
}

#[test]
fn checkpoint_layout_contains_only_expected_paths() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.project.path().join("a/b")).unwrap();
    fs::write(fixture.project.path().join("a/b/file"), "old").unwrap();
    fixture.tools("layout").write("a/b/file", "new").unwrap();
    let call_root = fixture.call_root("layout");
    assert!(call_root.join("files/a/b/file").is_file());
    assert!(!call_root.join("metadata.json").exists());
    let entries = walk_relative(&call_root);
    assert_eq!(entries, ["files", "files/a", "files/a/b", "files/a/b/file"]);
}

fn walk_relative(root: &Path) -> Vec<String> {
    fn visit(root: &Path, path: &Path, output: &mut Vec<String>) {
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            output.push(
                entry
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            if entry.is_dir() {
                visit(root, &entry, output);
            }
        }
    }
    let mut output = Vec::new();
    visit(root, root, &mut output);
    output
}

#[test]
fn issue112_absolute_external_read_edit_and_replace_all() {
    let fixture = Fixture::new();
    let sibling = tempdir().unwrap();
    let path = sibling.path().join("note.txt");
    fs::write(&path, "old\nold\n").unwrap();
    let tools = fixture.tools("external");
    tools.read(path.to_str().unwrap(), None, None).unwrap();
    let input = serde_json::json!({"file_path":path,"old_string":"old","new_string":"new","replace_all":true});
    let prepared = tools.prepare_edit_json(&input.to_string()).unwrap();
    let output = tools.execute_edit(prepared).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new\nnew\n");
    assert_eq!(
        EditSuccessOutput::from_json(&output.text)
            .unwrap()
            .replacements,
        2
    );
}

#[test]
fn issue112_read_versions_are_shared_only_explicitly_and_rechecked_after_approval() {
    let f = Fixture::new();
    let path = f.project.path().join("a");
    fs::write(&path, "old").unwrap();
    let first = f.tools("first");
    let raw = r#"{"file_path":"a","old_string":"old","new_string":"new"}"#;
    assert_eq!(
        invalid(first.prepare_edit_json(raw).unwrap_err()).code(),
        MutationErrorCode::FileNotRead
    );
    first.read("a", None, None).unwrap();
    let isolated = f.tools("isolated");
    assert_eq!(
        invalid(isolated.prepare_edit_json(raw).unwrap_err()).code(),
        MutationErrorCode::FileNotRead
    );
    let shared = f.tools("shared").with_read_state(first.read_state());
    let prepared = shared.prepare_edit_json(raw).unwrap();
    fs::write(&path, "changed").unwrap();
    assert_eq!(
        mutation_code(shared.execute_edit(prepared).unwrap_err()),
        MutationErrorCode::TargetChanged
    );
    assert!(!f.call_root("shared").exists());
    assert_eq!(
        invalid(shared.prepare_edit_json(raw).unwrap_err()).code(),
        MutationErrorCode::TargetChanged
    );
    // Identical bytes with changed mtime are safe, and success refreshes the read version.
    fs::write(&path, "old").unwrap();
    shared
        .execute_edit(shared.prepare_edit_json(raw).unwrap())
        .unwrap();
    let next = f.tools("next").with_read_state(first.read_state());
    next.execute_edit(
        next.prepare_edit_json(r#"{"file_path":"a","old_string":"new","new_string":"final"}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "final");
}

#[test]
fn issue112_quote_fallback_exact_priority_deletion_and_no_whitespace_fuzz() {
    let cases = [
        (
            "prefix “old” suffix\n",
            "\"old\"",
            "\"new\"",
            "prefix “new” suffix\n",
        ),
        ("‘old’ and don't", "'old'", "'new'", "‘new’ and don't"),
        (
            "“old” and \"old\"",
            "\"old\"",
            "\"new\"",
            "“old” and \"new\"",
        ),
        ("before\nremove\nafter\n", "remove", "", "before\nafter\n"),
        ("aaaa", "aa", "b", "ba"),
    ];
    for (content, old, new, expected) in cases {
        let f = Fixture::new();
        fs::write(f.project.path().join("a"), content).unwrap();
        let tools = f.tools("edit");
        if content == "aaaa" {
            assert_eq!(
                mutation_code(tools.edit("a", old, new).unwrap_err()),
                MutationErrorCode::EditMultipleMatches
            );
        } else {
            tools.edit("a", old, new).unwrap();
            assert_eq!(
                fs::read_to_string(f.project.path().join("a")).unwrap(),
                expected
            );
        }
    }
    let f = Fixture::new();
    fs::write(f.project.path().join("a"), "  indented\n").unwrap();
    let tools = f.tools("indent");
    assert_eq!(
        mutation_code(tools.edit("a", "    indented", "changed").unwrap_err()),
        MutationErrorCode::EditNoMatch
    );
    assert_eq!(
        invalid(
            tools
                .prepare_edit_json(r#"{"file_path":"a","old_string":"same","new_string":"same"}"#)
                .unwrap_err()
        )
        .code(),
        MutationErrorCode::EditNoChange
    );
}

#[test]
fn issue112_encoding_roundtrips_and_explicit_write_endings() {
    for utf16 in [false, true] {
        let f = Fixture::new();
        let path = f.project.path().join("a");
        let original = "\u{feff}old\r\nkeep\r\n";
        let original = if utf16 {
            original.encode_utf16().flat_map(u16::to_le_bytes).collect()
        } else {
            original.as_bytes().to_vec()
        };
        fs::write(&path, &original).unwrap();
        let tools = f.tools("edit");
        tools.edit("a", "old\n", "new\r\n").unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            crate::text_file::TextFile::decode(&bytes).unwrap().content,
            "\u{feff}new\nkeep\n"
        );
        assert_eq!(
            fs::read(f.call_root("edit").join("files/a")).unwrap(),
            original
        );
        let write = f.tools("write").with_read_state(tools.read_state());
        write
            .execute_write(
                write
                    .prepare_write_json(r#"{"file_path":"a","content":"explicit\n"}"#)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(
            write.read("a", None, None).unwrap().text,
            "1 | \u{feff}explicit"
        );
        let bytes = fs::read(path).unwrap();
        if utf16 {
            assert!(bytes.starts_with(&[0xff, 0xfe]));
        } else {
            assert!(bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        }
        assert!(
            !crate::text_file::TextFile::decode(&bytes)
                .unwrap()
                .content
                .contains('\r')
        );
    }
}

#[test]
fn issue112_empty_creation_replace_all_identity_and_external_checkpoint_confinement() {
    let f = Fixture::new();
    let external = tempdir().unwrap();
    let path = external.path().join("nested/new.txt");
    let tools = f.tools("new");
    let raw = serde_json::json!({"file_path":path,"old_string":"","new_string":"one one"});
    tools
        .execute_edit(tools.prepare_edit_json(&raw.to_string()).unwrap())
        .unwrap();
    let next = f.tools("all").with_read_state(tools.read_state());
    let raw = serde_json::json!({"file_path":path,"old_string":"one","new_string":"two","replace_all":true});
    let all = next.prepare_edit_json(&raw.to_string()).unwrap();
    let mut once_raw = raw.clone();
    once_raw["replace_all"] = false.into();
    assert_ne!(
        all.audit().to_json().unwrap(),
        next.prepare_edit_json(&once_raw.to_string())
            .unwrap()
            .audit()
            .to_json()
            .unwrap()
    );
    next.execute_edit(all).unwrap();
    let map: serde_json::Value =
        serde_json::from_slice(&fs::read(f.call_root("all").join("target.json")).unwrap()).unwrap();
    assert_eq!(map["path"], path.canonicalize().unwrap().to_str().unwrap());
    let key = map["artifact"].as_str().unwrap();
    assert!(key.starts_with("files/external-"));
    assert!(!Path::new(key).is_absolute());
    assert_eq!(
        fs::read_to_string(f.call_root("all").join(key)).unwrap(),
        "one one"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "two two");
    let blocked =
        serde_json::json!({"file_path":f.checkpoints.path().join("tamper"),"content":"bad"});
    assert!(next.prepare_write_json(&blocked.to_string()).is_err());
    assert!(!f.checkpoints.path().join("tamper").exists());
}

#[test]
fn issue112_empty_existing_files_need_read_and_null_replace_all_is_false() {
    let f = Fixture::new();
    fs::write(f.project.path().join("empty"), " \t\n").unwrap();
    let tools = f.tools("empty");
    let raw = r#"{"file_path":"empty","old_string":"","new_string":"body","replace_all":null}"#;
    assert_eq!(
        invalid(tools.prepare_edit_json(raw).unwrap_err()).code(),
        MutationErrorCode::FileNotRead
    );
    assert_eq!(
        invalid(
            tools
                .prepare_write_json(r#"{"file_path":"empty","content":"body"}"#)
                .unwrap_err()
        )
        .code(),
        MutationErrorCode::FileNotRead
    );
    tools.read("empty", None, None).unwrap();
    tools
        .execute_edit(tools.prepare_edit_json(raw).unwrap())
        .unwrap();
    assert_eq!(
        fs::read_to_string(f.project.path().join("empty")).unwrap(),
        "body"
    );
    let invalid_type =
        r#"{"file_path":"empty","old_string":"body","new_string":"x","replace_all":"yes"}"#;
    assert_eq!(
        invalid(tools.prepare_edit_json(invalid_type).unwrap_err()).code(),
        MutationErrorCode::WrongReplaceAllType
    );
    assert_eq!(
        invalid(
            tools
                .prepare_write_json(r#"{"path":"empty","file_path":"empty","content":"x"}"#)
                .unwrap_err()
        )
        .code(),
        MutationErrorCode::UnexpectedField
    );
}

#[test]
fn issue112_approval_pins_canonical_target_even_when_other_target_was_already_read() {
    let f = Fixture::new();
    let external = tempdir().unwrap();
    fs::create_dir(f.project.path().join("dir")).unwrap();
    fs::write(f.project.path().join("dir/a"), "old").unwrap();
    fs::write(external.path().join("a"), "old").unwrap();
    let tools = f.tools("pin");
    tools.read("dir/a", None, None).unwrap();
    tools
        .read(external.path().join("a").to_str().unwrap(), None, None)
        .unwrap();
    let prepared = tools
        .prepare_write_json(r#"{"file_path":"dir/a","content":"new"}"#)
        .unwrap();
    fs::rename(f.project.path().join("dir"), f.project.path().join("moved")).unwrap();
    symlink(external.path(), f.project.path().join("dir")).unwrap();
    assert_eq!(
        mutation_code(tools.execute_write(prepared).unwrap_err()),
        MutationErrorCode::TargetChanged
    );
    assert_eq!(
        fs::read_to_string(external.path().join("a")).unwrap(),
        "old"
    );
    assert!(!f.call_root("pin").exists());
}

#[test]
fn issue112_oversized_text_is_rejected_before_allocation() {
    let f = Fixture::new();
    let path = f.project.path().join("large");
    fs::File::create(&path)
        .unwrap()
        .set_len(1024 * 1024 * 1024 + 1)
        .unwrap();
    let tools = f.tools("large");
    assert_eq!(
        mutation_code(tools.read("large", None, None).unwrap_err()),
        MutationErrorCode::FileTooLarge
    );
    assert_eq!(
        invalid(
            tools
                .prepare_write_json(r#"{"file_path":"large","content":"tiny"}"#)
                .unwrap_err()
        )
        .code(),
        MutationErrorCode::FileTooLarge
    );
    assert!(!f.call_root("large").exists());
}

#[test]
fn issue112_bom_only_empty_edit_and_mixed_newline_detection() {
    for utf16 in [false, true] {
        let f = Fixture::new();
        let path = f.project.path().join("bom");
        let body = "\u{feff} \t\n";
        let bytes = if utf16 {
            body.encode_utf16().flat_map(u16::to_le_bytes).collect()
        } else {
            body.as_bytes().to_vec()
        };
        fs::write(&path, bytes).unwrap();
        let tools = f.tools("bom");
        let raw = r#"{"file_path":"bom","old_string":"","new_string":"filled"}"#;
        assert_eq!(
            invalid(tools.prepare_edit_json(raw).unwrap_err()).code(),
            MutationErrorCode::FileNotRead
        );
        tools.read("bom", None, None).unwrap();
        tools
            .execute_edit(tools.prepare_edit_json(raw).unwrap())
            .unwrap();
        assert_eq!(
            crate::text_file::TextFile::decode(&fs::read(path).unwrap())
                .unwrap()
                .content,
            "\u{feff}filled"
        );
    }
    for (body, expected) in [
        ("old\r\nx\n", "new\nx\n"),
        ("old\r\nx\r\ny\n", "new\r\nx\r\ny\r\n"),
    ] {
        let f = Fixture::new();
        fs::write(f.project.path().join("mixed"), body).unwrap();
        f.tools("mixed").edit("mixed", "old", "new").unwrap();
        assert_eq!(
            fs::read_to_string(f.project.path().join("mixed")).unwrap(),
            expected
        );
    }
}

#[test]
fn issue112_completed_replay_survives_deleted_targets_and_encoding_changes() {
    let f = Fixture::new();
    let path = f.project.path().join("a");
    fs::write(&path, "\u{feff}old").unwrap();
    let tools = f.tools("write");
    tools.read("a", None, None).unwrap();
    let raw = r#"{"file_path":"a","content":"new"}"#;
    let prepared = tools.prepare_write_json(raw).unwrap();
    let audit = prepared.audit().clone();
    tools.execute_write(prepared).unwrap();
    fs::write(&path, [0xff, 0]).unwrap();
    assert_eq!(
        tools
            .audit_replay_json(MutationTool::Write, raw, &audit)
            .unwrap()
            .to_json()
            .unwrap(),
        audit.to_json().unwrap()
    );
    fs::remove_file(&path).unwrap();
    assert_eq!(
        tools
            .audit_replay_json(MutationTool::Write, raw, &audit)
            .unwrap()
            .to_json()
            .unwrap(),
        audit.to_json().unwrap()
    );
    let edit_raw = r#"{"file_path":"a","old_string":"old","new_string":"new","replace_all":true}"#;
    let edit_audit = crate::WriteEditAudit::edit_with_replace_all("a", "old", "new", true).unwrap();
    assert_eq!(
        tools
            .audit_replay_json(MutationTool::Edit, edit_raw, &edit_audit)
            .unwrap()
            .to_json()
            .unwrap(),
        edit_audit.to_json().unwrap()
    );
    assert!(!path.exists());
    assert!(
        tools
            .audit_replay_json(MutationTool::Edit, edit_raw, &audit)
            .is_err()
    );
    let different =
        r#"{"file_path":"a","old_string":"old","new_string":"new","replace_all":false}"#;
    assert_ne!(
        tools
            .audit_replay_json(MutationTool::Edit, different, &edit_audit)
            .unwrap()
            .to_json()
            .unwrap(),
        edit_audit.to_json().unwrap()
    );
}

#[test]
fn issue112_git_components_are_protected_on_case_insensitive_volumes() {
    let f = Fixture::new();
    fs::create_dir(f.project.path().join(".git")).unwrap();
    fs::write(f.project.path().join(".git/config"), "keep").unwrap();
    let tools = f.tools("git-case");
    for name in [".git/config", ".GIT/config", ".Git/config"] {
        let absolute = f.project.path().join(name);
        assert_eq!(
            mutation_code(
                tools
                    .read(absolute.to_str().unwrap(), None, None)
                    .unwrap_err()
            ),
            MutationErrorCode::PathGit
        );
        let raw = serde_json::json!({"file_path":absolute,"content":"bad"});
        assert_eq!(
            invalid(tools.prepare_write_json(&raw.to_string()).unwrap_err()).code(),
            MutationErrorCode::PathGit
        );
    }
    assert_eq!(
        fs::read_to_string(f.project.path().join(".git/config")).unwrap(),
        "keep"
    );
    assert!(!f.call_root("git-case").exists());
}
