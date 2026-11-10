use super::*;

pub(super) enum ResetWrite {
    PauseScheduling,
    Begin {
        authority_epoch: String,
        source: ResetConfigSource,
    },
    Revoke {
        record: ResetRecord,
    },
    Finish {
        record: ResetRecord,
        saved_source: ResetConfigSource,
    },
}

impl crate::database_writer::Sealed for ResetWrite {}

impl crate::database_writer::DatabaseCommand for ResetWrite {
    type Output = Result<Option<ResetRecord>, PermissionResetError>;

    fn apply(self, store: &Store) -> Self::Output {
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        if !vega_store::thread_admission::is_database_paused(store.database_identity())
            .map_err(unavailable)?
        {
            return Err(PermissionResetError::Busy);
        }
        let tx = store.immediate_transaction().map_err(unavailable)?;
        let result = match self {
            Self::PauseScheduling => {
                crate::automation::pause_for_reset_in_transaction(&tx, crate::threads::now_ms())
                    .map_err(unavailable)?;
                None
            }
            Self::Begin {
                authority_epoch,
                source,
            } => {
                let state =
                    vega_store::permission_contracts::load_state(&tx).map_err(unavailable)?;
                if state.reset_pending {
                    return Err(PermissionResetError::Busy);
                }
                if state.authority_epoch != authority_epoch {
                    return Err(PermissionResetError::ConfigurationConflict);
                }
                require_settled(&tx)?;
                let record =
                    persistence::begin(&tx, &authority_epoch, &source, crate::threads::now_ms())
                        .map_err(unavailable)?;
                crate::automation::revoke_in_transaction(&tx, crate::threads::now_ms())
                    .map_err(unavailable)?;
                tx.execute("UPDATE default_permission_intent SET generation=?1,pending_id=NULL,source_json=NULL,target_json=NULL WHERE singleton=1", [ulid::Ulid::generate().to_string()]).map_err(unavailable)?;
                tx.execute(
                    "DELETE FROM draft_creation_intents WHERE state='prepared'",
                    [],
                )
                .map_err(unavailable)?;
                Some(record)
            }
            Self::Revoke { record } => {
                verify_pending(&tx, &record)?;
                require_settled(&tx)?;
                crate::automation::revoke_in_transaction(&tx, crate::threads::now_ms())
                    .map_err(unavailable)?;
                None
            }
            Self::Finish {
                record,
                saved_source,
            } => {
                verify_pending(&tx, &record)?;
                require_settled(&tx)?;
                persistence::finish(&tx, &record, &saved_source).map_err(unavailable)?;
                None
            }
        };
        tx.commit_with_rollback()
            .map_err(|_| PermissionResetError::OutcomeUnknown)?;
        Ok(result)
    }
}
