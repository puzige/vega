use super::*;
use vega_conversation::shell_recovery::ShellRecoveryService;

impl VegaWindow {
    pub(super) fn request_shell_recovery(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ShellRecoveryRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.source.id, cx) {
            stream.update(cx, |stream, cx| {
                stream.finish_shell_recovery(request, Err(ShellRecoveryError::Changed), cx)
            });
            return;
        }
        let Some(path) = self.context_database(cx) else {
            stream.update(cx, |stream, cx| {
                stream.finish_shell_recovery(request, Err(ShellRecoveryError::Unavailable), cx)
            });
            return;
        };
        let mutation = !matches!(
            request.action,
            ShellRecoveryAction::Read | ShellRecoveryAction::Check(_)
        );
        let lease = if mutation {
            let Some(lease) =
                self.trusted_actions
                    .acquire(TrustedActionKind::ShellRecovery, 0, request.sequence)
            else {
                stream.update(cx, |stream, cx| {
                    stream.finish_shell_recovery(request, Err(ShellRecoveryError::Unavailable), cx)
                });
                return;
            };
            Some(lease)
        } else {
            None
        };
        let request = request.clone();
        let worker_request = request.clone();
        let worker = cx.background_executor().spawn(async move {
            let service = ShellRecoveryService::new(path);
            let source = &worker_request.source;
            match &worker_request.action {
                ShellRecoveryAction::Read => {
                    service.read(source).map(ShellRecoveryOutcome::Projection)
                }
                ShellRecoveryAction::Check(projection) => service
                    .check(source, projection)
                    .map(ShellRecoveryOutcome::Projection),
                ShellRecoveryAction::Prepare(projection) => service
                    .prepare(source, projection)
                    .map(Box::new)
                    .map(ShellRecoveryOutcome::Confirmation),
                ShellRecoveryAction::Resume(confirmation) => service
                    .resume(confirmation)
                    .map(ShellRecoveryOutcome::Projection),
                ShellRecoveryAction::Cancel(operation_id) => {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| ShellRecoveryError::Unavailable)?;
                    runtime
                        .block_on(service.cancel(source, operation_id))
                        .map(ShellRecoveryOutcome::Projection)
                }
                ShellRecoveryAction::Retry(projection) => {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| ShellRecoveryError::Unavailable)?;
                    runtime
                        .block_on(service.retry(source, projection))
                        .map(ShellRecoveryOutcome::Projection)
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(lease) = lease {
                    this.trusted_actions.release(lease);
                }
                stream.update(cx, |stream, cx| {
                    stream.finish_shell_recovery(&request, result, cx)
                });
            });
        })
        .detach();
    }
}
