#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContextUsageDisplay {
    pub(crate) estimated_tokens: Option<u64>,
    pub(crate) model_input_limit: Option<u64>,
    pub(crate) percent_used: Option<u128>,
    pub(crate) fill_fraction: f32,
}

impl ContextUsageDisplay {
    pub(crate) fn new(estimated_tokens: Option<u64>, model_input_limit: Option<u64>) -> Self {
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
        }
    }

    pub(crate) fn percentage_label(&self) -> Option<String> {
        self.percentage_value()
            .map(|percent| format!("估算使用量：{percent}"))
    }

    pub(crate) fn compact_usage_label(&self) -> String {
        let Some(estimate) = self.estimated_tokens else {
            return "尚无数据".into();
        };
        match self.model_input_limit {
            Some(limit) => format!(
                "{} / {} tokens",
                compact_token_count(estimate),
                compact_token_count(limit)
            ),
            None => format!("{} tokens", compact_token_count(estimate)),
        }
    }

    pub(crate) fn estimate_label(&self) -> String {
        let Some(estimate) = self.estimated_tokens else {
            return "估算输入：尚无数据".into();
        };
        format!(
            "估算输入：{} tokens（含系统提示和实际工具定义）",
            grouped_token_count(estimate)
        )
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
            || "容量未配置".to_string(),
            |limit| {
                format!(
                    "当前模型设置输入上限：{} tokens（非供应商验证容量）",
                    grouped_token_count(limit)
                )
            },
        )
    }

    pub(crate) fn accessibility_label(&self) -> String {
        let mut parts = vec!["上下文用量".to_string(), self.estimate_label()];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue64_context_usage_known_capacity_formats_estimate_and_rounded_percent() {
        let display = ContextUsageDisplay::new(Some(135_000), Some(258_000));
        assert_eq!(display.percent_used, Some(52));
        assert_eq!(display.percentage_value().as_deref(), Some("52.3%"));
        assert_eq!(display.fill_fraction, 135_000.0 / 258_000.0);
        assert_eq!(display.compact_usage_label(), "135k / 258k tokens");
        assert_eq!(
            display.percentage_label().as_deref(),
            Some("估算使用量：52.3%")
        );
        assert!(display.estimate_label().contains("135,000 tokens"));
        assert!(display.capacity_label().contains("258,000 tokens"));
        assert!(display.capacity_label().contains("当前模型设置输入上限"));
        assert!(display.accessibility_label().contains("非供应商验证容量"));
    }

    #[test]
    fn issue64_context_usage_unknown_or_zero_capacity_has_no_percent_or_fill() {
        for model_input_limit in [None, Some(0)] {
            let display = ContextUsageDisplay::new(Some(135_000), model_input_limit);
            assert_eq!(display.model_input_limit, None);
            assert_eq!(display.percent_used, None);
            assert_eq!(display.fill_fraction, 0.0);
            assert_eq!(display.compact_usage_label(), "135k tokens");
            assert_eq!(display.capacity_label(), "容量未配置");
            assert!(!display.accessibility_label().contains('%'));
        }
    }

    #[test]
    fn issue64_context_usage_over_limit_preserves_percent_and_caps_fill() {
        let display = ContextUsageDisplay::new(Some(5), Some(2));
        assert_eq!(display.percent_used, Some(250));
        assert_eq!(display.fill_fraction, 1.0);
        assert_eq!(
            display.percentage_label().as_deref(),
            Some("估算使用量：250%")
        );
    }

    #[test]
    fn issue64_context_usage_rounding_and_extreme_integer_values_are_stable() {
        let half_percent = ContextUsageDisplay::new(Some(1), Some(8));
        assert_eq!(half_percent.percent_used, Some(13));
        let zero = ContextUsageDisplay::new(Some(0), Some(1));
        assert_eq!(zero.percent_used, Some(0));
        assert_eq!(zero.fill_fraction, 0.0);
        assert_eq!(zero.percentage_value().as_deref(), Some("0%"));
        let extreme = ContextUsageDisplay::new(Some(u64::MAX), Some(1));
        assert_eq!(extreme.percent_used, Some(u128::from(u64::MAX) * 100));
        assert_eq!(extreme.fill_fraction, 1.0);
        assert!(
            extreme
                .accessibility_label()
                .contains("18,446,744,073,709,551,615")
        );
        assert_eq!(compact_token_count(u64::MAX), "18446744073709551.6k");
    }

    #[test]
    fn issue64_context_usage_without_an_estimate_keeps_unknown_total() {
        let display = ContextUsageDisplay::new(None, Some(258_000));
        assert_eq!(display.estimated_tokens, None);
        assert_eq!(display.percent_used, None);
        assert_eq!(display.compact_usage_label(), "尚无数据");
        assert_eq!(display.percentage_value(), None);
    }
}
