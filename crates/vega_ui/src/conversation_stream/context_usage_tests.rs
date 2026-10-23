use super::{ContextUsageProjection, ConversationStream, RunningMessageDefault};
use crate::settings::SettingsOpen;
use gpui_kit::{Context, TestAppContext, WindowHandle, px, size};
use vega_conversation::types::{
    ContextAccountingRecord, ContextAccountingSource, ContextAccountingStage,
    ContextCompactionStatus, ContextCompactionStatusRecord, ContextCompactionUsageState,
    ConversationEvent, ConversationStopReason, Microcents, PermissionMode, TaskBackend, Thread,
    ThreadMode, ThreadStatus, TokenUsage,
};
use vega_theme::Theme;

const THREAD: &str = "context-thread";
const MODEL: &str = "context-model";
const MESSAGE: &str = "primary-message";

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: THREAD.into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Context usage".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::Confirm,
        model: MODEL.into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    }
}

fn open_stream(cx: &mut TestAppContext) -> WindowHandle<ConversationStream> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        cx.set_global(RunningMessageDefault::default());
        crate::init(cx);
    });
    cx.open_window(size(px(1000.0), px(700.0)), |_, cx| {
        ConversationStream::new(thread(), cx)
    })
}

fn start_run(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.begin_composer_run(cx);
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: MESSAGE.into(),
            seq: 1,
        },
        cx,
    );
}

fn response(stream: &mut ConversationStream, id: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::AssistantResponseStarted {
            message_id: MESSAGE.into(),
            response_id: id.into(),
            text_start_bytes: 0,
        },
        cx,
    );
}

fn accounting(
    revision: u64,
    stage: ContextAccountingStage,
    predicted_input: u64,
) -> ContextAccountingRecord {
    ContextAccountingRecord {
        source: ContextAccountingSource::Estimated,
        stage,
        provider_input_baseline: None,
        incremental_estimate: predicted_input,
        predicted_input,
        input_budget: 400_000,
        trigger_tokens: 360_000,
        target_tokens: 200_000,
        revision,
        covered_messages: 0,
    }
}

fn preflight(
    stream: &mut ConversationStream,
    revision: u64,
    estimate: u64,
    cx: &mut Context<ConversationStream>,
) {
    assert!(stream.apply_context_accounting(
        THREAD,
        MODEL,
        MESSAGE,
        accounting(revision, ContextAccountingStage::PrimaryPreflight, estimate),
        cx,
    ));
}

fn usage(input: u64, output: u64) -> TokenUsage {
    TokenUsage {
        input,
        output,
        cache_read: None,
        cache_write: None,
    }
}

fn usage_event(usage: TokenUsage) -> ConversationEvent {
    ConversationEvent::UsageUpdated {
        message_id: MESSAGE.into(),
        usage,
        cost: Microcents(0),
        pricing: None,
    }
}

fn primary_usage(
    stream: &mut ConversationStream,
    usage: TokenUsage,
    cx: &mut Context<ConversationStream>,
) {
    assert!(stream.apply_context_usage(THREAD, MODEL, MESSAGE, usage, cx));
    stream.apply_event(usage_event(usage), cx);
}

fn projection(
    stream: &mut ConversationStream,
    limit: Option<u64>,
    estimate: Option<u64>,
    cx: &mut Context<ConversationStream>,
) {
    assert!(stream.apply_context_projection(
        THREAD,
        MODEL,
        None,
        ContextUsageProjection::new(limit, estimate),
        true,
        cx,
    ));
}

#[gpui_kit::test]
fn first_primary_total_calibrates_both_directions_without_adding_cache(cx: &mut TestAppContext) {
    for estimate in [160_000, 100_000] {
        let window = open_stream(cx);
        window.update(cx, |stream, _, cx| {
            start_run(stream, cx);
            preflight(stream, 0, estimate, cx);
            assert_eq!(stream.context_usage_source(), (Some(estimate), Some(400_000)));
            assert_eq!(stream.context_usage_display().usage_label(), "估算已用 / 配置输入上限");
            response(stream, "first", cx);
            primary_usage(stream, TokenUsage {
                input: 120_000,
                output: 30_000,
                cache_read: Some(80_000),
                cache_write: Some(10_000),
            }, cx);
            assert_eq!(stream.context_usage_source(), (Some(150_000), Some(400_000)));
            let display = stream.context_usage_display();
            assert_eq!(display.compact_usage_label(), "150k / 400k tokens");
            assert_eq!(display.usage_label(), "最近调用总量 / 配置输入上限");
            assert_eq!(display.estimate_label(), "最近调用总量：150,000 tokens（输入＋输出）");
            assert_eq!(display.percentage_value().as_deref(), Some("37.5%"));
            assert_eq!(display.fill_fraction, 0.375);
            assert_eq!(display.accessibility_label(), "上下文用量；最近调用总量：150,000 tokens（输入＋输出）；最近调用使用量：37.5%；当前模型设置输入上限：400,000 tokens（非供应商验证容量）");
        }).expect("update primary calibration window");
    }
}

