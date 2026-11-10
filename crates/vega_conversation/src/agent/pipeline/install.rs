use super::*;
use crate::database_writer::{DatabaseCommand, Sealed};

pub(super) struct InitializePreparation {
    pub database_path: PathBuf,
    pub thread_id: String,
    pub assistant_message_id: String,
    pub user_message_id: String,
    pub user_content: String,
    pub images: Vec<crate::types::ImageAttachment>,
    pub config: authority::CommitAuthority,
    pub ordinary: Option<super::super::ordinary::OrdinaryPreparation>,
    pub delegated_owner: Option<Arc<vega_store::delegated_ownership::DelegatedOwnership>>,
    pub recover: bool,
    pub uses_existing_user: bool,
}

impl Sealed for InitializePreparation {}

impl DatabaseCommand for InitializePreparation {
    type Output = Result<(authority::RunAuthority, Duration), ConversationError>;

    fn apply(self, store: &Store) -> Self::Output {
        self.apply_with_admission(store, Duration::from_secs(10))
    }

    fn apply_with_admission(self, store: &Store, remaining: Duration) -> Self::Output {
        let result = (|| {
            let permission = crate::permission_contracts::coordinate()?;
            validate_admission(
                store,
                &self.config,
                self.ordinary.as_ref(),
                &self.database_path,
                &self.thread_id,
                self.delegated_owner.as_deref(),
            )?;
            let tx = store
                .immediate_transaction_with_budget(
                    || cancelled(self.ordinary.as_ref(), &self.config),
                    remaining,
                )
                .map_err(runtime_store_error)?;
            let remaining = tx.admission_remaining().ok_or_else(|| {
                ConversationError::Store("preparation admission budget unavailable".into())
            })?;
            let result = (|| {
                validate_admission(
                    store,
                    &self.config,
                    self.ordinary.as_ref(),
                    &self.database_path,
                    &self.thread_id,
                    self.delegated_owner.as_deref(),
                )?;
                if self.recover {
                    vega_store::recovery::recover_thread_with_owner(
                        &tx,
                        &self.thread_id,
                        now_ms(),
                        self.delegated_owner.as_deref(),
                    )
                    .map_err(runtime_store_error)?;
                }
                authority::verified(
                    store,
                    &self.config,
                    &self.thread_id,
                    &self.assistant_message_id,
                    &self.user_message_id,
                    self.uses_existing_user,
                    &self.user_content,
                    &self.images,
                    self.ordinary.as_ref(),
                )
            })();
            let result = match result {
                Ok(result) => {
                    tx.commit().map_err(runtime_store_error)?;
                    Ok((result, remaining))
                }
                Err(error) => {
                    rollback_preparation(
                        tx,
                        self.ordinary.as_ref(),
                        None,
                        &self.user_message_id,
                        &self.user_content,
                        &self.images,
                    );
                    Err(error)
                }
            };
            drop(permission);
            result
        })();
        if let Some(ordinary) = self.ordinary {
            ordinary.complete(result.as_ref().map(|_| ()));
        }
        result
    }
}

pub(super) fn cancelled(
    ordinary: Option<&super::super::ordinary::OrdinaryPreparation>,
    config: &authority::CommitAuthority,
) -> bool {
    config
        .steering_cancel
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
        || ordinary.is_some_and(|ordinary| {
            let state = ordinary.run.submission.snapshot();
            state.unavailable || state.retired
        })
}

