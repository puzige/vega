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

#[test]
fn completed_shell_output_distinguishes_signal_from_exit_code() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::time::Instant;

    for (raw, code, diagnostic) in [
        (0, 0, None),
        (7 << 8, 7, None),
        (libc::SIGTRAP, -1, Some("Process terminated by signal 5\n")),
        (libc::SIGTERM, -1, Some("Process terminated by signal 15\n")),
    ] {
        for text in ["", "output", "output\n"] {
            let mut collector = crate::output::BashOutputCollector::new();
            collector.push(text.as_bytes());
            let collected = collector.finish();
            let original = collected.text.clone();
            let output = super::build_output(ExitStatus::from_raw(raw), Instant::now(), collected);
            assert_eq!(output.exit_code, code);
            assert!(output.text.starts_with(&original));
            assert!(!output.truncated);
            if let Some(diagnostic) = diagnostic {
                assert!(output.text.ends_with(diagnostic));
                if !original.is_empty() {
                    assert!(output.text.contains("output\nProcess terminated"));
                }
            } else {
                assert_eq!(output.text, original);
            }
        }
    }
}

#[tokio::test]
async fn shell_reader_retains_startup_stderr_and_drains_both_streams() {
    for (stdout, stderr) in [
        ("command output\n", "startup diagnostic\n"),
        ("", "startup diagnostic\n"),
        ("command output\n", ""),
    ] {
        let output = super::read_output(
            stdout.as_bytes(),
            stderr.as_bytes(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(output.text.contains(stdout.trim_end()));
        assert!(output.text.contains(stderr.trim_end()));
        assert_eq!(output.text.len(), stdout.len() + stderr.len() - 1);
        assert!(!output.truncated);
    }
}

#[tokio::test]
async fn shell_reader_cancels_while_both_streams_remain_open() {
    let (stdout, _stdout_writer) = tokio::io::duplex(64);
    let (stderr, _stderr_writer) = tokio::io::duplex(64);
    let stop = CancellationToken::new();
    stop.cancel();
    let output = super::read_output(stdout, stderr, stop).await.unwrap();
    assert!(output.text.is_empty());
    assert!(!output.truncated);
}

#[tokio::test]
async fn shell_reader_applies_one_output_limit_to_both_streams() {
    let stdout = "stdout\n".repeat(crate::output::BASH_MAX_LINES_PER_SIDE * 2);
    let stderr = "stderr\n".repeat(crate::output::BASH_MAX_LINES_PER_SIDE * 2);
    let output = super::read_output(
        stdout.as_bytes(),
        stderr.as_bytes(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(output.truncated);
    assert!(
        output
            .text
            .contains(crate::output::BASH_OUTPUT_MIDDLE_MARKER)
    );
    assert!(output.text.lines().count() <= crate::output::BASH_MAX_LINES_PER_SIDE * 2 + 1);
}

#[tokio::test]
async fn shell_reader_preserves_each_stream_order() {
    let stdout = "stdout-first\nstdout-second\n";
    let stderr = "stderr-first\nstderr-second\n";
    let output = super::read_output(
        stdout.as_bytes(),
        stderr.as_bytes(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    for stream in ["stdout", "stderr"] {
        assert!(
            output.text.find(&format!("{stream}-first")).unwrap()
                < output.text.find(&format!("{stream}-second")).unwrap()
        );
    }
}

#[test]
fn signal_diagnostic_has_fixed_overhead_on_truncated_output() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::time::Instant;

    let mut collector = crate::output::BashOutputCollector::new();
    let line = "x".repeat(crate::output::BASH_MAX_LINE_BYTES / 2) + "\n";
    for _ in 0..300 {
        collector.push(line.as_bytes());
    }
    let collected = collector.finish();
    let payload_len = collected.text.len();
    assert!(collected.truncated);
    assert!(payload_len <= 2 * crate::output::BASH_MAX_BYTES_PER_SIDE);
    let output = super::build_output(
        ExitStatus::from_raw(libc::SIGTRAP),
        Instant::now(),
        collected,
    );
    assert!(output.truncated);
    assert_eq!(
        output.text.len() - payload_len,
        "\nProcess terminated by signal 5\n".len()
    );
    assert!(output.text.len() <= 2 * crate::output::BASH_MAX_BYTES_PER_SIDE + 42);
}
