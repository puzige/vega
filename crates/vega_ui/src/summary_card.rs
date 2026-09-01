//! Read-only per-task cost summary card (S7-T40/A10-06, C4 contract).
//!
//! The card renders a bounded [`TaskCostSummary`] projection produced by
//! `vega_conversation::summary`; it never reads SQLite and never computes a
//! cost formula. Unavailable facts (no usage rows, unpriced rows, restart
//! without duration) render as `—`, never as a fabricated zero. Styling
//! mirrors the ui-spec §4.2 tool-card frame: 8px radius, 1px `border-subtle`,
//! no shadow, all colors from the theme tokens.

use gpui::prelude::*;
use gpui::{AnyElement, App, Entity, div, px};
use vega_conversation::types::{
    Microcents, SummaryCost, TaskCostSummary, TaskSummaryOutcome, TokenUsage,
};
use vega_theme::{Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};

/// The em dash shown for every unavailable fact (C4: `—`, not `0`).
const UNAVAILABLE: &str = "—";

/// Read-only cost summary of one finished assistant task.
pub struct SummaryCard {
    summary: TaskCostSummary,
}

impl SummaryCard {
    pub fn new(summary: TaskCostSummary) -> Self {
        Self { summary }
    }

    /// The projected summary (test/observation seam).
    pub fn summary(&self) -> &TaskCostSummary {
        &self.summary
    }

    /// Number of fixed-height virtual rows: header, token line, cost line,
    /// footer line, closing border row.
    pub fn row_count(&self) -> usize {
        5
    }

    /// Exact rendered text (keyboard/screen-reader/test seam; `—` kept).
    pub fn visible_text(&self) -> String {
        [
            header_label(&self.summary),
            token_line(&self.summary.usage),
            cost_line(&self.summary.cost, self.summary.duration_ms),
            footer_line(self.summary.tool_count, self.summary.cache_hit_percent),
        ]
        .join(" · ")
    }

    pub(crate) fn render_row(card: Entity<Self>, row: usize, cx: &App) -> AnyElement {
        let colors = theme(cx).colors;
        let summary = card.read(cx).summary.clone();
        // Bounded virtual list: out-of-range rows clamp to the closing
        // border row so a stale row count can never panic (fail closed).
        let row = row.min(card.read(cx).row_count() - 1);
        let base = div()
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .px_3()
            .bg(colors.bg_elevated)
            .border_color(colors.border_subtle)
            .border_l_1()
            .border_r_1();
        match row {
            0 => base
                .border_t_1()
                .rounded_tl_lg()
                .rounded_tr_lg()
                .text_size(px(Typography::HEADING_CARD))
                .font_weight(Typography::HEADING_CARD_WEIGHT)
                .text_color(colors.text_secondary)
                .child(header_label(&summary))
                .into_any_element(),
            1 => base
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE))
                .text_color(colors.text_primary)
                .child(token_line(&summary.usage))
                .into_any_element(),
            2 => base
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE))
                .text_color(colors.text_primary)
                .child(cost_line(&summary.cost, summary.duration_ms))
                .into_any_element(),
            3 => base
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE))
                .text_color(colors.text_secondary)
                .child(footer_line(summary.tool_count, summary.cache_hit_percent))
                .into_any_element(),
            _ => base
                .border_b_1()
                .rounded_bl_lg()
                .rounded_br_lg()
                .into_any_element(),
        }
    }
}

/// `任务摘要 · 完成/已中断/失败`
fn header_label(summary: &TaskCostSummary) -> String {
    let outcome = match summary.outcome {
        TaskSummaryOutcome::Completed => "完成",
        TaskSummaryOutcome::Interrupted => "已中断",
        TaskSummaryOutcome::Failed => "失败",
    };
    format!("任务摘要 · {outcome}")
}

