mod parser;
mod permission;
mod reset;
mod runtime;
pub use parser::parse_command;
pub use permission::AutomationPermissionService;
pub(crate) use reset::pause_for_reset_in_transaction;
pub(crate) use reset::revoke_in_transaction;
pub(crate) use runtime::AutomationBinding;

use crate::types::*;
use serde::{Serialize, de::DeserializeOwned};
use vega_store::Connection;
use vega_store::{Store, automation as db};

pub const LOOP_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| i64::try_from(value.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn decode<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|_| vega_i18n::t("domain.automation.error.read").into())
}
fn encode<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|_| vega_i18n::t("domain.automation.error.encode").into())
}
fn storage(_: impl std::fmt::Display) -> String {
    vega_i18n::t("domain.automation.error.save").into()
}
fn load_from(conn: &Connection, thread: &str) -> Result<Vec<AutomationTask>, String> {
    db::tasks(conn, thread)
        .map_err(storage)?
        .iter()
        .map(|value| decode(value))
        .collect()
}
fn save(conn: &Connection, task: &AutomationTask) -> Result<(), String> {
    db::save_task(
        conn,
        &task.thread_id,
        task.kind.key(),
        &task.identity,
        task.revision,
        &encode(task)?,
    )
    .map_err(storage)
}

pub fn load(store: &Store, thread: &str) -> Result<Vec<AutomationTask>, String> {
    load_from(store.conn(), thread)
}

pub fn authorize_full_access(
    store: &Store,
    expected: &AutomationPermissionAuthorization,
    confirmed: bool,
    now: i64,
) -> Result<AutomationTask, String> {
    let _coordination = crate::permission_contracts::coordinate().map_err(storage)?;
    crate::permission_contracts::require_admission_open(store).map_err(storage)?;
    if !confirmed || !expected.fence.is_valid() || expected.fence.permission_mode != "full_access" {
        return Err(vega_i18n::t("domain.automation.error.changed").into());
    }
    let tx = store.immediate_transaction().map_err(storage)?;
    let thread = &expected.fence.thread_id;
    let owner = vega_store::threads::find(&tx, thread)
        .map_err(storage)?
        .ok_or(vega_i18n::t("domain.automation.error.deleted"))?;
    if owner.status != "active"
        || owner.backend != "native"
        || crate::permission_contracts::capture_fence(store, &owner).map_err(storage)?
            != expected.fence
    {
        return Err(vega_i18n::t("domain.automation.error.changed").into());
    }
    let mut tasks = load_from(&tx, thread)?;
    let task = tasks
        .iter_mut()
        .find(|task| {
            task.kind == expected.kind
                && task.identity == expected.identity
                && task.revision == expected.revision
        })
        .ok_or(vega_i18n::t("domain.automation.error.changed"))?;
    task.permission_authorization = Some(expected.clone());
    task.updated_at_ms = now;
    save(&tx, task)?;
    let result = task.clone();
    tx.commit().map_err(storage)?;
    Ok(result)
}

pub fn timing_snapshot(
    store: &Store,
    thread: &str,
) -> Result<(Vec<AutomationTask>, Option<AutomationRunClaim>), String> {
    let tx = store.immediate_transaction().map_err(storage)?;
    let tasks = load_from(&tx, thread)?;
    let run = db::active_run(&tx, thread)
        .map_err(storage)?
        .map(|data| decode(&data))
        .transpose()?;
    tx.commit().map_err(storage)?;
    Ok((tasks, run))
}

pub fn command(
    store: &Store,
    thread: &str,
    command: &AutomationCommand,
    now: i64,
) -> Result<Vec<AutomationTask>, String> {
    let _coordination = crate::permission_contracts::coordinate().map_err(storage)?;
    crate::permission_contracts::require_admission_open(store).map_err(storage)?;
    let tx = store.immediate_transaction().map_err(storage)?;
    let tasks = command_in_transaction(&tx, thread, command, now)?;
    tx.commit().map_err(storage)?;
    Ok(tasks)
}

pub fn command_for_draft(
    store: &Store,
    draft: &Thread,
    command: &AutomationCommand,
    now: i64,
) -> Result<(Vec<AutomationTask>, Thread), String> {
    let creation =
        crate::permission_contracts::DraftCreationGuard::begin(store, draft).map_err(storage)?;
    let tx = store.immediate_transaction().map_err(storage)?;
    let thread = creation.materialize(&tx).map_err(storage)?;
    let tasks = command_in_transaction(&tx, &thread.id, command, now)?;
    tx.commit().map_err(storage)?;
    Ok((tasks, thread))
}

