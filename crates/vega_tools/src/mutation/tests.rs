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

    let invalid_raw = r#"{"path":"../escape","content":"invalid-secret"}"#;
    let invalid = invalid(base.audit_write_json(invalid_raw).unwrap_err());
    assert_eq!(invalid.code(), MutationErrorCode::PathParent);
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
fn write_overwrite_preserves_binary_preimage_and_permissions_without_metadata() {
    let fixture = Fixture::new();
    let target = fixture.project.path().join("src/data.bin");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"\xff\x00old").unwrap();
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
        b"\xff\x00old"
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
fn edit_unique_match_handles_non_utf8_and_records_original_bytes() {
    let fixture = Fixture::new();
    let target = fixture.project.path().join("binary.dat");
    fs::write(&target, b"\xffOLD\xfe").unwrap();
    let output = fixture
        .tools("binary-edit")
        .edit("binary.dat", "OLD", "NEW")
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"\xffNEW\xfe");
    assert_eq!(
        fs::read(fixture.call_root("binary-edit").join("files/binary.dat")).unwrap(),
        b"\xffOLD\xfe"
    );
    let success = EditSuccessOutput::from_json(&output.text).unwrap();
    assert_eq!(success.replacements, 1);
    assert_eq!(success.bytes_written, 5);
}

#[test]
fn edit_zero_and_overlapping_multiple_matches_create_no_checkpoint_or_change() {
    for (call, content, old, expected) in [
        (
            "zero",
            "prefix secret suffix",
            "absent",
            MutationErrorCode::EditNoMatch,
        ),
        (
            "multiple",
            "aaa secret context",
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
    let invalid = invalid(
        fixture
            .tools("empty")
            .prepare_edit_json(r#"{"path":"a","old_string":"","new_string":"x"}"#)
            .unwrap_err(),
    );
    assert_eq!(invalid.code(), MutationErrorCode::EditEmptyOldString);
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
        ("/absolute", MutationErrorCode::PathAbsolute),
        ("../parent", MutationErrorCode::PathParent),
        ("", MutationErrorCode::PathRoot),
        ("missing/child", MutationErrorCode::ParentNotFound),
        ("inside-link", MutationErrorCode::PathSymlink),
        ("outside-link", MutationErrorCode::PathSymlink),
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
    fs::write(&target, b"old\0bytes").unwrap();
    let tools = fixture.tools("atomic-fail");
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
    assert_eq!(fs::read(&target).unwrap(), b"old\0bytes");
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
fn read_behavior_still_follows_internal_symlink_while_write_rejects_it() {
    let fixture = Fixture::new();
    fs::write(fixture.project.path().join("real"), "inside\n").unwrap();
    symlink("real", fixture.project.path().join("alias")).unwrap();
    let tools = fixture.tools("read-write-symlink");
    assert_eq!(tools.read("alias", None, None).unwrap().text, "1 | inside");
    let raw = r#"{"path":"alias","content":"new"}"#;
    assert_eq!(
        invalid(tools.prepare_write_json(raw).unwrap_err()).code(),
        MutationErrorCode::PathSymlink
    );
    assert_eq!(
        fs::read_to_string(fixture.project.path().join("real")).unwrap(),
        "inside\n"
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
fn parent_directory_is_never_created_implicitly() {
    let fixture = Fixture::new();
    let tools = fixture.tools("parent");
    let invalid = invalid(
        tools
            .prepare_write_json(r#"{"path":"missing/file","content":"x"}"#)
            .unwrap_err(),
    );
    assert_eq!(invalid.code(), MutationErrorCode::ParentNotFound);
    assert!(!fixture.project.path().join("missing").exists());
    assert!(!fixture.call_root("parent").exists());
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