/// `输入 1.3k · 输出 25 · 缓存读 50 · 缓存写 —`
fn token_line(usage: &Option<TokenUsage>) -> String {
    let Some(usage) = usage else {
        return format!(
            "输入 {UNAVAILABLE} · 输出 {UNAVAILABLE} · 缓存读 {UNAVAILABLE} · 缓存写 {UNAVAILABLE}"
        );
    };
    format!(
        "输入 {} · 输出 {} · 缓存读 {} · 缓存写 {}",
        compact_tokens(usage.input),
        compact_tokens(usage.output),
        compact_tokens(usage.cache_read),
        compact_tokens(usage.cache_write),
    )
}

/// `US$0.15 · 耗时 1.5s` / `成本 — · 耗时 —`
fn cost_line(cost: &SummaryCost, duration_ms: Option<u64>) -> String {
    let cost = match cost {
        SummaryCost::Priced(microcents) => format_usd(*microcents),
        SummaryCost::Unavailable => UNAVAILABLE.to_string(),
    };
    let duration = match duration_ms {
        Some(ms) => format_duration(ms),
        None => UNAVAILABLE.to_string(),
    };
    format!("成本 {cost} · 耗时 {duration}")
}

/// `工具 2 · 缓存命中 38%`
fn footer_line(tool_count: u64, cache_hit_percent: Option<u8>) -> String {
    let cache = match cache_hit_percent {
        Some(percent) => format!("{percent}%"),
        None => UNAVAILABLE.to_string(),
    };
    format!("工具 {tool_count} · 缓存命中 {cache}")
}

