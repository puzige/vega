mod offline;

use super::*;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use tokio::io::{AsyncWriteExt, ReadBuf};
use vega_credentials::{CredentialBinding, CredentialError};

#[tokio::test]
async fn full_access_constructs_a_direct_shell_without_touching_binding_credentials() {
    for binding_count in [None, Some(0), Some(1)] {
        for reader_available in [false, true] {
            let project = TempRoot::create().expect("project fixture");
            let temporary = TempRoot::create().expect("command temp fixture");
            std::fs::write(project.path().join(".git"), b"invalid git indirection")
                .expect("git discovery must not run");
            let protected_root = project.path().join("credentials-\"quoted");
            std::fs::create_dir(&protected_root).expect("protected root");
            let reads = Arc::new(AtomicUsize::new(0));
            let mut tools = Tools::new(project.path()).expect("tools");
            if let Some(count) = binding_count {
                let observed = reads.clone();
                let bindings = (0..count)
                    .map(|_| CredentialBinding {
                        key_ref: "optional-fixture-ref".into(),
                        env: "FIXTURE_TOKEN".into(),
                        origin: "https://fixture.invalid".into(),
                        header: "Authorization".into(),
                        prefix: "Bearer ".into(),
                        path: "/api".into(),
                    })
                    .collect();
                tools = tools.with_credential_policy(
                    crate::ToolCredentialPolicy::new(
                        protected_root.clone(),
                        bindings,
                        Arc::new(move |_| {
                            observed.fetch_add(1, Ordering::SeqCst);
                            if reader_available {
                                Ok("synthetic-fixture-secret".into())
                            } else {
                                Err(CredentialError::Unavailable)
                            }
                        }),
                    )
                    .expect("binding policy"),
                );
            }
            let command_text = "  printf '%s\\n' \"$FIXTURE_TOKEN\"\n";
            let prepared = tools
                .prepare_bash_json(
                    &serde_json::json!({"command": command_text, "timeout": 7}).to_string(),
                )
                .expect("prepared");
            assert_eq!(prepared.command(), command_text);
            assert_eq!(prepared.timeout_ms(), 7_000);
            let launch = tools
                .prepare_bash_launch(
                    &prepared,
                    BashLaunchPolicy::FullAccess,
                    &CancellationToken::new(),
                    &temporary,
                )
                .await
                .expect("direct launch plan");
            assert_eq!(launch.command.get_program(), OsStr::new("/bin/zsh"));
            assert_eq!(
                launch.command.get_args().collect::<Vec<_>>(),
                vec![
                    OsStr::new("-lc"),
                    OsStr::new(&format!("exec 2>&1\n{command_text}")),
                ]
            );
            assert_eq!(launch.command.get_current_dir(), Some(project.path()));
            let environment = launch
                .command
                .get_envs()
                .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
                .collect::<BTreeMap<_, _>>();
            let expected_environment = ["TMPDIR", "TMP", "TEMP", "TEMPDIR"]
                .map(|key| {
                    (
                        OsString::from(key),
                        Some(temporary.path().as_os_str().to_owned()),
                    )
                })
                .into_iter()
                .collect::<BTreeMap<_, _>>();
            assert_eq!(environment, expected_environment);
            assert!(launch.credential_lease.is_none());
            assert_eq!(reads.load(Ordering::SeqCst), 0);
            assert_eq!(
                std::fs::read_dir(temporary.path())
                    .expect("temp entries")
                    .count(),
                0
            );
            if binding_count.is_some() {
                assert!(tools.check_credential_path(&protected_root).is_err());
                assert!(tools.credential_policy.is_some());
            }
            drop(launch);
            temporary.cleanup().expect("command temp cleanup");
            project.cleanup().expect("project cleanup");
        }
    }
}