#[gpui_kit::test]
fn duplicate_usage_keeps_only_the_current_response_high_water_mark(cx: &mut TestAppContext) {
    let window = open_stream(cx);
    window
        .update(cx, |stream, _, cx| {
            start_run(stream, cx);
            preflight(stream, 0, 160_000, cx);
            response(stream, "first", cx);
            primary_usage(stream, usage(120_000, 30_000), cx);
            primary_usage(stream, usage(120_000, 30_000), cx);
            response(stream, "first", cx);
            primary_usage(stream, usage(80_000, 20_000), cx);
            assert_eq!(
                stream.context_usage_source(),
                (Some(150_000), Some(400_000))
            );
            primary_usage(stream, usage(130_000, 30_000), cx);
            assert_eq!(
                stream.context_usage_source(),
                (Some(160_000), Some(400_000))
            );
            preflight(stream, 1, 180_000, cx);
            assert_eq!(
                stream.context_usage_source(),
                (Some(180_000), Some(400_000))
            );
            assert_eq!(
                stream.context_usage_display().usage_label(),
                "估算已用 / 配置输入上限"
            );
            response(stream, "second", cx);
            primary_usage(stream, usage(80_000, 20_000), cx);
            assert_eq!(
                stream.context_usage_source(),
                (Some(100_000), Some(400_000))
            );
            assert_eq!(
                stream.context_usage_display().percentage_value().as_deref(),
                Some("25%")
            );
        })
        .expect("update response ownership window");
}

#[gpui_kit::test]
fn real_usage_survives_stale_estimates_until_a_new_accounting_boundary(cx: &mut TestAppContext) {
    let window = open_stream(cx);
    window
        .update(cx, |stream, _, cx| {
            start_run(stream, cx);
            preflight(stream, 2, 160_000, cx);
            response(stream, "first", cx);
            primary_usage(stream, usage(120_000, 30_000), cx);
            preflight(stream, 2, 190_000, cx);
            assert!(!stream.apply_context_accounting(
                THREAD,
                MODEL,
                MESSAGE,
                accounting(1, ContextAccountingStage::PrimaryPreflight, 210_000),
                cx
            ));
            projection(stream, Some(500_000), Some(220_000), cx);
            assert!(stream.apply_context_status(
                THREAD,
                MODEL,
                ContextCompactionStatusRecord {
                    generation: 1,
                    status: ContextCompactionStatus::Ready,
                    updated_at: 1,
                    estimated_tokens: Some(230_000),
                    input_budget: Some(500_000),
                    target_tokens: None,
                    source_version: None,
                    failure: None,
                    usage: ContextCompactionUsageState::Unknown,
                },
                cx
            ));
            assert_eq!(
                stream.context_usage_source(),
                (Some(150_000), Some(500_000))
            );
            assert_eq!(
                stream.context_usage_display().usage_label(),
                "最近调用总量 / 配置输入上限"
            );
            preflight(stream, 3, 170_000, cx);
            assert_eq!(
                stream.context_usage_source(),
                (Some(170_000), Some(400_000))
            );
            assert_eq!(
                stream.context_usage_display().usage_label(),
                "估算已用 / 配置输入上限"
            );
            assert!(!stream.apply_context_usage(
                THREAD,
                MODEL,
                MESSAGE,
                usage(120_000, 30_000),
                cx
            ));
            response(stream, "second", cx);
            primary_usage(stream, usage(80_000, 20_000), cx);
            assert_eq!(
                stream.context_usage_source(),
                (Some(100_000), Some(400_000))
            );
        })
        .expect("update accounting precedence window");
}

