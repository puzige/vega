use super::*;
use std::os::unix::process::ExitStatusExt;

fn writer(redactor: BashSessionRedactor) -> SessionWriter {
    SessionWriter {
        inner: Arc::new(SessionInner {
            data: Mutex::new(SessionData {
                state: "running",
                terminal: false,
                started_at: Some(Instant::now()),
                duration_ms: 0,
                exit_code: None,
                error: None,
                cleanup_safe: false,
                command_result: None,
                exit_signal: None,
                output_incomplete: false,
                resources_closed: false,
                process_resolved: false,
                cleanup_reasons: Vec::new(),
                cleanup_deadline: None,
                output: OutputRing::new(),
                _retained_temp: None,
                _retained_child: None,
                retained_reader: None,
                retained_tracker: None,
                recovery_busy: false,
            }),
            cancel: CancellationToken::new(),
            launched: CancellationToken::new(),
            settled: CancellationToken::new(),
            redactor,
        }),
    }
}

fn passthrough() -> BashSessionRedactor {
    BashSessionRedactor::new(
        Arc::new(|text| Ok(text.into())),
        Arc::new(|text| Ok(text.len())),
        MAX_PENDING_BYTES,
    )
    .unwrap()
}

#[test]
fn never_started_failure_retains_terminal_owner_with_truthful_cleanup_facts() {
    let writer = writer(passthrough());
    writer.inner.data().started_at = None;
    writer.finish(
        BashAttempt {
            result: Err(BashError::new(BashErrorCode::SpawnFailed)),
            cleanup_safe: true,
        },
        None,
    );
    let snapshot = writer.inner.snapshot(None).unwrap();
    assert_eq!(snapshot.state, "failed");
    assert!(snapshot.cleanup_safe);
    assert!(snapshot.process_resolved);
    assert!(snapshot.resources_closed);
    assert_eq!(snapshot.exit_code, None);
    assert_eq!(snapshot.command_result, None);
}

#[test]
fn output_cursor_rejects_partial_characters_and_reports_eviction_without_corruption() {
    let mut ring = OutputRing::new();
    ring.append("汉字").unwrap();
    assert!(ring.read(Some(1)).is_err());
    assert!(ring.read(Some(7)).is_err());
    let (text, cursor, gap) = ring.read(None).unwrap();
    assert_eq!(text, "汉字");
    assert_eq!(cursor, 6);
    assert!(!gap);
    ring.append(&"汉".repeat(RING_BYTES / 3 + 4)).unwrap();
    let (text, cursor, gap) = ring.read(Some(0)).unwrap();
    assert!(gap);
    assert!(ring.truncated);
    assert!(text.len() <= SNAPSHOT_BYTES);
    assert!(text.chars().all(|ch| ch == '汉'));
    assert_eq!((cursor - ring.retained_from) % 3, 0);
    assert_eq!(
        ring.read(Some(ring.end)).unwrap(),
        (String::new(), ring.end, false)
    );
}

#[test]
fn split_secret_and_temporary_path_are_redacted_before_any_observable_bytes() {
    let writer = writer(BashSessionRedactor::buffered(Arc::new(|text| {
        Ok(text.replace("secret", "[REDACTED]"))
    })));
    let mut collector = OutputCollector::new(Some(writer.clone()), "/private/tmp/session".into());
    collector.push(0, b"sec", false).unwrap();
    assert!(writer.inner.snapshot(None).unwrap().output.is_empty());
    collector
        .push(1, b"ret /private/tmp/session/file", false)
        .unwrap();
    collector.push(0, b"", true).unwrap();
    assert!(writer.inner.snapshot(None).unwrap().output.is_empty());
    collector.push(1, b"", true).unwrap();
    assert!(!collector.finish().unwrap().truncated);
    let output = writer.inner.snapshot(None).unwrap().output;
    assert!(output.contains("[REDACTED]"));
    assert!(output.contains("[VEGA_TEMP]/file"));
    assert!(!output.contains("secret"));
    assert!(!output.contains("/private/tmp/session"));
}

#[test]
fn failed_redaction_publishes_no_secret_and_is_not_normal_output_completion() {
    let writer = writer(
        BashSessionRedactor::new(
            Arc::new(|_| Err("redaction unavailable".into())),
            Arc::new(|text| Ok(text.len())),
            MAX_PENDING_BYTES,
        )
        .unwrap(),
    );
    let mut collector = OutputCollector::new(Some(writer.clone()), "/temporary".into());
    collector.push(0, b"sensitive content", false).unwrap();
    assert!(collector.finish().is_err());
    assert!(writer.inner.snapshot(None).unwrap().output.is_empty());
}

