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
