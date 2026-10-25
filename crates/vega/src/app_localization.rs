use std::collections::HashMap;
use std::sync::LazyLock;

static MESSAGES: LazyLock<HashMap<String, &'static str>> = LazyLock::new(|| {
    let mut messages = HashMap::new();
    for source in [
        include_str!("../../vega_i18n/locales/app.en.toml"),
        include_str!("../../vega_i18n/locales/app.zh-CN.toml"),
    ] {
        for line in source.lines() {
            let Some((key, value)) = line.split_once(" = ") else {
                continue;
            };
            let Some(key) = key.strip_prefix('"').and_then(|key| key.strip_suffix('"')) else {
                continue;
            };
            if (key.starts_with("app.update.")
                || key.starts_with("app.navigation.")
                || matches!(key, "app.setup.pending" | "app.setup.task_unavailable"))
                && let Ok(value) = serde_json::from_str::<String>(value)
            {
                messages.insert(value, key);
            }
        }
    }
    messages
});

const SYSTEM_ERROR_KEYS: &[&str] = &[
    "app.automation.cancelled",
    "app.automation.credential_check",
    "app.automation.credentials_in_task",
    "app.automation.goal_usage",
    "app.automation.loop_usage",
    "app.automation.native_commands",
    "app.automation.native_only",
    "app.automation.no_attachments",
    "app.automation.not_set",
    "app.automation.pause_failed",
    "app.automation.read_failed",
    "app.automation.read_reopen",
    "app.automation.recovery_failed",
    "app.automation.save_failed",
    "app.automation.saving",
    "app.automation.start_failed",
    "app.automation.store_reopen",
    "app.automation.store_unavailable",
    "app.catalog.budget_invalid",
    "app.catalog.cache_create",
    "app.catalog.cache_format",
    "app.catalog.cache_integrity",
    "app.catalog.cache_limit",
    "app.catalog.cache_save",
    "app.catalog.cache_unavailable",
    "app.catalog.cache_write",
    "app.catalog.effort_invalid",
    "app.catalog.effort_limit",
    "app.catalog.encoding",
    "app.catalog.fetch_failed",
    "app.catalog.format_invalid",
    "app.catalog.mapping_format",
    "app.catalog.mapping_limit",
    "app.catalog.mapping_target",
    "app.catalog.model_identity",
    "app.catalog.network",
    "app.catalog.path_invalid",
    "app.catalog.path_unavailable",
    "app.catalog.price_format",
    "app.catalog.provider_count",
    "app.catalog.provider_identity",
    "app.catalog.reasoning_invalid",
    "app.catalog.response_limit",
    "app.catalog.response_read",
    "app.catalog.service_unavailable",
    "app.error.config_read",
    "app.error.credentials_read",
    "app.error.provider_model",
    "app.error.provider_required",
    "app.error.session_unavailable",
    "app.model_context.database_unavailable",
    "app.model_context.read_failed",
    "app.model_context.save_failed",
    "app.pending.answer_first",
    "app.pending.credentials_in_message",
    "app.pending.credentials_read",
    "app.pending.default_save",
    "app.pending.finished",
    "app.pending.plan_read",
    "app.pending.read_failed",
    "app.pending.read_reopen",
    "app.pending.resolve_plan",
    "app.pending.running",
    "app.pending.save_failed",
    "app.pending.state_read",
    "app.pending.unavailable",
    "app.privacy.changed",
    "app.privacy.read_failed",
    "app.privacy.read_retry",
    "app.privacy.wait",
    "domain.automation.error.accounting",
    "domain.automation.error.archived",
    "domain.automation.error.budget",
    "domain.automation.error.budget_reset",
    "domain.automation.error.cancelled",
    "domain.automation.error.changed",
    "domain.automation.error.deleted",
    "domain.automation.error.encode",
    "domain.automation.error.ended",
    "domain.automation.error.exists",
    "domain.automation.error.instruction",
    "domain.automation.error.invalid",
    "domain.automation.error.missing",
    "domain.automation.error.native",
    "domain.automation.error.processing",
    "domain.automation.error.read",
    "domain.automation.error.restore",
    "domain.automation.error.revision",
    "domain.automation.error.revision_limit",
    "domain.automation.error.running",
    "domain.automation.error.save",
    "domain.automation.error.store_unavailable",
    "domain.automation.error.usage_limit",
    "domain.automation.parse.budget",
    "domain.automation.parse.instruction",
    "domain.automation.parse.instruction_size",
    "domain.automation.parse.interval",
    "domain.automation.parse.interval_range",
    "domain.automation.parse.unknown_option",
    "domain.conversation.unsupported_historical_consumer",
    "domain.pending.error.attachments",
    "domain.pending.error.changed",
    "domain.pending.error.delivered",
    "domain.pending.error.delivering",
    "domain.pending.error.full",
    "domain.pending.error.prepare",
    "domain.pending.error.save",
    "domain.privacy.changed",
    "domain.privacy.cleanup_failed",
    "domain.privacy.exception_failed",
    "domain.privacy.findings_changed",
    "domain.privacy.nothing",
    "domain.privacy.too_many",
    "domain.privacy.unavailable",
];

