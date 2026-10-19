use crate::VegaError;
use crate::provider::{
    ReasoningChoice, ReasoningDisabledWire, ReasoningProtocol, ReasoningReplayPolicy,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[cfg(test)]
#[path = "chat_compat_catalog_tests.rs"]
mod catalog_tests;

mod catalog;
use catalog::{Catalog, CatalogEntry, Transport};

pub(crate) const RULES_VERSION: &str = "vega.chat.v3.pi-ai-0.84.4.1";
pub const REASONING_EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningSupport {
    Required,
    Optional,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChatThinkingFormat {
    #[default]
    Openai,
    Deepseek,
    Zai,
    Qwen,
    Together,
    Baseten,
    Openrouter,
    #[serde(rename = "ant-ling")]
    AntLing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatCompatibility {
    pub protocol: ReasoningProtocol,
    pub replay_policy: ReasoningReplayPolicy,
    pub support: ReasoningSupport,
    pub efforts: Vec<String>,
    pub supports_disabled: bool,
    pub disabled_wire: Option<ReasoningDisabledWire>,
    pub supports_enabled: bool,
    pub profile_id: String,
    pub(crate) format: ChatThinkingFormat,
    pub(crate) transport: Transport,
    pub(crate) known_model: bool,
    pub(crate) reasoning: bool,
    pub(crate) fallback: bool,
    pub(crate) effort_map: BTreeMap<String, String>,
    pub(crate) off_effort: Option<String>,
    pub(crate) template_enabled: bool,
    pub(crate) reasoning_source_override: bool,
}

impl ChatCompatibility {
    pub fn supports_choice(&self, choice: &ReasoningChoice) -> bool {
        match choice {
            ReasoningChoice::ProviderDefault => true,
            ReasoningChoice::Enabled => self.supports_enabled,
            ReasoningChoice::Disabled => self.supports_disabled,
            ReasoningChoice::Effort(effort) => self.efforts.contains(effort),
        }
    }

    fn identify(&mut self) {
        self.profile_id.clear();
        let mut digest = Sha256::new();
        digest.update(RULES_VERSION.as_bytes());
        digest.update([u8::from(self.fallback)]);
        digest.update(format!("{self:?}").as_bytes());
        self.profile_id = format!("{:x}", digest.finalize());
    }
}

pub(crate) fn protocol_error() -> VegaError {
    VegaError::ProviderDiagnostic {
        kind: crate::ProviderFailureKind::Protocol,
        status: None,
        message: "Chat Completions protocol data is invalid or unsupported".into(),
        retryable: false,
        retry_count: None,
        request_id: None,
    }
}

fn decode_catalog(bytes: &[u8]) -> Result<Catalog, VegaError> {
    catalog::decode(bytes)
}

fn catalog() -> Result<&'static Catalog, VegaError> {
    static DATA: OnceLock<Result<Catalog, ()>> = OnceLock::new();
    DATA.get_or_init(|| {
        decode_catalog(include_bytes!("chat_compat/pi-ai-0.84.4.json")).map_err(|_| ())
    })
    .as_ref()
    .map_err(|_| protocol_error())
}

pub fn chat_compatibility(base_url: Option<&str>, model: &str) -> Option<ChatCompatibility> {
    resolve(base_url, model).ok()
}

pub(crate) fn resolve(base_url: Option<&str>, model: &str) -> Result<ChatCompatibility, VegaError> {
    let data = catalog()?;
    let (service, entry) = data.find(base_url, model);
    let mut transport = Transport::for_service(service, model);
    data.apply_service_transport(service, &mut transport);
    if let Some(entry) = entry {
        entry.compat.apply_transport(&mut transport);
    }
    let format = entry
        .and_then(|entry| entry.compat.thinking_format)
        .unwrap_or_else(|| data.service_format(service));
    let mut profile = ChatCompatibility {
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        replay_policy: ReasoningReplayPolicy::StandardChat,
        support: ReasoningSupport::Optional,
        efforts: Vec::new(),
        supports_disabled: false,
        disabled_wire: None,
        supports_enabled: false,
        profile_id: String::new(),
        format,
        transport,
        known_model: entry.is_some(),
        reasoning: entry.is_some_and(|entry| entry.reasoning),
        fallback: entry.is_none(),
        effort_map: BTreeMap::new(),
        off_effort: None,
        template_enabled: false,
        reasoning_source_override: service == Some("opencode-go"),
    };
    if let Some(entry) = entry {
        apply_model(&mut profile, entry, service);
    } else {
        profile.efforts = REASONING_EFFORTS
            .iter()
            .map(|effort| (*effort).into())
            .collect();
        profile.effort_map = profile
            .efforts
            .iter()
            .map(|effort| (effort.clone(), effort.clone()))
            .collect();
        if profile.format == ChatThinkingFormat::Baseten {
            profile.format = ChatThinkingFormat::Openai;
        }
    }
    if service == Some("deepseek") && model == "deepseek-flash" {
        profile.known_model = true;
        profile.fallback = false;
        profile.reasoning = true;
        profile.format = ChatThinkingFormat::Deepseek;
        profile.efforts = ["low", "high", "max"]
            .into_iter()
            .map(str::to_string)
            .collect();
        profile.effort_map = profile
            .efforts
            .iter()
            .map(|effort| (effort.clone(), effort.clone()))
            .collect();
        profile.supports_disabled = true;
    }
    if service == Some("deepseek") && profile.known_model {
        profile.protocol = ReasoningProtocol::DeepSeekChatCompletions;
        profile.replay_policy = ReasoningReplayPolicy::DeepSeekTools;
    }
    if matches!(service, Some("zai" | "zai-coding-cn"))
        && matches!(model, "glm-4.7" | "glm-5.3" | "glm-5.3-flash")
    {
        profile.protocol = ReasoningProtocol::ZhipuChatCompletions;
        profile.replay_policy = ReasoningReplayPolicy::GlmPreserved;
        if model != "glm-4.7" {
            profile.support = ReasoningSupport::Required;
            profile.supports_disabled = false;
        }
    }
    if model == "step-5-preview" {
        profile.efforts = ["low", "medium", "high"]
            .into_iter()
            .map(str::to_string)
            .collect();
        profile.effort_map = profile
            .efforts
            .iter()
            .map(|effort| (effort.clone(), effort.clone()))
            .collect();
        profile.supports_disabled = false;
        profile.supports_enabled = false;
        if service.is_none() {
            profile.known_model = true;
            profile.reasoning = true;
            profile.fallback = false;
        }
    }
    profile.disabled_wire = profile
        .supports_disabled
        .then_some(ReasoningDisabledWire::Resolved);
    profile.identify();
    Ok(profile)
}

fn apply_model(profile: &mut ChatCompatibility, entry: &CatalogEntry, service: Option<&str>) {
    if !entry.reasoning {
        profile.support = ReasoningSupport::Unsupported;
        return;
    }
    let format = profile.format;
    let boolean = matches!(
        format,
        ChatThinkingFormat::Deepseek
            | ChatThinkingFormat::Zai
            | ChatThinkingFormat::Qwen
            | ChatThinkingFormat::Together
            | ChatThinkingFormat::Baseten
    );
    let effort = matches!(
        format,
        ChatThinkingFormat::Openrouter | ChatThinkingFormat::AntLing
    ) || entry
        .compat
        .supports_reasoning_effort
        .unwrap_or_else(|| catalog::default_effort(service));
    profile.template_enabled = entry
        .compat
        .chat_template_args
        .as_ref()
        .is_some_and(|args| args.enable_thinking.is_some());
    let boolean = boolean && (format != ChatThinkingFormat::Baseten || profile.template_enabled);
    profile.supports_enabled = boolean && !effort;
    if effort {
        for name in REASONING_EFFORTS {
            let mapped = entry.thinking_level_map.get(*name);
            let value = match mapped {
                Some(Some(value)) => Some(value.as_str()),
                None if !matches!(*name, "xhigh" | "max")
                    && format != ChatThinkingFormat::AntLing =>
                {
                    Some(*name)
                }
                _ => None,
            };
            if let Some(value) = value {
                profile.efforts.push((*name).into());
                profile.effort_map.insert((*name).into(), value.into());
            }
        }
    }
    profile.off_effort = entry.thinking_level_map.get("off").and_then(Option::clone);
    profile.supports_disabled = match format {
        ChatThinkingFormat::Openai => effort && profile.off_effort.is_some(),
        ChatThinkingFormat::AntLing => false,
        ChatThinkingFormat::Openrouter => entry.thinking_level_map.get("off") != Some(&None),
        _ => boolean && entry.thinking_level_map.get("off") != Some(&None),
    };
    if entry
        .compat
        .requires_reasoning_content_on_assistant_messages
        == Some(true)
    {
        profile.replay_policy = ReasoningReplayPolicy::DeepSeekTools;
        profile.protocol = ReasoningProtocol::DeepSeekChatCompletions;
    }
}
