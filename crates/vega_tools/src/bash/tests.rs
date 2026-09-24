use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tempfile::{TempDir, tempdir};
use tokio_util::sync::CancellationToken;

use crate::Tools;
use crate::error::BashErrorCode;
use crate::sandbox::{ExecutionHooks, ScanFailure, TestPathHook};

use super::{TEMP_PATH_PLACEHOLDER, redact_temp_path};

fn tools() -> (TempDir, Tools) {
    let project = tempdir().unwrap();
    let tools = Tools::new(project.path()).unwrap();
    (project, tools)
}

fn capture_temp() -> (Arc<Mutex<Option<std::path::PathBuf>>>, TestPathHook) {
    let captured = Arc::new(Mutex::new(None));
    let hook_capture = captured.clone();
    let hook = Arc::new(move |path: &std::path::Path| {
        assert_eq!(
            fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        *hook_capture.lock().unwrap() = Some(path.to_path_buf());
    });
    (captured, hook)
}

fn captured_path(captured: &Arc<Mutex<Option<std::path::PathBuf>>>) -> std::path::PathBuf {
    captured.lock().unwrap().clone().unwrap()
}

#[test]
fn bash_input_is_strict_and_defaults_timeout() {
    let (_project, tools) = tools();
    let default = tools.prepare_bash_json(r#"{"cmd":"pwd"}"#).unwrap();
    assert_eq!(default.command(), "pwd");
    assert_eq!(default.timeout_ms(), 120_000);
    assert!(!format!("{default:?}").contains("pwd"));

    let custom = tools
        .prepare_bash_json(r#"{"cmd":"pwd","timeout_ms":321}"#)
        .unwrap();
    assert_eq!(custom.timeout_ms(), 321);

    let explicit_null = tools
        .prepare_bash_json(r#"{"cmd":"pwd","timeout_ms":null}"#)
        .unwrap();
    assert_eq!(explicit_null.timeout_ms(), 120_000);

    for raw in [
        "{}",
        r#"{"command":"pwd"}"#,
        r#"{"cmd":"pwd","command":"false"}"#,
        r#"{"cmd":""}"#,
        r#"{"cmd":1}"#,
        r#"{"cmd":"pwd","cwd":"/"}"#,
        r#"{"cmd":"pwd","full_access":true}"#,
        r#"{"cmd":"pwd","timeout_ms":0}"#,
        r#"{"cmd":"pwd","timeout_ms":-1}"#,
        r#"{"cmd":"pwd","timeout_ms":1.5}"#,
        r#"{"cmd":"pwd","timeout_ms":18446744073709551616}"#,
        r#"{"cmd":"a","cmd":"b"}"#,
    ] {
        assert_eq!(
            tools.prepare_bash_json(raw).unwrap_err().code(),
            BashErrorCode::InvalidInput,
            "{raw}"
        );
    }
}

#[test]
fn bash_temp_redaction_replaces_exact_paths_after_stream_assembly_in_place() {
    let path = "/private/tmp/.vega-bash-0123456789abcdef";
    let mut output = format!("prefix {path}/a\n{path}\nsuffix");
    let capacity = output.capacity();
    redact_temp_path(&mut output, path);
    assert_eq!(
        output,
        format!("prefix {TEMP_PATH_PLACEHOLDER}/a\n{TEMP_PATH_PLACEHOLDER}\nsuffix")
    );
    assert!(!output.contains(path));
    assert_eq!(output.capacity(), capacity);
}

#[tokio::test]
async fn sandbox_temp_hardlink_is_rejected_before_any_child_spawn_and_cleaned() {
    let (_project, tools) = tools();
    let external = tempfile::tempdir_in("/private/tmp").unwrap();
    let external_file = external.path().join("external");
    fs::write(&external_file, "unchanged").unwrap();
    let prepared = tools
        .prepare_bash_json(r#"{"cmd":"print escaped > marker"}"#)
        .unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let (captured, capture_hook) = capture_temp();
    let external_for_hook = external_file.clone();
    let inject_hook: TestPathHook = Arc::new(move |path| {
        capture_hook(path);
        fs::hard_link(&external_for_hook, path.join("linked")).unwrap();
    });
    let hooks = ExecutionHooks {
        spawn_count: Some(count.clone()),
        after_temp_created: Some(inject_hook),
        ..ExecutionHooks::default()
    };
    let error = tools
        .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
        .await
        .unwrap_err();
    assert_eq!(error.code(), BashErrorCode::HardlinkPreflight);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(fs::read_to_string(external_file).unwrap(), "unchanged");
    assert!(!captured_path(&captured).exists());
}

#[tokio::test]
async fn sandbox_hardlink_preflight_rejects_visible_hidden_and_ignored_before_spawn() {
    for relative in ["linked", ".hidden-linked", "ignored/linked"] {
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside");
        fs::write(&outside_file, "unchanged").unwrap();
        let linked = project.path().join(relative);
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        fs::hard_link(&outside_file, &linked).unwrap();
        fs::write(project.path().join(".gitignore"), "ignored/\n").unwrap();
        let tools = Tools::new(project.path()).unwrap();
        let prepared = tools
            .prepare_bash_json(r#"{"cmd":"print escaped > marker"}"#)
            .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let hooks = ExecutionHooks {
            spawn_count: Some(count.clone()),
            ..ExecutionHooks::default()
        };
        let error = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap_err();
        assert_eq!(error.code(), BashErrorCode::HardlinkPreflight, "{relative}");
        assert_eq!(count.load(Ordering::SeqCst), 0, "{relative}");
        assert!(!project.path().join("marker").exists());
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "unchanged");
    }
}

#[tokio::test]
async fn sandbox_scan_failures_are_zero_spawn() {
    for failure in [ScanFailure::Traversal, ScanFailure::Metadata] {
        let (project, tools) = tools();
        fs::write(project.path().join("ordinary"), "one-link").unwrap();
        let prepared = tools
            .prepare_bash_json(r#"{"cmd":"print escaped > marker"}"#)
            .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let hooks = ExecutionHooks {
            spawn_count: Some(count.clone()),
            scan_failure: Some(failure),
            ..ExecutionHooks::default()
        };
        let error = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap_err();
        assert_eq!(error.code(), BashErrorCode::HardlinkPreflight);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(!project.path().join("marker").exists());
    }
}

#[tokio::test]
async fn sandbox_temp_is_cleaned_after_pre_spawn_rejection() {
    let (_project, tools) = tools();
    let prepared = tools
        .prepare_bash_json(r#"{"cmd":"print escaped"}"#)
        .unwrap();
    let (rejected_temp, hook) = capture_temp();
    let hooks = ExecutionHooks {
        scan_failure: Some(ScanFailure::Traversal),
        after_temp_created: Some(hook),
        ..ExecutionHooks::default()
    };
    let error = tools
        .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
        .await
        .unwrap_err();
    assert_eq!(error.code(), BashErrorCode::HardlinkPreflight);
    assert!(!captured_path(&rejected_temp).exists());
}

#[tokio::test]
async fn full_access_preserves_scope_binding_and_precancel() {
    let (project, tools) = tools();
    let other = Tools::new(project.path()).unwrap();
    let prepared = tools
        .prepare_bash_json(r#"{"cmd":"print unsafe > marker"}"#)
        .unwrap();
    assert_eq!(
        other
            .execute_bash_full_access(prepared, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        BashErrorCode::ScopeMismatch
    );
    let prepared = tools
        .prepare_bash_json(r#"{"cmd":"print unsafe > marker"}"#)
        .unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        tools
            .execute_bash_full_access(prepared, cancel)
            .await
            .unwrap_err()
            .code(),
        BashErrorCode::Cancelled
    );
    assert!(!project.path().join("marker").exists());
}
