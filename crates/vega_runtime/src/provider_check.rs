//! One-shot bounded OpenAI connection checks. Never retries or retains response text.
use crate::{ChatMessage, ChatRequest, ChatRole};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const MAX_BYTES: usize = 1024 * 1024;

/// Content-free transport failures adapted at the conversation boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckError {
    Invalid,
    Cancelled,
    Timeout,
    Network,
    Http(u16),
    Malformed,
    Limit,
}

/// Shared model grammar used by config commands and remote discovery.
pub fn valid_model_id(model: &str) -> bool {
    let mut bytes = model.bytes();
    model.len() <= 200
        && bytes.next().is_some_and(|b| b.is_ascii_alphanumeric())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b"._:/-".contains(&b))
        && !model.contains("..")
        && !model.contains("//")
        && !model.ends_with('/')
}

/// Reject URL components that could expose secrets or change endpoint authority.
pub fn valid_base_url(base: &str) -> bool {
    reqwest::Url::parse(base).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && !base
                .split("//")
                .nth(1)
                .unwrap_or_default()
                .split('/')
                .next()
                .unwrap_or_default()
                .contains('@')
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && !base.chars().any(char::is_control)
    })
}

async fn response_bytes(
    base: &str,
    key: &str,
    model: Option<&str>,
    responses: bool,
    cancel: CancellationToken,
) -> Result<Vec<u8>, CheckError> {
    if !valid_base_url(base) || key.is_empty() {
        return Err(CheckError::Invalid);
    }
    let action = async {
        let client = reqwest::Client::builder()
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| CheckError::Network)?;
        let request = if let Some(model) = model {
            if !valid_model_id(model) {
                return Err(CheckError::Invalid);
            }
            let definition = crate::model_compat::lookup_model_definition(
                &crate::model_compat::ModelRoute::new(
                    if responses {
                        crate::model_compat::ModelApi::Responses
                    } else {
                        crate::model_compat::ModelApi::ChatCompletions
                    },
                    base,
                )
                .map_err(|_| CheckError::Invalid)?,
                &crate::model_compat::ModelIdentity {
                    provider: "connection-check".into(),
                    model: model.into(),
                },
            )
            .map_err(|_| CheckError::Invalid)?;
            let model_snapshot = crate::model_compat::resolve_model(&definition, None)
                .map_err(|_| CheckError::Invalid)?;
            let request = ChatRequest {
                model: model.into(),
                messages: vec![ChatMessage::new(ChatRole::User, "Reply with OK.")],
                max_tokens: Some(128),
                model_snapshot: Some(Box::new(model_snapshot)),
                ..Default::default()
            };
            let body = crate::openai::encode_request(base, &request, responses)
                .map_err(|_| CheckError::Invalid)?;
            let path = if responses {
                "responses"
            } else {
                "chat/completions"
            };
            client
                .post(format!("{}/{path}", base.trim_end_matches('/')))
                .json(&body)
        } else {
            client.get(format!("{}/models", base.trim_end_matches('/')))
        };
        let request = request.bearer_auth(key).build().map_err(transport_error)?;
        let response = send_request(&client, request)
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(CheckError::Http(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BYTES as u64)
        {
            return Err(CheckError::Limit);
        }
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(transport_error)?;
            if chunk.len() > MAX_BYTES - body.len() {
                return Err(CheckError::Limit);
            }
            body.extend_from_slice(&chunk);
        }
        // Remote servers may echo credentials even in ostensibly successful payloads.
        if body.windows(key.len()).any(|part| part == key.as_bytes()) {
            return Err(CheckError::Malformed);
        }
        Ok(body)
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(CheckError::Cancelled),
        result = tokio::time::timeout(Duration::from_secs(15), action) => result.unwrap_or(Err(CheckError::Timeout)),
    }
}

fn transport_error(error: reqwest::Error) -> CheckError {
    if error.is_timeout() {
        CheckError::Timeout
    } else {
        CheckError::Network
    }
}

/// Fetch the actual provider model endpoint with bounded body and candidate count.
pub async fn discover(
    base: &str,
    key: &str,
    cancel: CancellationToken,
) -> Result<Vec<String>, CheckError> {
    let bytes = response_bytes(base, key, None, false, cancel).await?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| CheckError::Malformed)?;
    let data = value
        .get("data")
        .and_then(|value| value.as_array())
        .ok_or(CheckError::Malformed)?;
    if data.len() > 1000 {
        return Err(CheckError::Limit);
    }
    let mut ids = Vec::new();
    for item in data {
        let id = item
            .get("id")
            .and_then(|id| id.as_str())
            .ok_or(CheckError::Malformed)?;
        if !valid_model_id(id) || id.contains(key) {
            return Err(CheckError::Malformed);
        }
        if !ids.iter().any(|existing| existing == id) {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

/// Test using the production chat encoder; only actual nonempty completed text succeeds.
pub async fn probe(
    base: &str,
    key: &str,
    model: &str,
    cancel: CancellationToken,
) -> Result<(), CheckError> {
    let bytes = response_bytes(base, key, Some(model), false, cancel).await?;
    let source = futures::stream::once(async { Ok::<_, std::io::Error>(bytes) });
    let mut events = Box::pin(source.eventsource());
    let mut content = false;
    let mut sentinel = false;
    let mut parser = crate::openai::SseAssembler::default();
    while let Some(event) = events.next().await {
        let event = event.map_err(|_| CheckError::Malformed)?;
        let data = event.data.trim();
        if data == "[DONE]" {
            sentinel = true;
            continue;
        }
        if sentinel {
            return Err(CheckError::Malformed);
        }
        for event in parser.absorb(data).map_err(|_| CheckError::Malformed)? {
            match event {
                crate::ProviderEvent::TextDelta(text)
                | crate::ProviderEvent::ThinkingDelta(text) => content |= !text.trim().is_empty(),
                crate::ProviderEvent::ToolUse { .. } => return Err(CheckError::Malformed),
                _ => {}
            }
        }
    }
    let terminal = parser.finalize().map_err(|_| CheckError::Malformed)?;
    if content
        && terminal.iter().any(|event| {
            matches!(
                event,
                crate::ProviderEvent::Done {
                    stop_reason: crate::StopReason::End | crate::StopReason::Length
                }
            )
        })
    {
        Ok(())
    } else {
        Err(CheckError::Malformed)
    }
}

pub async fn probe_responses(
    base: &str,
    key: &str,
    model: &str,
    cancel: CancellationToken,
) -> Result<(), CheckError> {
    let bytes = response_bytes(base, key, Some(model), true, cancel.clone()).await?;
    let source = futures::stream::once(async { Ok::<_, std::io::Error>(bytes) });
    let mut events = Box::pin(source.eventsource());
    let mut parser = crate::openai::responses::Assembler::default();
    let mut content = false;
    while let Some(event) = events.next().await {
        if cancel.is_cancelled() {
            return Err(CheckError::Cancelled);
        }
        let event = event.map_err(|_| CheckError::Malformed)?;
        if parser.terminal && event.data.trim() == "[DONE]" {
            continue;
        }
        for event in parser
            .absorb(&event.data)
            .map_err(|_| CheckError::Malformed)?
        {
            if let crate::ProviderEvent::TextDelta(text) = event {
                content |= !text.trim().is_empty();
            }
        }
    }
    if content && parser.terminal {
        Ok(())
    } else {
        Err(CheckError::Malformed)
    }
}

async fn send_request(
    client: &reqwest::Client,
    request: reqwest::Request,
) -> Result<reqwest::Response, reqwest::Error> {
    client.execute(request).await
}
