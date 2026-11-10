use super::*;
use crate::agent::diagnostics::DiagnosticsFlushError;

fn usage() -> ContextCompactionUsage {
    ContextCompactionUsage {
        usage: vega_runtime::RuntimeTokenUsage {
            input: 17,
            output: 5,
            cache_read: None,
            cache_write: None,
        },
        cost_microcents: 0,
        pricing: None,
    }
}

fn success() -> ContextCompactionResult {
    ContextCompactionResult {
        messages: Vec::new(),
        source_version: 7,
        source_fingerprint: None,
        replay_cold_prefix_messages: None,
        replay_interleaved_user_boundary: None,
        reactive_retained_groups: None,
        usages: vec![usage()],
        usage_complete: true,
        settled_usage: None,
    }
}

#[test]
fn manual_compaction_reports_flush_failure_after_success_without_losing_usage() {
    let store = vega_store::Store::open_existing(":memory:").unwrap();
    let sql_error = store.conn().execute_batch("not valid SQL").unwrap_err();
    let expected_error = sql_error.to_string();
    let error = match finish_manual_diagnostics(
        Ok(success()),
        Err(DiagnosticsFlushError::Diagnostic(sql_error.into())),
    ) {
        Err(error) => error,
        Ok(_) => panic!("diagnostics failure must not be reported as persisted"),
    };
    match error.error.as_ref() {
        VegaError::Store(actual) => assert_eq!(actual.to_string(), expected_error),
        other => panic!("expected original SQL error, got {other}"),
    }
    assert_eq!(error.usages, vec![usage()]);
    assert!(error.usage_complete);
}

#[test]
fn manual_compaction_preserves_original_failure_and_cancellation_when_flush_fails() {
    for original in [
        VegaError::Cancelled,
        context_error(ContextRuntimeError::SummaryEmpty),
    ] {
        let cancelled = matches!(original, VegaError::Cancelled);
        let original = ContextCompactionFailure::with_usages(original, vec![usage()], false);
        let original_address = original.error.as_ref() as *const VegaError;
        let error = match finish_manual_diagnostics(
            Err(original),
            Err(DiagnosticsFlushError::BufferUnavailable),
        ) {
            Err(error) => error,
            Ok(_) => panic!("original failure must survive diagnostics failure"),
        };
        assert_eq!(error.error.as_ref() as *const VegaError, original_address);
        assert_eq!(
            matches!(error.error.as_ref(), VegaError::Cancelled),
            cancelled
        );
        if !cancelled {
            assert!(matches!(
                error.error.as_ref(),
                VegaError::Context(ContextRuntimeError::SummaryEmpty)
            ));
        }
        assert_eq!(error.usages, vec![usage()]);
        assert!(!error.usage_complete);
    }
}

#[test]
fn manual_compaction_preserves_success_after_diagnostics_acknowledgement() {
    let result = match finish_manual_diagnostics(Ok(success()), Ok(())) {
        Ok(result) => result,
        Err(_) => panic!("successful flush must preserve compaction success"),
    };
    assert_eq!(result.source_version, 7);
    assert_eq!(result.usages, vec![usage()]);
    assert!(result.usage_complete);
}
