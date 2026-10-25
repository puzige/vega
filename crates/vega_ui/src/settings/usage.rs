//! Read-only Settings usage dashboard, populated by the app-owned worker.
use super::*;
use crate::interaction::InteractionExt;
use gpui_kit::{Bounds, PathBuilder, Rgba, canvas, fill, point, size};
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

fn usage_metric(
    label: &'static str,
    value: gpui_kit::StyledText,
    colors: vega_theme::ThemeColors,
) -> Div {
    div()
        .flex_1()
        .flex_basis(px(Layout::SETTINGS_LABEL_WIDTH))
        .min_w_0()
        .max_w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .w_full()
                .min_w_0()
                .text_align(gpui_kit::TextAlign::Left)
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
        let header = settings_actions()
            .child(self.usage_button(
                "usage-refresh",
                if self.usage.loading {
                    t("settings.usage.refreshing")
                } else {
                    t("settings.action.refresh")
                },
                false,
                |this, cx| this.request_usage_reload(cx),
                cx,
            ))
            .children(
                [
                    ("usage-7days", t("settings.usage.last_7_days"), 7),
                    ("usage-30days", t("settings.usage.last_30_days"), 30),
                ]
                .into_iter()
                .map(|(id, label, days)| {
                    self.usage_button(
                        id,
                        label,
                        (if self.usage.range_days == 30 { 30 } else { 7 }) == days,
                        move |this, cx| {
                            this.usage.range_days = days;
                            cx.notify();
                        },
                        cx,
                    )
                }),
            )
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(t("settings.usage.utc")),
            );
        let mut page = settings_section().w_full().child(header);
        if self.usage.error {
            page = page.child(settings_status(
                t("settings.usage.load_failed"),
                colors.danger,
                colors,
            ));
        }
        if let Some(data) = self.usage.dashboard.clone() {
            if data.lifetime.calls == 0 {
                page = page.child(settings_status(
                    t("settings.usage.empty"),
                    colors.text_secondary,
                    colors,
                ));
            } else {
                page = page.child(self.render_usage_data(&data, cx));
            }
        } else if !self.usage.error {
            page = page.child(settings_status(
                t("settings.usage.loading"),
                colors.text_secondary,
                colors,
            ));
        }
        page.into_any_element()
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
        let enabled = id != "usage-refresh" || !self.usage.loading;
        let action = std::rc::Rc::new(action);
        let keyboard_action = action.clone();
        settings_action_chrome(colors)
            .id(id)
            .interaction_surface(
                if selected {
                    colors.bg_active
                } else {
                    colors.bg_elevated
                },
                if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                },
                colors.bg_active,
                enabled,
            )
            .track_focus(&self.usage_focuses[index])
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        keyboard_action(this, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .tab_stop(true)
            .when(enabled, |button| button.cursor_pointer())
            .when(!enabled, |button| button.text_color(colors.text_tertiary))
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(selected, |d| {
                d.bg(colors.bg_active).text_color(colors.text_primary)
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    action(this, cx);
                }
            }))
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
            (
                t("settings.usage.lifetime_tokens"),
                compact(data.lifetime.total_tokens),
            ),
            (
                t("settings.usage.lifetime_cost"),
                cost_label(&data.lifetime),
            ),
            (t("settings.usage.peak_day"), compact(peak)),
            (t("settings.usage.active_days"), active.to_string()),
            (t("settings.usage.longest_streak"), longest.to_string()),
        ];
        let metric_band = settings_group(colors)
            .flex_row()
            .flex_wrap()
            .gap(px(Layout::SETTINGS_FIELD_GAP))
            .children(metrics.into_iter().map(|(label, value)| {
                usage_metric(label, gpui_kit::StyledText::new(value), colors)
            }));
        let heatmap = div()
            .flex()
            .flex_col()
            .gap_3()
            .p(px(Layout::SETTINGS_GROUP_PADDING))
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_subtle)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().child(t("settings.usage.activity")))
                    .child(
                        div().flex().gap_1().children(
                            [
                                ("usage-daily", t("settings.usage.daily")),
                                ("usage-weekly", t("settings.usage.weekly")),
                                ("usage-cumulative", t("settings.usage.cumulative")),
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
                        div().child(
                            data.days
                                .first()
                                .map(|day| utc_date(day.start_ms))
                                .unwrap_or_default(),
                        ),
                    )
                    .child(
                        div().child(
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
                    .child(t("settings.usage.heatmap_weeks"))
                    .child(t("settings.usage.heatmap_scale")),
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
                    series
                        .model
                        .clone()
                        .unwrap_or_else(|| t("settings.usage.other_model").into()),
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
        let canvas_bounds =
            std::rc::Rc::new(std::cell::Cell::new(None::<Bounds<gpui_kit::Pixels>>));
        let clicked_bounds = canvas_bounds.clone();
        let trend = div()
            .p(px(Layout::SETTINGS_GROUP_PADDING))
            .border_1()
            .border_color(colors.border_subtle)
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .bg(colors.bg_subtle)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(t("settings.usage.daily_model_tokens"))
                    .child(
                        div()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child(t_args(
                                "settings.usage.token_range",
                                &[("count", &peak_model.to_string())],
                            )),
                    ),
            )
            .child(
                div()
                    .id("usage-trend-chart")
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui_kit::MouseDownEvent, _, cx| {
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
                .flex()
                .flex_col()
                .gap_1()
                .text_size(px(Typography::METADATA))
                .child(div().text_color(colors.text_secondary).child(t_args(
                    "settings.usage.chart_hint",
                    &[(
                        "date",
                        dates.get(selected).map(String::as_str).unwrap_or_default(),
                    )],
                )))
                .children(models.iter().map(|(name, values, color)| {
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(div().size(px(8.)).rounded_full().bg(*color))
                        .child(t_args(
                            "settings.usage.day_tokens",
                            &[
                                ("model", name),
                                (
                                    "count",
                                    &(values.get(selected).copied().unwrap_or(0)).to_string(),
                                ),
                            ],
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
                        .child(div().flex_1().min_w_0().child(name.clone()))
                        .child(div().text_color(colors.text_secondary).child(format!(
                            "{} · {:.1}%",
                            compact(total),
                            100. * total as f64 / selected_total.max(1) as f64
                        )))
                })
            }));
        let distribution = div()
            .p(px(Layout::SETTINGS_GROUP_PADDING))
            .border_1()
            .border_color(colors.border_subtle)
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .bg(colors.bg_subtle)
            .child(t("settings.usage.model_usage"))
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
                            .child(donut_canvas(models, colors))
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
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
                                            .child(t("settings.usage.tokens")),
                                    ),
                            ),
                    )
                    .child(legend),
            )
            .when(selected_total == 0, |d| {
                d.child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(t("settings.usage.no_tokens")),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(metric_band)
            .child(heatmap)
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
        return t("settings.usage.unpriced").into();
    }
    let cost = crate::summary_card::format_usd(vega_conversation::types::Microcents(
        totals.priced_cost_microcents,
    ));
    if totals.unpriced_calls > 0 {
        t_args(
            "settings.usage.unpriced_calls",
            &[
                ("cost", &cost),
                ("count", &totals.unpriced_calls.to_string()),
            ],
        )
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
    layout: std::rc::Rc<std::cell::Cell<Option<Bounds<gpui_kit::Pixels>>>>,
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
