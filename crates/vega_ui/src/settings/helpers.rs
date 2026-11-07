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
    label: &'static str,
    description: Option<String>,
    control: impl IntoElement,
    colors: vega_theme::ThemeColors,
) -> Div {
    div()
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

pub(crate) fn pricing_error_label(code: PricingSettingsErrorCode) -> &'static str {
    match code {
        PricingSettingsErrorCode::Io => t("settings.pricing.error.io"),
        PricingSettingsErrorCode::MalformedCatalog => t("settings.pricing.error.malformed"),
        PricingSettingsErrorCode::LockedProfile => t("settings.pricing.error.locked"),
        PricingSettingsErrorCode::InvalidInput => t("settings.pricing.error.invalid"),
        PricingSettingsErrorCode::ModelNotPriced => t("settings.pricing.error.unpriced"),
        PricingSettingsErrorCode::TargetChanged => t("settings.pricing.error.conflict"),
        PricingSettingsErrorCode::RecoveryRequired => t("settings.pricing.error.recovery"),
        PricingSettingsErrorCode::Busy => t("settings.pricing.error.busy"),
        PricingSettingsErrorCode::LimitExceeded => t("settings.pricing.error.limit"),
    }
}

pub(crate) fn pricing_notice_label(notice: PricingNotice) -> &'static str {
    match notice {
        PricingNotice::DurabilityUnknownReconciled => t("settings.pricing.notice.durability"),
        PricingNotice::ExternalWinnerAdopted => t("settings.pricing.notice.external"),
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
        ProviderModelsError::Empty => t("settings.provider.models.empty").to_string(),
        ProviderModelsError::InputTooLarge => t_args(
            "settings.provider.models.too_large",
            &[(
                "limit",
                &(PROVIDER_MODELS_INPUT_BYTES_LIMIT / 1024).to_string(),
            )],
        ),
        ProviderModelsError::TooMany { line } => t_args(
            "settings.provider.models.too_many",
            &[
                ("line", &line.to_string()),
                ("limit", &PROVIDER_MODEL_COUNT_MAX.to_string()),
            ],
        ),
        ProviderModelsError::TooLong { line } => t_args(
            "settings.provider.models.too_long",
            &[
                ("line", &line.to_string()),
                ("limit", &PROVIDER_MODEL_ID_MAX_BYTES.to_string()),
            ],
        ),
        ProviderModelsError::Duplicate { line } => t_args(
            "settings.provider.models.duplicate",
            &[("line", &line.to_string())],
        ),
        ProviderModelsError::Invalid { line } => t_args(
            "settings.provider.models.invalid",
            &[("line", &line.to_string())],
        ),
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
