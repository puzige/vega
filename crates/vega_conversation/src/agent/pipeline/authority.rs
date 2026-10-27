use super::*;

pub(super) struct RunAuthority {
    pub fence: crate::types::PermissionFence,
    pub thread: vega_store::threads::ThreadRow,
    pub run_mode: ThreadMode,
    pub permission_mode: crate::types::PermissionMode,
    pub resumed_user: Option<String>,
}

impl RunAuthority {
    pub fn matches(&self, other: &Self) -> bool {
        self.fence == other.fence
            && self.thread.model == other.thread.model
            && self.thread.backend == other.thread.backend
            && self.thread.status == other.thread.status
            && self.resumed_user == other.resumed_user
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn verify_pending_sources(
    store: &Store,
    config: &PersistenceActorConfig,
    thread_id: &str,
    user_id: &str,
    _content: &str,
    images: &[crate::types::ImageAttachment],
    uses_existing_user: bool,
) -> Result<Option<crate::types::PermissionFence>, ConversationError> {
    if let Some(id) = &config.pending_message_id {
        if uses_existing_user || id != user_id {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let accepted = config
            .pending_authority
            .as_ref()
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        if accepted.message.images != images {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        return crate::pending_messages::verify_dispatch(store, accepted, thread_id, id).map(Some);
    }
    if config.pending_authority.is_some() {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    if uses_existing_user && !config.delegated && config.credential_continuation_id.is_none() {
        return crate::plans::approved_authority(store, thread_id, user_id).map(Some);
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn verified(
    store: &Store,
    config: &PersistenceActorConfig,
    thread_id: &str,
    assistant_message_id: &str,
    user_message_id: &str,
    uses_existing_user: bool,
    user_content: &str,
    images: &[crate::types::ImageAttachment],
    ordinary: Option<&super::super::ordinary::OrdinaryPreparation>,
) -> Result<RunAuthority, ConversationError> {
    let conn = store.conn();
    let thread = vega_store::threads::find(conn, thread_id)
        .map_err(runtime_store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_owned()))?;
    let current_fence = if config.delegated {
        let authority = config
            .delegated_authority
            .as_ref()
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        if !authority.matches_fence() {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let (parent_thread, parent_message) = config
            .delegated_parent
            .as_ref()
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        let fence = crate::permission_contracts::capture_delegated_fence(
            store,
            &thread,
            parent_thread,
            parent_message,
            assistant_message_id,
            &authority.permission_fence,
        )?;
        let encoded = vega_store::permission_contracts::load_delegated_fence(
            conn,
            thread_id,
            assistant_message_id,
        )
        .map_err(runtime_store_error)?;
        let admitted: crate::types::PermissionFence =
            serde_json::from_str(&encoded).map_err(|_| ConversationError::InvalidTaskIdentity)?;
        if admitted != fence {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        admitted
    } else {
        if config.delegated_authority.is_some() {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        crate::permission_contracts::capture_fence(store, &thread)?
    };
    let pending = verify_pending_sources(
        store,
        config,
        thread_id,
        user_message_id,
        user_content,
        images,
        uses_existing_user,
    )?;
    let fence = if let Some(ordinary) = ordinary {
        let accepted = ordinary.run.submission.permission_fence();
        match accepted {
            Ok(accepted) if accepted == current_fence => accepted,
            _ => {
                let _ = ordinary
                    .run
                    .submission
                    .invalidate_recovery(ordinary.worker.attempt_id());
                return Err(ConversationError::InvalidTaskIdentity);
            }
        }
    } else if let Some(pending) = pending {
        if pending != current_fence {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        pending
    } else {
        current_fence
    };
    let automation_fence = fence
        .delegation
        .as_ref()
        .map(|source| source.parent_fence.as_ref())
        .unwrap_or(&fence);
    if let Some(claim) = &config.automation_claim
        && (claim.thread_id != automation_fence.thread_id
            || claim.permission_fence.as_ref() != Some(automation_fence)
            || (!config.delegated
                && !crate::automation::current_claim(store, claim)
                    .map_err(|_| ConversationError::InvalidTaskIdentity)?))
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let thread_grant = thread
        .full_access_grant
        .as_deref()
        .map(serde_json::from_str::<crate::types::FullAccessGrant>)
        .transpose()
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
    let resumed_user = if let Some(expected) = &config.interrupted_assistant {
        if !crate::permission_contracts::interrupted_authority_matches(conn, expected, &fence)? {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let tail = messages::interrupted_tail(conn, thread_id).map_err(runtime_store_error)?;
        if ordinary.is_some()
            || uses_existing_user
            || config.delegated
            || config.credential_continuation_id.is_some()
            || config.pending_message_id.is_some()
            || config.automation_claim.is_some()
            || !user_content.is_empty()
            || !images.is_empty()
            || tail
                .as_ref()
                .is_none_or(|(id, seq)| id != &expected.message_id || *seq != expected.seq)
            || messages::next_seq(conn, thread_id).map_err(runtime_store_error)?
                != expected.seq.checked_add(1).ok_or_else(|| {
                    ConversationError::CorruptRow("interrupted sequence overflow".into())
                })?
        {
            return Err(ConversationError::CorruptRow(
                "interrupted task changed; reopen the task before resuming".into(),
            ));
        }
        Some(conn.query_row(
            "SELECT id FROM messages WHERE thread_id = ?1 AND role = 'user' AND seq < ?2 ORDER BY seq DESC LIMIT 1",
            (thread_id, expected.seq),
            |row| row.get::<_, String>(0),
        ).map_err(runtime_store_error)?)
    } else {
        None
    };
    if let Some(ordinary) = ordinary
        && (thread.model != ordinary.run.submission.raw().model()
            || thread.backend != "native"
            || thread.status == "archived"
            || ordinary.run.thread.as_ref().is_some_and(|snapshot| {
                snapshot.project_id != thread.project_id
                    || snapshot.mode.as_str() != thread.mode
                    || snapshot.permission_mode.as_str() != thread.permission_mode
                    || snapshot.permission_revision != thread.permission_revision
                    || snapshot.full_access_grant != thread_grant
            }))
    {
        let _ = ordinary
            .run
            .submission
            .invalidate_recovery(ordinary.worker.attempt_id());
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let run_mode = ThreadMode::parse(&fence.run_mode)
        .ok_or_else(|| ConversationError::CorruptRow(format!("run mode: {}", thread.mode)))?;
    let permission_mode =
        crate::types::PermissionMode::parse(&fence.permission_mode).ok_or_else(|| {
            ConversationError::CorruptRow(format!("permission_mode: {}", thread.permission_mode))
        })?;
    if !config.delegated && permission_mode == crate::types::PermissionMode::FullAccess {
        crate::permission_contracts::verify_thread_grant(conn, thread_id, thread_grant.as_ref())?;
    }
    Ok(RunAuthority {
        fence,
        thread,
        run_mode,
        permission_mode,
        resumed_user,
    })
}
