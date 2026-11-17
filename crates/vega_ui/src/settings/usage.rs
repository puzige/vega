use std::sync::Arc;

use super::*;
use vega_conversation::types::{UsageDashboard, UsageDashboardError, UsageTotals};

mod nsh_usage;

pub struct UsageReloadRequested;
impl EventEmitter<UsageReloadRequested> for SettingsView {}

#[derive(Default)]
pub(crate) struct UsageState {
    dashboard: Option<Arc<UsageDashboard>>,
    error: bool,
    loading: bool,
    heatmap_mode: usize,
    range_days: usize,
    selected_day: Option<usize>,
    revision: u64,
    presentation: Option<nsh_usage::Presentation>,
}

impl SettingsView {
    pub fn begin_usage_load(&mut self, cx: &mut Context<Self>) {
        self.usage.loading = true;
        self.usage.revision = self.usage.revision.wrapping_add(1);
        cx.notify();
    }

    pub fn apply_usage_dashboard(
        &mut self,
        result: Result<UsageDashboard, UsageDashboardError>,
        cx: &mut Context<Self>,
    ) {
        self.usage.loading = false;
        self.usage.revision = self.usage.revision.wrapping_add(1);
        match result {
            Ok(dashboard) => {
                self.usage.dashboard = Some(Arc::new(dashboard));
                self.usage.error = false;
            }
            Err(_) => self.usage.error = true,
        }
        cx.notify();
    }

    pub(crate) fn request_usage_reload(&mut self, cx: &mut Context<Self>) {
        if !self.usage.loading {
            self.usage.loading = true;
            self.usage.revision = self.usage.revision.wrapping_add(1);
            cx.emit(UsageReloadRequested);
            cx.notify();
        }
    }
}

fn weekday_offset(data: &UsageDashboard) -> usize {
    data.days
        .first()
        .map(|day| (day.start_ms.div_euclid(86_400_000) + 3).rem_euclid(7) as usize)
        .unwrap_or(0)
}

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

fn palette(index: usize, colors: vega_theme::ThemeColors) -> gpui_kit::Rgba {
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
