#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextUsageSource {
    EstimatedInput,
    LastCallTotal,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContextUsageDisplay {
    pub(crate) estimated_tokens: Option<u64>,
    pub(crate) model_input_limit: Option<u64>,
    pub(crate) percent_used: Option<u128>,
    pub(crate) fill_fraction: f32,
    source: ContextUsageSource,
}

impl ContextUsageDisplay {
    pub(crate) fn new(
        estimated_tokens: Option<u64>,
        model_input_limit: Option<u64>,
        source: ContextUsageSource,
    ) -> Self {
        let model_input_limit = model_input_limit.filter(|limit| *limit > 0);
        let known = estimated_tokens.zip(model_input_limit);
        let percent_used = known.map(|(estimate, limit)| {
            (u128::from(estimate) * 100 + u128::from(limit) / 2) / u128::from(limit)
        });
        let fill_fraction = known.map_or(0.0, |(estimate, limit)| {
            ((estimate as f64) / (limit as f64)).min(1.0) as f32
        });
        Self {
            estimated_tokens,
            model_input_limit,
            percent_used,
            fill_fraction,
            source,
        }
    }

    pub(crate) fn percentage_label(&self) -> Option<String> {
        self.percentage_value().map(|percent| match self.source {
            ContextUsageSource::EstimatedInput => {
                vega_i18n::t_args("ui.estimated_usage_percent", &[("percent", &percent)])
            }
            ContextUsageSource::LastCallTotal => {
                vega_i18n::t_args("ui.last_call_usage_percent", &[("percent", &percent)])
            }
        })
    }

    pub(crate) fn usage_label(&self) -> &'static str {
        match self.source {
            ContextUsageSource::EstimatedInput => {
                vega_i18n::t("ui.estimated_usage_configured_input_limit")
            }
            ContextUsageSource::LastCallTotal => {
                vega_i18n::t("ui.last_call_total_configured_input_limit")
            }
        }
    }

    pub(crate) fn compact_usage_label(&self) -> String {
        let Some(estimate) = self.estimated_tokens else {
            return vega_i18n::t("ui.no_data_yet").into();
        };
        match self.model_input_limit {
            Some(limit) => vega_i18n::t_args(
                "ui.token_usage",
                &[
                    ("used", &compact_token_count(estimate)),
                    ("limit", &compact_token_count(limit)),
                ],
            ),
            None => vega_i18n::t_args(
                "ui.token_count",
                &[("tokens", &compact_token_count(estimate))],
            ),
        }
    }

    pub(crate) fn estimate_label(&self) -> String {
        let Some(estimate) = self.estimated_tokens else {
            return vega_i18n::t("ui.estimated_input_no_data_yet").into();
        };
        match self.source {
            ContextUsageSource::EstimatedInput => vega_i18n::t_args(
                "ui.estimated_input_tokens_tokens_including_the_system_prompt",
                &[("tokens", &grouped_token_count(estimate))],
            ),
            ContextUsageSource::LastCallTotal => vega_i18n::t_args(
                "ui.last_call_total_tokens_tokens_input_output",
                &[("tokens", &grouped_token_count(estimate))],
            ),
        }
    }

    pub(crate) fn percentage_value(&self) -> Option<String> {
        self.estimated_tokens
            .zip(self.model_input_limit)
            .map(|(estimate, limit)| {
                let tenths =
                    (u128::from(estimate) * 1000 + u128::from(limit) / 2) / u128::from(limit);
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
            self.estimate_label(),
        ];
        if let Some(percentage) = self.percentage_label() {
            parts.push(percentage);
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