fn command_in_transaction(
    conn: &Connection,
    thread: &str,
    command: &AutomationCommand,
    now: i64,
) -> Result<Vec<AutomationTask>, String> {
    if vega_store::permission_contracts::load_state(conn)
        .map_err(storage)?
        .reset_pending
    {
        return Err(vega_i18n::t("domain.automation.error.changed").into());
    }
    let owner = vega_store::threads::find(conn, thread)
        .map_err(storage)?
        .ok_or(vega_i18n::t("domain.automation.error.deleted"))?;
    if owner.status != "active" {
        return Err(vega_i18n::t("domain.automation.error.restore").into());
    }
    if owner.backend != "native" {
        return Err(vega_i18n::t("domain.automation.error.native").into());
    }
    let mut tasks = load_from(conn, thread)?;
    match command {
        AutomationCommand::Status(_) => {}
        AutomationCommand::Edit {
            kind,
            identity,
            revision,
            instruction,
        } => {
            if instruction.trim().is_empty() || instruction.len() > 16384 {
                return Err(vega_i18n::t("domain.automation.error.instruction").into());
            }
            let task = tasks
                .iter_mut()
                .find(|task| task.kind == *kind)
                .filter(|task| task.identity == *identity && task.revision == *revision)
                .ok_or(vega_i18n::t("domain.automation.error.changed"))?;
            if task.instruction != *instruction {
                task.revision = task
                    .revision
                    .checked_add(1)
                    .filter(|value| i64::try_from(*value).is_ok())
                    .ok_or(vega_i18n::t("domain.automation.error.revision"))?;
                task.permission_authorization = None;
                task.instruction = instruction.clone();
                task.updated_at_ms = now;
                save(conn, task)?;
            }
        }
        AutomationCommand::Create {
            kind,
            instruction,
            token_budget,
            interval_seconds,
        } => {
            if instruction.trim().is_empty()
                || instruction.len() > 16384
                || token_budget.is_some_and(|value| value == 0 || value > i64::MAX as u64)
                || interval_seconds.is_some_and(|value| !(60..=86400).contains(&value))
            {
                return Err(vega_i18n::t("domain.automation.error.invalid").into());
            }
            if tasks
                .iter()
                .any(|task| task.kind == *kind && !task.status.replaceable())
            {
                return Err(vega_i18n::t("domain.automation.error.exists").into());
            }
            let task = AutomationTask {
                thread_id: thread.into(),
                kind: *kind,
                identity: ulid::Ulid::generate().to_string(),
                revision: 1,
                permission_authorization: None,
                instruction: instruction.clone(),
                status: AutomationStatus::Active,
                token_budget: *token_budget,
                used_tokens: 0,
                usage_complete: true,
                elapsed_ms: 0,
                interval_seconds: *interval_seconds,
                next_due_ms: now,
                expires_at_ms: (*kind == AutomationKind::Loop)
                    .then_some(now.saturating_add(LOOP_LIFETIME_MS)),
                no_progress: 0,
                blocker_count: 0,
                blocker: None,
                reason: None,
                system_reason: None,
                updated_at_ms: now,
            };
            save(conn, &task)?;
        }
        AutomationCommand::Control(kind, control) => {
            if *control == AutomationControl::Clear {
                db::delete_task(conn, thread, kind.key()).map_err(storage)?;
            } else {
                let task = tasks
                    .iter_mut()
                    .find(|task| task.kind == *kind)
                    .ok_or(vega_i18n::t("domain.automation.error.missing"))?;
                if *control == AutomationControl::Resume {
                    if task.status.replaceable() {
                        return Err(vega_i18n::t("domain.automation.error.ended").into());
                    }
                    if task
                        .token_budget
                        .is_some_and(|budget| task.used_tokens >= budget || !task.usage_complete)
                    {
                        return Err(vega_i18n::t("domain.automation.error.budget_reset").into());
                    }
                    task.status = AutomationStatus::Active;
                    task.next_due_ms = now;
                    task.expires_at_ms = (task.kind == AutomationKind::Loop)
                        .then_some(now.saturating_add(LOOP_LIFETIME_MS));
                    task.no_progress = 0;
                    task.blocker_count = 0;
                    task.blocker = None;
                    task.reason = None;
                    task.system_reason = None;
                } else if !task.status.replaceable() {
                    task.status = AutomationStatus::Paused;
                    task.set_system_reason(AutomationReason::UserPaused);
                }
                task.revision = task
                    .revision
                    .checked_add(1)
                    .ok_or(vega_i18n::t("domain.automation.error.revision"))?;
                task.updated_at_ms = now;
                save(conn, task)?;
            }
        }
    }
    let tasks = load_from(conn, thread)?;
    Ok(tasks)
}

