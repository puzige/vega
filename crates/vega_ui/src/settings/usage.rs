//! Read-only Settings usage dashboard, populated by the app-owned worker.
use super::*;
use gpui::{Bounds, PathBuilder, Rgba, canvas, fill, point, size};
use vega_conversation::types::{UsageDashboard, UsageDashboardError, UsageTotals};

/// Requests a fresh persisted-usage projection from the app worker.
pub struct UsageReloadRequested;
impl EventEmitter<UsageReloadRequested> for SettingsView {}

#[derive(Default)]
pub(crate) struct UsageState {
    dashboard: Option<UsageDashboard>,
    error: bool,
    loading: bool,
    heatmap_mode: usize,
    range_days: usize,
    selected_day: Option<usize>,
}

impl SettingsView {
    /// Marks an app-owned dashboard load pending, including the initial load.
    pub fn begin_usage_load(&mut self, cx: &mut Context<Self>) {
        self.usage.loading = true;
        cx.notify();
    }

    /// Applies a worker result without reading the database on the UI thread.
    pub fn apply_usage_dashboard(
        &mut self,
        result: Result<UsageDashboard, UsageDashboardError>,
        cx: &mut Context<Self>,
    ) {
        self.usage.loading = false;
        match result {
            Ok(dashboard) => {
                self.usage.dashboard = Some(dashboard);
                self.usage.error = false;
            }
            Err(_) => self.usage.error = true,
        }
        cx.notify();
    }

    pub(crate) fn request_usage_reload(&mut self, cx: &mut Context<Self>) {
        if !self.usage.loading {
            self.usage.loading = true;
            cx.emit(UsageReloadRequested);
            cx.notify();
        }
    }

