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

pub(crate) fn pricing_status(label: &'static str, color: gpui::Rgba) -> Div {
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
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut gpui::App) + 'static,
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

pub(crate) fn section_title(label: &'static str, color: gpui::Rgba) -> Div {
    div()
        .text_size(px(Typography::HEADING_BLOCK))
        .font_weight(Typography::HEADING_BLOCK_WEIGHT)
        .text_color(color)
        .child(label)
}

pub(crate) fn field_label(label: &'static str, color: gpui::Rgba) -> Div {
    div()
        .w(px(72.))
        .text_color(color)
        .text_size(px(Typography::BODY))
        .child(label)
}

/// Whether the add-provider form may be submitted: name, base_url, and key
/// must all be non-empty (name/base_url trimmed). Empty fields keep the
/// submit button inert (ui-spec §4.6: no error modal).
pub(crate) fn form_is_submittable(name: &str, base_url: &str, key: &str) -> bool {
    !name.trim().is_empty() && !base_url.trim().is_empty() && !key.is_empty()
}

/// Inserts `entry` into `providers`, appending when no provider with the same
/// name exists and replacing the existing one otherwise (the form does not
/// edit models, so a replacement keeps the stored models). Returns whether an
/// existing entry was replaced.
pub(crate) fn upsert_provider(providers: &mut Vec<ProviderConfig>, entry: ProviderConfig) -> bool {
    if let Some(existing) = providers
        .iter_mut()
        .find(|provider| provider.name == entry.name)
    {
        let models = std::mem::take(&mut existing.models);
        *existing = entry;
        existing.models = models;
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
    for provider in providers {
        for model in &provider.models {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
    }
    models
}