pub(crate) fn pause_in_transaction(
    conn: &Connection,
    thread: &str,
    reason: &str,
) -> Result<(), String> {
    for mut task in load_from(conn, thread)? {
        if task.status == AutomationStatus::Active {
            task.status = AutomationStatus::Paused;
            task.set_pause_reason(reason);
            task.revision = task
                .revision
                .checked_add(1)
                .ok_or(vega_i18n::t("domain.automation.error.revision_limit"))?;
            task.updated_at_ms = now_ms();
            save(conn, &task)?;
        }
    }
    Ok(())
}

pub fn pause_all(store: &Store, thread: &str, reason: &str) -> Result<Vec<AutomationTask>, String> {
    let tx = store.immediate_transaction().map_err(storage)?;
    pause_in_transaction(&tx, thread, reason)?;
    let tasks = load_from(&tx, thread)?;
    tx.commit().map_err(storage)?;
    Ok(tasks)
}

pub fn pause_matching(
    store: &Store,
    thread: &str,
    expected: &[AutomationTask],
    reason: &str,
) -> Result<(), String> {
    let tx = store.immediate_transaction().map_err(storage)?;
    for mut task in load_from(&tx, thread)? {
        if task.status == AutomationStatus::Active
            && expected.iter().any(|snapshot| {
                snapshot.identity == task.identity && snapshot.revision == task.revision
            })
        {
            task.status = AutomationStatus::Paused;
            task.set_pause_reason(reason);
            task.revision = task
                .revision
                .checked_add(1)
                .ok_or(vega_i18n::t("domain.automation.error.revision_limit"))?;
            task.updated_at_ms = now_ms();
            save(&tx, &task)?;
        }
    }
    tx.commit().map_err(storage)
}

pub fn recover_startup(store: &Store) -> Result<(), String> {
    let _coordination = crate::permission_contracts::coordinate().map_err(storage)?;
    crate::permission_contracts::require_admission_open(store).map_err(storage)?;
    if vega_store::permission_reset::load(store.conn())
        .map_err(storage)?
        .is_some_and(|record| !record.completed)
    {
        return Err(vega_i18n::t("domain.automation.error.changed").into());
    }
    for data in db::active_runs(store.conn()).map_err(storage)? {
        let run: AutomationRunClaim = decode(&data)?;
        finish(store, &run, false, false, 0, now_ms())?;
    }
    let tx = store.immediate_transaction().map_err(storage)?;
    let tasks: Vec<AutomationTask> = db::all_tasks(&tx)
        .map_err(storage)?
        .iter()
        .map(|value| decode(value))
        .collect::<Result<_, _>>()?;
    for task in tasks {
        pause_in_transaction(&tx, &task.thread_id, "app_restarted")?;
    }
    db::recover_claims(&tx).map_err(storage)?;
    tx.commit().map_err(storage)
}

pub fn due_kind(tasks: &[AutomationTask], now: i64) -> Option<AutomationKind> {
    [AutomationKind::Loop, AutomationKind::Goal]
        .into_iter()
        .find(|kind| {
            tasks.iter().any(|task| {
                task.kind == *kind
                    && task.status == AutomationStatus::Active
                    && task.next_due_ms <= now
                    && !task.expires_at_ms.is_some_and(|expiry| expiry <= now)
            })
        })
}

pub fn refresh(store: &Store, thread: &str, now: i64) -> Result<Vec<AutomationTask>, String> {
    let tx = store.immediate_transaction().map_err(storage)?;
    if vega_store::threads::find(&tx, thread)
        .map_err(storage)?
        .is_none()
    {
        return Ok(Vec::new());
    }
    for mut task in load_from(&tx, thread)? {
        if task.status == AutomationStatus::Active
            && task.expires_at_ms.is_some_and(|expiry| expiry <= now)
        {
            task.status = AutomationStatus::Expired;
            task.set_system_reason(AutomationReason::LoopExpired);
            task.revision = task
                .revision
                .checked_add(1)
                .ok_or(vega_i18n::t("domain.automation.error.revision_limit"))?;
            save(&tx, &task)?;
        }
    }
    let result = load_from(&tx, thread)?;
    tx.commit().map_err(storage)?;
    Ok(result)
}

