use super::*;
use tokio_util::sync::CancellationToken;
use vega_store::write_coordination::{WriteCategory, WriteTransaction};

pub fn save_collapsed(
    store: &Store,
    changes: &[(SidebarCollapseTarget, bool)],
) -> Result<SidebarCollapseUpdate, SidebarOrganizationError> {
    save_collapsed_with_cancel(store, changes, &CancellationToken::new()).map(|(_, update)| update)
}

pub fn save_collapsed_with_cancel(
    store: &Store,
    changes: &[(SidebarCollapseTarget, bool)],
    cancellation: &CancellationToken,
) -> Result<(u64, SidebarCollapseUpdate), SidebarOrganizationError> {
    check_cancellation(cancellation)?;
    let path = store
        .database_path()
        .ok_or_else(|| storage("database path unavailable"))?;
    crate::database_writer::commit(
        path,
        store.database_identity(),
        SaveCollapse {
            changes: changes.to_vec(),
            cancellation: cancellation.clone(),
        },
    )
    .map_err(storage)?
}
struct SaveCollapse {
    changes: Vec<(SidebarCollapseTarget, bool)>,
    cancellation: CancellationToken,
}
impl crate::database_writer::Sealed for SaveCollapse {}
impl crate::database_writer::DatabaseCommand for SaveCollapse {
    type Output = Result<(u64, SidebarCollapseUpdate), SidebarOrganizationError>;
    fn apply(self, store: &Store) -> Self::Output {
        let changes = &self.changes;
        let cancellation = &self.cancellation;
        let tx = WriteTransaction::new_with_cancel(store.conn(), WriteCategory::Sidebar, || {
            cancellation.is_cancelled()
        })
        .map_err(storage)?;
        check_cancellation(cancellation)?;
        let (revision, collapsed) = sql::read_collapse(store.conn()).map_err(storage)?;
        let revision = u64::try_from(revision).map_err(storage)?;
        let persisted: Vec<SidebarCollapseTarget> =
            serde_json::from_str(&collapsed).map_err(storage)?;
        let mut collapsed = persisted.clone();
        for (index, (target, should_collapse)) in changes.iter().enumerate() {
            validate_target(store, target)?;
            if changes[index + 1..]
                .iter()
                .any(|(later, _)| later == target)
            {
                continue;
            }
            if *should_collapse {
                if !collapsed.contains(target) {
                    collapsed.push(target.clone());
                }
            } else {
                collapsed.retain(|existing| existing != target);
            }
        }
        let mut update = SidebarCollapseUpdate {
            revision,
            collapsed,
        };
        if update.collapsed != persisted {
            update.revision = sql::next_revision(revision).ok_or(
                SidebarOrganizationError::LimitKey("domain.organization.validation.revision"),
            )?;
            let collapsed = serde_json::to_string(&update.collapsed).map_err(storage)?;
            check_cancellation(cancellation)?;
            sql::write_collapse(store.conn(), update.revision as i64, &collapsed)
                .map_err(storage)?;
        }
        check_cancellation(cancellation)?;
        tx.commit().map_err(storage)?;
        Ok((revision, update))
    }
}

fn check_cancellation(cancellation: &CancellationToken) -> Result<(), SidebarOrganizationError> {
    if cancellation.is_cancelled() {
        Err(SidebarOrganizationError::Store(
            "sidebar collapse save cancelled".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_target(
    store: &Store,
    target: &SidebarCollapseTarget,
) -> Result<(), SidebarOrganizationError> {
    let missing = match target {
        SidebarCollapseTarget::Project(id) => {
            if vega_store::projects::project_exists(store.conn(), id).map_err(storage)? {
                None
            } else {
                Some("domain.organization.validation.project")
            }
        }
        SidebarCollapseTarget::Group(id) => {
            if sql::group_exists(store.conn(), id).map_err(storage)? {
                None
            } else {
                Some("domain.organization.validation.group")
            }
        }
        SidebarCollapseTarget::Timeline(_)
        | SidebarCollapseTarget::Pinned
        | SidebarCollapseTarget::Ungrouped => None,
    };
    match missing {
        Some(key) => Err(SidebarOrganizationError::InvalidKey(key)),
        None => Ok(()),
    }
}