    pub(crate) fn render_usage(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut page = div()
            .debug_selector(|| "usage-dashboard".into())
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(section_title("使用统计", colors.text_primary))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(colors.bg_hover)
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child("应用用量"),
                    ),
            );
        if self.usage.error {
            page = page.child(
                div()
                    .debug_selector(|| "usage-error".into())
                    .text_color(colors.danger)
                    .child("使用统计读取失败，请刷新重试"),
            );
        }
        if let Some(data) = self.usage.dashboard.clone() {
            if data.lifetime.calls == 0 {
                page = page.child(
                    div()
                        .debug_selector(|| "usage-empty".into())
                        .py_6()
                        .text_color(colors.text_secondary)
                        .child("暂无使用记录。完成对话后将在这里显示实际用量。"),
                );
            } else {
                page = page.child(self.render_usage_data(&data, cx));
            }
        } else if !self.usage.error {
            page = page.child(
                div()
                    .py_6()
                    .text_color(colors.text_secondary)
                    .child("正在加载使用统计…"),
            );
        }
        page.child(div().flex().justify_end().child(self.usage_button(
            "usage-refresh",
            if self.usage.loading {
                "刷新中…"
            } else {
                "刷新"
            },
            false,
            |this, cx| this.request_usage_reload(cx),
            cx,
        )))
        .into_any_element()
    }

    fn usage_button(
        &self,
        id: &'static str,
        label: &'static str,
        selected: bool,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let index = match id {
            "usage-daily" => 0,
            "usage-weekly" => 1,
            "usage-cumulative" => 2,
            "usage-7days" => 3,
            "usage-30days" => 4,
            _ => 5,
        };
        let action = std::rc::Rc::new(action);
        let keyboard_action = action.clone();
        div()
            .id(id)
            .track_focus(&self.usage_focuses[index])
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard_action(this, cx);
                    cx.stop_propagation();
                }
            }))
            .debug_selector(move || id.into())
            .tab_stop(true)
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(Typography::METADATA))
            .text_color(colors.text_secondary)
            .when(selected, |d| {
                d.bg(colors.bg_active).text_color(colors.text_primary)
            })
            .hover(|d| d.bg(colors.bg_hover))
            .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
            .child(label)
            .into_any_element()
    }

    fn render_usage_data(&self, data: &UsageDashboard, cx: &Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let range = if self.usage.range_days == 30 { 30 } else { 7 };
        let active = data.days.iter().filter(|day| day.totals.calls > 0).count();
        let peak = data
            .days
            .iter()
            .map(|day| day.totals.total_tokens)
            .max()
            .unwrap_or(0);
        let mut streak = 0;
        let mut longest = 0;
        for day in &data.days {
            streak = if day.totals.calls > 0 { streak + 1 } else { 0 };
            longest = longest.max(streak);
        }
        let metrics = [
            ("累计 Tokens", compact(data.lifetime.total_tokens)),
            ("累计估算费用", cost_label(&data.lifetime)),
            ("单日峰值 · 近365日", compact(peak)),
            ("活跃天数 · 近365日", active.to_string()),
            ("最长连续天数 · 近365日", longest.to_string()),
        ];
        let metric_band = div()
            .flex()
            .flex_wrap()
            .gap_4()
            .px_4()
            .py_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .bg(colors.bg_hover)
            .children(
                metrics
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, value))| {
                        div()
                            .flex_1()
                            .min_w(px(95.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .when(index > 0, |d| {
                                d.border_l_1().border_color(colors.border_subtle)
                            })
                            .child(
                                div()
                                    .text_size(px(Typography::HEADING_CARD))
                                    .text_color(colors.text_primary)
                                    .child(value),
                            )
                            .child(
                                div()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_secondary)
                                    .child(label),
                            )
                    }),
            );
        let heatmap = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .bg(colors.bg_hover)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().child("使用活动 · 近365日"))
                    .child(
                        div().flex().gap_1().children(
                            [
                                ("usage-daily", "每日"),
                                ("usage-weekly", "每周"),
                                ("usage-cumulative", "累计"),
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(mode, (id, label))| {
                                self.usage_button(
                                    id,
                                    label,
                                    self.usage.heatmap_mode == mode,
                                    move |this, cx| {
                                        this.usage.heatmap_mode = mode;
                                        cx.notify();
                                    },
                                    cx,
                                )
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(
                        div()
                            .debug_selector(|| "usage-activity-start-date".into())
                            .child(
                                data.days
                                    .first()
                                    .map(|day| utc_date(day.start_ms))
                                    .unwrap_or_default(),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "usage-activity-end-date".into())
                            .child(
                                data.days
                                    .last()
                                    .map(|day| utc_date(day.start_ms))
                                    .unwrap_or_default(),
                            ),
                    ),
            )
            .child(heatmap_canvas(data, self.usage.heatmap_mode, colors))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child("周一至周日 · 每列一周")
                    .child("浅 → 深 · Tokens"),
            );
        let models: Vec<_> = data
            .models
            .iter()
            .enumerate()
            .map(|(index, series)| {
                let values: Vec<u64> = series
                    .days
                    .iter()
                    .rev()
                    .take(range)
                    .map(|day| day.totals.total_tokens)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                (
                    series.model.clone().unwrap_or_else(|| "其他模型".into()),
                    values,
                    palette(index, colors),
                )
            })
            .collect();
        let selected_total: u64 = models
            .iter()
            .flat_map(|(_, values, _)| values)
            .copied()
            .sum();
        let range_row = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child("按 UTC 日期统计"),
            )
            .child(
                div().flex().gap_1().children(
                    [("usage-7days", "近7日", 7), ("usage-30days", "近30日", 30)]
                        .into_iter()
                        .map(|(id, label, days)| {
                            self.usage_button(
                                id,
                                label,
                                range == days,
                                move |this, cx| {
                                    this.usage.range_days = days;
                                    cx.notify();
                                },
                                cx,
                            )
                        }),
                ),
            );
        let selected = self.usage.selected_day.unwrap_or(range - 1).min(range - 1);
        let dates: Vec<_> = data
            .days
            .iter()
            .rev()
            .take(range)
            .rev()
            .map(|day| utc_date(day.start_ms))
            .collect();
        let peak_model = models
            .iter()
            .flat_map(|(_, values, _)| values)
            .copied()
            .max()
            .unwrap_or(0);
        let canvas_bounds = std::rc::Rc::new(std::cell::Cell::new(None::<Bounds<gpui::Pixels>>));
        let clicked_bounds = canvas_bounds.clone();
        let trend = div()
            .debug_selector(|| "usage-trend".into())
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .bg(colors.bg_hover)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child("每日模型 Tokens")
                    .child(
                        div()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child(format!("0 — {} Tokens", peak_model)),
                    ),
            )
            .child(
                div()
                    .id("usage-trend-chart")
                    .debug_selector(|| "usage-trend-chart".into())
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if let Some(bounds) = clicked_bounds.get() {
                                let fraction = ((event.position.x - bounds.origin.x)
                                    / bounds.size.width)
                                    .clamp(0., 1.);
                                this.usage.selected_day =
                                    Some((fraction * (range - 1) as f32).round() as usize);
                                cx.notify();
                            }
                        }),
                    )
                    .child(trend_canvas(models.clone(), colors, canvas_bounds)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(dates.first().cloned().unwrap_or_default())
                    .child(dates.last().cloned().unwrap_or_default()),
            );
        let trend = trend.child(
            div()
                .debug_selector(|| "usage-daily-detail".into())
                .flex()
                .flex_col()
                .gap_1()
                .text_size(px(Typography::METADATA))
                .child(div().text_color(colors.text_secondary).child(format!(
                    "{} · 点击曲线查看当日用量",
                    dates.get(selected).cloned().unwrap_or_default()
                )))
                .children(models.iter().map(|(name, values, color)| {
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(div().size(px(8.)).rounded_full().bg(*color))
                        .child(format!(
                            "{} · {} Tokens",
                            name,
                            values.get(selected).copied().unwrap_or(0)
                        ))
                })),
        );
        let legend = div()
            .flex_1()
            .min_w(px(180.))
            .flex()
            .flex_col()
            .gap_2()
            .children(models.iter().filter_map(|(name, values, color)| {
                let total: u64 = values.iter().sum();
                (total > 0).then(|| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().size(px(8.)).rounded_full().bg(*color))
                        .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        .child(div().text_color(colors.text_secondary).child(format!(
                            "{} · {:.1}%",
                            compact(total),
                            100. * total as f64 / selected_total.max(1) as f64
                        )))
                })
            }));
        let distribution = div()
            .debug_selector(|| "usage-distribution".into())
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .bg(colors.bg_hover)
            .child("模型用量")
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .relative()
                            .size(px(145.))
                            .flex_shrink_0()
                            .debug_selector(|| "usage-donut-frame".into())
                            .child(donut_canvas(models, colors))
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .debug_selector(|| "usage-donut-total".into())
                                    .child(
                                        div()
                                            .text_size(px(Typography::HEADING_CARD))
                                            .text_color(colors.text_primary)
                                            .child(compact(selected_total)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(Typography::METADATA))
                                            .text_color(colors.text_secondary)
                                            .child("Tokens"),
                                    ),
                            ),
                    )
                    .child(legend),
            )
            .when(selected_total == 0, |d| {
                d.child(
                    div()
                        .text_color(colors.text_secondary)
                        .child("此时间段暂无 Tokens 用量"),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(metric_band)
            .child(heatmap)
            .child(range_row)
            .child(trend)
            .child(distribution)
            .into_any_element()
    }
}

fn weekday_offset(data: &UsageDashboard) -> usize {
    data.days
        .first()
        .map(|day| (day.start_ms.div_euclid(86_400_000) + 3).rem_euclid(7) as usize)
        .unwrap_or(0)
}
// Gregorian civil date from Unix days, avoiding local timezone or additional dependencies.
fn utc_date(ms: i64) -> String {
    let z = ms.div_euclid(86_400_000) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn compact(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.2}M", value as f64 / 1_000_000.)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.)
    } else {
        value.to_string()
    }
}
fn cost_label(totals: &UsageTotals) -> String {
    if totals.calls == totals.unpriced_calls {
        return "未计价".into();
    }
    let cost = crate::summary_card::format_usd(vega_conversation::types::Microcents(
        totals.priced_cost_microcents,
    ));
    if totals.unpriced_calls > 0 {
        format!("{cost} + {}次未计价", totals.unpriced_calls)
    } else {
        cost
    }
}
fn palette(index: usize, colors: vega_theme::ThemeColors) -> Rgba {
    let colors = [
        colors.accent,
        colors.success,
        colors.warning,
        colors.text_secondary,
        colors.danger,
    ];
    colors[index % colors.len()].opacity(1. - (index / colors.len()).min(3) as f32 * 0.15)
}
fn heatmap_values(data: &UsageDashboard, mode: usize) -> Vec<u64> {
    let values: Vec<_> = data
        .days
        .iter()
        .map(|day| day.totals.total_tokens)
        .collect();
    match mode {
        1 => {
            let offset = weekday_offset(data);
            let mut padded = vec![0; offset];
            padded.extend(values);
            padded
                .chunks(7)
                .flat_map(|week| std::iter::repeat_n(week.iter().copied().sum(), week.len()))
                .skip(offset)
                .collect()
        }
        2 => {
            let mut total = 0u64;
            values
                .into_iter()
                .map(|value| {
                    total = total.saturating_add(value);
                    total
                })
                .collect()
        }
        _ => values,
    }
}
fn heatmap_canvas(
    data: &UsageDashboard,
    mode: usize,
    colors: vega_theme::ThemeColors,
) -> AnyElement {
    let values = heatmap_values(data, mode);
    let offset = weekday_offset(data);
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let max = values.iter().copied().max().unwrap_or(1).max(1) as f32;
            let cell = (bounds.size.width / 53.).min(px(15.));
            for (index, value) in values.iter().enumerate() {
                let index = index + offset;
                let rect = Bounds::new(
                    bounds.origin + point(cell * (index / 7) as f32, cell * (index % 7) as f32),
                    size(cell - px(2.), cell - px(2.)),
                );
                window.paint_quad(fill(
                    rect,
                    if *value == 0 {
                        colors.border_subtle
                    } else {
                        colors.accent.opacity(0.2 + 0.8 * *value as f32 / max)
                    },
                ));
            }
        },
    )
    .w_full()
    .h(px(110.))
    .into_any_element()
}