fn validate_admission(
    store: &Store,
    config: &authority::CommitAuthority,
    ordinary: Option<&super::super::ordinary::OrdinaryPreparation>,
    database: &std::path::Path,
    thread: &str,
    owner: Option<&vega_store::delegated_ownership::DelegatedOwnership>,
) -> Result<(), ConversationError> {
    crate::permission_contracts::require_admission_open(store)?;
    super::super::ordinary::verified_admission(store, thread, config.admission.as_ref())?;
    if cancelled(ordinary, config) {
        return Err(ConversationError::Runtime(Arc::new(VegaError::Cancelled)));
    }
    if vega_store::permission_reset::load(store.conn())
        .map_err(runtime_store_error)?
        .is_some_and(|record| !record.completed)
        || owner.is_some_and(|owner| !owner.verifies(database, thread))
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    if let Some(ordinary) = ordinary {
        ordinary.run.verify(store, thread)?;
    }
    if let Some(row) =
        vega_store::subagents::find(store.conn(), thread).map_err(runtime_store_error)?
    {
        let release = vega_store::delegated_stages::release_state(
            store.conn(),
            thread,
            &row.child_message_id,
        )
        .map_err(runtime_store_error)?;
        if release == "quarantined"
            || (owner.is_none()
                && release == "held"
                && !matches!(row.state.as_str(), "running" | "cancelling"))
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
    }
    Ok(())
}

pub(super) struct InstallPreparation {
    pub database_path: PathBuf,
    pub thread_id: String,
    pub assistant_message_id: String,
    pub user_message_id: String,
    pub user_content: String,
    pub images: Vec<crate::types::ImageAttachment>,
    pub config: authority::CommitAuthority,
    pub ordinary: Option<super::super::ordinary::OrdinaryPreparation>,
    pub delegated_owner: Option<Arc<vega_store::delegated_ownership::DelegatedOwnership>>,
    pub uses_existing_user: bool,
    pub expected: authority::RunAuthority,
    pub install: vega_store::preparation::OwnedDelegatedInstall,
    pub redaction_authority: Option<Arc<crate::privacy::PreparedRedactionAuthority>>,
    pub skill_run: Option<vega_runtime::skills::SkillRun>,
    pub audits: Vec<vega_runtime::skills::ActivationAudit>,
    pub context_policy: Option<vega_store::context_compaction::ModelContextPolicy>,
    pub exact_rules: Vec<RuntimeExactRule>,
    pub admission_remaining: Duration,
}

pub(super) enum InstallOutcome {
    Conflict {
        remaining: Duration,
    },
    Committed {
        legacy_cleanup: bool,
        next_tool_seq: i64,
        assistant_seq: i64,
        title_request: bool,
        user_sent_at_ms: Option<i64>,
        user_timestamp_break_before: bool,
    },
}

impl Sealed for InstallPreparation {}

impl DatabaseCommand for InstallPreparation {
    type Output = Result<InstallOutcome, ConversationError>;

    fn apply(self, store: &Store) -> Self::Output {
        let remaining = self.admission_remaining;
        self.apply_with_admission(store, remaining)
    }

