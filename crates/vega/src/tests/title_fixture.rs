//! #65 approved fixture adaptation: body and auxiliary requests have separate scripts.
use super::*;

pub(super) struct PrimaryWithAuxiliaryFixture {
    primary: Arc<dyn vega_runtime::Provider>,
    auxiliary: vega_runtime::MockProvider,
}

pub(super) fn with_auxiliary_title_fixture(
    primary: Arc<dyn vega_runtime::Provider>,
) -> Arc<PrimaryWithAuxiliaryFixture> {
    Arc::new(PrimaryWithAuxiliaryFixture {
        primary,
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
        self.auxiliary.chat_stream(request, cancel)
    }
}

#[test]
fn automatic_title_fixture_preserves_body_rounds_and_counts_auxiliary_separately() {
    use vega_runtime::Provider;
    let primary = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        vec![vega_runtime::ScriptStep::text("first body round")],
        vec![vega_runtime::ScriptStep::text("second body round")],
    ]));
    let fixture = with_auxiliary_title_fixture(primary.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let request = |text| vega_runtime::ChatRequest {
            model: "owned".into(),
            messages: vec![vega_runtime::ChatMessage::new(
                vega_runtime::ChatRole::User,
                text,
            )],
            ..Default::default()
        };
        let _first = fixture
            .chat_stream(request("first"), tokio_util::sync::CancellationToken::new())
            .await
            .unwrap();
        let _title = fixture
            .chat_stream_once(request("title"), tokio_util::sync::CancellationToken::new())
            .await
            .unwrap();
        let _second = fixture
            .chat_stream(
                request("second"),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
    });
    let body = primary.requests();
    assert_eq!(body.len(), 2);
    assert_eq!(body[0].messages[0].content, "first");
    assert_eq!(body[1].messages[0].content, "second");
    let auxiliary = fixture.auxiliary.requests();
    assert_eq!(auxiliary.len(), 1);
    assert_eq!(auxiliary[0].messages[0].content, "title");
}