type ModelPoints = Vec<(String, Vec<u64>, Rgba)>;
fn trend_canvas(
    models: ModelPoints,
    colors: vega_theme::ThemeColors,
    layout: std::rc::Rc<std::cell::Cell<Option<Bounds<gpui::Pixels>>>>,
) -> AnyElement {
    canvas(
        move |bounds, _, _| {
            layout.set(Some(bounds));
        },
        move |bounds, _, window, _| {
            let max = models
                .iter()
                .flat_map(|(_, v, _)| v)
                .copied()
                .max()
                .unwrap_or(1)
                .max(1) as f32;
            for fraction in [0., 0.5, 1.] {
                let mut path = PathBuilder::stroke(px(1.));
                let y = bounds.origin.y + bounds.size.height * fraction;
                path.move_to(point(bounds.origin.x, y));
                path.line_to(point(bounds.right(), y));
                if let Ok(path) = path.build() {
                    window.paint_path(path, colors.border_subtle);
                }
            }
            for (_, values, color) in &models {
                if values.iter().all(|value| *value == 0) {
                    continue;
                }
                let mut path = PathBuilder::stroke(px(2.));
                for (index, value) in values.iter().enumerate() {
                    let p = bounds.origin
                        + point(
                            bounds.size.width * index as f32
                                / values.len().saturating_sub(1).max(1) as f32,
                            bounds.size.height * (1. - *value as f32 / max),
                        );
                    if index == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, *color);
                }
            }
        },
    )
    .w_full()
    .h(px(150.))
    .into_any_element()
}
fn donut_canvas(models: ModelPoints, colors: vega_theme::ThemeColors) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let totals: Vec<_> = models
                .iter()
                .map(|(_, values, color)| (values.iter().copied().sum::<u64>(), *color))
                .collect();
            let total: u64 = totals.iter().map(|(value, _)| value).sum();
            let segments = if total == 0 {
                vec![(1, colors.bg_hover)]
            } else {
                totals
            };
            let mut angle = -std::f32::consts::FRAC_PI_2;
            for (value, color) in segments {
                let sweep = value as f32 / total.max(1) as f32 * std::f32::consts::TAU;
                let mut path = PathBuilder::stroke(px(18.));
                for step in 0..=100 {
                    let a = angle + sweep * step as f32 / 100.;
                    let p = bounds.center() + point(px(a.cos() * 55.), px(a.sin() * 55.));
                    if step == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
                angle += sweep;
            }
        },
    )
    .size(px(145.))
    .flex_shrink_0()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext, WindowBounds, WindowOptions};
    use vega_conversation::types::{UsageDay, UsageModelSeries};
    fn sample(calls: u64) -> UsageDashboard {
        let totals = UsageTotals {
            calls,
            total_tokens: calls * 123,
            priced_cost_microcents: 1,
            unpriced_calls: calls.saturating_sub(1),
            ..Default::default()
        };
        let days: Vec<_> = (0..365)
            .map(|i| UsageDay {
                start_ms: i * 86_400_000,
                totals: if i == 364 {
                    totals.clone()
                } else {
                    UsageTotals::default()
                },
            })
            .collect();
        UsageDashboard {
            generated_at_ms: 365 * 86_400_000,
            timezone_label: "UTC".into(),
            lifetime: totals,
            models: vec![UsageModelSeries {
                model: Some("model-a".into()),
                days: days[335..].to_vec(),
            }],
            days,
        }
    }

    #[test]
    fn costs_keep_microcents_and_unpriced_coverage_and_heatmap_modes_keep_calendar() {
        let data = sample(2);
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(951782400000), "2000-02-29");
        assert_eq!(weekday_offset(&data), 3);
        assert_eq!(cost_label(&data.lifetime), "US$0.000001 + 1次未计价");
        assert_eq!(heatmap_values(&data, 0).len(), 365);
        assert_eq!(heatmap_values(&data, 1).len(), 365);
        assert_eq!(heatmap_values(&data, 2).len(), 365);
        assert_eq!(heatmap_values(&data, 2)[364], 246);
        assert_eq!(
            cost_label(&UsageTotals {
                calls: 1,
                unpriced_calls: 1,
                ..Default::default()
            }),
            "未计价"
        );
    }

    #[gpui::test]
    async fn settings_usage_real_render_controls_refresh_empty_and_failure(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            crate::init(cx);
        });
        let view = cx.new(SettingsView::new_for_test);
        view.update(cx, |view, _| view.section = 4);
        let reloads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = reloads.clone();
        cx.update(|cx| {
            cx.subscribe(&view, move |_, _: &UsageReloadRequested, _| {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
            .detach();
        });
        let root = view.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(960.), px(900.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, _| root,
            )
            .expect("usage settings window")
        });
        view.update(cx, |view, cx| view.apply_usage_dashboard(Ok(sample(0)), cx));
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("usage-empty").is_some());
        assert!(visual.debug_bounds("usage-trend").is_none());
        let refresh = visual
            .debug_bounds("usage-refresh")
            .expect("refresh button");
        visual.simulate_click(refresh.center(), gpui::Modifiers::default());
        visual.run_until_parked();
        assert_eq!(reloads.load(std::sync::atomic::Ordering::SeqCst), 1);
        view.update(&mut visual, |view, cx| {
            view.apply_usage_dashboard(Err(UsageDashboardError::Unavailable), cx)
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("usage-error").is_some());
        view.update(&mut visual, |view, cx| {
            view.apply_usage_dashboard(Ok(sample(2)), cx)
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("usage-trend").is_some());
        assert!(visual.debug_bounds("usage-error").is_none());
        let activity_start = visual
            .debug_bounds("usage-activity-start-date")
            .expect("exact activity start");
        let activity_end = visual
            .debug_bounds("usage-activity-end-date")
            .expect("exact activity end");
        assert!(activity_end.origin.x > activity_start.origin.x);
        let donut_frame = visual
            .debug_bounds("usage-donut-frame")
            .expect("donut frame");
        let donut_total = visual
            .debug_bounds("usage-donut-total")
            .expect("centered donut total");
        assert_eq!(donut_frame.center(), donut_total.center());
        assert_eq!(donut_frame.size, size(px(145.), px(145.)));
        let chart = visual
            .debug_bounds("usage-trend-chart")
            .expect("clickable daily chart");
        visual.simulate_click(chart.center(), gpui::Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            view.read_with(&visual, |v, _| v.usage.selected_day),
            Some(3)
        );
        assert!(visual.debug_bounds("usage-daily-detail").is_some());
        let weekly = visual.debug_bounds("usage-weekly").expect("weekly control");
        visual.simulate_click(weekly.center(), gpui::Modifiers::default());
        visual.run_until_parked();
        assert_eq!(view.read_with(&visual, |v, _| v.usage.heatmap_mode), 1);
        let thirty = visual.debug_bounds("usage-30days").expect("range control");
        visual.simulate_click(thirty.center(), gpui::Modifiers::default());
        visual.run_until_parked();
        assert_eq!(view.read_with(&visual, |v, _| v.usage.range_days), 30);
        visual.update(|_, cx| {
            cx.set_global(vega_theme::Theme::dark());
            cx.refresh_windows();
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("usage-trend").is_some());
    }
}