#[test]
fn bounded_output_suppression_is_explicit_truncation_not_a_reader_failure() {
    let writer = writer(passthrough());
    let mut collector = OutputCollector::new(Some(writer.clone()), "/temporary".into());
    collector
        .push(0, &vec![b'x'; MAX_PENDING_BYTES + 1], false)
        .unwrap();
    collector.push(0, b"", true).unwrap();
    collector.push(1, b"", true).unwrap();
    assert!(collector.finish().unwrap().truncated);
    let snapshot = writer.inner.snapshot(None).unwrap();
    assert!(snapshot.truncated);
    assert!(!snapshot.output_incomplete);
    assert!(snapshot.output.is_empty());
}

#[tokio::test]
async fn historical_output_loss_survives_cleanup_retry_with_known_exit_intact() {
    let writer = writer(passthrough());
    writer.exited(ExitStatus::from_raw(0));
    writer.begin_cleanup("exited");
    writer.process_cleanup(true, Vec::new());
    writer.reader_closed(true);
    writer.finish(
        BashAttempt {
            result: Err(BashError::new(BashErrorCode::OutputIncomplete)),
            cleanup_safe: false,
        },
        None,
    );
    let session = BashSession {
        owner: Arc::new(SessionOwner {
            inner: writer.inner.clone(),
        }),
    };
    let snapshot = session.retry_cleanup().await.unwrap();
    assert_eq!(snapshot.state, "outcome_unknown");
    assert_eq!(snapshot.command_result.as_deref(), Some("exited"));
    assert_eq!(snapshot.exit_code, Some(0));
    assert!(snapshot.process_resolved);
    assert!(snapshot.resources_closed);
    assert!(snapshot.output_incomplete);
    assert!(!snapshot.cleanup_safe);
}

#[tokio::test]
async fn pending_reader_remains_non_bypassable_until_actual_completion() {
    let writer = writer(passthrough());
    writer.exited(ExitStatus::from_raw(0));
    writer.begin_cleanup("exited");
    writer.process_cleanup(true, Vec::new());
    writer.reader_closed(true);
    let close = CancellationToken::new();
    let stop = close.clone();
    writer.retain_reader(tokio::spawn(async move {
        stop.cancelled().await;
        Ok(CollectedBashOutput {
            text: String::new(),
            truncated: false,
        })
    }));
    writer.finish(
        BashAttempt {
            result: Err(BashError::new(BashErrorCode::OutputIncomplete)),
            cleanup_safe: false,
        },
        None,
    );
    assert!(!writer.inner.snapshot(None).unwrap().resources_closed);
    close.cancel();
    let session = BashSession {
        owner: Arc::new(SessionOwner {
            inner: writer.inner.clone(),
        }),
    };
    let snapshot = session.retry_cleanup().await.unwrap();
    assert!(snapshot.resources_closed);
    assert!(snapshot.output_incomplete);
    assert!(!snapshot.cleanup_safe);
    assert_eq!(snapshot.exit_code, Some(0));
}

#[tokio::test]
async fn dropped_cleanup_retry_returns_pending_reader_to_owner() {
    let writer = writer(passthrough());
    writer.exited(ExitStatus::from_raw(0));
    writer.begin_cleanup("exited");
    writer.process_cleanup(true, Vec::new());
    writer.reader_closed(true);
    let close = CancellationToken::new();
    let stop = close.clone();
    writer.retain_reader(tokio::spawn(async move {
        stop.cancelled().await;
        Ok(CollectedBashOutput {
            text: String::new(),
            truncated: false,
        })
    }));
    writer.finish(
        BashAttempt {
            result: Err(BashError::new(BashErrorCode::OutputIncomplete)),
            cleanup_safe: false,
        },
        None,
    );
    let session = BashSession {
        owner: Arc::new(SessionOwner {
            inner: writer.inner.clone(),
        }),
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(1), session.retry_cleanup())
            .await
            .is_err()
    );
    {
        let data = writer.inner.data();
        assert!(!data.recovery_busy);
        assert!(data.retained_reader.is_some());
        assert!(!data.resources_closed);
        assert!(!data.cleanup_safe);
    }
    close.cancel();
    let snapshot = session.retry_cleanup().await.unwrap();
    assert!(snapshot.resources_closed);
    assert!(snapshot.output_incomplete);
    assert_eq!(snapshot.exit_code, Some(0));
}

#[test]
fn resumed_workspace_invalidates_shared_old_reads_even_when_file_bytes_match() {
    let root = std::env::temp_dir().join(format!("vega-read-epoch-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let path = root.join("file");
    let reads = crate::ReadState::default();
    let shared = reads.clone();
    reads.record(&path, b"unchanged").unwrap();
    assert!(shared.validate(&path, b"unchanged").is_ok());
    let in_flight_epoch = crate::ReadState::observation_epoch(&path).unwrap();
    crate::ReadState::invalidate_workspace(&root).unwrap();
    assert!(shared.validate(&path, b"unchanged").is_err());
    reads
        .record_observation(&path, b"unchanged", in_flight_epoch)
        .unwrap();
    assert!(shared.validate(&path, b"unchanged").is_err());
    reads.record(&path, b"unchanged").unwrap();
    assert!(shared.validate(&path, b"unchanged").is_ok());
    std::fs::remove_dir(root).unwrap();
}
