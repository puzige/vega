use super::*;
use crate::model_compat::{
    ModelApi, ModelCapabilityOverride, ModelDefinition, ModelIdentity, ModelRoute, ResolvedModel,
    decode_model_catalog, freeze_choice, lookup_model_definition, resolve_model,
};
use crate::{
    AgentRequest, ChatMessage, ChatRole, ChatToolCall, ImageAttachment, RuntimeRunMode,
    RuntimeToolConfig, ToolChoice, ToolDefinition,
};
use serde_json::{Value, json};

mod catalog;
mod chat;
mod errors;
mod http;
mod lifecycle;
mod responses;
mod stream;
mod transport;

fn identity(provider: &str, model: &str) -> ModelIdentity {
    ModelIdentity {
        provider: provider.into(),
        model: model.into(),
    }
}

fn route(api: ModelApi, endpoint: &str) -> ModelRoute {
    ModelRoute::new(api, endpoint).expect("valid model route")
}

fn definition(mut model: Value) -> ModelDefinition {
    let provider = model["provider"].as_str().unwrap_or("fixture").to_string();
    let name = model["id"].as_str().unwrap_or("fixture-model").to_string();
    let endpoint = model["baseUrl"]
        .as_str()
        .unwrap_or("https://fixture.invalid/v1")
        .to_string();
    let api = if model["api"] == "openai-responses" {
        ModelApi::Responses
    } else {
        ModelApi::ChatCompletions
    };
    for (key, default) in [
        ("provider", json!(provider)),
        ("id", json!(name)),
        ("baseUrl", json!(endpoint)),
        ("api", json!("openai-completions")),
        ("reasoning", json!(true)),
        ("input", json!(["text"])),
    ] {
        if model.get(key).is_none() {
            model[key] = default;
        }
    }
    let bytes = serde_json::to_vec(&json!({"version":1,"models":[model]})).unwrap();
    decode_model_catalog(&bytes)
        .expect("valid source facts")
        .lookup(&route(api, &endpoint), &identity(&provider, &name))
        .expect("catalog identity")
}

fn resolved(model: Value) -> ResolvedModel {
    resolve_model(&definition(model), None).expect("resolved model")
}

fn fixed(api: ModelApi, endpoint: &str, provider: &str, model: &str) -> ResolvedModel {
    let source = lookup_model_definition(&route(api, endpoint), &identity(provider, model))
        .expect("fixed source lookup");
    resolve_model(&source, None).expect("fixed model resolution")
}

fn override_for(
    model: &ResolvedModel,
    efforts: Option<&[&str]>,
    image_input: Option<bool>,
) -> ModelCapabilityOverride {
    ModelCapabilityOverride {
        identity: model.identity.clone(),
        allowed_efforts: efforts.map(|values| values.iter().map(|value| (*value).into()).collect()),
        image_input,
    }
}

fn bound_request(model: &ResolvedModel, choice: ReasoningChoice) -> ChatRequest {
    ChatRequest {
        model: model.identity.model.clone(),
        messages: vec![
            ChatMessage::new(ChatRole::System, "Follow the requested format."),
            ChatMessage::new(ChatRole::User, "Return a concise answer."),
        ],
        reasoning: Some(freeze_choice(model, choice).expect("supported choice")),
        model_snapshot: Some(Box::new(model.clone())),
        ..Default::default()
    }
}

fn tool(name: &str, strict: bool) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: "Read an owned test file".into(),
        input_schema: json!({
            "type":"object", "properties":{"path":{"type":"string"}},
            "required":["path"], "additionalProperties":false
        }),
        strict,
    }
}

fn pixels() -> ImageAttachment {
    ImageAttachment::from_bytes(include_bytes!("fixtures/red.png").to_vec())
        .expect("real decoded PNG")
}

fn blue_pixels() -> ImageAttachment {
    ImageAttachment::from_bytes(include_bytes!("fixtures/blue.png").to_vec())
        .expect("real decoded blue PNG")
}

fn data_url(image: &ImageAttachment) -> String {
    use base64::Engine;
    format!(
        "data:{};base64,{}",
        image.mime_type(),
        base64::engine::general_purpose::STANDARD.encode(image.bytes())
    )
}

fn chat_chunk(delta: Value, finish: Value) -> Value {
    json!({"id":"response-fixture","choices":[{"delta":delta,"finish_reason":finish}]})
}

fn decode_chat(chunks: &[Value], finalize: bool) -> Result<Vec<ProviderEvent>, VegaError> {
    let mut assembler = super::sse::SseAssembler::default();
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(assembler.absorb(&chunk.to_string())?);
    }
    if finalize {
        events.extend(assembler.finalize()?);
    }
    Ok(events)
}

fn decode_responses(chunks: &[Value]) -> Result<Vec<ProviderEvent>, VegaError> {
    let mut assembler = super::responses::Assembler::default();
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(assembler.absorb(&chunk.to_string())?);
    }
    Ok(events)
}

fn text(events: &[ProviderEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::TextDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn thinking(events: &[ProviderEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::ThinkingDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn calls(events: &[ProviderEvent]) -> Vec<(String, String, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::ToolUse {
                id,
                name,
                input_json,
            } => Some((id.clone(), name.clone(), input_json.clone())),
            _ => None,
        })
        .collect()
}
