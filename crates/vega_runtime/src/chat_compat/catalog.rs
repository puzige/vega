use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Transport {
    pub(crate) developer: bool,
    pub(crate) max_tokens: bool,
    pub(crate) usage: bool,
    pub(crate) strict: bool,
    pub(crate) tool_stream: bool,
}

impl Transport {
    pub(crate) fn unknown() -> Self {
        Self {
            developer: false,
            max_tokens: true,
            usage: true,
            strict: true,
            tool_stream: false,
        }
    }
    pub(crate) fn for_service(service: Option<&str>, model: &str) -> Self {
        let Some(service) = service else {
            return Self::unknown();
        };
        let nonstandard = matches!(
            service,
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
        );
        Self {
            developer: if service == "openrouter" {
                model.starts_with("anthropic/") || model.starts_with("openai/")
            } else {
                !nonstandard
            },
            max_tokens: matches!(
                service,
                "deepseek"
                    | "moonshotai"
                    | "moonshotai-cn"
                    | "cloudflare-ai-gateway"
                    | "together"
                    | "nvidia"
                    | "ant-ling"
                    | "zai"
                    | "zai-coding-cn"
            ),
            usage: true,
            strict: !matches!(
                service,
                "moonshotai" | "moonshotai-cn" | "together" | "cloudflare-ai-gateway" | "nvidia"
            ),
            tool_stream: false,
        }
    }
}

