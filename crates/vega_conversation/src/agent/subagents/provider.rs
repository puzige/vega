use super::*;

pub(super) enum ChildTransport<'a> {
    Borrowed(&'a dyn Provider),
    Owned(Arc<dyn Provider>),
}

pub(super) struct ChildProvider<'a> {
    pub(super) inner: ChildTransport<'a>,
    pub(super) reader: Option<vega_runtime::CredentialReader>,
}

impl ChildProvider<'_> {
    fn provider(&self) -> &dyn Provider {
        match &self.inner {
            ChildTransport::Borrowed(provider) => *provider,
            ChildTransport::Owned(provider) => provider.as_ref(),
        }
    }

    fn validate(&self, request: &vega_runtime::ChatRequest) -> Result<(), VegaError> {
        if let Some(reader) = &self.reader {
            super::super::credential_guard::reject_owner_secret(request, reader.as_ref())?;
        }
        Ok(())
    }
}

impl Provider for ChildProvider<'_> {
    fn prepare_accounting_request(
        &self,
        request: vega_runtime::ChatRequest,
    ) -> BoxFuture<'_, Result<Option<vega_runtime::ChatRequest>, VegaError>> {
        Box::pin(async move {
            self.validate(&request)?;
            self.provider().prepare_accounting_request(request).await
        })
    }

    fn compaction_provider_factory(&self) -> Option<vega_runtime::CompactionProviderFactory> {
        let factory = self.provider().compaction_provider_factory()?;
        let reader = self.reader.clone();
        Some(Box::new(move || {
            Ok(Arc::new(ChildProvider {
                inner: ChildTransport::Owned(factory()?),
                reader,
            }))
        }))
    }

    fn model_snapshot(&self) -> Option<vega_runtime::model_compat::ResolvedModel> {
        self.provider().model_snapshot()
    }

    fn chat_stream_with_submission(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
        submission: vega_runtime::ProviderRequestSubmission,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        if let Err(error) = self.validate(&request) {
            return async move { Err(error) }.boxed();
        }
        self.provider()
            .chat_stream_with_submission(request, cancel, submission)
    }

    fn chat_stream_with_metadata(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        if let Err(error) = self.validate(&request) {
            return async move { Err(error) }.boxed();
        }
        self.provider().chat_stream_with_metadata(request, cancel)
    }

    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        if let Err(error) = self.validate(&request) {
            return async move { Err(error) }.boxed();
        }
        self.provider().chat_stream(request, cancel)
    }

    fn chat_stream_recovering(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        if let Err(error) = self.validate(&request) {
            return async move { Err(error) }.boxed();
        }
        self.provider().chat_stream_recovering(request, cancel)
    }

    fn chat_stream_once_with_metadata(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        if let Err(error) = self.validate(&request) {
            return async move { Err(error) }.boxed();
        }
        self.provider()
            .chat_stream_once_with_metadata(request, cancel)
    }

    fn chat_stream_once(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        if let Err(error) = self.validate(&request) {
            return async move { Err(error) }.boxed();
        }
        self.provider().chat_stream_once(request, cancel)
    }
}
