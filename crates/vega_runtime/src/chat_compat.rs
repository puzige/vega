use crate::provider::{ReasoningDisabledWire, ReasoningProtocol, ReasoningReplayPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningSupport {
    Required,
    Optional,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatCompatibility {
    pub protocol: ReasoningProtocol,
    pub replay_policy: ReasoningReplayPolicy,
    pub support: ReasoningSupport,
    pub efforts: &'static [&'static str],
    pub supports_disabled: bool,
    pub disabled_wire: Option<ReasoningDisabledWire>,
    pub default_effort: Option<&'static str>,
}

#[derive(Clone, PartialEq, Eq)]
struct EndpointKey {
    username: String,
    password: Option<String>,
    host: String,
    port: Option<u16>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

impl EndpointKey {
    fn parse(value: &str) -> Option<Self> {
        let url = reqwest::Url::parse(value).ok()?;
        if url.scheme() != "https" {
            return None;
        }
        let host = url.host_str()?.to_string();
        let mut path = url.path().to_string();
        if path.ends_with('/') {
            path.pop();
        }
        Some(Self {
            username: url.username().to_string(),
            password: url.password().map(str::to_string),
            host,
            port: url.port(),
            path,
            query: url.query().map(str::to_string),
            fragment: url.fragment().map(str::to_string),
        })
    }
}

pub fn chat_compatibility(base_url: Option<&str>, model: &str) -> Option<ChatCompatibility> {
    if model == "step-5-preview" {
        return Some(ChatCompatibility {
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            replay_policy: ReasoningReplayPolicy::StandardChat,
            support: ReasoningSupport::Optional,
            efforts: &["low", "medium", "high"],
            supports_disabled: false,
            disabled_wire: None,
            default_effort: None,
        });
    }
    let endpoint = EndpointKey::parse(base_url?)?;
    let deepseek_endpoint = ["https://api.deepseek.com", "https://api.deepseek.com/v1"]
        .into_iter()
        .filter_map(EndpointKey::parse)
        .any(|candidate| candidate == endpoint);
    if deepseek_endpoint && matches!(model, "deepseek-flash" | "deepseek-v4-pro") {
        return Some(ChatCompatibility {
            protocol: ReasoningProtocol::DeepSeekChatCompletions,
            replay_policy: ReasoningReplayPolicy::DeepSeekTools,
            support: ReasoningSupport::Optional,
            efforts: &["low", "high", "max"],
            supports_disabled: true,
            disabled_wire: Some(ReasoningDisabledWire::ThinkingTypeDisabled),
            default_effort: None,
        });
    }
    let glm_endpoint = [
        "https://api.z.ai/api/paas/v4",
        "https://api.z.ai/api/coding/paas/v4",
        "https://open.bigmodel.cn/api/paas/v4",
        "https://open.bigmodel.cn/api/coding/paas/v4",
    ]
    .into_iter()
    .filter_map(EndpointKey::parse)
    .any(|candidate| candidate == endpoint);
    if glm_endpoint && matches!(model, "glm-4.7" | "glm-5.3" | "glm-5.3-flash") {
        let required = matches!(model, "glm-5.3" | "glm-5.3-flash");
        return Some(ChatCompatibility {
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            replay_policy: ReasoningReplayPolicy::GlmPreserved,
            support: if required {
                ReasoningSupport::Required
            } else {
                ReasoningSupport::Optional
            },
            efforts: &["low", "high", "max"],
            supports_disabled: !required,
            disabled_wire: (!required).then_some(ReasoningDisabledWire::ThinkingTypeDisabled),
            default_effort: Some("high"),
        });
    }
    None
}
