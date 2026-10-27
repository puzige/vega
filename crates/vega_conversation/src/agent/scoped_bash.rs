use super::*;
use crate::command_scope::CommandScopeRun;
use crate::types::{CommandScopeDecision, CommandScopeError, PermissionFence};
use vega_tools::{BashError, BashErrorCode, BashLaunchPolicy, BashOutput, PreparedBash, Tools};

pub(super) struct ScopedBashAuthority {
    pub database_path: PathBuf,
    pub fence: PermissionFence,
    pub run_id: String,
}

impl ScopedBashAuthority {
    pub(super) async fn execute(
        &self,
        hook: &dyn PermissionHook,
        call_id: String,
        tools: Tools,
        prepared: PreparedBash,
        policy: BashLaunchPolicy,
        cancel: CancellationToken,
    ) -> Result<BashOutput, BashError> {
        let path = self.database_path.clone();
        let fence = self.fence.clone();
        let run_id = self.run_id.clone();
        let worker_cancel = cancel.clone();
        let (run, pending, scope, prepared, tools) = tokio::task::spawn_blocking(move || {
            let store = Store::open(path).map_err(scope_unavailable)?;
            let bindings = tools.bash_binding_snapshot()?;
            let requested = prepared
                .requested_scope()
                .cloned()
                .ok_or_else(scope_mismatch)?;
            let scope = tools.prepare_bash_scope(&prepared, policy, requested, bindings)?;
            let reader_tools = tools.clone();
            let run = CommandScopeRun::new(
                &store,
                fence,
                run_id,
                worker_cancel,
                Arc::new(move || {
                    reader_tools
                        .bash_binding_snapshot()
                        .map_err(|_| CommandScopeError::StaleAuthority)
                }),
            )
            .map_err(scope_unavailable)?;
            let pending = run
                .request_prepared(
                    &store,
                    &call_id,
                    &prepared,
                    &scope,
                    policy,
                    PERMISSION_TIMEOUT,
                )
                .map_err(scope_unavailable)?;
            Ok::<_, BashError>((run, pending, scope, prepared, tools))
        })
        .await
        .map_err(scope_unavailable)??;
        let request = pending.request().clone();
        let decision = tokio::select! {
            biased;
            _ = cancel.cancelled() => CommandScopeDecision::Timeout,
            result = tokio::time::timeout(PERMISSION_TIMEOUT, hook.request_scope(request.clone(), cancel.clone())) => {
                result.ok().and_then(Result::ok).unwrap_or(CommandScopeDecision::Timeout)
            }
        };
        let path = self.database_path.clone();
        let (receipt, scope, prepared, tools) = tokio::task::spawn_blocking(move || {
            let store = Store::open(path).map_err(scope_unavailable)?;
            scope.validate(&prepared, policy)?;
            let token = run
                .decide(&store, pending, decision)
                .map_err(scope_error)?
                .ok_or_else(|| BashError::new(BashErrorCode::ScopeDenied))?;
            let receipt = run
                .consume(&store, token, &request, &prepared)
                .map_err(scope_unavailable)?;
            Ok::<_, BashError>((receipt, scope, prepared, tools))
        })
        .await
        .map_err(scope_unavailable)??;
        if receipt.request().directories() != scope.directories()
            || receipt.request().bindings() != scope.bindings()
        {
            return Err(scope_mismatch());
        }
        tools
            .execute_bash_scoped(prepared, policy, Some(scope), cancel)
            .await
    }
}

fn scope_mismatch() -> BashError {
    BashError::new(BashErrorCode::ScopeMismatch)
}

fn scope_unavailable(_: impl std::fmt::Display) -> BashError {
    scope_mismatch()
}

fn scope_error(error: CommandScopeError) -> BashError {
    BashError::new(if error == CommandScopeError::Cancelled {
        BashErrorCode::Cancelled
    } else if matches!(
        error,
        CommandScopeError::Expired | CommandScopeError::OwnerGone
    ) {
        BashErrorCode::ScopeDenied
    } else {
        BashErrorCode::ScopeMismatch
    })
}
