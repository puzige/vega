use super::*;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

struct RecoveryStream {
    provider: OpenAiProvider,
    request: ChatRequest,
    body: serde_json::Value,
    cancel: CancellationToken,
    stream: Option<EventStream>,
    metadata: ProviderResponseMetadata,
    attempt: u32,
    delay: Option<Duration>,
    completed: bool,
    /// Whether this attempt already forwarded user-visible content (text or a
    /// tool call). Replaying such an attempt would duplicate what the user
    /// already saw, so it is never retried.
    emitted_visible: bool,
    ended: bool,
}

impl OpenAiProvider {
    pub(super) fn recovering_stream(
        self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, VegaError> {
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let request = self.prepare_request(request)?;
        let body = self.request_body(&request)?;
        let state = RecoveryStream {
            provider: self,
            request,
            body,
            cancel,
            stream: None,
            metadata: ProviderResponseMetadata::default(),
            attempt: 0,
            delay: None,
            completed: false,
            emitted_visible: false,
            ended: false,
        };
        Ok(ProviderStream {
            metadata: ProviderResponseMetadata::default(),
            events: Box::pin(futures::stream::unfold(state, |mut state| async move {
                if state.ended {
                    return None;
                }
                let item = state.next().await;
                item.map(|item| (item, state))
            })),
        })
    }
}

impl RecoveryStream {
    async fn next(&mut self) -> Option<Result<ProviderEvent, VegaError>> {
        if self.cancel.is_cancelled() {
            self.ended = true;
            return Some(Err(VegaError::Cancelled));
        }
        if let Some(delay) = self.delay.take()
            && let Err(error) = sleep_cancellable(delay, &self.cancel).await
        {
            self.ended = true;
            return Some(Err(error));
        }
        let (error, retry_after) = if let Some(stream) = &mut self.stream {
            let item = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => Some(Err(VegaError::Cancelled)),
                item = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()) => {
                    item.unwrap_or_else(|_| Some(Err(transport_timeout_error())))
                },
            };
            match item {
                Some(Ok(event)) => {
                    self.completed |= matches!(event, ProviderEvent::Done { .. });
                    self.emitted_visible |= match &event {
                        ProviderEvent::TextDelta(text) => {
                            text.chars().any(|character| !character.is_whitespace())
                        }
                        ProviderEvent::ToolUse { .. } => true,
                        _ => false,
                    };
                    return Some(Ok(event));
                }
                Some(Err(error)) => (error, None),
                None if self.completed => {
                    self.ended = true;
                    return None;
                }
                None if self.cancel.is_cancelled() => (VegaError::Cancelled, None),
                None => (incomplete_stream_error(), None),
            }
        } else {
            if let Some(guard) = &self.provider.before_attempt
                && let Err(error) = guard(&self.request)
            {
                self.ended = true;
                return Some(Err(error));
            }
            // Metadata describes the response of the current attempt only; a
            // previous attempt's status must not be reported as this one's.
            self.metadata = ProviderResponseMetadata::default();
            let sent = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    self.ended = true;
                    return Some(Err(VegaError::Cancelled));
                }
                sent = tokio::time::timeout(REQUEST_TIMEOUT, self.provider.send_attempt(&self.body)) => sent,
            };
            match sent {
                Ok(Ok(response)) if response.status().is_success() => {
                    self.metadata = ProviderResponseMetadata {
                        http_status: Some(response.status().as_u16()),
                        request_id: request_id_from_headers(response.headers()),
                        retry_count: Some(self.attempt),
                    };
                    self.stream = Some(if self.provider.responses_api {
                        responses::event_stream_with_recovery(response, self.cancel.clone(), true)
                    } else {
                        sse::event_stream_with_recovery(response, self.cancel.clone(), true)
                    });
                    return Some(Ok(ProviderEvent::ResponseMetadata(self.metadata.clone())));
                }
                Ok(Ok(response)) => {
                    let status = response.status().as_u16();
                    let retry_after = retry_after_of(&response);
                    let request_id = request_id_from_headers(response.headers());
                    let retryable = status == 408 || is_retryable_status(status);
                    let snippet = tokio::time::timeout(
                        REQUEST_TIMEOUT,
                        error_snippet(response, &self.cancel),
                    )
                    .await
                    .unwrap_or_else(|_| "<response body unavailable>".into());
                    let snippet = self.provider.redact_key(&snippet);
                    if self.cancel.is_cancelled() {
                        self.ended = true;
                        return Some(Err(VegaError::Cancelled));
                    }
                    (
                        VegaError::ProviderDiagnostic {
                            kind: if retryable {
                                ProviderFailureKind::Http
                            } else {
                                ProviderFailureKind::Rejected
                            },
                            status: Some(status),
                            message: format!("provider request failed (HTTP {status}): {snippet}"),
                            retryable,
                            protocol_reason: None,
                            retry_count: Some(self.attempt),
                            request_id,
                        },
                        retry_after,
                    )
                }
                Ok(Err(error)) => (
                    VegaError::ProviderDiagnostic {
                        kind: if error.is_builder() {
                            ProviderFailureKind::Rejected
                        } else {
                            ProviderFailureKind::Transport
                        },
                        status: None,
                        message: "provider request transport failed".into(),
                        retryable: !error.is_builder()
                            && !error.is_redirect()
                            && !error.is_decode(),
                        protocol_reason: None,
                        retry_count: Some(self.attempt),
                        request_id: None,
                    },
                    None,
                ),
                Err(_) => (transport_timeout_error(), None),
            }
        };
        let retryable = matches!(
            &error,
            VegaError::ProviderDiagnostic {
                kind: ProviderFailureKind::Transport
                    | ProviderFailureKind::StreamInterrupted
                    | ProviderFailureKind::Timeout
                    | ProviderFailureKind::Http,
                retryable: true,
                ..
            }
        ) || (!self.emitted_visible
            && !self.completed
            && matches!(
                &error,
                VegaError::ProviderDiagnostic {
                    // A stream that failed on an upstream capacity notice or an
                    // unparseable frame before showing anything can be replayed
                    // safely: nothing was displayed and no tool ran. After the
                    // first visible delta the same failure stays terminal so a
                    // consumed attempt is never duplicated.
                    kind: ProviderFailureKind::Overloaded | ProviderFailureKind::Protocol,
                    ..
                }
            ));
        if retryable && !self.completed && self.attempt < self.provider.recovery_retry.max_retries {
            self.stream = None;
            self.emitted_visible = false;
            self.delay = Some(
                self.provider
                    .recovery_retry
                    .delay_for(self.attempt, retry_after),
            );
            self.attempt += 1;
            Some(Ok(ProviderEvent::RetryScheduled {
                attempt: self.attempt,
                max_retries: self.provider.recovery_retry.max_retries,
            }))
        } else {
            self.ended = true;
            Some(Err(match error {
                VegaError::ProviderDiagnostic {
                    kind,
                    status,
                    message,
                    protocol_reason,
                    request_id,
                    ..
                } => VegaError::ProviderDiagnostic {
                    kind,
                    // An established response keeps its status so the failure
                    // line can state the fact instead of implying the
                    // connection never opened.
                    status: status.or(self.metadata.http_status),
                    message,
                    retryable: false,
                    protocol_reason,
                    retry_count: Some(self.attempt),
                    request_id: request_id.or_else(|| self.metadata.request_id.clone()),
                },
                other => other,
            }))
        }
    }
}

