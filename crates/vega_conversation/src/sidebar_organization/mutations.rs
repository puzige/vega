//! Validation and atomic organization mutations.
use super::*;

/// Applies one action atomically against the expected revision and returns the committed snapshot.
/// All validation and optional task creation run inside one immediate transaction.
pub fn apply(
    store: &Store,
    expected_revision: u64,
    action: SidebarOrganizationAction,
) -> Result<SidebarOrganizationOutcome, SidebarOrganizationError> {
    let creation = match &action {
        SidebarOrganizationAction::CreateThreadInGroup { draft, .. } => {
            Some(crate::permission_contracts::DraftCreationGuard::begin(draft).map_err(storage)?)
        }
        _ => None,
    };
    let tx = store.immediate_transaction().map_err(storage)?;
    let result = apply_in_transaction(store, &tx, expected_revision, action, creation.as_ref())?;
    tx.commit().map_err(storage)?;
    Ok(result)
}

pub(super) fn apply_in_transaction(
    store: &Store,
    transaction: &vega_store::Transaction<'_>,
    expected_revision: u64,
    action: SidebarOrganizationAction,
    creation: Option<&crate::permission_contracts::DraftCreationGuard>,
) -> Result<SidebarOrganizationOutcome, SidebarOrganizationError> {
    let mut state = read_snapshot(store)?;
    if state.revision != expected_revision {
        return Err(SidebarOrganizationError::Conflict {
            expected: expected_revision,
            actual: state.revision,
        });
    }
    let created_thread = mutate(transaction, &mut state, action, creation)?;
    let existing: std::collections::HashSet<_> =
        state.threads.iter().map(|t| t.id.as_str()).collect();
    let mut seen = std::collections::HashSet::new();
    state
        .preferences
        .manual_thread_order
        .retain(|id| existing.contains(id.as_str()) && seen.insert(id.clone()));
    state.preferences.sort = SidebarTaskSort::Manual;
    state.revision = sql::next_revision(state.revision).ok_or(
        SidebarOrganizationError::LimitKey("domain.organization.validation.revision"),
    )?;
    let rows = sql::OrganizationRows {
        revision: state.revision as i64,
        preferences: serde_json::to_string(&state.preferences).map_err(storage)?,
        collapsed: serde_json::to_string(&state.collapsed).map_err(storage)?,
        groups: state
            .groups
            .iter()
            .map(|g| {
                Ok((
                    g.id.clone(),
                    g.name.clone(),
                    serde_json::to_string(&g.color).map_err(storage)?,
                ))
            })
            .collect::<Result<_, SidebarOrganizationError>>()?,
        memberships: state
            .memberships
            .iter()
            .map(|m| (m.thread_id.clone(), m.group_id.clone()))
            .collect(),
        project_order: state.project_order.clone(),
    };
    sql::write(store.conn(), &rows).map_err(storage)?;
    // Return precisely the same ordering and typed decoding as a subsequent read.
    let result = read_snapshot(store)?;
    Ok(SidebarOrganizationOutcome {
        snapshot: result,
        created_thread,
    })
}
fn invalid_key(key: &'static str) -> SidebarOrganizationError {
    SidebarOrganizationError::InvalidKey(key)
}
fn name(value: String) -> Result<String, SidebarOrganizationError> {
    let value = value.trim();
    if !(1..=64).contains(&value.chars().count()) {
        return Err(invalid_key("domain.organization.validation.name"));
    }
    Ok(value.into())
}
fn group<'a>(
    state: &'a mut SidebarOrganizationSnapshot,
    id: &str,
) -> Result<&'a mut SidebarGroup, SidebarOrganizationError> {
    state
        .groups
        .iter_mut()
        .find(|g| g.id == id)
        .ok_or_else(|| invalid_key("domain.organization.validation.group"))
}
fn move_before(
    ids: &mut Vec<String>,
    id: &str,
    before: Option<&str>,
) -> Result<(), SidebarOrganizationError> {
    if before == Some(id) {
        return Err(invalid_key("domain.organization.validation.self_anchor"));
    }
    let from = ids
        .iter()
        .position(|v| v == id)
        .ok_or_else(|| invalid_key("domain.organization.validation.missing"))?;
    if before.is_some_and(|anchor| !ids.iter().any(|v| v == anchor)) {
        return Err(invalid_key("domain.organization.validation.destination"));
    }
    let moved = ids.remove(from);
    let at = before
        .and_then(|anchor| ids.iter().position(|v| v == anchor))
        .unwrap_or(ids.len());
    ids.insert(at, moved);
    Ok(())
}
fn validate_collapse(
    state: &SidebarOrganizationSnapshot,
    target: &SidebarCollapseTarget,
) -> Result<(), SidebarOrganizationError> {
    match target {
        SidebarCollapseTarget::Project(id) if !state.projects.iter().any(|p| p.id == *id) => {
            Err(invalid_key("domain.organization.validation.project"))
        }
        SidebarCollapseTarget::Group(id) if !state.groups.iter().any(|g| g.id == *id) => {
            Err(invalid_key("domain.organization.validation.group"))
        }
        _ => Ok(()),
    }
}
fn mutate(
    transaction: &vega_store::Transaction<'_>,
    state: &mut SidebarOrganizationSnapshot,
    action: SidebarOrganizationAction,
    creation: Option<&crate::permission_contracts::DraftCreationGuard>,
) -> Result<Option<Thread>, SidebarOrganizationError> {
    use SidebarOrganizationAction::*;
    match action {
        CreateGroup { name: title, color } => {
            let title = name(title)?;
            if state.groups.len() >= MAX_SIDEBAR_GROUPS {
                return Err(SidebarOrganizationError::LimitKey(
                    "domain.organization.validation.groups_limit",
                ));
            }
            state.groups.push(SidebarGroup {
                id: crate::threads::new_thread_id(),
                name: title,
                color,
            });
        }
        RenameGroup {
            group_id,
            name: title,
        } => {
            let title = name(title)?;
            group(state, &group_id)?.name = title;
        }
        SetGroupColor { group_id, color } => group(state, &group_id)?.color = color,
        DissolveGroup { group_id } => {
            group(state, &group_id)?;
            state.groups.retain(|g| g.id != group_id);
            state.memberships.retain(|m| m.group_id != group_id);
            state
                .collapsed
                .retain(|t| *t != SidebarCollapseTarget::Group(group_id.clone()));
        }
        MoveGroup {
            group_id,
            before_id,
        } => {
            let mut ids = state.groups.iter().map(|g| g.id.clone()).collect();
            move_before(&mut ids, &group_id, before_id.as_deref())?;
            state
                .groups
                .sort_by_key(|g| ids.iter().position(|id| *id == g.id));
        }
        MoveProject {
            project_id,
            before_id,
        } => {
            let mut ids = state.project_order.clone();
            for p in &state.projects {
                if !ids.contains(&p.id) {
                    ids.push(p.id.clone());
                }
            }
            move_before(&mut ids, &project_id, before_id.as_deref())?;
            state.project_order = ids;
        }
        ReorderThread {
            thread_id,
            before_id,
        } => {
            reorder_thread(state, &thread_id, before_id.as_deref())?;
        }
        MoveThread {
            thread_id,
            group_id,
            before_id,
        } => move_thread(state, thread_id, group_id, before_id)?,
        RevealProject { project_id } => {
            validate_collapse(state, &SidebarCollapseTarget::Project(project_id.clone()))?;
            state.preferences.view = SidebarView::Projects;
            state.preferences.project_view = SidebarProjectView::ByProject;
            state
                .collapsed
                .retain(|target| *target != SidebarCollapseTarget::Project(project_id.clone()));
        }
        SetPreferences(mut preferences) => {
            preferences.manual_thread_order = state.preferences.manual_thread_order.clone();
            state.preferences = preferences;
        }
        SetCollapsed { target, collapsed } => {
            validate_collapse(state, &target)?;
            state.collapsed.retain(|t| *t != target);
            if collapsed {
                state.collapsed.push(target);
            }
        }
        CollapseAll => {
            let targets: Vec<_> = match state.preferences.view {
                SidebarView::Groups => state
                    .groups
                    .iter()
                    .map(|g| SidebarCollapseTarget::Group(g.id.clone()))
                    .chain([SidebarCollapseTarget::Ungrouped])
                    .collect(),
                SidebarView::Projects => match state.preferences.project_view {
                    SidebarProjectView::ByProject => state
                        .projects
                        .iter()
                        .map(|p| SidebarCollapseTarget::Project(p.id.clone()))
                        .collect(),
                    SidebarProjectView::Timeline => [
                        SidebarTimelineBucket::Today,
                        SidebarTimelineBucket::Yesterday,
                        SidebarTimelineBucket::Last7Days,
                        SidebarTimelineBucket::Last30Days,
                        SidebarTimelineBucket::Earlier,
                    ]
                    .into_iter()
                    .map(SidebarCollapseTarget::Timeline)
                    .chain([SidebarCollapseTarget::Pinned])
                    .collect(),
                },
            };
            for target in targets {
                if !state.collapsed.contains(&target) {
                    state.collapsed.push(target);
                }
            }
        }
        CreateThreadInGroup { group_id, draft } => {
            group(state, &group_id)?;
            if !state.projects.iter().any(|p| p.id == draft.project_id) {
                return Err(invalid_key("domain.organization.validation.project"));
            }
            if state.threads.len() >= MAX_SIDEBAR_THREADS {
                return Err(SidebarOrganizationError::LimitKey(
                    "domain.organization.validation.tasks_limit",
                ));
            }
            let thread = creation
                .ok_or_else(|| invalid_key("domain.organization.validation.permissions"))?
                .materialize(transaction)
                .map_err(storage)?;
            if thread.id != draft.id || thread.project_id != draft.project_id {
                return Err(invalid_key("domain.organization.validation.permissions"));
            }
            state.memberships.push(SidebarMembership {
                thread_id: thread.id.clone(),
                group_id,
            });
            state.threads.push(thread.clone());
            return Ok(Some(thread));
        }
    }
    Ok(None)
}
fn move_thread(
    state: &mut SidebarOrganizationSnapshot,
    thread_id: String,
    group_id: Option<String>,
    before_id: Option<String>,
) -> Result<(), SidebarOrganizationError> {
    if !state.threads.iter().any(|t| t.id == thread_id) {
        return Err(invalid_key("domain.organization.validation.task"));
    }
    if before_id.as_deref() == Some(&thread_id) {
        return Err(invalid_key("domain.organization.validation.task_anchor"));
    }
    match group_id {
        None => {
            if before_id.is_some() {
                return Err(invalid_key("domain.organization.validation.ungroup_anchor"));
            }
            state.memberships.retain(|m| m.thread_id != thread_id);
        }
        Some(group_id) => {
            group(state, &group_id)?;
            if before_id.as_ref().is_some_and(|before| {
                !state
                    .memberships
                    .iter()
                    .any(|m| m.thread_id == *before && m.group_id == group_id)
            }) {
                return Err(invalid_key("domain.organization.validation.group_anchor"));
            }
            state.memberships.retain(|m| m.thread_id != thread_id);
            let at = before_id
                .and_then(|before| state.memberships.iter().position(|m| m.thread_id == before))
                .unwrap_or(state.memberships.len());
            state.memberships.insert(
                at,
                SidebarMembership {
                    thread_id,
                    group_id,
                },
            );
        }
    }
    Ok(())
}

fn reorder_thread(
    state: &mut SidebarOrganizationSnapshot,
    thread_id: &str,
    before_id: Option<&str>,
) -> Result<(), SidebarOrganizationError> {
    let source = state
        .threads
        .iter()
        .find(|thread| thread.id == thread_id)
        .ok_or_else(|| invalid_key("domain.organization.thread_missing"))?;
    if let Some(before_id) = before_id {
        let anchor = state
            .threads
            .iter()
            .find(|thread| thread.id == before_id)
            .ok_or_else(|| invalid_key("domain.organization.target_missing"))?;
        if source.pinned != anchor.pinned
            || (!source.pinned && source.project_binding() != anchor.project_binding())
        {
            return Err(invalid_key("domain.organization.same_region"));
        }
    }
    let mut order = sorted_threads(&state.threads, &state.preferences)
        .into_iter()
        .map(|thread| thread.id)
        .collect();
    move_before(&mut order, thread_id, before_id)?;
    state.preferences.manual_thread_order = order;
    state.preferences.sort = SidebarTaskSort::Manual;
    Ok(())
}
