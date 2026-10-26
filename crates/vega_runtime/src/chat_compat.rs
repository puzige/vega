use crate::model_compat::{ModelApi, ModelDefinition, ModelIdentity, ModelRoute};
use crate::{
    ReasoningChoice, ReasoningDisabledWire, ReasoningProtocol, ReasoningReplayPolicy, VegaError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const RULES_VERSION: &str = crate::model_compat::RULES_VERSION;
pub const REASONING_EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningSupport {
    Required,
    Optional,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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
    #[serde(rename = "string-thinking")]
    StringThinking,
    #[serde(rename = "qwen-chat-template")]
    QwenChatTemplate,
    #[serde(rename = "chat-template")]
    ChatTemplate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Transport {
    pub(crate) developer: bool,
    pub(crate) max_tokens: bool,
    pub(crate) usage: bool,
    pub(crate) strict: bool,
    pub(crate) tool_stream: bool,
    pub(crate) store: bool,
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
    pub(crate) reasoning: bool,
    pub(crate) effort_map: BTreeMap<String, String>,
    pub(crate) off_effort: Option<String>,
    pub(crate) template_args: Option<BTreeMap<String, serde_json::Value>>,
    pub(crate) template_kwargs: Option<BTreeMap<String, serde_json::Value>>,
    pub(crate) thinking_as_text: bool,
}

impl ChatCompatibility {
    pub fn supports_choice(&self, choice: &ReasoningChoice) -> bool {
        match choice {
            ReasoningChoice::ProviderDefault => true,
            ReasoningChoice::Enabled => self.supports_enabled,
            ReasoningChoice::Disabled => self.supports_disabled,
            ReasoningChoice::Effort(v) => self.efforts.contains(v),
        }
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

pub fn chat_compatibility(base_url: Option<&str>, model: &str) -> Option<ChatCompatibility> {
    resolve(base_url, model).ok()
}

pub(crate) fn resolve(base_url: Option<&str>, model: &str) -> Result<ChatCompatibility, VegaError> {
    let route = ModelRoute::new(
        ModelApi::ChatCompletions,
        base_url.ok_or_else(protocol_error)?,
    )?;
    let definition = crate::model_compat::lookup_model_definition(
        &route,
        &ModelIdentity {
            provider: "unconfigured".into(),
            model: model.into(),
        },
    )?;
    Ok(crate::model_compat::resolve_model(&definition, None)?
        .compatibility()
        .clone())
}

pub(crate) fn from_definition(def: &ModelDefinition) -> ChatCompatibility {
    let service = def.service.as_deref();
    let known_service = matches!(
        service,
        Some(
            "ant-ling"
                | "baseten"
                | "cerebras"
                | "cloudflare-ai-gateway"
                | "cloudflare-workers-ai"
                | "deepseek"
                | "fireworks"
                | "github-copilot"
                | "groq"
                | "huggingface"
                | "moonshotai"
                | "moonshotai-cn"
                | "nvidia"
                | "openai"
                | "opencode"
                | "opencode-go"
                | "openrouter"
                | "qwen-token-plan"
                | "qwen-token-plan-cn"
                | "qwen-token-plan-individual"
                | "together"
                | "xai"
                | "xiaomi"
                | "xiaomi-token-plan-ams"
                | "xiaomi-token-plan-cn"
                | "xiaomi-token-plan-sgp"
                | "zai"
                | "zai-coding-cn"
        )
    );
    let nonstandard = matches!(
        service,
        Some(
            "nvidia"
                | "cerebras"
                | "together"
                | "deepseek"
                | "zai"
                | "zai-coding-cn"
                | "moonshotai"
                | "moonshotai-cn"
                | "opencode"
                | "opencode-go"
                | "cloudflare-workers-ai"
                | "cloudflare-ai-gateway"
                | "ant-ling"
        )
    );
    let mut transport = Transport {
        developer: known_service
            && !nonstandard
            && (service != Some("openrouter")
                || def.identity.model.starts_with("openai/")
                || def.identity.model.starts_with("anthropic/")),
        max_tokens: !known_service
            || matches!(
                service,
                Some(
                    "deepseek"
                        | "moonshotai"
                        | "moonshotai-cn"
                        | "cloudflare-ai-gateway"
                        | "together"
                        | "nvidia"
                        | "ant-ling"
                        | "zai"
                        | "zai-coding-cn"
                )
            ),
        usage: true,
        strict: !matches!(
            service,
            Some("moonshotai" | "moonshotai-cn" | "together" | "cloudflare-ai-gateway" | "nvidia")
        ),
        tool_stream: false,
        store: known_service,
    };
    let compat = &def.compat;
    if let Some(value) = compat.supports_developer_role {
        transport.developer = value;
    }
    if let Some(value) = compat.supports_usage_in_streaming {
        transport.usage = value;
    }
    if let Some(value) = compat.supports_strict_mode {
        transport.strict = value;
    }
    if let Some(value) = compat.supports_store {
        transport.store = value;
    }
    if let Some(value) = &compat.max_tokens_field {
        transport.max_tokens = value == "max_tokens";
    }
    if let Some(value) = compat.zai_tool_stream {
        transport.tool_stream = value;
    }
    let mut format = compat.thinking_format.unwrap_or(match service {
        Some("deepseek" | "opencode-go") => ChatThinkingFormat::Deepseek,
        Some("zai" | "zai-coding-cn") => ChatThinkingFormat::Zai,
        Some("together") => ChatThinkingFormat::Together,
        Some("ant-ling") => ChatThinkingFormat::AntLing,
        Some("openrouter") => ChatThinkingFormat::Openrouter,
        Some("qwen-token-plan" | "qwen-token-plan-cn" | "qwen-token-plan-individual") => {
            ChatThinkingFormat::Qwen
        }
        _ => ChatThinkingFormat::Openai,
    });
    if def.route.api == ModelApi::Responses
        || def.reasoning.is_none() && format == ChatThinkingFormat::Baseten
    {
        format = ChatThinkingFormat::Openai;
    }
    let mut facts = ChatCompatibility {
        protocol: if def.route.api == ModelApi::Responses {
            ReasoningProtocol::ResponsesApi
        } else {
            ReasoningProtocol::OpenAiChatCompletions
        },
        replay_policy: if def.route.api == ModelApi::Responses {
            ReasoningReplayPolicy::ResponsesItems
        } else {
            ReasoningReplayPolicy::StandardChat
        },
        support: ReasoningSupport::Optional,
        efforts: Vec::new(),
        supports_disabled: false,
        disabled_wire: None,
        supports_enabled: false,
        profile_id: String::new(),
        format,
        transport,
        reasoning: def.reasoning == Some(true),
        effort_map: BTreeMap::new(),
        off_effort: def.thinking_level_map.get("off").and_then(Clone::clone),
        template_args: compat.chat_template_args.clone(),
        template_kwargs: compat.chat_template_kwargs.clone(),
        thinking_as_text: compat.requires_thinking_as_text == Some(true),
    };
    let fallback = def.reasoning.is_none();
    if def.reasoning == Some(false) {
        facts.support = ReasoningSupport::Unsupported;
        return facts;
    }
    let effort = fallback
        || def.route.api == ModelApi::Responses
        || matches!(
            format,
            ChatThinkingFormat::Openrouter | ChatThinkingFormat::AntLing
        )
        || compat.supports_reasoning_effort.unwrap_or(!matches!(
            service,
            Some(
                "zai"
                    | "zai-coding-cn"
                    | "moonshotai"
                    | "moonshotai-cn"
                    | "together"
                    | "cloudflare-ai-gateway"
                    | "nvidia"
                    | "ant-ling"
            )
        ));
    let effort = effort
        && (format != ChatThinkingFormat::ChatTemplate
            || compat.chat_template_kwargs.as_ref().is_some_and(|values| {
                values
                    .values()
                    .any(|value| value["$var"] == "thinking.effort")
            }));
    let boolean = matches!(
        format,
        ChatThinkingFormat::Deepseek
            | ChatThinkingFormat::Zai
            | ChatThinkingFormat::Qwen
            | ChatThinkingFormat::Together
            | ChatThinkingFormat::QwenChatTemplate
    ) || matches!(
        format,
        ChatThinkingFormat::Baseten | ChatThinkingFormat::ChatTemplate
    ) && compat
        .chat_template_args
        .as_ref()
        .into_iter()
        .chain(compat.chat_template_kwargs.as_ref())
        .any(|map| {
            map.values()
                .any(|value| value["$var"] == "thinking.enabled")
        });
    if effort {
        for name in REASONING_EFFORTS {
            let value = if fallback {
                Some((*name).to_string())
            } else {
                match def.thinking_level_map.get(*name) {
                    Some(Some(value)) => Some(value.clone()),
                    None if !matches!(*name, "xhigh" | "max")
                        && format != ChatThinkingFormat::AntLing =>
                    {
                        Some((*name).into())
                    }
                    _ => None,
                }
            };
            if let Some(value) = value {
                facts.efforts.push((*name).into());
                facts.effort_map.insert((*name).into(), value);
            }
        }
    }
    facts.supports_enabled = !fallback && boolean && !effort;
    facts.supports_disabled = !fallback
        && match format {
            ChatThinkingFormat::Openai | ChatThinkingFormat::StringThinking => {
                effort && facts.off_effort.is_some()
            }
            ChatThinkingFormat::AntLing => false,
            ChatThinkingFormat::ChatTemplate => {
                def.thinking_level_map.get("off") != Some(&None)
                    && (boolean || facts.off_effort.is_some())
            }
            ChatThinkingFormat::Openrouter => def.thinking_level_map.get("off") != Some(&None),
            _ => boolean && def.thinking_level_map.get("off") != Some(&None),
        };
    if !fallback && def.route.api == ModelApi::ChatCompletions {
        if compat.requires_reasoning_content_on_assistant_messages == Some(true)
            || service == Some("deepseek")
        {
            facts.protocol = ReasoningProtocol::DeepSeekChatCompletions;
            facts.replay_policy = ReasoningReplayPolicy::DeepSeekTools;
        }
        if matches!(service, Some("zai" | "zai-coding-cn")) && format == ChatThinkingFormat::Zai {
            facts.protocol = ReasoningProtocol::ZhipuChatCompletions;
            facts.replay_policy = ReasoningReplayPolicy::GlmPreserved;
        }
    }
    facts.disabled_wire = facts
        .supports_disabled
        .then_some(ReasoningDisabledWire::Resolved);
    facts
}