pub fn claim(
    store: &Store,
    thread: &str,
    source: Option<AutomationKind>,
    now: i64,
) -> Result<Option<AutomationRunClaim>, String> {
    let _coordination = crate::permission_contracts::coordinate().map_err(storage)?;
    crate::permission_contracts::require_admission_open(store).map_err(storage)?;
    let tx = store.immediate_transaction().map_err(storage)?;
    let owner = vega_store::threads::find(&tx, thread)
        .map_err(storage)?
        .ok_or(vega_i18n::t("domain.automation.error.deleted"))?;
    if owner.status != "active" {
        return Err(vega_i18n::t("domain.automation.error.archived").into());
    }
    if let Some(data) = db::active_run(&tx, thread).map_err(storage)? {
        let prior: AutomationRunClaim = decode(&data)?;
        if source.is_none() && prior.source.is_some() && !prior.started {
            db::save_run(&tx, thread, &prior.id, true, &data).map_err(storage)?;
        } else if source.is_some() {
            return Ok(None);
        } else {
            return Err(vega_i18n::t("domain.automation.error.running").into());
        }
    }
    let tasks: Vec<_> = load_from(&tx, thread)?
        .into_iter()
        .filter(|task| task.status == AutomationStatus::Active)
        .collect();
    if tasks.is_empty() {
        tx.commit().map_err(storage)?;
        return Ok(None);
    }
    if let Some(kind) = source {
        if due_kind(&tasks, now) != Some(kind) {
            return Ok(None);
        }
        let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM pending_messages WHERE thread_id=?1 AND state!='delivered') OR EXISTS(SELECT 1 FROM messages WHERE thread_id=?1 AND plan_status='pending') OR EXISTS(SELECT 1 FROM credential_continuations WHERE thread_id=?1 AND state IN ('ready','claimed','unknown'))",[thread],|row| row.get(0)).map_err(storage)?;
        if pending {
            return Ok(None);
        }
    }
    if tasks.iter().any(|task| {
        task.kind == AutomationKind::Goal
            && task
                .token_budget
                .is_some_and(|budget| !task.usage_complete || task.used_tokens >= budget)
    }) {
        return Err(vega_i18n::t("domain.automation.error.budget").into());
    }
    let permission_fence =
        crate::permission_contracts::capture_fence(store, &owner).map_err(storage)?;
    if permission_fence.permission_mode == "full_access"
        && tasks.iter().any(|task| {
            !task
                .permission_authorization
                .as_ref()
                .is_some_and(|authorization| {
                    authorization.kind == task.kind
                        && authorization.identity == task.identity
                        && authorization.revision == task.revision
                        && authorization.fence == permission_fence
                })
        })
    {
        return Err(vega_i18n::t("domain.automation.error.changed").into());
    }
    let run = AutomationRunClaim {
        id: ulid::Ulid::generate().to_string(),
        thread_id: thread.into(),
        source,
        started: source.is_none(),
        tasks,
        permission_fence: Some(permission_fence),
        usage_ids: Vec::new(),
        tokens: 0,
        goal_report: None,
        wakeup: None,
    };
    db::insert_run(&tx, thread, &run.id, &encode(&run)?).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(Some(run))
}

pub fn activate_claim(
    store: &Store,
    claim: &AutomationRunClaim,
) -> Result<Option<AutomationRunClaim>, String> {
    let _coordination = crate::permission_contracts::coordinate().map_err(storage)?;
    crate::permission_contracts::require_admission_open(store).map_err(storage)?;
    let tx = store.immediate_transaction().map_err(storage)?;
    if !current_claim(store, claim)? {
        return Ok(None);
    }
    let Some((_, data)) = db::run(&tx, &claim.thread_id, &claim.id).map_err(storage)? else {
        return Ok(None);
    };
    let mut run: AutomationRunClaim = decode(&data)?;
    if run.started {
        return Ok(None);
    }
    let current = load_from(&tx, &claim.thread_id)?;
    run.tasks.retain(|expected| {
        current.iter().any(|task| {
            task.identity == expected.identity
                && task.revision == expected.revision
                && task.status == AutomationStatus::Active
        })
    });
    run.started = true;
    db::save_run(&tx, &run.thread_id, &run.id, false, &encode(&run)?).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(Some(run))
}