#[gpui_kit::test]
fn summary_and_subagent_usage_do_not_replace_the_primary_compaction_target(
    cx: &mut TestAppContext,
) {
    let window = open_stream(cx);
    window
        .update(cx, |stream, _, cx| {
            start_run(stream, cx);
            preflight(stream, 0, 160_000, cx);
            response(stream, "first", cx);
            primary_usage(stream, usage(120_000, 30_000), cx);
            stream.apply_event(
                ConversationEvent::ContextCompactionUsageUpdated {
                    usage: usage(300_000, 40_000),
                    cost: None,
                    pricing: None,
                },
                cx,
            );
            stream.apply_subagent_usage(&usage_event(usage(200_000, 30_000)), cx);
            assert_eq!(
                stream.context_usage_source(),
                (Some(150_000), Some(400_000))
            );
            preflight(stream, 1, 190_000, cx);
            assert!(stream.apply_context_accounting(
                THREAD,
                MODEL,
                MESSAGE,
                accounting(1, ContextAccountingStage::PostSummaryTarget, 40_000),
                cx
            ));
            assert_eq!(stream.context_usage_source(), (Some(40_000), Some(400_000)));
            assert_eq!(
                stream.context_usage_display().usage_label(),
                "估算已用 / 配置输入上限"
            );
            assert!(!stream.apply_context_accounting(
                THREAD,
                MODEL,
                MESSAGE,
                accounting(1, ContextAccountingStage::PrimaryPreflight, 190_000),
                cx
            ));
            assert!(!stream.apply_context_usage(
                THREAD,
                MODEL,
                MESSAGE,
                usage(300_000, 40_000),
                cx
            ));
            response(stream, "after-summary", cx);
            primary_usage(stream, usage(40_000, 10_000), cx);
            assert!(stream.apply_context_accounting(
                THREAD,
                MODEL,
                MESSAGE,
                accounting(1, ContextAccountingStage::PostSummaryTarget, 40_000),
                cx
            ));
            assert_eq!(stream.context_usage_source(), (Some(50_000), Some(400_000)));
            assert_eq!(
                stream.context_usage_display().estimate_label(),
                "最近调用总量：50,000 tokens（输入＋输出）"
            );
        })
        .expect("update compaction ownership window");
}

#[gpui_kit::test]
fn invalid_or_unowned_usage_cannot_change_the_context_display(cx: &mut TestAppContext) {
    let window = open_stream(cx);
    window
        .update(cx, |stream, _, cx| {
            assert!(!stream.apply_context_usage(
                THREAD,
                MODEL,
                MESSAGE,
                usage(120_000, 30_000),
                cx
            ));
            assert_eq!(stream.context_usage_source(), (None, None));
            start_run(stream, cx);
            preflight(stream, 0, 160_000, cx);
            assert!(!stream.apply_context_usage(
                THREAD,
                MODEL,
                MESSAGE,
                usage(120_000, 30_000),
                cx
            ));
            response(stream, "first", cx);
            primary_usage(stream, usage(120_000, 30_000), cx);
            let expected = stream.context_usage_display();
            for (thread, model, message, usage) in [
                ("other-thread", MODEL, MESSAGE, usage(200_000, 30_000)),
                (THREAD, "other-model", MESSAGE, usage(200_000, 30_000)),
                (THREAD, MODEL, "other-message", usage(200_000, 30_000)),
                (THREAD, MODEL, MESSAGE, usage(0, 300_000)),
                (THREAD, MODEL, MESSAGE, usage(u64::MAX, 1)),
            ] {
                assert!(!stream.apply_context_usage(thread, model, message, usage, cx));
                assert_eq!(stream.context_usage_display(), expected);
            }
            let mut changed = thread();
            changed.model = "replacement-model".into();
            stream.apply_thread(changed, cx);
            assert_eq!(stream.context_usage_source(), (None, None));
            assert!(!stream.apply_context_usage(
                THREAD,
                MODEL,
                MESSAGE,
                usage(200_000, 30_000),
                cx
            ));
            assert!(!stream.apply_context_usage(
                THREAD,
                "replacement-model",
                MESSAGE,
                usage(200_000, 30_000),
                cx
            ));
            assert_eq!(stream.context_usage_source(), (None, None));
        })
        .expect("update invalid usage window");
    let codex = open_stream(cx);
    codex
        .update(cx, |stream, _, cx| {
            let mut changed = thread();
            changed.backend = TaskBackend::Codex;
            stream.apply_thread(changed, cx);
            start_run(stream, cx);
            response(stream, "codex-response", cx);
            assert!(!stream.apply_context_usage(
                THREAD,
                MODEL,
                MESSAGE,
                usage(120_000, 30_000),
                cx
            ));
            assert_eq!(stream.context_usage_source(), (None, None));
        })
        .expect("update non-native usage window");
}

