use super::*;

use gpui_kit::{TestAppContext, WindowHandle};
use vega_conversation::types::{
    ContextAccountingRecord, ContextAccountingSource, ContextAccountingStage, TaskBackend,
    ThreadMode, ThreadStatus,
};

fn setup(cx: &mut TestAppContext) -> WindowHandle<ConversationStream> {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                ConversationStream::new(
                    Thread {
                        parent_thread_id: None,
                        id: "context-thread".into(),
                        backend: TaskBackend::Native,
                        project_id: "project".into(),
                        title: "Context".into(),
                        model: "mock".into(),
                        mode: ThreadMode::Execute,
                        permission_mode: PermissionMode::Confirm,
                        status: ThreadStatus::Active,
                        pinned: false,
                        unread: false,
                        created_at: 1,
                        updated_at: 1,
                    },
                    cx,
                )
            })
        })
        .expect("window")
    })
}

fn accounting(predicted_input: u64, input_budget: u64, revision: u64) -> ContextAccountingRecord {
    ContextAccountingRecord {
        source: ContextAccountingSource::Estimated,
        stage: ContextAccountingStage::PrimaryPreflight,
        provider_input_baseline: None,
        incremental_estimate: predicted_input,
        predicted_input,
        input_budget,
        trigger_tokens: 360_000,
        target_tokens: 180_000,
        revision,
        covered_messages: 2,
    }
}

/// C490-02: the runtime's real input budget becomes the ring denominator during
/// the run instead of waiting for a post-run projection refresh.
#[gpui_kit::test]
async fn issue490_accounting_supplies_runtime_input_budget_as_denominator(cx: &mut TestAppContext) {
    let window = setup(cx);
    window.update(cx, |stream, _, cx| {
        stream.begin_composer_run(cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run-1".into(),
                seq: 1,
            },
            cx,
        );
        assert!(stream.apply_context_accounting(
            "context-thread",
            "mock",
            "run-1",
            accounting(120_000, 400_000, 1),
            cx,
        ));
        assert_eq!(
            stream.context_usage_source(),
            (Some(120_000), Some(400_000))
        );
    });
}

/// C490-05: a projection refresh during the run updates the denominator but must
/// not overwrite the live accounting numerator.
#[gpui_kit::test]
async fn issue490_projection_limit_is_kept_while_accounting_owns_numerator(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window.update(cx, |stream, _, cx| {
        stream.begin_composer_run(cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run-1".into(),
                seq: 1,
            },
            cx,
        );
        assert!(stream.apply_context_accounting(
            "context-thread",
            "mock",
            "run-1",
            accounting(120_000, 400_000, 1),
            cx,
        ));
        assert!(stream.apply_context_projection(
            "context-thread",
            "mock",
            None,
            ContextUsageProjection::new(Some(500_000), Some(999_000)),
            false,
            cx,
        ));
        assert_eq!(
            stream.context_usage_source(),
            (Some(120_000), Some(500_000))
        );
    });
}

/// C490-04: with no accounting and no projection the ring has no denominator and
/// no numerator; nothing is invented.
#[gpui_kit::test]
async fn issue490_without_accounting_no_denominator_is_invented(cx: &mut TestAppContext) {
    let window = setup(cx);
    window.update(cx, |stream, _, _cx| {
        assert_eq!(stream.context_usage_source(), (None, None));
    });
}

/// C490-03: outside a run the projection is the denominator source.
#[gpui_kit::test]
async fn issue490_projection_limit_applies_when_no_run_is_active(cx: &mut TestAppContext) {
    let window = setup(cx);
    window.update(cx, |stream, _, cx| {
        assert!(stream.apply_context_projection(
            "context-thread",
            "mock",
            None,
            ContextUsageProjection::new(Some(500_000), Some(5_000)),
            false,
            cx,
        ));
        assert_eq!(stream.context_usage_source(), (Some(5_000), Some(500_000)));
    });
}

/// C490-07: switching the thread model clears both values until the new model's
/// projection arrives.
#[gpui_kit::test]
async fn issue490_model_change_clears_usage(cx: &mut TestAppContext) {
    let window = setup(cx);
    window.update(cx, |stream, _, cx| {
        assert!(stream.apply_context_projection(
            "context-thread",
            "mock",
            None,
            ContextUsageProjection::new(Some(500_000), Some(5_000)),
            false,
            cx,
        ));
        let mut thread = stream.thread.clone();
        thread.model = "other".into();
        stream.apply_thread(thread, cx);
        assert_eq!(stream.context_usage_source(), (None, None));
    });
}