pub(super) fn incomplete_stream_error() -> VegaError {
    VegaError::ProviderDiagnostic {
        kind: ProviderFailureKind::StreamInterrupted,
        status: None,
        message: "provider stream ended before completion".into(),
        retryable: true,
        protocol_reason: None,
        retry_count: None,
        request_id: None,
    }
}

fn transport_timeout_error() -> VegaError {
    VegaError::ProviderDiagnostic {
        kind: ProviderFailureKind::Timeout,
        status: None,
        message: "provider request timed out".into(),
        retryable: true,
        protocol_reason: None,
        retry_count: None,
        request_id: None,
    }
}

#[cfg(test)]
mod protocol_diagnostics_tests {
    use super::*;

    fn state(visible: bool, exhausted: bool) -> RecoveryStream {
        let provider =
            OpenAiProvider::new("https://example.invalid/v1", "synthetic").expect("client");
        let attempt = if exhausted {
            provider.recovery_retry.max_retries
        } else {
            0
        };
        let error = crate::chat_compat::protocol_error_with_reason(
            crate::ProviderProtocolReason::InvalidJson,
        );
        RecoveryStream {
            provider,
            request: ChatRequest::default(),
            body: serde_json::json!({}),
            cancel: CancellationToken::new(),
            stream: Some(Box::pin(futures::stream::iter(vec![Err(error)]))),
            metadata: ProviderResponseMetadata {
                http_status: Some(200),
                request_id: Some("synthetic-request".into()),
                retry_count: None,
            },
            attempt,
            delay: None,
            completed: false,
            emitted_visible: visible,
            ended: false,
        }
    }

    #[tokio::test]
    async fn terminal_protocol_error_preserves_reason_and_response_metadata() {
        for (visible, exhausted) in [(true, false), (false, true)] {
            let mut state = state(visible, exhausted);
            let expected_attempt = state.attempt;
            let error = state
                .next()
                .await
                .expect("event")
                .err()
                .expect("terminal error");
            let metrics = error.diagnostic_metrics();
            assert_eq!(metrics.protocol_reason, Some("invalid_json"));
            assert_eq!(metrics.provider.http_status, Some(200));
            assert_eq!(metrics.provider.retry_count, Some(expected_attempt));
            assert_eq!(
                metrics.provider.request_id.as_deref(),
                Some("synthetic-request")
            );
            assert!(state.ended);
        }
        let mut state = state(false, false);
        assert!(matches!(
            state.next().await,
            Some(Ok(ProviderEvent::RetryScheduled { attempt: 1, .. }))
        ));
        assert!(!state.ended);
        assert!(state.stream.is_none());
    }
}
