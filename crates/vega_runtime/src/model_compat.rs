use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use crate::chat_compat::ChatThinkingFormat;
use crate::{FrozenReasoning, ReasoningChoice, VegaError};

pub const RULES_VERSION: &str = "vega.model.pi-ai-0.84.4.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelApi {
    #[serde(rename = "openai-completions")]
    ChatCompletions,
    #[serde(rename = "openai-responses")]
    Responses,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentity {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRoute {
    pub api: ModelApi,
    pub base_url: String,
}

impl ModelRoute {
    pub fn new(api: ModelApi, endpoint: &str) -> Result<Self, VegaError> {
        let mut url = reqwest::Url::parse(endpoint).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let path = url.path().trim_end_matches('/').to_string();
        url.set_path(&path);
        Ok(Self {
            api,
            base_url: url.as_str().trim_end_matches('/').to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCompat {
    pub thinking_format: Option<ChatThinkingFormat>,
    pub supports_store: Option<bool>,
    pub supports_developer_role: Option<bool>,
    pub supports_reasoning_effort: Option<bool>,
    pub supports_usage_in_streaming: Option<bool>,
    pub max_tokens_field: Option<String>,
    pub supports_strict_mode: Option<bool>,
    pub requires_reasoning_content_on_assistant_messages: Option<bool>,
    pub zai_tool_stream: Option<bool>,
    pub chat_template_args: Option<BTreeMap<String, serde_json::Value>>,
    pub chat_template_kwargs: Option<BTreeMap<String, serde_json::Value>>,
    pub requires_thinking_as_text: Option<bool>,
    pub supports_long_cache_retention: Option<bool>,
    pub cache_control_format: Option<String>,
    pub deferred_tools_mode: Option<String>,
    pub send_session_affinity_headers: Option<bool>,
    pub session_affinity_format: Option<String>,
    pub supports_additional_tools: Option<bool>,
    pub supports_explicit_prompt_cache_mode: Option<bool>,
    #[serde(rename = "supportsOpenAIGrammarTools")]
    pub supports_open_ai_grammar_tools: Option<bool>,
    pub supports_tool_search: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelDefinition {
    pub identity: ModelIdentity,
    pub route: ModelRoute,
    pub reasoning: Option<bool>,
    pub thinking_level_map: BTreeMap<String, Option<String>>,
    pub input: Option<Vec<String>>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u32>,
    pub compat: ModelCompat,
    pub(crate) service: Option<String>,
    pub(crate) source_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilityOverride {
    pub identity: ModelIdentity,
    pub allowed_efforts: Option<Vec<String>>,
    pub image_input: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelCapabilities {
    pub source_supported_levels: Vec<String>,
    pub encodable_efforts: Vec<String>,
    pub allowed_efforts: Vec<String>,
    pub image_input: bool,
    pub supports_enabled: bool,
    pub supports_disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    pub identity: ModelIdentity,
    pub definition: ModelDefinition,
    pub capabilities: ModelCapabilities,
    pub fingerprint: String,
    pub(crate) compatibility: Box<crate::ChatCompatibility>,
}

impl ResolvedModel {
    pub fn compatibility(&self) -> &crate::ChatCompatibility {
        &self.compatibility
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    provider: String,
    id: String,
    api: ModelApi,
    base_url: String,
    reasoning: Option<bool>,
    #[serde(default)]
    thinking_level_map: BTreeMap<String, Option<String>>,
    input: Option<Vec<String>>,
    context_window: Option<u64>,
    max_tokens: Option<u32>,
    #[serde(default)]
    compat: ModelCompat,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    models: Vec<Entry>,
}

pub struct ModelCatalog {
    models: Vec<ModelDefinition>,
}

pub(crate) fn invalid() -> VegaError {
    VegaError::ReasoningSelectionInvalid {
        message: "model capabilities are invalid or unavailable".into(),
    }
}

fn same_endpoint(pattern: &ModelRoute, actual: &ModelRoute) -> bool {
    if pattern.api != actual.api {
        return false;
    }
    let Ok(pattern) = reqwest::Url::parse(&pattern.base_url) else {
        return false;
    };
    let Ok(actual) = reqwest::Url::parse(&actual.base_url) else {
        return false;
    };
    pattern.scheme() == actual.scheme()
        && pattern.host_str() == actual.host_str()
        && pattern.port() == actual.port()
        && pattern.path().split('/').count() == actual.path().split('/').count()
        && pattern
            .path()
            .split('/')
            .zip(actual.path().split('/'))
            .all(|(a, b)| {
                a == b
                    || matches!(
                        a,
                        "{CLOUDFLARE_ACCOUNT_ID}"
                            | "{CLOUDFLARE_GATEWAY_ID}"
                            | "%7BCLOUDFLARE_ACCOUNT_ID%7D"
                            | "%7BCLOUDFLARE_GATEWAY_ID%7D"
                    ) && !b.is_empty()
            })
}

pub fn decode_model_catalog(bytes: &[u8]) -> Result<ModelCatalog, VegaError> {
    let doc: Document = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if doc.version != 1 {
        return Err(invalid());
    }
    let mut models: Vec<ModelDefinition> = Vec::new();
    for entry in doc.models {
        if entry.provider.trim().is_empty()
            || entry.id.trim().is_empty()
            || entry.input.as_ref().is_some_and(|input| {
                input
                    .iter()
                    .any(|item| !matches!(item.as_str(), "text" | "image"))
            })
            || entry.thinking_level_map.iter().any(|(key, value)| {
                (!crate::REASONING_EFFORTS.contains(&key.as_str()) && key != "off")
                    || value.as_ref().is_some_and(|value| value.trim().is_empty())
            })
            || entry
                .compat
                .max_tokens_field
                .as_deref()
                .is_some_and(|field| !matches!(field, "max_tokens" | "max_completion_tokens"))
            || !valid_template(entry.compat.chat_template_args.as_ref())
            || !valid_template(entry.compat.chat_template_kwargs.as_ref())
        {
            return Err(invalid());
        }
        let mut model = ModelDefinition {
            identity: ModelIdentity {
                provider: entry.provider.clone(),
                model: entry.id,
            },
            route: ModelRoute::new(entry.api, &entry.base_url)?,
            reasoning: entry.reasoning,
            thinking_level_map: entry.thinking_level_map,
            input: entry.input,
            context_window: entry.context_window,
            max_output_tokens: entry.max_tokens,
            compat: entry.compat,
            service: Some(entry.provider),
            source_fingerprint: String::new(),
        };
        let canonical = serde_json::to_vec(&(
            &model.route,
            &model.identity.model,
            &model.reasoning,
            &model.thinking_level_map,
            &model.input,
            &model.context_window,
            &model.max_output_tokens,
            &model.compat,
        ))
        .map_err(|_| invalid())?;
        model.source_fingerprint = format!("{:x}", Sha256::digest(canonical));
        if models.iter().any(|prior| {
            prior.route == model.route
                && prior.identity.model == model.identity.model
                && prior.source_fingerprint != model.source_fingerprint
        }) {
            return Err(invalid());
        }
        models.push(model);
    }
    models.sort_by(|a, b| a.identity.provider.cmp(&b.identity.provider));
    Ok(ModelCatalog { models })
}

fn valid_template(template: Option<&BTreeMap<String, serde_json::Value>>) -> bool {
    template.is_none_or(|template| {
        template.values().all(|value| {
            if let Some(object) = value.as_object() {
                return matches!(
                    object.get("$var").and_then(serde_json::Value::as_str),
                    Some("thinking.enabled" | "thinking.effort")
                ) && object
                    .keys()
                    .all(|key| matches!(key.as_str(), "$var" | "omitWhenOff"))
                    && object
                        .get("omitWhenOff")
                        .is_none_or(serde_json::Value::is_boolean);
            }
            !value.is_array()
        })
    })
}

impl ModelCatalog {
    pub fn lookup(
        &self,
        route: &ModelRoute,
        identity: &ModelIdentity,
    ) -> Result<ModelDefinition, VegaError> {
        if identity.provider.trim().is_empty() || identity.model.trim().is_empty() {
            return Err(invalid());
        }
        let alias = match route.base_url.as_str() {
            "https://api.together.xyz/v1" => Some("https://api.together.ai/v1"),
            "https://api.deepseek.com/v1" => Some("https://api.deepseek.com"),
            "https://api.z.ai/api/paas/v4" => Some("https://api.z.ai/api/coding/paas/v4"),
            "https://open.bigmodel.cn/api/paas/v4" => {
                Some("https://open.bigmodel.cn/api/coding/paas/v4")
            }
            _ => None,
        };
        let canonical = alias
            .map(|base| ModelRoute::new(route.api, base))
            .transpose()?
            .unwrap_or_else(|| route.clone());
        let exact = self
            .models
            .iter()
            .find(|model| {
                model.identity.model == identity.model && same_endpoint(&model.route, route)
            })
            .or_else(|| {
                self.models.iter().find(|model| {
                    model.identity.model == identity.model
                        && same_endpoint(&model.route, &canonical)
                })
            });
        let service = exact
            .or_else(|| {
                self.models
                    .iter()
                    .find(|model| same_endpoint(&model.route, route))
            })
            .or_else(|| {
                self.models
                    .iter()
                    .find(|model| same_endpoint(&model.route, &canonical))
            })
            .and_then(|model| model.service.clone())
            .or_else(|| {
                self.models
                    .iter()
                    .find(|model| model.route.base_url == canonical.base_url)
                    .and_then(|model| model.service.clone())
            });
        let mut model = exact.cloned().unwrap_or_else(|| ModelDefinition {
            identity: identity.clone(),
            route: route.clone(),
            reasoning: None,
            thinking_level_map: BTreeMap::new(),
            input: None,
            context_window: None,
            max_output_tokens: None,
            compat: ModelCompat::default(),
            service: service.clone(),
            source_fingerprint: "unknown".into(),
        });
        if exact.is_none() {
            let entries = self
                .models
                .iter()
                .filter(|m| m.service == service && m.route.api == route.api)
                .collect::<Vec<_>>();
            if let Some(first) = entries.first() {
                if entries
                    .iter()
                    .all(|m| m.compat.supports_store == first.compat.supports_store)
                {
                    model.compat.supports_store = first.compat.supports_store;
                }
                if entries.iter().all(|m| {
                    m.compat.supports_developer_role == first.compat.supports_developer_role
                }) {
                    model.compat.supports_developer_role = first.compat.supports_developer_role;
                }
                if entries
                    .iter()
                    .all(|m| m.compat.max_tokens_field == first.compat.max_tokens_field)
                {
                    model.compat.max_tokens_field = first.compat.max_tokens_field.clone();
                }
                if entries
                    .iter()
                    .all(|m| m.compat.supports_strict_mode == first.compat.supports_strict_mode)
                {
                    model.compat.supports_strict_mode = first.compat.supports_strict_mode;
                }
                if entries.iter().all(|m| {
                    m.compat.supports_usage_in_streaming == first.compat.supports_usage_in_streaming
                }) {
                    model.compat.supports_usage_in_streaming =
                        first.compat.supports_usage_in_streaming;
                }
            }
        }
        model.identity = identity.clone();
        model.route = route.clone();
        model.service = service;
        Ok(model)
    }
}

pub fn lookup_model_definition(
    route: &ModelRoute,
    identity: &ModelIdentity,
) -> Result<ModelDefinition, VegaError> {
    static CATALOG: OnceLock<Result<ModelCatalog, ()>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            decode_model_catalog(include_bytes!("model_compat/pi-ai-0.84.4.json")).map_err(|_| ())
        })
        .as_ref()
        .map_err(|_| invalid())?
        .lookup(route, identity)
}

pub fn resolve_model(
    definition: &ModelDefinition,
    overrides: Option<&ModelCapabilityOverride>,
) -> Result<ResolvedModel, VegaError> {
    let mut compatibility = crate::chat_compat::from_definition(definition);
    let logical = if definition.reasoning == Some(false) {
        vec!["off".into()]
    } else if definition.reasoning.is_none() {
        crate::REASONING_EFFORTS
            .iter()
            .map(|v| (*v).into())
            .collect()
    } else {
        ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .filter(|level| match definition.thinking_level_map.get(*level) {
                Some(None) => false,
                Some(Some(_)) => true,
                None => !matches!(*level, "xhigh" | "max"),
            })
            .map(str::to_string)
            .collect()
    };
    let mut capabilities = ModelCapabilities {
        source_supported_levels: logical,
        encodable_efforts: compatibility.efforts.clone(),
        allowed_efforts: compatibility.efforts.clone(),
        image_input: definition
            .input
            .as_ref()
            .is_some_and(|input| input.iter().any(|v| v == "image")),
        supports_enabled: compatibility.supports_enabled,
        supports_disabled: compatibility.supports_disabled,
    };
    if let Some(overrides) = overrides {
        if overrides.identity != definition.identity {
            return Err(invalid());
        }
        if let Some(allowed) = &overrides.allowed_efforts {
            if allowed.iter().enumerate().any(|(i, effort)| {
                !capabilities.encodable_efforts.contains(effort) || allowed[..i].contains(effort)
            }) {
                return Err(invalid());
            }
            capabilities.allowed_efforts = crate::REASONING_EFFORTS
                .iter()
                .filter(|effort| allowed.iter().any(|v| v == **effort))
                .map(|v| (*v).to_string())
                .collect();
        }
        if let Some(image) = overrides.image_input {
            capabilities.image_input = image;
        }
    }
    compatibility.efforts = capabilities.allowed_efforts.clone();
    let mut digest = Sha256::new();
    digest.update(RULES_VERSION);
    digest.update(include_bytes!("model_compat/pi-ai-0.84.4.sources.json"));
    digest.update(serde_json::to_vec(&(definition, &capabilities)).map_err(|_| invalid())?);
    let fingerprint = format!("{:x}", digest.finalize());
    compatibility.profile_id = fingerprint.clone();
    Ok(ResolvedModel {
        identity: definition.identity.clone(),
        definition: definition.clone(),
        capabilities,
        fingerprint,
        compatibility: Box::new(compatibility),
    })
}

pub fn freeze_choice(
    model: &ResolvedModel,
    choice: ReasoningChoice,
) -> Result<FrozenReasoning, VegaError> {
    let facts = &model.compatibility;
    let frozen = FrozenReasoning {
        compatibility: Some(facts.clone()),
        provider: model.identity.provider.clone(),
        model: model.identity.model.clone(),
        protocol: facts.protocol,
        replay_policy: facts.replay_policy,
        choice,
        supports_disabled: facts.supports_disabled,
        disabled_wire: facts.disabled_wire,
        declared_efforts: facts.efforts.clone(),
    };
    frozen.validate()?;
    Ok(frozen)
}

pub(crate) fn request_model(
    request: &crate::ChatRequest,
    api: ModelApi,
    base: &str,
) -> Result<ResolvedModel, VegaError> {
    let model = match &request.model_snapshot {
        Some(model) => model.as_ref().clone(),
        None => resolve_model(
            &lookup_model_definition(
                &ModelRoute::new(api, base)?,
                &ModelIdentity {
                    provider: request
                        .reasoning
                        .as_ref()
                        .map_or_else(|| "unconfigured".into(), |r| r.provider.clone()),
                    model: request.model.clone(),
                },
            )?,
            None,
        )?,
    };
    if model.identity.model != request.model
        || model.definition.route != ModelRoute::new(api, base)?
    {
        return Err(invalid());
    }
    if let Some(reasoning) = &request.reasoning {
        reasoning.validate()?;
        if reasoning.provider != model.identity.provider || reasoning.model != model.identity.model
        {
            return Err(invalid());
        }
        if reasoning
            .compatibility
            .as_ref()
            .is_some_and(|facts| facts.profile_id != model.fingerprint)
        {
            return Err(invalid());
        }
        freeze_choice(&model, reasoning.choice.clone())?;
    }
    crate::images::validate_messages(&request.messages)?;
    if request
        .messages
        .iter()
        .any(|message| !message.images.is_empty())
        && !model.capabilities.image_input
    {
        return Err(VegaError::ModelImagesUnsupported);
    }
    Ok(model)
}
