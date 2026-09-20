//! Owner-only final provider boundary for previously persisted untrusted data.

use std::sync::Arc;

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::{ChatRequest, EventStream, Provider, VegaError};

/// Re-reads owner-held credentials immediately before every model request.
/// The reader never enters a Debug value, request, event, audit, or log.
pub struct OwnerCredentialProvider {
    provider: Arc<dyn Provider>,
    known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
}

impl OwnerCredentialProvider {
    /// Wrap a provider with the current owner-only credential authority.
    pub fn new(
        provider: Arc<dyn Provider>,
        known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    ) -> Self {
        Self { provider, known }
    }

    /// The same final projection check for transports with internal retries.
    /// OpenAI calls it again immediately before every HTTP attempt, while the
    /// outer wrapper still protects other providers and auxiliary requests.
    pub fn pre_attempt_guard(
        known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    ) -> impl Fn(&ChatRequest) -> Result<(), VegaError> + Send + Sync + 'static {
        move |request| reject_owner_secret(request, known.as_ref())
    }
}

fn blocked() -> VegaError {
    VegaError::Provider {
        status: None,
        message: "owner credential appeared in provider projection".into(),
        retryable: false,
    }
}

fn contains_json_secret(value: &serde_json::Value, secret: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text.contains(secret),
        serde_json::Value::Array(items) => {
            items.iter().any(|item| contains_json_secret(item, secret))
        }
        serde_json::Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| key.contains(secret) || contains_json_secret(value, secret)),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            false
        }
    }
}

fn reject_owner_secret(
    request: &ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
) -> Result<(), VegaError> {
    let known = reader().map_err(|_| blocked())?;
    for secret in known.iter().filter(|secret| !secret.is_empty()) {
        if request.messages.iter().any(|message| {
            message.content.contains(secret)
                || message
                    .tool_call_id
                    .as_deref()
                    .is_some_and(|id| id.contains(secret))
                || message
                    .reasoning_content
                    .as_deref()
                    .is_some_and(|reasoning| reasoning.contains(secret))
                || message.tool_calls.iter().any(|call| {
                    call.id.contains(secret)
                        || call.name.contains(secret)
                        || call.input_json.contains(secret)
                })
        }) || request.tools.iter().any(|tool| {
            tool.name.contains(secret)
                || tool.description.contains(secret)
                || contains_json_secret(&tool.input_schema, secret)
                || serde_json::to_string(&tool.input_schema)
                    .map_or(true, |schema| schema.contains(secret))
        }) {
            return Err(blocked());
        }
    }
    Ok(())
}

impl Provider for OwnerCredentialProvider {
    fn chat_stream_once(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        Box::pin(async move {
            reject_owner_secret(&request, known.as_ref())?;
            provider.chat_stream_once(request, cancel).await
        })
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        Box::pin(async move {
            reject_owner_secret(&request, known.as_ref())?;
            provider.chat_stream(request, cancel).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use vega_runtime::{ChatMessage, OpenAiProvider, RetryPolicy};

    async fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let count = stream.read(&mut buffer).await.expect("owned HTTP request");
            assert!(count > 0, "HTTP request closed before headers");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let body_start = header_end + 4;
                let headers = String::from_utf8_lossy(&bytes[..body_start]);
                let body_len = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if bytes.len() >= body_start + body_len {
                    return bytes;
                }
            }
        }
    }

    #[tokio::test]
    async fn issue73_openai_429_retry_rechecks_rotated_keystore_before_second_wire() {
        const ROTATED: &str = "fake-key-rotated-after-429-73";
        let config = tempfile::tempdir().expect("owner config");
        vega_store::keystore::set_key(config.path(), "provider-owned", "fake-initial-key-73")
            .expect("initial owner key");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("owned HTTP fixture");
        let base_url = format!(
            "http://{}/v1",
            listener.local_addr().expect("fixture address")
        );
        let credential_root = config.path().to_path_buf();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.expect("first request");
            let first_request = read_http_request(&mut first).await;
            assert!(String::from_utf8_lossy(&first_request).contains(ROTATED));
            vega_store::keystore::set_key(&credential_root, "provider-owned", ROTATED)
                .expect("rotate owner key during 429 backoff");
            first
                .write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .expect("first response");
            let next = tokio::time::timeout(Duration::from_millis(500), listener.accept()).await;
            if let Ok(Ok((mut second, _))) = next {
                let _ = read_http_request(&mut second).await;
                second
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 14\r\nConnection: close\r\n\r\ndata: [DONE]\n\n")
                    .await
                    .expect("second response");
                true
            } else {
                false
            }
        });
        let reader_root = config.path().to_path_buf();
        let reader: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync> = Arc::new(move || {
            vega_store::keystore::get_key(&reader_root, "provider-owned")
                .map(|key| vec![key])
                .map_err(|_| ())
        });
        let provider = OpenAiProvider::new(base_url, "fake-initial-key-73")
            .expect("owned provider")
            .with_retry_policy(RetryPolicy {
                base_delay: Duration::from_millis(1),
                ..RetryPolicy::default()
            })
            .with_pre_attempt_guard(OwnerCredentialProvider::pre_attempt_guard(reader.clone()));
        let guarded = OwnerCredentialProvider::new(Arc::new(provider), reader);
        let request = ChatRequest {
            model: "fixture-model".into(),
            messages: vec![ChatMessage::tool_result("historical-mcp", ROTATED)],
            ..ChatRequest::default()
        };
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            guarded.chat_stream(request, CancellationToken::new()),
        )
        .await
        .expect("provider request did not stall");
        let second_wire = server.await.expect("owned fixture completed");
        assert!(
            !second_wire,
            "rotated owner secret reached a retry HTTP request"
        );
        assert!(matches!(
            outcome,
            Err(VegaError::Provider {
                retryable: false,
                ..
            })
        ));
    }
}
