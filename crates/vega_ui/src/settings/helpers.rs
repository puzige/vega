use super::*;

pub(crate) fn read_rate_inputs(inputs: &[Entity<TextInput>], cx: &App) -> PricingRateInputs {
    PricingRateInputs {
        input_usd_per_million: inputs[0].read(cx).text().to_string(),
        output_usd_per_million: inputs[1].read(cx).text().to_string(),
        cache_read_usd_per_million: inputs[2].read(cx).text().to_string(),
        cache_write_usd_per_million: inputs[3].read(cx).text().to_string(),
    }
}

pub(crate) fn rate_values(rates: &PricingRateInputs) -> [&str; 4] {
    [
        &rates.input_usd_per_million,
        &rates.output_usd_per_million,
        &rates.cache_read_usd_per_million,
        &rates.cache_write_usd_per_million,
    ]
}

pub(crate) fn pricing_mutation_input_bytes(mutation: &PricingMutation) -> Option<usize> {
    fn add_rates(total: usize, rates: &PricingRateInputs) -> Option<usize> {
        rate_values(rates)
            .into_iter()
            .try_fold(total, |total, value| total.checked_add(value.len()))
    }
    match mutation {
        PricingMutation::AddCustom { model, rates }
        | PricingMutation::UpdateCustom { model, rates }
        | PricingMutation::UpdateBuiltinBase { model, rates } => add_rates(model.len(), rates),
        PricingMutation::UpdateDeepSeek { model, base, peak } => {
            add_rates(add_rates(model.len(), base)?, peak)
        }
        PricingMutation::ResetBuiltin { model } | PricingMutation::DeleteCustom { model } => {
            Some(model.len())
        }
    }
}

pub(crate) fn pricing_status(label: &'static str, color: gpui_kit::Rgba) -> Div {
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(color)
        .text_color(color)
        .child(label)
}

pub(crate) fn action_button(
    label: &'static str,
    colors: vega_theme::ThemeColors,
    focus: Option<FocusHandle>,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> Div {
    action_button_owned(label.to_string(), colors, focus, listener)
}

/// Button variant for settings controls whose label reflects the current
/// explicit capability value. It shares the same tracked focus and mouse
/// listener behavior as the pricing controls, so keyboard and pointer paths
/// remain one action route.
pub(crate) fn action_button_owned(
    label: String,
    colors: vega_theme::ThemeColors,
    focus: Option<FocusHandle>,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> Div {
    let enabled = focus.is_some();
    div()
        .when_some(focus, |button, focus| button.track_focus(&focus))
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.bg_elevated)
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
                .on_mouse_up(MouseButton::Left, listener)
        })
        .when(!enabled, |button| button.text_color(colors.text_tertiary))
        .child(label)
}

pub(crate) fn pricing_error_label(code: PricingSettingsErrorCode) -> &'static str {
    match code {
        PricingSettingsErrorCode::Io => "定价文件读写失败，请检查数据目录后重新加载",
        PricingSettingsErrorCode::MalformedCatalog => {
            "定价文件已损坏；原文件已保留，请外部修复后重新加载"
        }
        PricingSettingsErrorCode::LockedProfile => "内置模型结构不完整或 metadata 被修改",
        PricingSettingsErrorCode::InvalidInput => "模型 ID 或价格格式无效",
        PricingSettingsErrorCode::ModelNotPriced => "当前模型未配置价格，请先添加后重试",
        PricingSettingsErrorCode::TargetChanged => "定价文件在保存期间变化，请重新加载",
        PricingSettingsErrorCode::RecoveryRequired => "保存结果无法确认，请重新加载恢复",
        PricingSettingsErrorCode::Busy => "已有定价操作正在进行",
        PricingSettingsErrorCode::LimitExceeded => "定价文件或条目超过安全上限",
    }
}

pub(crate) fn pricing_notice_label(notice: PricingNotice) -> &'static str {
    match notice {
        PricingNotice::DurabilityUnknownReconciled => "保存已复验，但目录 durability 曾无法确认",
        PricingNotice::ExternalWinnerAdopted => "保存期间文件发生变化，已采用外部有效版本",
    }
}

pub(crate) fn section_title(label: &'static str, color: gpui_kit::Rgba) -> Div {
    div()
        .text_size(px(Typography::HEADING_BLOCK))
        .font_weight(Typography::HEADING_BLOCK_WEIGHT)
        .text_color(color)
        .child(label)
}

pub(crate) fn field_label(label: &'static str, color: gpui_kit::Rgba) -> Div {
    div()
        .w(px(72.))
        .text_color(color)
        .text_size(px(Typography::BODY))
        .child(label)
}

/// Maximum UTF-8 bytes retained for one provider model ID.
pub(crate) const PROVIDER_MODEL_ID_MAX_BYTES: usize = 200;
/// Maximum number of model IDs accepted in one provider entry.
pub(crate) const PROVIDER_MODEL_COUNT_MAX: usize = 1_000;
/// Maximum UTF-8 bytes accepted by the multiline models field.
pub(crate) const PROVIDER_MODELS_INPUT_BYTES_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderModelsError {
    Empty,
    InputTooLarge,
    TooMany { line: usize },
    TooLong { line: usize },
    Duplicate { line: usize },
    Invalid { line: usize },
}

