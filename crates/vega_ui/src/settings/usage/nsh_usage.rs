use std::sync::Arc;

use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::UsageDashboard;
use vega_theme::{Layout, ThemeColors, Typography};

use super::{SettingsView, compact, cost_label, heatmap_values, palette, utc_date, weekday_offset};
use crate::nsh::{ProjectionId, Surface};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    dashboard: Option<Arc<UsageDashboard>>,
    revision: u64,
    loading: bool,
    error: bool,
    visible: bool,
    range: usize,
    mode: usize,
    selected: Option<usize>,
    colors: ThemeColors,
}

#[derive(Clone, Copy)]
enum Action {
    Refresh,
    Range(usize),
    Mode(usize),
    Day(usize),
}

struct Decision {
    observed: Snapshot,
    action: Action,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Decide(Box<Decision>),
    Layout(f32),
}

fn day_count(snapshot: &Snapshot) -> usize {
    snapshot
        .dashboard
        .as_ref()
        .map_or(0, |data| data.days.len().min(snapshot.range))
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: Action) -> bool {
    current.visible
        && current.revision == observed.revision
        && current.range == observed.range
        && current.mode == observed.mode
        && current.selected == observed.selected
        && match action {
            Action::Refresh => !current.loading,
            Action::Range(days) => matches!(days, 7 | 30),
            Action::Mode(mode) => mode <= 2,
            Action::Day(index) => index < day_count(current),
        }
}