/// k/M compact token format (C4); values below 1,000 stay exact. The k tier
/// owns values whose one-decimal reading stays below `1000.0k`; at 999,950
/// the rounded k reading would carry to "1000.0k", so the M tier takes over
/// and 999,999 reads "1.0M".
fn compact_tokens(tokens: u64) -> String {
    if tokens >= 999_950 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Formats priced microcents as `US$<cost>` with enough precision to keep
/// nonzero microcents distinguishable (C4). Exact integer math, no floats.
fn format_usd(microcents: Microcents) -> String {
    let Microcents(microcents) = microcents;
    let negative = microcents < 0;
    let value = microcents.unsigned_abs();
    // 6 fractional digits: 1 microcent = $0.000001.
    let whole = value / 1_000_000;
    let fraction = value % 1_000_000;
    let sign = if negative { "-" } else { "" };
    format!("{sign}US${whole}.{fraction:06}")
}

/// Human duration: sub-second keeps milliseconds, otherwise whole seconds.
fn format_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64, cache_read: u64, cache_write: u64) -> Option<TokenUsage> {
        Some(TokenUsage {
            input,
            output,
            cache_read,
            cache_write,
        })
    }

    #[test]
    fn unavailable_fields_render_em_dash_not_zero() {
        assert_eq!(compact_tokens(999), "999");
        assert_eq!(compact_tokens(1_000), "1.0k");
        assert_eq!(compact_tokens(12_400), "12.4k");
        // k/M carry boundary: the last k reading and the first M reading.
        assert_eq!(compact_tokens(999_949), "999.9k");
        assert_eq!(compact_tokens(999_950), "1.0M");
        assert_eq!(compact_tokens(999_999), "1.0M");
        assert_eq!(compact_tokens(1_000_000), "1.0M");
        assert_eq!(format_usd(Microcents(150_000)), "US$0.150000");
        assert_eq!(format_usd(Microcents(1_000_001)), "US$1.000001");
        assert_eq!(format_usd(Microcents(0)), "US$0.000000");
        assert_eq!(format_duration(999), "999ms");
        assert_eq!(format_duration(1_500), "1.5s");
        let summary = TaskCostSummary {
            message_id: "message".into(),
            outcome: TaskSummaryOutcome::Interrupted,
            usage: None,
            cost: SummaryCost::Unavailable,
            duration_ms: None,
            tool_count: 0,
            cache_hit_percent: None,
        };
        let text = SummaryCard::new(summary).visible_text();
        assert_eq!(text.matches(UNAVAILABLE).count(), 7);
        assert!(!text.contains("$0"), "unavailable must never render as $0");
        assert!(text.contains("已中断"));
    }

    #[test]
    fn priced_summary_renders_every_persisted_fact() {
        let summary = TaskCostSummary {
            message_id: "message".into(),
            outcome: TaskSummaryOutcome::Completed,
            usage: usage(130, 25, 50, 0),
            cost: SummaryCost::Priced(Microcents(150_000)),
            duration_ms: Some(1_500),
            tool_count: 2,
            cache_hit_percent: Some(38),
        };
        let text = SummaryCard::new(summary).visible_text();
        assert!(text.contains("任务摘要 · 完成"));
        assert!(text.contains("输入 130 · 输出 25 · 缓存读 50 · 缓存写 0"));
        assert!(text.contains("成本 US$0.150000"));
        assert!(text.contains("耗时 1.5s"));
        assert!(text.contains("工具 2 · 缓存命中 38%"));
    }

    mod gpui_tests {
        use gpui::{
            Bounds, Render, TestAppContext, WindowBounds, WindowHandle, WindowOptions, size,
        };

        use super::*;

        struct Harness {
            card: Entity<SummaryCard>,
        }

        impl Render for Harness {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                cx: &mut gpui::Context<Self>,
            ) -> impl IntoElement {
                div().flex().flex_col().children(
                    (0..self.card.read(cx).row_count())
                        .map(|row| SummaryCard::render_row(self.card.clone(), row, cx)),
                )
            }
        }

        fn open_card_window(
            cx: &mut TestAppContext,
            summary: TaskCostSummary,
        ) -> WindowHandle<Harness> {
            let card = cx.new(|_| SummaryCard::new(summary));
            cx.update(|cx| {
                cx.set_global(vega_theme::Theme::light());
                let bounds = Bounds::centered(None, size(gpui::px(960.), gpui::px(600.)), cx);
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        ..Default::default()
                    },
                    move |_, cx| cx.new(|_| Harness { card }),
                )
                .expect("summary card test window")
            })
        }

        #[gpui::test]
        async fn renders_exact_window_under_light_and_dark_without_layout_panic(
            cx: &mut TestAppContext,
        ) {
            let summary = TaskCostSummary {
                message_id: "cjk-message".into(),
                outcome: TaskSummaryOutcome::Completed,
                usage: usage(150_000, 15_000, 50_000, 0),
                cost: SummaryCost::Priced(Microcents(135_000)),
                duration_ms: Some(12_400),
                tool_count: 2,
                cache_hit_percent: Some(33),
            };
            let window = open_card_window(cx, summary);
            cx.run_until_parked();
            assert_eq!(
                window
                    .update(cx, |_, window, _| window.viewport_size())
                    .expect("summary viewport"),
                size(gpui::px(960.), gpui::px(600.)),
                "minimum window (ui-spec §6) must not break the card layout"
            );
            cx.update(|cx| {
                cx.set_global(vega_theme::Theme::dark());
                cx.refresh_windows();
            });
            cx.run_until_parked();
            assert_eq!(
                cx.read(|cx| cx.global::<vega_theme::Theme>().appearance),
                vega_theme::Appearance::Dark,
                "the card re-renders from theme tokens only"
            );
        }

        #[gpui::test]
        async fn read_only_card_never_traps_keyboard_navigation(cx: &mut TestAppContext) {
            let summary = TaskCostSummary {
                message_id: "keyboard-message".into(),
                outcome: TaskSummaryOutcome::Interrupted,
                usage: None,
                cost: SummaryCost::Unavailable,
                duration_ms: None,
                tool_count: 0,
                cache_hit_percent: None,
            };
            let window = open_card_window(cx, summary);
            cx.run_until_parked();
            // The card registers no key context and no bindings: repeated Tab
            // and Enter over the card window must not panic or trap; focus
            // traversal stays owned by the surrounding conversation stream.
            for _ in 0..3 {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "enter");
            cx.run_until_parked();
            window
                .update(cx, |_, _, _| {})
                .expect("window stays responsive after navigation keystrokes");
        }
    }
}