/// Parses the Settings models field without touching config, local credential store, or the
/// provider. IDs are exact and case-sensitive after line-edge trimming.
pub(crate) fn parse_provider_models(input: &str) -> Result<Vec<String>, ProviderModelsError> {
    if input.len() > PROVIDER_MODELS_INPUT_BYTES_LIMIT {
        return Err(ProviderModelsError::InputTooLarge);
    }

    let mut models = Vec::new();
    for (line_index, raw_line) in input.lines().enumerate() {
        let line = line_index + 1;
        let model = raw_line.trim();
        if model.is_empty() {
            continue;
        }
        if model.len() > PROVIDER_MODEL_ID_MAX_BYTES {
            return Err(ProviderModelsError::TooLong { line });
        }
        if models.len() >= PROVIDER_MODEL_COUNT_MAX {
            return Err(ProviderModelsError::TooMany { line });
        }
        if !is_provider_model_id(model) {
            return Err(ProviderModelsError::Invalid { line });
        }
        if models.iter().any(|existing| existing == model) {
            return Err(ProviderModelsError::Duplicate { line });
        }
        models.push(model.to_string());
    }

    if models.is_empty() {
        return Err(ProviderModelsError::Empty);
    }
    Ok(models)
}

fn is_provider_model_id(model: &str) -> bool {
    let mut bytes = model.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    if !bytes.all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte)) {
        return false;
    }
    !model.contains("..") && !model.contains("//") && !model.ends_with('/')
}

/// User-facing validation copy for the Provider form. It names the failed
/// constraint and never includes arbitrary input or credential values.
pub(crate) fn provider_models_error_label(error: &ProviderModelsError) -> String {
    match error {
        ProviderModelsError::Empty => "请至少填写一个模型 ID".to_string(),
        ProviderModelsError::InputTooLarge => {
            format!(
                "模型列表过大（最多 {} KiB）",
                PROVIDER_MODELS_INPUT_BYTES_LIMIT / 1024
            )
        }
        ProviderModelsError::TooMany { line } => {
            format!("第 {line} 行超出模型数量上限（最多 {PROVIDER_MODEL_COUNT_MAX} 个）")
        }
        ProviderModelsError::TooLong { line } => {
            format!("第 {line} 行模型 ID 过长（最多 {PROVIDER_MODEL_ID_MAX_BYTES} 字节）")
        }
        ProviderModelsError::Duplicate { line } => {
            format!("第 {line} 行模型 ID 与前面重复")
        }
        ProviderModelsError::Invalid { line } => {
            format!("第 {line} 行模型 ID 无效：请使用字母、数字、点、下划线、冒号、斜杠或连字符")
        }
    }
}

/// Whether a new-provider form may be submitted. Existing providers use
/// [`provider_form_is_submittable`] so an empty key can retain its reference.
pub(crate) fn form_is_submittable(name: &str, base_url: &str, key: &str) -> bool {
    !name.trim().is_empty() && !base_url.trim().is_empty() && !key.is_empty()
}

/// Whether a provider form has the required visible fields. An existing
/// provider may leave the key blank because the stored reference is retained.
pub(crate) fn provider_form_is_submittable(
    name: &str,
    base_url: &str,
    key: &str,
    existing: bool,
) -> bool {
    if existing {
        !name.trim().is_empty() && !base_url.trim().is_empty()
    } else {
        form_is_submittable(name, base_url, key)
    }
}

/// Resolves the non-secret local credential store reference for a Provider submission.
pub(crate) fn provider_key_ref(existing: Option<&ProviderConfig>, name: &str, key: &str) -> String {
    if key.is_empty() {
        existing
            .map(|provider| provider.key_ref.clone())
            .unwrap_or_else(|| name.to_string())
    } else {
        name.to_string()
    }
}

/// Inserts `entry` into `providers`, appending when no provider with the same
/// name exists and replacing the existing one otherwise. Returns whether an
/// existing entry was replaced.
#[cfg(test)]
pub(crate) fn upsert_provider(providers: &mut Vec<ProviderConfig>, entry: ProviderConfig) -> bool {
    if let Some(existing) = providers
        .iter_mut()
        .find(|provider| provider.name == entry.name)
    {
        *existing = entry;
        true
    } else {
        providers.push(entry);
        false
    }
}

/// Applies a permission-mode choice, rejecting values outside the fixed set.
pub(crate) fn select_permission_mode(
    config: &mut AppConfig,
    mode: &str,
) -> Result<(), &'static str> {
    if !PERMISSION_MODES.contains(&mode) {
        return Err("unknown permission mode");
    }
    config.defaults.permission_mode = mode.to_string();
    Ok(())
}

/// Sets the default model for new conversations.
pub(crate) fn set_default_model(config: &mut AppConfig, model: &str) {
    config.defaults.model = model.to_string();
}

/// Union of every provider's models in first-seen order, deduplicated.
pub fn all_models(providers: &[ProviderConfig]) -> Vec<String> {
    let mut models = Vec::new();
    for provider in providers.iter().filter(|provider| provider.enabled) {
        for model in &provider.models {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
    }
    models
}