impl SettingsView {
    fn usage_snapshot(&self, cx: &App, id: ProjectionId) -> Snapshot {
        Snapshot {
            id,
            dashboard: self.usage.dashboard.clone(),
            revision: self.usage.revision,
            loading: self.usage.loading,
            error: self.usage.error,
            visible: self.section == 4
                && cx
                    .try_global::<super::super::SettingsOpen>()
                    .is_some_and(|open| open.0),
            range: if self.usage.range_days == 30 { 30 } else { 7 },
            mode: self.usage.heatmap_mode,
            selected: self.usage.selected_day,
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(crate) fn sync_usage_presentation(&self, cx: &App) {
        if let Some(page) = &self.usage.presentation {
            page.surface
                .publish(self.usage_snapshot(cx, page.id.clone()));
        }
    }

    pub(crate) fn render_usage(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.usage.presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new(self.usage_snapshot(cx, id.clone()), render, window, cx);
            self.usage.presentation = Some(Presentation {
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted =
                                        this.usage.presentation.as_ref().is_some_and(|page| {
                                            page.id.matches(&command.observed.id)
                                                && accepts(
                                                    &this.usage_snapshot(cx, page.id.clone()),
                                                    &command.observed,
                                                    command.action,
                                                )
                                        });
                                    if accepted {
                                        match command.action {
                                            Action::Refresh => this.request_usage_reload(cx),
                                            Action::Range(days) => this.usage.range_days = days,
                                            Action::Mode(mode) => this.usage.heatmap_mode = mode,
                                            Action::Day(index) => {
                                                this.usage.selected_day = Some(index)
                                            }
                                        }
                                        cx.notify();
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.usage.presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            if let Some(page) = &this.usage.presentation {
                                page.surface
                                    .publish(this.usage_snapshot(cx, page.id.clone()));
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.usage.presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.usage_snapshot(cx, page.id.clone());
        let Some(page) = &mut self.usage.presentation else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        gpui_kit::div()
            .w_full()
            .flex_shrink_0()
            .h(gpui_kit::px(page.height))
            .child(page.surface.entity())
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Actions {
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let observed = self.state.get_untracked();
        if self.pending.get_untracked() || !accepts(&observed, &observed, action) {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            let accepted = commands
                .send(Request::Decide(Box::new(Decision {
                    observed,
                    action,
                    reply,
                })))
                .await
                .is_ok()
                && matches!(response.await, Ok(true));
            self.failed.set(!accepted);
            self.pending.set(false);
        });
    }
}

fn group(state: Signal<Snapshot>) -> nsh_core::element::View {
    view()
        .w_full()
        .flex_shrink_0()
        .gap_3()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .rounded(px(Layout::PANEL_RADIUS))
        .border(px(1.))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .bg(move || crate::nsh::color(state.get().colors.bg_subtle))
}

fn local(state: Signal<Snapshot>, key: &'static str) -> AnyElement {
    text(move || state.with(|_| vega_i18n::t(key))).into_element()
}

fn control(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    key: &'static str,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    let selected = move || match action {
        Action::Range(days) => actions.state.get().range == days,
        Action::Mode(mode) => actions.state.get().mode == mode,
        _ => false,
    };
    let label = move || {
        actions.state.with(|snapshot| {
            vega_i18n::t(if matches!(action, Action::Refresh) && snapshot.loading {
                "settings.usage.refreshing"
            } else {
                key
            })
        })
    };
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(label)
        .flex_row()
        .items_center()
        .gap_2()
        .accessibility_state(move || AccessibilityState {
            selected: selected(),
            ..Default::default()
        })
        .disabled(move || {
            actions.pending.get()
                || (matches!(action, Action::Refresh) && actions.state.get().loading)
        })
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px_3()
        .py_2()
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .bg(move || {
            crate::nsh::color(if selected() {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_elevated
            })
        })
        .on_press(move |_: &PressEvent| actions.send(commands.clone(), action))
        .child(when(selected, || text("✓")))
        .child(text(label))
        .into_element()
}

#[derive(Clone, PartialEq)]
struct Model {
    name: String,
    values: Vec<u64>,
}

#[derive(Clone, Default, PartialEq)]
struct Charts {
    metrics: Vec<(String, String)>,
    heat: Vec<u64>,
    offset: usize,
    first: String,
    last: String,
    dates: Vec<String>,
    models: Vec<Model>,
    selected: usize,
    total: u64,
    peak: u64,
}

fn charts(snapshot: &Snapshot) -> Charts {
    let Some(data) = &snapshot.dashboard else {
        return Charts::default();
    };
    let mut streak = 0;
    let mut longest = 0;
    for day in &data.days {
        streak = if day.totals.calls > 0 { streak + 1 } else { 0 };
        longest = longest.max(streak);
    }
    let metrics = [
        (
            "settings.usage.lifetime_tokens",
            compact(data.lifetime.total_tokens),
        ),
        ("settings.usage.lifetime_cost", cost_label(&data.lifetime)),
        (
            "settings.usage.peak_day",
            compact(
                data.days
                    .iter()
                    .map(|day| day.totals.total_tokens)
                    .max()
                    .unwrap_or(0),
            ),
        ),
        (
            "settings.usage.active_days",
            data.days
                .iter()
                .filter(|day| day.totals.calls > 0)
                .count()
                .to_string(),
        ),
        ("settings.usage.longest_streak", longest.to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (vega_i18n::t(key).to_string(), value))
    .collect();
    let models: Vec<_> = data
        .models
        .iter()
        .map(|model| Model {
            name: model
                .model
                .clone()
                .unwrap_or_else(|| vega_i18n::t("settings.usage.other_model").to_string()),
            values: model
                .days
                .iter()
                .rev()
                .take(snapshot.range)
                .map(|day| day.totals.total_tokens)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
        })
        .collect();
    let dates: Vec<_> = data
        .days
        .iter()
        .rev()
        .take(snapshot.range)
        .rev()
        .map(|day| utc_date(day.start_ms))
        .collect();
    Charts {
        metrics,
        heat: heatmap_values(data, snapshot.mode),
        offset: weekday_offset(data),
        first: data
            .days
            .first()
            .map(|day| utc_date(day.start_ms))
            .unwrap_or_default(),
        last: data
            .days
            .last()
            .map(|day| utc_date(day.start_ms))
            .unwrap_or_default(),
        selected: snapshot
            .selected
            .unwrap_or(dates.len().saturating_sub(1))
            .min(dates.len().saturating_sub(1)),
        total: models.iter().flat_map(|model| &model.values).copied().sum(),
        peak: models
            .iter()
            .flat_map(|model| &model.values)
            .copied()
            .max()
            .unwrap_or(0),
        dates,
        models,
    }
}

fn heatmap(data: &Charts, width: f32, colors: ThemeColors) -> Painter {
    let mut painter = Painter::new();
    let maximum = data.heat.iter().copied().max().unwrap_or(1).max(1) as f32;
    let cell = (width / 53.).min(15.);
    for (index, value) in data.heat.iter().enumerate() {
        let index = index + data.offset;
        let color = if *value == 0 {
            colors.border_subtle
        } else {
            colors.accent.opacity(0.2 + 0.8 * *value as f32 / maximum)
        };
        painter = painter
            .fill_color(crate::nsh::color(color).to_hsla())
            .fill_rect(
                cell * (index / 7) as f32,
                cell * (index % 7) as f32,
                (cell - 2.).max(0.),
                (cell - 2.).max(0.),
            );
    }
    painter
}

fn trend(data: &Charts, width: f32, colors: ThemeColors) -> Painter {
    let mut painter = Painter::new()
        .stroke_color(crate::nsh::color(colors.border_subtle).to_hsla())
        .line_width(1.);
    for fraction in [0., 0.5, 1.] {
        painter = painter
            .begin_path()
            .move_to(0., 150. * fraction)
            .line_to(width, 150. * fraction)
            .stroke();
    }
    for (index, model) in data.models.iter().enumerate() {
        if model.values.iter().all(|value| *value == 0) {
            continue;
        }
        painter = painter
            .stroke_color(crate::nsh::color(palette(index, colors)).to_hsla())
            .line_width(2.)
            .begin_path();
        for (day, value) in model.values.iter().enumerate() {
            let x = width * day as f32 / model.values.len().saturating_sub(1).max(1) as f32;
            let y = 150. * (1. - *value as f32 / data.peak.max(1) as f32);
            painter = if day == 0 {
                painter.move_to(x, y)
            } else {
                painter.line_to(x, y)
            };
        }
        painter = painter.stroke();
    }
    if !data.dates.is_empty() {
        let x = width * data.selected as f32 / data.dates.len().saturating_sub(1).max(1) as f32;
        painter = painter
            .stroke_color(crate::nsh::color(colors.text_secondary).to_hsla())
            .line_width(1.)
            .begin_path()
            .move_to(x, 0.)
            .line_to(x, 150.)
            .stroke();
    }
    painter
}

fn donut(data: &Charts, colors: ThemeColors) -> Painter {
    let mut painter = Painter::new().line_width(18.);
    if data.total == 0 {
        return painter
            .stroke_color(crate::nsh::color(colors.bg_hover).to_hsla())
            .begin_path()
            .arc(72.5, 72.5, 55., 0., std::f32::consts::TAU)
            .stroke();
    }
    let mut angle = -std::f32::consts::FRAC_PI_2;
    for (index, model) in data.models.iter().enumerate() {
        let total: u64 = model.values.iter().sum();
        if total == 0 {
            continue;
        }
        let end = angle + total as f32 / data.total as f32 * std::f32::consts::TAU;
        painter = painter
            .stroke_color(crate::nsh::color(palette(index, colors)).to_hsla())
            .begin_path()
            .arc(72.5, 72.5, 55., angle, end)
            .stroke();
        angle = end;
    }
    painter
}

fn day_at(x: f32, width: f32, count: usize) -> Option<usize> {
    if !x.is_finite() || !width.is_finite() || width <= 0. || count == 0 {
        return None;
    }
    Some(((x / width).clamp(0., 1.) * count.saturating_sub(1) as f32).round() as usize)
}

fn render_data(actions: Actions, commands: mpsc::Sender<Request>) -> AnyElement {
    let state = actions.state;
    let data = computed(move || state.with(charts));
    let heat_width = signal(0.0f32);
    let trend_width = signal(0.0f32);
    let chart_ref = element_ref();
    let focused = signal(false);
    chart_ref.on_focus_in(move || focused.set(true));
    chart_ref.on_focus_out(move || focused.set(false));
    let click = commands.clone();
    let keys = commands.clone();
    view()
        .w_full()
        .gap_4()
        .child(
            group(state)
                .flex_row()
                .flex_wrap()
                .gap(px(Layout::SETTINGS_FIELD_GAP))
                .child(each(
                    move || data.get().metrics,
                    |(label, _)| label.clone(),
                    move |metric, _| {
                        view()
                            .flex_1()
                            .min_w(px(Layout::SETTINGS_LABEL_WIDTH))
                            .gap_1()
                            .child(
                                text(move || metric.get().1)
                                    .text_size(px(Typography::HEADING_CARD)),
                            )
                            .child(
                                text(move || metric.get().0)
                                    .text_size(px(Typography::METADATA))
                                    .text_color(move || {
                                        crate::nsh::color(state.get().colors.text_secondary)
                                    }),
                            )
                    },
                )),
        )
        .child(
            group(state)
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(local(state, "settings.usage.activity"))
                        .children(
                            [
                                ("settings.usage.daily", 0),
                                ("settings.usage.weekly", 1),
                                ("settings.usage.cumulative", 2),
                            ]
                            .into_iter()
                            .map({
                                let commands = commands.clone();
                                move |(key, mode)| {
                                    control(actions, commands.clone(), Action::Mode(mode), key)
                                }
                            }),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .justify_between()
                        .text_size(px(Typography::METADATA))
                        .child(text(move || data.get().first))
                        .child(text(move || data.get().last)),
                )
                .child(
                    canvas(move || heatmap(&data.get(), heat_width.get(), state.get().colors))
                        .w_full()
                        .h(px(110.))
                        .on_layout(move |bounds: &Bounds<Points>| {
                            if (heat_width.get_untracked() - bounds.size.width.0).abs() > 0.5 {
                                heat_width.set(bounds.size.width.0);
                            }
                        }),
                )
                .child(
                    view()
                        .flex_row()
                        .justify_between()
                        .text_size(px(Typography::METADATA))
                        .text_color(move || crate::nsh::color(state.get().colors.text_tertiary))
                        .child(local(state, "settings.usage.heatmap_weeks"))
                        .child(local(state, "settings.usage.heatmap_scale")),
                ),
        )
        .child(
            group(state)
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .justify_between()
                        .gap_2()
                        .child(local(state, "settings.usage.daily_model_tokens"))
                        .child(
                            text(move || {
                                vega_i18n::t_args(
                                    "settings.usage.token_range",
                                    &[("count", &data.get().peak.to_string())],
                                )
                            })
                            .text_size(px(Typography::METADATA)),
                        ),
                )
                .child(
                    view()
                        .element_ref(&chart_ref)
                        .role(AccessibilityRole::Button)
                        .focusable(true)
                        .label(move || {
                            state.with(|_| vega_i18n::t("settings.usage.daily_model_tokens"))
                        })
                        .border(px(1.))
                        .border_color(move || {
                            crate::nsh::color(if focused.get() {
                                state.get().colors.accent
                            } else {
                                state.get().colors.border_subtle
                            })
                        })
                        .disabled(move || data.get().dates.is_empty())
                        .on_press(move |event: &PressEvent| {
                            chart_ref.focus();
                            if let Some(index) = day_at(
                                event.position.x.0,
                                trend_width.get_untracked(),
                                data.get_untracked().dates.len(),
                            ) {
                                actions.send(click.clone(), Action::Day(index));
                            }
                        })
                        .child(when(
                            move || focused.get() && state.get().visible && !actions.pending.get(),
                            move || {
                                for (key, direction) in
                                    [("left", -1), ("right", 1), ("home", -2), ("end", 2)]
                                {
                                    let commands = keys.clone();
                                    on_key_with(
                                        key,
                                        KeyOptions {
                                            when: nsh_core::keymap::When::NotEditing,
                                            ..Default::default()
                                        },
                                        move |_| {
                                            let current = data.get_untracked();
                                            if current.dates.is_empty() {
                                                return;
                                            }
                                            let index = match direction {
                                                -2 => 0,
                                                2 => current.dates.len() - 1,
                                                -1 => current.selected.saturating_sub(1),
                                                _ => (current.selected + 1)
                                                    .min(current.dates.len() - 1),
                                            };
                                            actions.send(commands.clone(), Action::Day(index));
                                        },
                                    );
                                }
                                view()
                            },
                        ))
                        .child(
                            canvas(move || {
                                trend(&data.get(), trend_width.get(), state.get().colors)
                            })
                            .w_full()
                            .h(px(150.))
                            .on_layout(
                                move |bounds: &Bounds<Points>| {
                                    if (trend_width.get_untracked() - bounds.size.width.0).abs()
                                        > 0.5
                                    {
                                        trend_width.set(bounds.size.width.0);
                                    }
                                },
                            ),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .justify_between()
                        .text_size(px(Typography::METADATA))
                        .child(text(move || {
                            data.get().dates.first().cloned().unwrap_or_default()
                        }))
                        .child(text(move || {
                            data.get().dates.last().cloned().unwrap_or_default()
                        })),
                )
                .child(
                    text(move || {
                        let data = data.get();
                        vega_i18n::t_args(
                            "settings.usage.chart_hint",
                            &[(
                                "date",
                                data.dates
                                    .get(data.selected)
                                    .map(String::as_str)
                                    .unwrap_or_default(),
                            )],
                        )
                    })
                    .text_size(px(Typography::METADATA)),
                )
                .child(each(
                    move || (0..data.get().models.len()).collect::<Vec<_>>(),
                    |index| *index,
                    move |index, _| {
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(view().size(px(8.)).rounded(px(4.)).bg(move || {
                                crate::nsh::color(palette(index.get(), state.get().colors))
                            }))
                            .child(
                                text(move || {
                                    let data = data.get();
                                    let Some(model) = data.models.get(index.get()) else {
                                        return String::new();
                                    };
                                    vega_i18n::t_args(
                                        "settings.usage.day_tokens",
                                        &[
                                            ("model", &model.name),
                                            (
                                                "count",
                                                &model
                                                    .values
                                                    .get(data.selected)
                                                    .copied()
                                                    .unwrap_or(0)
                                                    .to_string(),
                                            ),
                                        ],
                                    )
                                })
                                .text_size(px(Typography::METADATA)),
                            )
                    },
                )),
        )
        .child(
            group(state)
                .child(local(state, "settings.usage.model_usage"))
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_4()
                        .child(
                            view()
                                .relative()
                                .size(px(145.))
                                .flex_shrink_0()
                                .child(
                                    canvas(move || donut(&data.get(), state.get().colors))
                                        .size(px(145.)),
                                )
                                .child(
                                    view()
                                        .absolute()
                                        .inset_0()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            text(move || compact(data.get().total))
                                                .text_size(px(Typography::HEADING_CARD)),
                                        )
                                        .child(local(state, "settings.usage.tokens")),
                                ),
                        )
                        .child(view().flex_1().min_w(px(180.)).gap_2().child(each(
                            move || {
                                data.get()
                                    .models
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(index, model)| {
                                        (model.values.iter().sum::<u64>() > 0).then_some(index)
                                    })
                                    .collect::<Vec<_>>()
                            },
                            |index| *index,
                            move |index, _| {
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(view().size(px(8.)).rounded(px(4.)).bg(move || {
                                        crate::nsh::color(palette(index.get(), state.get().colors))
                                    }))
                                    .child(
                                        text(move || {
                                            data.get()
                                                .models
                                                .get(index.get())
                                                .map(|model| model.name.clone())
                                                .unwrap_or_default()
                                        })
                                        .flex_1(),
                                    )
                                    .child(
                                        text(move || {
                                            let data = data.get();
                                            let Some(model) = data.models.get(index.get()) else {
                                                return String::new();
                                            };
                                            let total: u64 = model.values.iter().sum();
                                            format!(
                                                "{} · {:.1}%",
                                                compact(total),
                                                100. * total as f64 / data.total.max(1) as f64
                                            )
                                        })
                                        .text_color(
                                            move || {
                                                crate::nsh::color(state.get().colors.text_secondary)
                                            },
                                        ),
                                    )
                            },
                        ))),
                )
                .child(when(
                    move || data.get().total == 0,
                    move || local(state, "settings.usage.no_tokens"),
                )),
        )
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
    };
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let data_commands = commands.clone();
    view()
        .w_full()
        .flex_shrink_0()
        .gap(px(Layout::SETTINGS_SECTION_GAP))
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(height)).await.is_err() {
                        actions.failed.set(true);
                    }
                });
            }
        })
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(control(
                    actions,
                    commands.clone(),
                    Action::Refresh,
                    "settings.action.refresh",
                ))
                .child(control(
                    actions,
                    commands.clone(),
                    Action::Range(7),
                    "settings.usage.last_7_days",
                ))
                .child(control(
                    actions,
                    commands,
                    Action::Range(30),
                    "settings.usage.last_30_days",
                ))
                .child(local(state, "settings.usage.utc")),
        )
        .child(when(
            move || state.get().error,
            move || {
                text(move || state.with(|_| vega_i18n::t("settings.usage.load_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .child(when(
            move || state.get().dashboard.is_none() && !state.get().error,
            move || local(state, "settings.usage.loading"),
        ))
        .child(when(
            move || {
                state
                    .get()
                    .dashboard
                    .as_ref()
                    .is_some_and(|data| data.lifetime.calls == 0)
            },
            move || local(state, "settings.usage.empty"),
        ))
        .child(when(
            move || {
                state
                    .get()
                    .dashboard
                    .as_ref()
                    .is_some_and(|data| data.lifetime.calls > 0)
            },
            move || render_data(actions, data_commands.clone()),
        ))
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}