pub(crate) fn default_effort(service: Option<&str>) -> bool {
    !matches!(
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
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Compat {
    pub(crate) thinking_format: Option<ChatThinkingFormat>,
    pub(crate) supports_developer_role: Option<bool>,
    pub(crate) supports_reasoning_effort: Option<bool>,
    pub(crate) supports_usage_in_streaming: Option<bool>,
    pub(crate) max_tokens_field: Option<String>,
    pub(crate) requires_reasoning_content_on_assistant_messages: Option<bool>,
    pub(crate) zai_tool_stream: Option<bool>,
    pub(crate) supports_strict_mode: Option<bool>,
    pub(crate) chat_template_args: Option<TemplateArgs>,
}

impl Compat {
    pub(crate) fn apply_transport(&self, transport: &mut Transport) {
        if let Some(value) = self.supports_developer_role {
            transport.developer = value;
        }
        if let Some(value) = self.supports_usage_in_streaming {
            transport.usage = value;
        }
        if let Some(value) = &self.max_tokens_field {
            transport.max_tokens = value == "max_tokens";
        }
        if let Some(value) = self.supports_strict_mode {
            transport.strict = value;
        }
        if let Some(value) = self.zai_tool_stream {
            transport.tool_stream = value;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TemplateArgs {
    pub(crate) enable_thinking: Option<TemplateVariable>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TemplateVariable {
    #[serde(rename = "$var")]
    pub(crate) variable: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CatalogEntry {
    pub(crate) provider: String,
    api: String,
    pub(crate) id: String,
    pub(crate) base_url: String,
    pub(crate) reasoning: bool,
    #[serde(default)]
    pub(crate) thinking_level_map: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub(crate) compat: Compat,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    models: Vec<CatalogEntry>,
}
pub(crate) struct Catalog {
    models: Vec<CatalogEntry>,
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Catalog, VegaError> {
    let document: Document = serde_json::from_slice(bytes).map_err(|_| protocol_error())?;
    if document.version != 1 {
        return Err(protocol_error());
    }
    let mut models: Vec<CatalogEntry> = Vec::new();
    for entry in document.models {
        if entry.api != "openai-completions"
            || entry.provider.trim().is_empty()
            || entry.id.trim().is_empty()
            || Endpoint::parse(&entry.base_url).is_none()
            || entry
                .compat
                .max_tokens_field
                .as_deref()
                .is_some_and(|field| !matches!(field, "max_tokens" | "max_completion_tokens"))
            || entry.thinking_level_map.iter().any(|(key, value)| {
                (!REASONING_EFFORTS.contains(&key.as_str()) && key != "off")
                    || value.as_ref().is_some_and(|value| value.trim().is_empty())
            })
            || entry
                .compat
                .chat_template_args
                .as_ref()
                .and_then(|args| args.enable_thinking.as_ref())
                .is_some_and(|var| var.variable != "thinking.enabled")
        {
            return Err(protocol_error());
        }
        if let Some(existing) = models.iter().find(|existing| {
            Endpoint::parse(&existing.base_url) == Endpoint::parse(&entry.base_url)
                && existing.id == entry.id
        }) && (existing.reasoning != entry.reasoning
            || existing.thinking_level_map != entry.thinking_level_map
            || existing.compat != entry.compat)
        {
            return Err(protocol_error());
        }
        models.push(entry);
    }
    Ok(Catalog { models })
}

#[derive(PartialEq, Eq)]
struct Endpoint {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: Vec<String>,
}
impl Endpoint {
    fn parse(value: &str) -> Option<Self> {
        let url = reqwest::Url::parse(value).ok()?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let path = url
            .path()
            .strip_suffix('/')
            .unwrap_or(url.path())
            .split('/')
            .map(str::to_string)
            .collect();
        Some(Self {
            scheme: url.scheme().into(),
            host: url.host_str()?.into(),
            port: url.port(),
            path,
        })
    }
    fn matches(&self, other: &Self) -> bool {
        self.scheme == other.scheme
            && self.host == other.host
            && self.port == other.port
            && self.path.len() == other.path.len()
            && self.path.iter().zip(&other.path).all(|(expected, actual)| {
                if expected == "{CLOUDFLARE_ACCOUNT_ID}"
                    || expected == "{CLOUDFLARE_GATEWAY_ID}"
                    || expected == "%7BCLOUDFLARE_ACCOUNT_ID%7D"
                    || expected == "%7BCLOUDFLARE_GATEWAY_ID%7D"
                {
                    !actual.is_empty()
                } else {
                    expected == actual
                }
            })
    }
}

impl Catalog {
    pub(crate) fn find(
        &self,
        base_url: Option<&str>,
        model: &str,
    ) -> (Option<&str>, Option<&CatalogEntry>) {
        let Some(endpoint) = base_url.and_then(Endpoint::parse) else {
            return (None, None);
        };
        let matches_endpoint = |entry: &&CatalogEntry| {
            Endpoint::parse(&entry.base_url).is_some_and(|pattern| pattern.matches(&endpoint))
        };
        let exact = self
            .models
            .iter()
            .find(|entry| entry.id == model && matches_endpoint(entry));
        let service = exact
            .or_else(|| self.models.iter().find(matches_endpoint))
            .map(|entry| entry.provider.as_str())
            .or_else(|| {
                for (alias, service) in [
                    ("https://api.together.xyz/v1", "together"),
                    ("https://api.deepseek.com/v1", "deepseek"),
                    ("https://api.z.ai/api/paas/v4", "zai"),
                    ("https://open.bigmodel.cn/api/paas/v4", "zai-coding-cn"),
                    (
                        "https://open.bigmodel.cn/api/coding/paas/v4",
                        "zai-coding-cn",
                    ),
                ] {
                    if Endpoint::parse(alias).as_ref() == Some(&endpoint) {
                        return Some(service);
                    }
                }
                None
            });
        let entry = exact.or_else(|| {
            service.and_then(|service| {
                self.models
                    .iter()
                    .find(|entry| entry.provider == service && entry.id == model)
            })
        });
        (service, entry)
    }
    pub(crate) fn apply_service_transport(&self, service: Option<&str>, transport: &mut Transport) {
        let Some(service) = service else {
            return;
        };
        let entries = self
            .models
            .iter()
            .filter(|entry| entry.provider == service)
            .collect::<Vec<_>>();
        if let Some(first) = entries.first() {
            let common = Compat {
                supports_developer_role: entries
                    .iter()
                    .all(|entry| {
                        entry.compat.supports_developer_role == first.compat.supports_developer_role
                    })
                    .then_some(first.compat.supports_developer_role)
                    .flatten(),
                supports_usage_in_streaming: entries
                    .iter()
                    .all(|entry| {
                        entry.compat.supports_usage_in_streaming
                            == first.compat.supports_usage_in_streaming
                    })
                    .then_some(first.compat.supports_usage_in_streaming)
                    .flatten(),
                max_tokens_field: entries
                    .iter()
                    .all(|entry| entry.compat.max_tokens_field == first.compat.max_tokens_field)
                    .then(|| first.compat.max_tokens_field.clone())
                    .flatten(),
                supports_strict_mode: entries
                    .iter()
                    .all(|entry| {
                        entry.compat.supports_strict_mode == first.compat.supports_strict_mode
                    })
                    .then_some(first.compat.supports_strict_mode)
                    .flatten(),
                ..Compat::default()
            };
            common.apply_transport(transport);
        }
    }
    pub(crate) fn service_format(&self, service: Option<&str>) -> ChatThinkingFormat {
        match service {
            Some("deepseek" | "opencode-go") => ChatThinkingFormat::Deepseek,
            Some("zai" | "zai-coding-cn") => ChatThinkingFormat::Zai,
            Some("together") => ChatThinkingFormat::Together,
            Some("ant-ling") => ChatThinkingFormat::AntLing,
            Some("openrouter") => ChatThinkingFormat::Openrouter,
            Some("qwen-token-plan" | "qwen-token-plan-cn" | "qwen-token-plan-individual") => {
                ChatThinkingFormat::Qwen
            }
            _ => ChatThinkingFormat::Openai,
        }
    }
}