#[tokio::test]
async fn launch_preparation_rejects_wrong_instances_roots_and_cancellation_before_side_effects() {
    let project = TempRoot::create().expect("project fixture");
    let other_project = TempRoot::create().expect("other project fixture");
    let temporary = TempRoot::create().expect("command temp fixture");
    let tools = Tools::new(project.path()).expect("tools");
    let other_instance = Tools::new(project.path()).expect("separate instance");
    let other_root = Tools::new(other_project.path()).expect("separate root");
    let prepared = tools
        .prepare_bash_json(r#"{"command":"pwd","timeout":null}"#)
        .expect("prepared");
    for policy in [
        BashLaunchPolicy::ReadOnly,
        BashLaunchPolicy::WorkspaceWrite,
        BashLaunchPolicy::FullAccess,
    ] {
        for wrong_tools in [&other_instance, &other_root] {
            let failure = match wrong_tools
                .prepare_bash_launch(&prepared, policy, &CancellationToken::new(), &temporary)
                .await
            {
                Err(failure) => failure,
                Ok(_) => panic!("wrong execution identity was accepted"),
            };
            assert_eq!(failure.error.code(), BashErrorCode::ScopeMismatch);
            assert!(failure.cleanup_safe);
        }
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let failure = match tools
            .prepare_bash_launch(&prepared, policy, &cancelled, &temporary)
            .await
        {
            Err(failure) => failure,
            Ok(_) => panic!("cancelled launch was accepted"),
        };
        assert_eq!(failure.error.code(), BashErrorCode::Cancelled);
        assert!(failure.cleanup_safe);
    }
    assert_eq!(
        std::fs::read_dir(temporary.path())
            .expect("temp entries")
            .count(),
        0
    );
    temporary.cleanup().expect("command temp cleanup");
    other_project.cleanup().expect("other project cleanup");
    project.cleanup().expect("project cleanup");
}

#[test]
fn provider_input_cannot_select_a_launch_policy_or_skip_approval() {
    for field in ["policy", "full_access", "profile", "approved"] {
        let mut input = serde_json::json!({"command": "pwd", "timeout": null});
        input[field] = serde_json::json!("full_access");
        assert_eq!(
            bash_input_error(&input.to_string()),
            Some(BashInputError::UnexpectedField)
        );
    }
    let exact = "  printf '%s' 'preserve bytes'\n";
    assert_eq!(
        parse_bash_input(&serde_json::json!({"command": exact, "timeout": 3}).to_string()),
        Ok((exact.into(), 3_000))
    );
}

#[tokio::test]
async fn reaped_shell_output_requires_eof_instead_of_treating_stopped_reads_as_success() {
    let (mut writer, output) = tokio::io::duplex(128);
    writer.write_all(b"complete output\n").await.expect("write");
    writer.shutdown().await.expect("EOF");
    let stop = CancellationToken::new();
    let mut reader = tokio::spawn(read_output(output, tokio::io::empty(), stop.clone()));
    let result = finish_reader(&mut reader, &stop)
        .await
        .expect("complete output");
    assert_eq!(result.text, "complete output");
    assert!(!result.truncated);
    assert!(!stop.is_cancelled());
    assert!(reader.is_finished());

    let (mut writer, output) = tokio::io::duplex(128);
    writer.write_all(b"partial output\n").await.expect("write");
    let stop = CancellationToken::new();
    let mut reader = tokio::spawn(read_output(output, tokio::io::empty(), stop.clone()));
    let error = match finish_reader(&mut reader, &stop).await {
        Err(error) => error,
        Ok(_) => panic!("open pipe reported complete output"),
    };
    assert_eq!(error.code(), BashErrorCode::OutputIncomplete);
    assert!(
        error
            .to_string()
            .contains("Background work may still be running")
    );
    assert!(stop.is_cancelled());
    assert!(reader.is_finished());
    assert!(writer.write_all(b"after completion").await.is_err());
}

struct FailedRead;

impl AsyncRead for FailedRead {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Err(std::io::Error::other("synthetic read failure")))
    }
}

#[tokio::test]
async fn output_io_failure_and_reader_abort_remain_failures() {
    let stop = CancellationToken::new();
    let mut reader = tokio::spawn(read_output(FailedRead, tokio::io::empty(), stop.clone()));
    let error = match finish_reader(&mut reader, &stop).await {
        Err(error) => error,
        Ok(_) => panic!("I/O failure reported complete output"),
    };
    assert_eq!(error.code(), BashErrorCode::OutputFailed);
    assert!(reader.is_finished());

    let mut reader = tokio::spawn(std::future::pending());
    reader.abort();
    let error = match finish_reader(&mut reader, &stop).await {
        Err(error) => error,
        Ok(_) => panic!("aborted reader reported complete output"),
    };
    assert_eq!(error.code(), BashErrorCode::OutputFailed);
    assert!(reader.is_finished());
}