const STORE_ERROR_DETAILS: &[&str] = &[
    "domain.pending.error.attachments",
    "domain.pending.error.changed",
    "domain.pending.error.delivered",
    "domain.pending.error.delivering",
    "domain.pending.error.full",
    "domain.pending.error.prepare",
    "domain.pending.error.save",
    "domain.privacy.changed",
    "domain.privacy.cleanup_failed",
    "domain.privacy.exception_failed",
    "domain.privacy.findings_changed",
    "domain.privacy.nothing",
    "domain.privacy.too_many",
    "domain.privacy.unavailable",
];

#[derive(Clone, Copy)]
enum SystemErrorCopy {
    Key(&'static str),
    Store(&'static str),
}

impl SystemErrorCopy {
    fn localized(self) -> vega_ui::localized_copy::LocalizedCopy {
        use vega_ui::localized_copy::LocalizedCopy;
        match self {
            Self::Key(key) => LocalizedCopy::new(key),
            Self::Store(key) => LocalizedCopy::with_args(
                "domain.conversation.store",
                vec![("detail", LocalizedCopy::new(key))],
            ),
        }
    }
}

static SYSTEM_ERRORS: LazyLock<HashMap<String, SystemErrorCopy>> = LazyLock::new(|| {
    let catalogs = [
        include_str!("../../vega_i18n/locales/app.en.toml"),
        include_str!("../../vega_i18n/locales/app.zh-CN.toml"),
        include_str!("../../vega_i18n/locales/domain.en.toml"),
        include_str!("../../vega_i18n/locales/domain.zh-CN.toml"),
    ]
    .map(|source| {
        source
            .lines()
            .filter_map(|line| {
                let (key, value) = line.split_once(" = ")?;
                let key = key.strip_prefix('"')?.strip_suffix('"')?;
                let value = serde_json::from_str::<String>(value).ok()?;
                Some((key, value))
            })
            .collect::<HashMap<_, _>>()
    });
    let mut messages = HashMap::new();
    for &key in SYSTEM_ERROR_KEYS {
        messages.insert(key.into(), SystemErrorCopy::Key(key));
        for catalog in &catalogs {
            if let Some(value) = catalog.get(key) {
                let canonical = if key == "domain.automation.error.encode"
                    && catalog.get("domain.automation.error.save") == Some(value)
                {
                    "domain.automation.error.save"
                } else {
                    key
                };
                messages.insert(value.clone(), SystemErrorCopy::Key(canonical));
            }
        }
    }
    for &key in STORE_ERROR_DETAILS {
        for wrapper_catalog in &catalogs {
            let Some(wrapper) = wrapper_catalog.get("domain.conversation.store") else {
                continue;
            };
            for detail_catalog in &catalogs {
                if let Some(detail) = detail_catalog.get(key) {
                    messages.insert(
                        wrapper.replace("{detail}", detail),
                        SystemErrorCopy::Store(key),
                    );
                }
            }
        }
    }
    messages
});

pub(crate) fn translated(message: &str) -> Option<&'static str> {
    MESSAGES.get(message).map(|key| vega_i18n::t(key))
}

pub(crate) fn localized_error(message: &str) -> vega_ui::localized_copy::LocalizedCopy {
    SYSTEM_ERRORS
        .get(message)
        .copied()
        .map(SystemErrorCopy::localized)
        .unwrap_or_else(|| vega_ui::localized_copy::LocalizedCopy::Text(message.into()))
}

pub(crate) fn translate_error(error: &mut Option<String>) {
    if let Some(message) = error
        && let Some(translated) = SYSTEM_ERRORS.get(message).copied()
    {
        *message = translated.localized().render();
    }
}

pub(crate) fn apply_automation(
    stream: &mut vega_ui::conversation_stream::ConversationStream,
    projection: vega_conversation::types::AutomationProjection,
    cx: &mut gpui_kit::Context<vega_ui::conversation_stream::ConversationStream>,
) {
    let error = projection.error.as_deref().map(localized_error);
    stream.apply_automation_localized(projection, error, cx);
}
