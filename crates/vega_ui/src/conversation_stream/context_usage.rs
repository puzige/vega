#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NextRequestSource {
    Estimated,
    UsageAnchored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NextRequestEstimate {
    pub(crate) tokens: u64,
    pub(crate) source: NextRequestSource,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContextUsageDisplay {
    pub(crate) last_call_total: Option<u64>,
    pub(crate) model_input_limit: Option<u64>,
    pub(crate) fill_fraction: f32,
    next_request: Option<NextRequestEstimate>,
}

impl ContextUsageDisplay {
    pub(crate) fn new(
        last_call_total: Option<u64>,
        model_input_limit: Option<u64>,
        next_request: Option<NextRequestEstimate>,
    ) -> Self {
        let model_input_limit = model_input_limit.filter(|limit| *limit > 0);
        let fill_fraction = last_call_total
            .zip(model_input_limit)
            .map_or(0.0, |(total, limit)| {
                ((total as f64) / (limit as f64)).min(1.0) as f32
            });
        Self {
            last_call_total,
            model_input_limit,
            fill_fraction,
            next_request,
        }
    }

    pub(crate) fn percentage_label(&self) -> Option<String> {
        self.percentage_value().map(|percent| {
            vega_i18n::t_args("ui.last_call_usage_percent", &[("percent", &percent)])
        })
    }

    pub(crate) fn compact_usage_label(&self) -> String {
        let Some(total) = self.last_call_total else {
            return vega_i18n::t("ui.no_data_yet").into();
        };
        match self.model_input_limit {
            Some(limit) => vega_i18n::t_args(
                "ui.context_window_tokens_used",
                &[
                    ("used", &compact_token_count(total)),
                    ("limit", &compact_token_count(limit)),
                ],
            ),
            None => vega_i18n::t_args(
                "ui.context_window_used",
                &[("tokens", &compact_token_count(total))],
            ),
        }
    }

    pub(crate) fn tooltip_status(&self) -> String {
        match self.last_call_total.zip(self.model_input_limit) {
            Some((used, limit)) => {
                let percent = (u128::from(used) * 100 + u128::from(limit) / 2) / u128::from(limit);
                let remaining = 100_u128.saturating_sub(percent);
                vega_i18n::t_args(
                    "ui.context_window_used_remaining",
                    &[
                        ("percent", &percent.to_string()),
                        ("remaining", &remaining.to_string()),
                    ],
                )
            }
            None if self.last_call_total.is_none() => vega_i18n::t("ui.no_data_yet").into(),
            None => vega_i18n::t("ui.capacity_not_configured").into(),
        }
    }

    pub(crate) fn tooltip_usage(&self) -> String {
        if self.last_call_total.is_some() {
            return self.compact_usage_label();
        }
        self.model_input_limit.map_or_else(
            || vega_i18n::t("ui.capacity_not_configured").into(),
            |limit| {
                vega_i18n::t_args(
                    "ui.context_window_capacity",
                    &[("tokens", &compact_token_count(limit))],
                )
            },
        )
    }

    pub(crate) fn last_call_label(&self) -> String {
        let Some(total) = self.last_call_total else {
            return vega_i18n::t("ui.no_data_yet").into();
        };
        vega_i18n::t_args(
            "ui.last_call_total_tokens_tokens_input_output",
            &[("tokens", &grouped_token_count(total))],
        )
    }

    pub(crate) fn next_request_label(&self) -> Option<String> {
        let next = self.next_request?;
        let tokens = grouped_token_count(next.tokens);
        Some(match next.source {
            NextRequestSource::Estimated => vega_i18n::t_args(
                "ui.estimated_next_request_tokens_tokens",
                &[("tokens", &tokens)],
            ),
            NextRequestSource::UsageAnchored => vega_i18n::t_args(
                "ui.calibrated_next_request_tokens_tokens",
                &[("tokens", &tokens)],
            ),
        })
    }

    pub(crate) fn percentage_value(&self) -> Option<String> {
        self.last_call_total
            .zip(self.model_input_limit)
            .map(|(total, limit)| {
                let tenths = (u128::from(total) * 1000 + u128::from(limit) / 2) / u128::from(limit);
                if tenths % 10 == 0 {
                    format!("{}%", tenths / 10)
                } else {
                    format!("{}.{:01}%", tenths / 10, tenths % 10)
                }
            })
    }

    pub(crate) fn capacity_label(&self) -> String {
        self.model_input_limit.map_or_else(
            || vega_i18n::t("ui.capacity_not_configured").to_string(),
            |limit| {
                vega_i18n::t_args(
                    "ui.current_model_input_limit_tokens_tokens_not_provider",
                    &[("tokens", &grouped_token_count(limit))],
                )
            },
        )
    }

    pub(crate) fn accessibility_label(&self) -> String {
        let mut parts = vec![
            vega_i18n::t("ui.context_usage").to_string(),
            self.last_call_label(),
        ];
        if let Some(percentage) = self.percentage_label() {
            parts.push(percentage);
        }
        if let Some(next) = self.next_request_label() {
            parts.push(next);
        }
        parts.push(self.capacity_label());
        parts.join("；")
    }
}

fn compact_token_count(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    let mut whole = tokens / 1_000;
    let mut tenth = (tokens % 1_000 + 50) / 100;
    if tenth == 10 {
        whole += 1;
        tenth = 0;
    }
    if tenth == 0 {
        format!("{whole}k")
    } else {
        format!("{whole}.{tenth}k")
    }
}

fn grouped_token_count(tokens: u64) -> String {
    let digits = tokens.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}
