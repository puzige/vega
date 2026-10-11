use super::*;
use crate::interaction::InteractionExt;

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

pub(crate) fn settings_section() -> Div {
    div()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(Layout::SETTINGS_CONTENT_GAP))
}

pub(crate) fn settings_group(colors: vega_theme::ThemeColors) -> Div {
    settings_section()
        .gap_0()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .rounded(px(Layout::PANEL_RADIUS))
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.bg_subtle)
}

pub(crate) fn settings_field(
    id: impl Into<String>,
    label: &'static str,
    description: Option<String>,
    control: impl IntoElement,
    colors: vega_theme::ThemeColors,
) -> Div {
    let id = id.into();
    let label_id = format!("{id}-label");
    let control_id = format!("{id}-control");
    div()
        .debug_selector(move || id.clone())
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .items_start()
        .gap_x(px(Layout::SETTINGS_FIELD_GAP))
        .gap_y_2()
        .py(px(Layout::SETTINGS_ROW_PADDING_Y))
        .border_b_1()
        .border_color(colors.border_subtle)
        .child(
            div()
                .debug_selector(move || label_id.clone())
                .w(px(Layout::SETTINGS_LABEL_WIDTH))
                .max_w_full()
                .flex_shrink_0()
                .py_1()
                .flex()
                .flex_col()
                .gap_1()
                .text_size(px(Typography::BODY))
                .child(label)
                .when_some(description, |view, description| {
                    view.child(
                        div()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child(description),
                    )
                }),
        )
        .child(
            div()
                .debug_selector(move || control_id.clone())
                .flex_1()
                .flex_basis(px(220.))
                .min_w_0()
                .max_w_full()
                .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                .flex()
                .flex_col()
                .justify_center()
                .child(control),
        )
}

pub(crate) fn settings_switch(
    id: impl Into<String>,
    label: impl Into<gpui_kit::SharedString>,
    value: bool,
    enabled: bool,
    colors: vega_theme::ThemeColors,
) -> crate::interaction::Interaction {
    let id = id.into();
    div()
        .id(id.clone())
        .interaction_surface(
            if value {
                colors.brand_primary
            } else {
                colors.bg_hover
            },
            if value {
                colors.brand_primary_strong
            } else {
                colors.border_subtle
            },
            if value {
                colors.brand_primary_strong
            } else {
                colors.bg_active
            },
            enabled,
        )
        .debug_selector(move || id.clone())
        .aria_label(label)
        .w(px(Layout::SETTINGS_SWITCH_WIDTH))
        .h(px(Layout::SETTINGS_SWITCH_HEIGHT))
        .flex_shrink_0()
        .p(px(1.))
        .rounded_full()
        .border_1()
        .border_color(if value {
            colors.brand_primary
        } else {
            colors.border_subtle
        })
        .flex()
        .items_center()
        .when(value, |switch| switch.justify_end())
        .when(!value, |switch| switch.justify_start())
        .when(enabled, |switch| {
            switch.focusable().tab_stop(true).cursor_pointer()
        })
        .when(!enabled, |switch| switch.opacity(0.5))
        .focus_visible(move |style| style.border_color(colors.text_primary))
        .child(
            div()
                .size(px(Layout::ICON_SIZE))
                .flex_shrink_0()
                .rounded_full()
                .bg(colors.bg_elevated),
        )
}

pub(crate) fn settings_actions() -> Div {
    div().min_w_0().flex().flex_wrap().items_center().gap_2()
}

pub(crate) fn settings_action_chrome(colors: vega_theme::ThemeColors) -> Div {
    div()
        .min_w_0()
        .max_w_full()
        .self_start()
        .flex_shrink_0()
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py_1()
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.bg_elevated)
        .text_size(px(Typography::BODY))
        .line_height(relative(Typography::BODY_LINE_HEIGHT))
        .text_color(colors.text_primary)
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
}

pub(crate) fn settings_status(
    label: impl Into<String>,
    color: gpui_kit::Rgba,
    colors: vega_theme::ThemeColors,
) -> Div {
    let icon = if color == colors.danger || color == colors.warning {
        crate::icons::Icon::Warning
    } else if color == colors.success {
        crate::icons::Icon::Check
    } else {
        crate::icons::Icon::Summary
    };
    div()
        .debug_selector(|| "settings-status".into())
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .items_start()
        .gap_2()
        .p_2()
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_1()
        .border_color(colors.border_subtle)
        .text_size(px(Typography::BODY))
        .line_height(relative(Typography::BODY_LINE_HEIGHT))
        .text_color(color)
        .child(
            div()
                .debug_selector(|| "settings-status-icon-line".into())
                .w(px(Layout::ICON_SIZE))
                .h(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .debug_selector(|| "settings-status-icon".into())
                        .size(px(Layout::ICON_SIZE))
                        .child(crate::icons::icon(icon, color)),
                ),
        )
        .child(
            div()
                .debug_selector(|| "settings-status-text".into())
                .min_w_0()
                .flex_1()
                .child(label.into()),
        )
}

pub(crate) fn action_button(
    label: &'static str,
    colors: vega_theme::ThemeColors,
    focus: Option<FocusHandle>,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
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
) -> AnyElement {
    let enabled = focus.is_some();
    let id = focus
        .as_ref()
        .map(gpui_kit::ElementId::from)
        .unwrap_or_else(|| label.clone().into());
    let destructive = label == "删除";
    let primary = label.starts_with("保存");
    settings_action_chrome(colors)
        .id(id)
        .interaction_surface(
            if enabled && primary {
                colors.brand_primary
            } else {
                gpui_kit::transparent_black().into()
            },
            if primary {
                colors.brand_primary_strong
            } else {
                colors.bg_hover
            },
            if primary {
                colors.brand_primary_strong
            } else {
                colors.bg_active
            },
            enabled,
        )
        .border_color(if destructive || primary {
            colors.border_subtle
        } else {
            gpui_kit::transparent_black().into()
        })
        .when_some(focus, |button, focus| button.track_focus(&focus))
        .focus_visible(move |style| style.border_color(colors.accent))
        .when(enabled && destructive, |button| {
            button.text_color(colors.danger)
        })
        .when(enabled && primary, |button| {
            button
                .bg(colors.brand_primary)
                .text_color(colors.brand_on_accent)
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .on_mouse_up(MouseButton::Left, listener)
        })
        .when(!enabled, |button| button.text_color(colors.text_tertiary))
        .child(label)
        .into_any_element()
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
