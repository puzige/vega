use super::*;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use vega_runtime::{VegaError, agent::automation::RuntimeAutomationHook};

pub(crate) struct AutomationBinding {
    path: PathBuf,
    pub(crate) claim: AutomationRunClaim,
    complete: Arc<AtomicBool>,
    pending_usage: Arc<AtomicBool>,
}

impl AutomationBinding {
    pub(crate) fn new(path: PathBuf, claim: AutomationRunClaim) -> Self {
        Self {
            path,
            claim,
            complete: Arc::new(AtomicBool::new(true)),
            pending_usage: Arc::new(AtomicBool::new(false)),
        }
    }
    pub(crate) fn usage_complete(&self) -> bool {
        self.complete.load(Ordering::SeqCst) && !self.pending_usage.load(Ordering::SeqCst)
    }
}

fn tool_error(message: impl Into<String>) -> VegaError {
    VegaError::Tool {
        tool: "automation".into(),
        message: message.into(),
    }
}

impl RuntimeAutomationHook for AutomationBinding {
    fn context(&self) -> serde_json::Value {
        let goal = self
            .claim
            .tasks
            .iter()
            .find(|task| task.kind == AutomationKind::Goal);
        let loop_task = self.claim.tasks.iter().find(|task| {
            self.claim.source == Some(AutomationKind::Loop) && task.kind == AutomationKind::Loop
        });
        serde_json::json!({"goal":goal,"loop":loop_task,"trigger":self.claim.source})
    }
    fn provider_started(&self) {
        if self.pending_usage.swap(true, Ordering::SeqCst) {
            self.complete.store(false, Ordering::SeqCst);
        }
    }
    fn provider_usage(&self) {
        self.pending_usage.store(false, Ordering::SeqCst);
    }
    fn missing_usage(&self) {
        self.complete.store(false, Ordering::SeqCst);
    }
    fn call(
        &self,
        tool: String,
        arguments: serde_json::Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<serde_json::Value, VegaError>> {
        let path = self.path.clone();
        let claim = self.claim.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                if cancel.is_cancelled() {
                    return Err(tool_error("自动任务已撤销"));
                }
                let store = Store::open(path).map_err(|_| tool_error("自动任务存储不可用"))?;
                let tx = store
                    .immediate_transaction()
                    .map_err(|_| tool_error("自动任务存储不可用"))?;
                let result =
                    call_in_transaction(&tx, &claim, &tool, arguments).map_err(tool_error)?;
                if cancel.is_cancelled() {
                    return Err(tool_error("自动任务已撤销"));
                }
                tx.commit().map_err(|_| tool_error("自动任务保存失败"))?;
                Ok(result)
            })
            .await
            .map_err(|_| tool_error("自动任务处理失败"))?
        })
    }
}

pub(super) fn call_in_transaction(
    conn: &Connection,
    claim: &AutomationRunClaim,
    tool: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let Some((finished, data)) = db::run(conn, &claim.thread_id, &claim.id).map_err(storage)?
    else {
        return Err("自动任务已撤销".into());
    };
    if finished {
        return Err("该回合已结束".into());
    }
    let mut run: AutomationRunClaim = decode(&data)?;
    let kind = if tool == "schedule_wakeup" {
        AutomationKind::Loop
    } else {
        AutomationKind::Goal
    };
    let expected = run
        .tasks
        .iter()
        .find(|task| task.kind == kind)
        .ok_or("本轮没有启用该任务")?;
    let task = load_from(conn, &run.thread_id)?
        .into_iter()
        .find(|task| {
            task.identity == expected.identity
                && task.revision == expected.revision
                && task.status == AutomationStatus::Active
        })
        .ok_or("自动任务已变更，请勿重放旧调用")?;
    match tool {
        "get_goal" => {
            if arguments
                .as_object()
                .is_none_or(|object| !object.is_empty())
            {
                return Err("get_goal 不接受参数".into());
            }
            let mut task = task;
            task.used_tokens = task.used_tokens.saturating_add(run.tokens);
            return Ok(
                serde_json::json!({"goal":task,"remaining_tokens":task.token_budget.map(|budget| budget.saturating_sub(task.used_tokens))}),
            );
        }
        "update_goal" => {
            if run.goal_report.is_some() {
                return Err("每个回合只能提交一次目标报告".into());
            }
            let report: GoalReport =
                serde_json::from_value(arguments).map_err(|_| "目标报告格式无效")?;
            if report.reason.trim().is_empty() || report.reason.len() > 16384 {
                return Err("请提供非空、可核实的进展或完成证据".into());
            }
            match report.status.as_str() {
                "active"
                    if matches!(
                        report.progress.as_deref(),
                        Some("progress" | "waiting" | "no_progress")
                    ) => {}
                "complete" => {}
                "blocked"
                    if report.blocker.as_ref().is_some_and(|blocker| {
                        !blocker.trim().is_empty() && blocker.len() <= 4096
                    }) => {}
                _ => return Err("目标报告状态、进展或 blocker 无效".into()),
            }
            run.goal_report = Some(report);
        }
        "schedule_wakeup" => {
            if run.source != Some(AutomationKind::Loop) {
                return Err("只有当前循环回合可以安排唤醒".into());
            }
            let report: WakeupReport =
                serde_json::from_value(arguments).map_err(|_| "唤醒参数无效")?;
            if report
                .delay_seconds
                .is_some_and(|seconds| !(60..=3600).contains(&seconds))
            {
                return Err("唤醒延迟须为 60–3600 秒的整数".into());
            }
            if task.interval_seconds.is_some() && (!report.stop || report.delay_seconds.is_some()) {
                return Err("固定间隔循环仅允许 stop:true 停止，不能调整唤醒时间".into());
            }
            if report
                .reason
                .as_ref()
                .is_some_and(|reason| reason.len() > 16384)
            {
                return Err("唤醒原因过长".into());
            }
            run.wakeup = Some(report);
        }
        _ => return Err("未知自动任务工具".into()),
    }
    db::save_run(conn, &run.thread_id, &run.id, false, &encode(&run)?).map_err(storage)?;
    Ok(serde_json::json!({"recorded":true,"applies_after_success":true}))
}
