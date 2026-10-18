use super::*;

pub(super) struct PrimaryWithAuxiliaryFixture {
    primary: Arc<dyn vega_runtime::Provider>,
    auxiliary: vega_runtime::MockProvider,
    predictions: vega_runtime::MockProvider,
}

pub(super) fn with_auxiliary_prediction_fixture(
    primary: Arc<dyn vega_runtime::Provider>,
    predictions: vega_runtime::MockProvider,
) -> Arc<PrimaryWithAuxiliaryFixture> {
    Arc::new(PrimaryWithAuxiliaryFixture {
        primary,
        predictions,
        // Still execute and record the auxiliary request. Empty usable text keeps
        // the specified first-message fallback without consuming any body round.
        auxiliary: vega_runtime::MockProvider::new(vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ])]),
    })
}

type ProviderFuture = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<vega_runtime::EventStream, vega_runtime::VegaError>>
            + Send,
    >,
>;

impl vega_runtime::Provider for PrimaryWithAuxiliaryFixture {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> ProviderFuture {
        self.primary.chat_stream(request, cancel)
    }
    fn chat_stream_once(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> ProviderFuture {
        if request.messages.first().is_some_and(|message| {
            message
                .content
                .starts_with("Predict one useful next user input")
        }) {
            self.predictions.chat_stream_once(request, cancel)
        } else {
            self.auxiliary.chat_stream(request, cancel)
        }
    }
}