#[gpui_kit::test]
fn terminal_runs_keep_real_usage_but_new_runs_and_reload_use_fallbacks(cx: &mut TestAppContext) {
    for cancelled in [false, true] {
        let window = open_stream(cx);
        window
            .update(cx, |stream, _, cx| {
                start_run(stream, cx);
                preflight(stream, 0, 160_000, cx);
                response(stream, "first", cx);
                primary_usage(stream, usage(120_000, 30_000), cx);
                let terminal = if cancelled {
                    ConversationEvent::Interrupted {
                        message_id: MESSAGE.into(),
                        execution_duration_ms: None,
                    }
                } else {
                    ConversationEvent::MessageFinished {
                        message_id: MESSAGE.into(),
                        stop_reason: ConversationStopReason::End,
                        execution_duration_ms: None,
                    }
                };
                stream.apply_event(terminal, cx);
                assert_eq!(
                    stream.context_usage_source(),
                    (Some(150_000), Some(400_000))
                );
                assert!(!stream.apply_context_usage(
                    THREAD,
                    MODEL,
                    MESSAGE,
                    usage(200_000, 30_000),
                    cx
                ));
                assert_eq!(stream.finish_composer_run(cancelled, cx), cancelled);
                projection(stream, Some(400_000), Some(190_000), cx);
                assert_eq!(
                    stream.context_usage_source(),
                    (Some(150_000), Some(400_000))
                );
                assert_eq!(
                    stream.context_usage_display().usage_label(),
                    "最近调用总量 / 配置输入上限"
                );
                assert!(!stream.apply_context_usage(
                    THREAD,
                    MODEL,
                    MESSAGE,
                    usage(200_000, 30_000),
                    cx
                ));
                start_run(stream, cx);
                assert_eq!(stream.context_usage_source(), (None, Some(400_000)));
                assert_eq!(
                    stream.context_usage_display().estimate_label(),
                    "估算输入：尚无数据"
                );
                preflight(stream, 0, 90_000, cx);
                assert_eq!(stream.context_usage_source(), (Some(90_000), Some(400_000)));
                assert_eq!(
                    stream.context_usage_display().usage_label(),
                    "估算已用 / 配置输入上限"
                );
                stream.finish_composer_run(false, cx);
                projection(stream, Some(400_000), Some(80_000), cx);
                assert_eq!(stream.context_usage_source(), (Some(80_000), Some(400_000)));
            })
            .expect("update terminal usage window");
    }
    let reloaded = open_stream(cx);
    reloaded
        .update(cx, |stream, _, cx| {
            assert_eq!(stream.context_usage_source(), (None, None));
            assert_eq!(
                stream.context_usage_display().compact_usage_label(),
                "尚无数据"
            );
            projection(stream, None, Some(70_000), cx);
            assert_eq!(stream.context_usage_source(), (Some(70_000), None));
            assert_eq!(stream.context_usage_display().percentage_value(), None);
            assert_eq!(
                stream.context_usage_display().capacity_label(),
                "容量未配置"
            );
            assert_eq!(
                stream.context_usage_display().estimate_label(),
                "估算输入：70,000 tokens（含系统提示和实际工具定义）"
            );
            start_run(stream, cx);
            response(stream, "without-budget", cx);
            primary_usage(stream, usage(120_000, 30_000), cx);
            assert_eq!(stream.context_usage_source(), (Some(150_000), None));
            assert_eq!(
                stream.context_usage_display().compact_usage_label(),
                "150k tokens"
            );
            assert_eq!(stream.context_usage_display().percentage_value(), None);
            response(stream, "without-budget-second", cx);
            assert_eq!(stream.context_usage_source(), (Some(150_000), None));
            primary_usage(stream, usage(80_000, 20_000), cx);
            assert_eq!(stream.context_usage_source(), (Some(100_000), None));
            assert_eq!(
                stream.context_usage_display().estimate_label(),
                "最近调用总量：100,000 tokens（输入＋输出）"
            );
            assert_eq!(stream.context_usage_display().percentage_value(), None);
            assert_eq!(
                stream.context_usage_display().capacity_label(),
                "容量未配置"
            );
            stream.finish_composer_run(false, cx);
            projection(stream, None, None, cx);
            assert_eq!(stream.context_usage_source(), (Some(100_000), None));
        })
        .expect("update reloaded usage window");
}