pub fn finish_unsettled_claim(
    store: &Store,
    claim: &AutomationRunClaim,
    now: i64,
) -> Result<(), String> {
    let Some((finished, data)) =
        db::run(store.conn(), &claim.thread_id, &claim.id).map_err(storage)?
    else {
        return Ok(());
    };
    if finished {
        return Ok(());
    }
    let run: AutomationRunClaim = decode(&data)?;
    finish(store, &run, false, !run.started, 0, now)
}

pub fn current_claim(store: &Store, claim: &AutomationRunClaim) -> Result<bool, String> {
    let Some((finished, data)) =
        db::run(store.conn(), &claim.thread_id, &claim.id).map_err(storage)?
    else {
        return Ok(false);
    };
    if finished {
        return Ok(false);
    }
    let saved: AutomationRunClaim = decode(&data)?;
    let Some(expected_fence) = &claim.permission_fence else {
        return Ok(false);
    };
    if saved.permission_fence != claim.permission_fence
        || saved.thread_id != claim.thread_id
        || saved.source != claim.source
        || saved.tasks != claim.tasks
        || !expected_fence.is_valid()
    {
        return Ok(false);
    }
    let owner = vega_store::threads::find(store.conn(), &claim.thread_id).map_err(storage)?;
    let Some(owner) = owner else {
        return Ok(false);
    };
    let current_fence =
        crate::permission_contracts::capture_fence(store, &owner).map_err(storage)?;
    if &current_fence != expected_fence
        || (current_fence.permission_mode == "full_access"
            && claim.tasks.iter().any(|task| {
                !task
                    .permission_authorization
                    .as_ref()
                    .is_some_and(|authorization| {
                        authorization.kind == task.kind
                            && authorization.identity == task.identity
                            && authorization.revision == task.revision
                            && &authorization.fence == expected_fence
                    })
            }))
    {
        return Ok(false);
    }
    if claim.tasks.is_empty()
        || claim
            .source
            .is_some_and(|kind| !claim.tasks.iter().any(|task| task.kind == kind))
    {
        return Ok(false);
    }
    let tasks = load(store, &claim.thread_id)?;
    Ok(claim
        .tasks
        .iter()
        .filter(|task| claim.source.is_none_or(|kind| kind == task.kind))
        .all(|expected| {
            tasks.iter().any(|task| {
                task.kind == expected.kind
                    && task.identity == expected.identity
                    && task.revision == expected.revision
                    && task.permission_authorization == expected.permission_authorization
                    && task.status == AutomationStatus::Active
            })
        }))
}

pub fn release_claim(store: &Store, claim: &AutomationRunClaim) -> Result<(), String> {
    db::save_run(
        store.conn(),
        &claim.thread_id,
        &claim.id,
        true,
        &encode(claim)?,
    )
    .map_err(storage)
}

