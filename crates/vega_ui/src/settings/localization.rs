use super::*;

#[derive(Clone)]
pub(super) enum SettingsText {
    Key(&'static str),
    Args {
        key: &'static str,
        args: Vec<(&'static str, String)>,
    },
    Provider(vega_conversation::types::ProviderSettingsError),
    UnsavedProvider(vega_conversation::types::ProviderSettingsError),
    Models(ProviderModelsError),
    Plain(String),
}

impl SettingsText {
    pub(super) fn key(key: &'static str) -> Self {
        Self::Key(key)
    }

    pub(super) fn args(key: &'static str, args: &[(&'static str, &str)]) -> Self {
        Self::Args {
            key,
            args: args
                .iter()
                .map(|(name, value)| (*name, (*value).to_string()))
                .collect(),
        }
    }

    pub(super) fn from_known(value: String, keys: &[&'static str]) -> Self {
        keys.iter()
            .copied()
            .find(|key| t(key) == value)
            .map_or_else(|| Self::Plain(value), Self::Key)
    }

    pub(super) fn is_key(&self, key: &str) -> bool {
        matches!(self, Self::Key(value) if *value == key)
    }

    pub(super) fn render(&self) -> String {
        match self {
            Self::Key(key) => t(key).to_string(),
            Self::Args { key, args } => t_args(
                key,
                &args
                    .iter()
                    .map(|(name, value)| (*name, value.as_str()))
                    .collect::<Vec<_>>(),
            ),
            Self::Provider(error) => error.to_string(),
            Self::UnsavedProvider(error) => t_args(
                "settings.provider.unsaved_error",
                &[("error", &error.to_string())],
            ),
            Self::Models(error) => provider_models_error_label(error),
            Self::Plain(value) => value.clone(),
        }
    }
}

impl From<String> for SettingsText {
    fn from(value: String) -> Self {
        Self::Plain(value)
    }
}

impl From<&'static str> for SettingsText {
    fn from(value: &'static str) -> Self {
        Self::Plain(value.to_string())
    }
}

impl SettingsView {
    pub(super) fn refresh_localized_inputs(&self, cx: &mut Context<Self>) {
        for input in [
            &self.name_input,
            &self.base_url_input,
            &self.models_input,
            &self.key_input,
            &self.codex_name_input,
            &self.codex_executable_input,
            &self.codex_args_input,
            &self.pricing_model_input,
        ]
        .into_iter()
        .chain(self.pricing_rate_inputs.iter())
        {
            input.update(cx, |_, cx| cx.notify());
        }
        self.refresh_catalog_localization(cx);
        self.refresh_provider_localization(cx);
        self.refresh_mcp_localization(cx);
    }
}

pub(super) fn permission_mode_label(mode: &str) -> &'static str {
    match mode {
        "readonly" => t("settings.permission.readonly"),
        "confirm" => t("settings.permission.confirm"),
        "auto" => t("settings.permission.auto"),
        "full_access" => t("settings.permission.full_access"),
        _ => t("settings.state.unknown"),
    }
}
