use super::*;

pub fn encode(
    endpoint: &str,
    responses: bool,
    request: &ChatRequest,
) -> Result<serde_json::Value, VegaError> {
    OpenAiProvider::new(endpoint, "owned-test-credential")?
        .with_responses_api(responses)
        .request_body(request)
}

pub fn decode(
    chunks: &[serde_json::Value],
    finalize: bool,
) -> Vec<Result<ProviderEvent, VegaError>> {
    let mut assembler = sse::SseAssembler::default();
    let mut events = Vec::new();
    for chunk in chunks {
        match assembler.absorb(&chunk.to_string()) {
            Ok(items) => events.extend(items.into_iter().map(Ok)),
            Err(error) => {
                events.push(Err(error));
                return events;
            }
        }
    }
    if finalize {
        match assembler.finalize() {
            Ok(items) => events.extend(items.into_iter().map(Ok)),
            Err(error) => events.push(Err(error)),
        }
    }
    events
}

pub fn stream(chunks: &[serde_json::Value]) -> EventStream {
    Box::pin(futures::stream::iter(decode(chunks, true)))
}