pub fn finish(
    store: &Store,
    claim: &AutomationRunClaim,
    success: bool,
    usage_complete: bool,
    elapsed_ms: u64,
    now: i64,
) -> Result<(), String> {
    let tx = store.immediate_transaction().map_err(storage)?;
    let Some((finished, data)) = db::run(&tx, &claim.thread_id, &claim.id).map_err(storage)? else {
        return Ok(());
    };
    if finished {
        return Ok(());
    }
    let run: AutomationRunClaim = decode(&data)?;
    let tokens = run.tokens;
    let complete = usage_complete;
    for mut task in load_from(&tx, &run.thread_id)? {
        let Some(expected) = run
            .tasks
            .iter()
            .find(|expected| expected.identity == task.identity)
        else {
            continue;
        };
        if task.kind == AutomationKind::Goal {
            task.used_tokens = task
                .used_tokens
                .checked_add(tokens)
                .ok_or(vega_i18n::t("domain.automation.error.usage_limit"))?;
            task.usage_complete &= complete;
        }
        task.elapsed_ms = task.elapsed_ms.saturating_add(elapsed_ms);
        if task.revision == expected.revision && task.status == AutomationStatus::Active {
            if !success {
                task.status = AutomationStatus::Paused;
                task.set_system_reason(AutomationReason::RunIncomplete);
            } else if task.kind == AutomationKind::Goal {
                apply_goal_report(&mut task, run.goal_report.as_ref());
                task.next_due_ms = now.saturating_add(1000);
            } else if run.source == Some(AutomationKind::Loop) {
                if run.wakeup.as_ref().is_some_and(|report| report.stop) {
                    task.status = AutomationStatus::Stopped;
                } else {
                    let interval = task
                        .interval_seconds
                        .or_else(|| run.wakeup.as_ref().and_then(|report| report.delay_seconds))
                        .unwrap_or(300);
                    task.next_due_ms = now.saturating_add((interval * 1000) as i64);
                }
                task.reason = run.wakeup.as_ref().and_then(|report| report.reason.clone());
                task.system_reason = None;
            }
        }
        if task.kind == AutomationKind::Goal && task.token_budget.is_some() {
            if !task.usage_complete {
                task.status = AutomationStatus::Paused;
                task.set_system_reason(AutomationReason::UsageUnverified);
            } else if !task.status.replaceable()
                && task
                    .token_budget
                    .is_some_and(|budget| task.used_tokens >= budget)
            {
                task.status = AutomationStatus::BudgetLimited;
                task.set_system_reason(AutomationReason::BudgetExhausted);
            }
        }
        task.updated_at_ms = now;
        save(&tx, &task)?;
    }
    db::save_run(&tx, &run.thread_id, &run.id, true, &data).map_err(storage)?;
    tx.commit().map_err(storage)
}

fn apply_goal_report(task: &mut AutomationTask, report: Option<&GoalReport>) {
    if let Some(report) = report {
        task.reason = Some(report.reason.clone());
        task.system_reason = None;
        if report.status == "complete" {
            task.status = AutomationStatus::Complete;
            return;
        }
        if report.status == "blocked" {
            if task.blocker == report.blocker {
                task.blocker_count = task.blocker_count.saturating_add(1);
            } else {
                task.blocker_count = 1;
                task.blocker = report.blocker.clone();
            }
            task.no_progress = 0;
            if task.blocker_count >= 3 {
                task.status = AutomationStatus::Blocked;
            }
            return;
        }
        if matches!(report.progress.as_deref(), Some("progress" | "waiting")) {
            task.no_progress = 0;
            task.blocker_count = 0;
            task.blocker = None;
            return;
        }
    }
    task.blocker_count = 0;
    task.blocker = None;
    task.no_progress = task.no_progress.saturating_add(1);
    if task.no_progress >= 3 {
        task.status = AutomationStatus::Blocked;
        task.set_system_reason(AutomationReason::NoProgress);
    }
}

pub(crate) fn record_usage(
    conn: &Connection,
    claim: &AutomationRunClaim,
    id: i64,
    input: u64,
    output: u64,
) -> Result<(), String> {
    let Some((finished, data)) = db::run(conn, &claim.thread_id, &claim.id).map_err(storage)?
    else {
        return Ok(());
    };
    if finished {
        return Ok(());
    }
    let mut run: AutomationRunClaim = decode(&data)?;
    if !run.usage_ids.contains(&id) {
        run.tokens = run
            .tokens
            .checked_add(input)
            .and_then(|tokens| tokens.checked_add(output))
            .ok_or(vega_i18n::t("domain.automation.error.usage_limit"))?;
        run.usage_ids.push(id);
        db::save_run(conn, &run.thread_id, &run.id, false, &encode(&run)?).map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn check_subagent_budget(
    conn: &Connection,
    claim: &AutomationRunClaim,
) -> Result<(), String> {
    let Some((finished, data)) = db::run(conn, &claim.thread_id, &claim.id).map_err(storage)?
    else {
        return Err("parent task is no longer active".into());
    };
    if finished {
        return Err("parent task is no longer active".into());
    }
    let run: AutomationRunClaim = decode(&data)?;
    for expected in &claim.tasks {
        let task = load_from(conn, &claim.thread_id)?
            .into_iter()
            .find(|task| {
                task.identity == expected.identity
                    && task.revision == expected.revision
                    && task.status == AutomationStatus::Active
            })
            .ok_or("parent task has changed")?;
        if task
            .token_budget
            .is_some_and(|budget| task.used_tokens.saturating_add(run.tokens) >= budget)
        {
            return Err("parent token budget is exhausted".into());
        }
    }
    Ok(())
}