    fn apply_with_admission(self, store: &Store, remaining: Duration) -> Self::Output {
        let Self {
            database_path,
            thread_id,
            assistant_message_id,
            user_message_id,
            user_content,
            images,
            config,
            ordinary,
            delegated_owner,
            uses_existing_user,
            expected,
            install,
            redaction_authority,
            skill_run,
            audits,
            context_policy,
            exact_rules,
            admission_remaining: _,
        } = self;
        let owned_ordinary = ordinary;
        let ordinary = owned_ordinary.as_ref();
        let result = (|| {
            let permission = crate::permission_contracts::coordinate()?;
            validate_admission(
                store,
                &config,
                ordinary,
                &database_path,
                &thread_id,
                delegated_owner.as_deref(),
            )?;
            let mut transaction = store
                .immediate_transaction_with_budget(|| cancelled(ordinary, &config), remaining)
                .map_err(runtime_store_error)?;
            let remaining = transaction.admission_remaining().ok_or_else(|| {
                ConversationError::Store("preparation admission budget unavailable".into())
            })?;
            let recovery = ordinary
                .map(|value| value.run.recovery())
                .transpose()?
                .flatten();
            let result = (|| -> Result<_, ConversationError> {
                validate_admission(
                    store,
                    &config,
                    ordinary,
                    &database_path,
                    &thread_id,
                    delegated_owner.as_deref(),
                )?;
                let current = authority::verified(
                    store,
                    &config,
                    &thread_id,
                    &assistant_message_id,
                    &user_message_id,
                    uses_existing_user,
                    &user_content,
                    &images,
                    ordinary,
                )?;
                if !expected.matches(&current) {
                    if let Some(ordinary) = ordinary {
                        let _ = ordinary
                            .run
                            .submission
                            .invalidate_recovery(ordinary.worker.attempt_id());
                    }
                    return Err(ConversationError::InvalidTaskIdentity);
                }
                if let Some(authority) = &redaction_authority {
                    authority
                        .validate(store)
                        .map_err(|error| ConversationError::Runtime(Arc::new(error.into())))?;
                }
                if let Some(run) = &skill_run {
                    run.validate_prepared_sources()
                        .map_err(skill_preparation_error)?;
                    let binding = run
                        .binding()
                        .ok_or(ConversationError::InvalidTaskIdentity)?;
                    let settings = vega_store::skills::read_settings(&transaction)
                        .map_err(|error| ConversationError::Store(error.to_string()))?;
                    if settings.consent_generation != binding.consent_generation()
                        || settings.revocation_generation != binding.revocation_generation()
                    {
                        return Err(ConversationError::InvalidTaskIdentity);
                    }
                }
                if let Some(expected_policy) = &context_policy {
                    let current_policy =
                        super::super::compaction::load_current_model_context_policy(
                            &transaction,
                            &expected_policy.provider,
                            &expected_policy.model,
                        )
                        .map_err(|error| ConversationError::Store(error.to_string()))?;
                    if &current_policy != expected_policy {
                        return Err(ConversationError::InvalidTaskIdentity);
                    }
                }
                if load_exact_rules(&transaction, &config, &current.thread.project_id)?
                    != exact_rules
                {
                    return Err(ConversationError::InvalidTaskIdentity);
                }
                if !install
                    .validate(&transaction)
                    .map_err(runtime_store_error)?
                {
                    return Ok(None);
                }
                let authority::RunAuthority {
                    fence: permission_fence,
                    thread,
                    run_mode,
                    ..
                } = current;
                let permission_fence_json = serde_json::to_string(&permission_fence)
                    .map_err(|error| ConversationError::Store(error.to_string()))?;
                let pending_original = config
                    .pending_message_id
                    .as_ref()
                    .map(|id| {
                        if id != &user_message_id || uses_existing_user {
                            return Err(ConversationError::Store(
                                vega_i18n::t("domain.pending.error.changed").into(),
                            ));
                        }
                        let row = vega_store::pending_messages::find_dispatchable(
                            &transaction,
                            &thread_id,
                            id,
                        )
                        .map_err(runtime_store_error)?
                        .ok_or_else(|| {
                            ConversationError::Store(
                                vega_i18n::t("domain.pending.error.delivered").into(),
                            )
                        })?;
                        let created_at = row.created_at;
                        let pending = crate::pending_messages::decode(row)?;
                        if pending.images != images {
                            return Err(ConversationError::Store(
                                vega_i18n::t("domain.pending.error.attachments").into(),
                            ));
                        }
                        Ok::<_, ConversationError>((pending.content, created_at))
                    })
                    .transpose()?;
                let pending_model_content = pending_original.as_ref().map(|_| user_content.clone());
                let continuation = config
                    .credential_continuation_id
                    .as_deref()
                    .map(|id| {
                        let receipt = vega_store::credential_continuation::find(&transaction, id)
                            .map_err(runtime_store_error)?
                            .ok_or_else(|| {
                                ConversationError::Runtime(Arc::new(
                                    VegaError::CredentialContinuationUnavailable,
                                ))
                            })?;
                        if receipt.thread_id != thread_id
                            || receipt.user_message_id != user_message_id
                            || !vega_store::credential_continuation::claim(
                                &transaction,
                                &receipt,
                                &assistant_message_id,
                                &permission_fence_json,
                            )
                            .map_err(runtime_store_error)?
                        {
                            return Err(ConversationError::Runtime(Arc::new(
                                VegaError::CredentialContinuationUnavailable,
                            )));
                        }
                        Ok::<_, ConversationError>(receipt)
                    })
                    .transpose()?;
                let legacy_cleanup = install
                    .install(&mut transaction)
                    .map_err(runtime_store_error)?;
                let now = install.assistant().created_at;
                let title_eligible_payload = !user_content.trim().is_empty() || !images.is_empty();
                if let (Some(ordinary), Some(receipt)) = (ordinary, &recovery) {
                    super::super::ordinary::verify_recovery(
                        &transaction,
                        &ordinary.run,
                        &thread_id,
                        receipt,
                    )?;
                } else if config.delegated {
                    let mismatch = || {
                        ConversationError::CorruptRow(
                            "delegated instruction identity mismatch".into(),
                        )
                    };
                    let execution = vega_store::subagents::execution(
                        &transaction,
                        &thread_id,
                        &assistant_message_id,
                    )
                    .map_err(runtime_store_error)?
                    .ok_or_else(mismatch)?;
                    let delegation = execution.record;
                    let existing = messages::find(&transaction, &user_message_id)
                        .map_err(runtime_store_error)?
                        .ok_or_else(mismatch)?;
                    let parent = messages::find(&transaction, &delegation.parent_message_id)
                        .map_err(runtime_store_error)?
                        .ok_or_else(mismatch)?;
                    if delegation.child_thread_id != thread_id
                        || delegation.child_message_id != assistant_message_id
                        || config.delegated_message_id.as_deref()
                            != Some(assistant_message_id.as_str())
                        || config
                            .delegated_parent
                            .as_ref()
                            .is_none_or(|(thread, message)| {
                                thread != &delegation.parent_thread_id
                                    || message != &delegation.parent_message_id
                            })
                        || thread.parent_thread_id.as_deref()
                            != Some(delegation.parent_thread_id.as_str())
                        || parent.thread_id != delegation.parent_thread_id
                        || parent.role != "assistant"
                        || !matches!(delegation.state.as_str(), "running" | "cancelling")
                        || existing.thread_id != thread_id
                        || existing.role != "user"
                        || existing.kind != "text"
                        || existing.status != "done"
                        || execution.instruction_message_id.as_deref()
                            != Some(user_message_id.as_str())
                        || existing.content != delegation.task
                        || user_content != delegation.task
                        || run_mode != ThreadMode::Execute
                    {
                        return Err(mismatch());
                    }
                    let next = messages::next_seq(&transaction, &thread_id)
                        .map_err(runtime_store_error)?;
                    if existing.seq.checked_add(1) != Some(next) {
                        return Err(ConversationError::CorruptRow(
                            "delegated instruction was already consumed".into(),
                        ));
                    }
                } else if uses_existing_user {
                    let existing = messages::find(&transaction, &user_message_id)
                        .map_err(runtime_store_error)?
                        .ok_or_else(|| ConversationError::NotFound(user_message_id.clone()))?;
                    if existing.thread_id != thread_id
                        || existing.role != "user"
                        || existing.kind != "text"
                        || existing.status != "done"
                        || existing.content != crate::plans::APPROVAL_INSTRUCTION
                        || user_content != crate::plans::APPROVAL_INSTRUCTION
                        || run_mode != ThreadMode::Execute
                        || !vega_store::permission_reset::replay_allowed(
                            &transaction,
                            &thread_id,
                            existing.seq,
                        )
                        .map_err(runtime_store_error)?
                    {
                        return Err(ConversationError::CorruptRow(
                            "approved instruction identity mismatch".to_string(),
                        ));
                    }
                    let next = messages::next_seq(&transaction, &thread_id)
                        .map_err(runtime_store_error)?;
                    if next != existing.seq + 1 {
                        return Err(ConversationError::CorruptRow(
                            "approved instruction was already consumed".to_string(),
                        ));
                    }
                    let plans = messages::plans_for_thread(&transaction, &thread_id)
                        .map_err(runtime_store_error)?;
                    let matching_approvals = plans
                        .iter()
                        .filter(|plan| plan.seq < existing.seq)
                        .filter(|plan| {
                            plan.plan_status.as_deref() == Some("approved")
                                && plan.plan_reviewed_at == Some(existing.created_at)
                        })
                        .count();
                    if matching_approvals != 1 {
                        return Err(ConversationError::CorruptRow(
                            "approved instruction has no matching plan".to_string(),
                        ));
                    }
                } else if continuation.is_none() && config.interrupted_assistant.is_none() {
                    let user_seq = messages::next_seq(&transaction, &thread_id)
                        .map_err(runtime_store_error)?;
                    messages::insert(
                        &transaction,
                        &messages::MessageRow {
                            id: user_message_id.clone(),
                            thread_id: thread_id.clone(),
                            seq: user_seq,
                            role: "user".to_string(),
                            kind: "text".to_string(),
                            content: pending_original
                                .as_ref()
                                .map(|(content, _)| content.clone())
                                .unwrap_or_else(|| user_content.clone()),
                            status: "done".to_string(),
                            created_at: pending_original
                                .as_ref()
                                .map(|(_, created_at)| *created_at)
                                .unwrap_or(now),
                            plan_status: None,
                            plan_review_note: None,
                            plan_reviewed_at: None,
                        },
                    )
                    .map_err(runtime_store_error)?;
                }
                for (ordinal, image) in images.iter().enumerate().filter(|_| recovery.is_none()) {
                    vega_store::image_attachments::insert(
                        &transaction,
                        &user_message_id,
                        ordinal,
                        image.bytes(),
                    )
                    .map_err(runtime_store_error)?;
                }

                let title_request = if recovery.is_none()
                    && !uses_existing_user
                    && continuation.is_none()
                    && title_eligible_payload
                {
                    match config.automatic_title.clone() {
                        Some(request)
                            if vega_store::threads::claim_auto_title(
                                &transaction,
                                &thread_id,
                                &user_message_id,
                                &request,
                            )
                            .map_err(runtime_store_error)? =>
                        {
                            true
                        }
                        _ => false,
                    }
                } else {
                    false
                };
                if let Some(id) = &config.pending_message_id
                    && !vega_store::pending_messages::mark_delivered(&transaction, &thread_id, id)
                        .map_err(runtime_store_error)?
                {
                    return Err(ConversationError::Store(
                        vega_i18n::t("domain.pending.error.delivered").into(),
                    ));
                }
                if let (Some(id), Some(content)) =
                    (&config.pending_message_id, pending_model_content)
                {
                    vega_store::pending_messages::preserve_model_input(
                        &transaction,
                        &thread_id,
                        id,
                        &content,
                    )
                    .map_err(runtime_store_error)?;
                }
                let assistant_seq =
                    messages::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
                if assistant_seq != install.assistant().seq {
                    return Err(ConversationError::InvalidTaskIdentity);
                }
                messages::insert(&transaction, install.assistant()).map_err(runtime_store_error)?;

                vega_store::permission_contracts::record_run_fence(
                    &transaction,
                    &assistant_message_id,
                    &thread_id,
                    &permission_fence_json,
                )
                .map_err(runtime_store_error)?;

                if let Some(receipt) = &continuation {
                    vega_store::credential_continuation::attach(
                        &transaction,
                        &receipt.id,
                        &assistant_message_id,
                    )
                    .map_err(runtime_store_error)?;
                }

                if let Some(run) = &skill_run {
                    let binding = run
                        .binding()
                        .ok_or(ConversationError::InvalidTaskIdentity)?;
                    for audit in audits {
                        vega_store::skills::append_activation_audit(
                            &transaction,
                            vega_store::skills::NewSkillActivationAudit {
                                run_id: binding.run_id(),
                                thread_id: binding.thread_id(),
                                name: &audit.name,
                                source_scope: audit.source_scope.map(|scope| match scope {
                                    vega_runtime::skills::SourceScope::Project => "project",
                                    vega_runtime::skills::SourceScope::VegaGlobal => "vega_global",
                                    vega_runtime::skills::SourceScope::Imported => "imported",
                                }),
                                content_sha256: audit.content_sha256.as_deref(),
                                origin: "explicit_user",
                                status: audit.status,
                                created_at: now,
                            },
                        )
                        .map_err(|error| ConversationError::Store(error.to_string()))?;
                    }
                }
                let next_tool_seq =
                    tool_calls::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
                let receipt = ordinary
                    .map(|ordinary| ordinary.bind_persisted(&transaction))
                    .transpose()?;
                if cancelled(ordinary, &config) {
                    return Err(ConversationError::Runtime(Arc::new(VegaError::Cancelled)));
                }
                if let Some(authority) = &redaction_authority {
                    authority
                        .validate(store)
                        .map_err(|error| ConversationError::Runtime(Arc::new(error.into())))?;
                }
                if let Some(run) = &skill_run {
                    run.validate_prepared_sources()
                        .map_err(skill_preparation_error)?;
                }
                let user_sent_at_ms = messages::find(&transaction, &user_message_id)
                    .map_err(runtime_store_error)?
                    .map(|row| row.created_at);
                let user_timestamp_break_before =
                    messages::timestamp_break_before(&transaction, &user_message_id)
                        .map_err(runtime_store_error)?;
                Ok(Some((
                    InstallOutcome::Committed {
                        legacy_cleanup,
                        next_tool_seq,
                        assistant_seq,
                        title_request,
                        user_sent_at_ms,
                        user_timestamp_break_before,
                    },
                    receipt,
                )))
            })();
            let result = match result {
                Ok(None) => {
                    transaction.rollback().map_err(runtime_store_error)?;
                    Ok(InstallOutcome::Conflict { remaining })
                }
                Ok(Some((outcome, receipt))) => {
                    if let Some(ordinary) = ordinary
                        && let Err(error) = ordinary.worker.begin_commit()
                    {
                        let rollback = transaction.rollback();
                        let _ = ordinary.worker.record_not_committed(
                            rollback.map_err(|_| crate::types::RollbackFailure::Failed),
                        );
                        return Err(super::super::ordinary::fact_error(error));
                    }
                    transaction.commit().map_err(runtime_store_error)?;
                    if let (Some(ordinary), Some(receipt)) = (ordinary, receipt) {
                        ordinary
                            .worker
                            .record_committed(receipt)
                            .map_err(super::super::ordinary::fact_error)?;
                    }
                    Ok(outcome)
                }
                Err(error) => {
                    rollback_preparation(
                        transaction,
                        ordinary,
                        recovery.as_deref(),
                        &user_message_id,
                        &user_content,
                        &images,
                    );
                    Err(error)
                }
            };
            drop(permission);
            result
        })();
        if let Some(ordinary) = owned_ordinary {
            ordinary.complete(result.as_ref().map(|_| ()));
        }
        result
    }
}

pub(super) fn load_exact_rules(
    connection: &vega_store::Connection,
    config: &authority::CommitAuthority,
    project_id: &str,
) -> Result<Vec<RuntimeExactRule>, ConversationError> {
    if let Some(authority) = &config.delegated_authority {
        return Ok(authority.exact_rules.clone());
    }
    if project_id.is_empty() {
        return Ok(Vec::new());
    }
    permissions::list_exact(connection, project_id)
        .map_err(|error| ConversationError::Store(error.to_string()))?
        .into_iter()
        .map(|rule| {
            if rule.pattern.is_empty() {
                return Err(ConversationError::InvalidTaskIdentity);
            }
            let tool = match rule.tool.as_str() {
                "bash" => RuntimeMutatingTool::Bash,
                "read" => RuntimeMutatingTool::Read,
                "write" => RuntimeMutatingTool::Write,
                "edit" => RuntimeMutatingTool::Edit,
                _ => return Err(ConversationError::InvalidTaskIdentity),
            };
            Ok(RuntimeExactRule {
                tool,
                pattern: rule.pattern,
            })
        })
        .collect()
}
