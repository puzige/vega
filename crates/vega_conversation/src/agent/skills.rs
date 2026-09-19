//! Trusted Store-to-runtime Skills projection for one direct user turn.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use vega_runtime::skills::{
    ActiveSkillSummary, PersistedSkillApproval, RunBinding, SkillApproval, SkillCatalog, SkillRun,
    SkillSelection, SkillSource, SourceScope,
};
use vega_store::messages;
use vega_store::skills::{self, SkillSnapshotRecord, SkillSourceRecord};

use crate::types::ConversationError;

pub(super) struct PreparedSkills {
    pub run: SkillRun,
    pub explicit: Vec<SkillSelection>,
}

/// A validated old run is inspectable after restart, but Vega's existing
/// interrupted-run recovery never resumes or replays its provider/tool calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillAssistantStatus {
    Streaming,
    Interrupted,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillRunRecovery {
    pub run_id: String,
    pub thread_id: String,
    pub assistant_status: SkillAssistantStatus,
    pub activations: Vec<ActiveSkillSummary>,
}

fn status_from_row(status: &str) -> Result<SkillAssistantStatus, ConversationError> {
    match status {
        "streaming" => Ok(SkillAssistantStatus::Streaming),
        "interrupted" => Ok(SkillAssistantStatus::Interrupted),
        "done" => Ok(SkillAssistantStatus::Done),
        "failed" => Ok(SkillAssistantStatus::Failed),
        _ => Err(invalid_store()),
    }
}

/// Read a Store-bound, digest-validated frozen run for provenance/status.
/// This never reads the Skill source files and never starts a provider or tool.
/// A new user retry establishes a new run and must recheck current consent.
pub fn recover_skill_run(
    store: &vega_store::Store,
    run_id: &str,
    thread_id: &str,
) -> Result<Option<SkillRunRecovery>, ConversationError> {
    let assistant = messages::find(store.conn(), run_id)
        .map_err(|_| invalid_store())?
        .ok_or_else(invalid_store)?;
    if assistant.thread_id != thread_id || assistant.role != "assistant" {
        return Err(invalid_store());
    }
    let status = status_from_row(&assistant.status)?;
    let Some(snapshot) =
        skills::load_recoverable_snapshot(store.conn(), run_id).map_err(|_| invalid_store())?
    else {
        return Ok(None);
    };
    if snapshot.thread_id != thread_id || snapshot.run_id != run_id {
        return Err(invalid_store());
    }
    let activations = validate_frozen_snapshot(&snapshot).ok_or_else(invalid_store)?;
    Ok(Some(SkillRunRecovery {
        run_id: snapshot.run_id,
        thread_id: snapshot.thread_id,
        assistant_status: status,
        activations,
    }))
}

/// Shared digest/binding/inner-format check for single-run recovery and the
/// bounded history-page projection. Caller supplies only Store-fetched bytes;
/// a rejected snapshot is never repaired from current source files.
pub(crate) fn validate_frozen_snapshot(
    snapshot: &SkillSnapshotRecord,
) -> Option<Vec<ActiveSkillSummary>> {
    let binding = RunBinding::from_trusted_parts(
        &snapshot.run_id,
        &snapshot.thread_id,
        snapshot.consent_generation,
        snapshot.revocation_generation,
        &snapshot.catalog_sha256,
    )
    .ok()?;
    let run =
        SkillRun::restore_snapshot(&snapshot.bytes, &binding, &snapshot.snapshot_sha256).ok()?;
    Some(run.active_summaries())
}

fn unavailable() -> ConversationError {
    ConversationError::Store("selected Skill is unavailable or changed".to_string())
}

fn invalid_store() -> ConversationError {
    ConversationError::Store("Skill settings are invalid".to_string())
}

/// Store-owned run authority, checked synchronously before every provider
/// round/tool dispatch and periodically while a provider stream is open.
/// A changed consent epoch is conservatively treated like revocation because
/// this run's frozen snapshot can no longer be durably advanced under 0013.
pub(super) struct SkillAuthority {
    pub probe: Arc<dyn Fn() -> bool + Send + Sync>,
    observed_revocation: Arc<AtomicBool>,
}

impl SkillAuthority {
    pub fn observed_revocation(&self) -> bool {
        self.observed_revocation.load(Ordering::SeqCst)
    }
}

pub(super) fn authority_probe(database_path: PathBuf, binding: &RunBinding) -> SkillAuthority {
    let expected_consent = binding.consent_generation();
    let expected_revocation = binding.revocation_generation();
    let observed_revocation = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&observed_revocation);
    let probe: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        let Ok(store) = vega_store::Store::open_read_only(&database_path) else {
            return false;
        };
        match skills::read_settings(store.conn()) {
            Ok(settings) => {
                if settings.revocation_generation != expected_revocation {
                    observed.store(true, Ordering::SeqCst);
                }
                settings.consent_generation == expected_consent
                    && settings.revocation_generation == expected_revocation
            }
            Err(_) => false,
        }
    });
    SkillAuthority {
        probe,
        observed_revocation,
    }
}

pub(super) async fn watch_authority(
    probe: Arc<dyn Fn() -> bool + Send + Sync>,
    run_cancel: CancellationToken,
    stop: CancellationToken,
) {
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return,
            _ = run_cancel.cancelled() => return,
            _ = ticker.tick() => {}
        }
        let probe = Arc::clone(&probe);
        let current = tokio::task::spawn_blocking(move || probe())
            .await
            .unwrap_or(false);
        if stop.is_cancelled() || run_cancel.is_cancelled() {
            return;
        }
        if !current {
            run_cancel.cancel();
            return;
        }
    }
}

fn rebound_source(
    record: &SkillSourceRecord,
    project_root: Option<&Path>,
    config_dir: Option<&Path>,
) -> Option<SkillSource> {
    // Older direct links and Settings-reviewed roots both bind the exact
    // registered project. Settings canonicalizes first; macOS may persist
    // /var/... but review /private/var/.... Only the recorded exact spelling
    // (raw or canonical) is eligible, followed by dev/inode validation.
    let bound_project_root = project_root.and_then(|root| {
        let configured = Path::new(&record.configured_root);
        if root.join(".agents/skills") == configured {
            Some(root.to_path_buf())
        } else {
            root.canonicalize()
                .ok()
                .filter(|canonical| canonical.join(".agents/skills") == configured)
        }
    });
    let source = match record.scope.as_str() {
        "project" => SkillSource::project_approved(bound_project_root.as_deref()?).ok()??,
        "vega_global" => SkillSource::vega_global(config_dir?).ok()??,
        "imported" => SkillSource::imported_approved(
            Path::new(&record.configured_root),
            usize::try_from(record.import_order).ok()?,
        )
        .ok()?,
        _ => return None,
    };
    let identity = source.identity();
    let expected_scope = match record.scope.as_str() {
        "project" => SourceScope::Project,
        "vega_global" => SourceScope::VegaGlobal,
        "imported" => SourceScope::Imported,
        _ => return None,
    };
    if identity.scope() != expected_scope
        || identity.canonical_root() != Path::new(&record.canonical_root)
        || identity.device().to_string() != record.root_dev
        || identity.inode().to_string() != record.root_ino
    {
        return None;
    }
    if record.scope == "vega_global"
        && config_dir.is_some_and(|root| root.join("skills") != Path::new(&record.configured_root))
    {
        return None;
    }
    Some(source)
}

pub(super) fn prepare_skill_run(
    store: &vega_store::Store,
    project_id: &str,
    thread_id: &str,
    run_id: &str,
    direct_user: bool,
) -> Result<Option<PreparedSkills>, ConversationError> {
    let config_dir = vega_store::paths::config_dir();
    prepare_skill_run_with_config_dir(
        store,
        project_id,
        thread_id,
        run_id,
        direct_user,
        config_dir.as_deref(),
    )
}

pub(super) fn prepare_skill_run_with_config_dir(
    store: &vega_store::Store,
    project_id: &str,
    thread_id: &str,
    run_id: &str,
    direct_user: bool,
    config_dir: Option<&Path>,
) -> Result<Option<PreparedSkills>, ConversationError> {
    if !direct_user {
        return Ok(None);
    }
    let settings = skills::read_settings(store.conn()).map_err(|_| invalid_store())?;
    let project = if project_id.is_empty() {
        None
    } else {
        vega_store::projects::find(store.conn(), project_id).map_err(|_| invalid_store())?
    };
    let project_settings = if project_id.is_empty() {
        skills::ProjectSkillSettings {
            enabled: false,
            automatic: false,
        }
    } else {
        skills::read_project_settings(store.conn(), project_id).map_err(|_| invalid_store())?
    };
    let sources = skills::list_sources_for_project(
        store.conn(),
        (!project_id.is_empty()).then_some(project_id),
    )
    .map_err(|_| invalid_store())?;
    let stored_approvals =
        skills::list_approved_skills(store.conn()).map_err(|_| invalid_store())?;
    let pins = skills::list_thread_pins(store.conn(), thread_id).map_err(|_| invalid_store())?;
    if sources.is_empty() && pins.is_empty() {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    let mut approvals = Vec::new();
    let mut source_by_id = HashMap::new();
    for record in &sources {
        let enabled = record.enabled
            && if record.scope == "project" {
                project_settings.enabled
            } else {
                settings.global_enabled
            };
        if !enabled {
            continue;
        }
        let source = match rebound_source(
            record,
            project.as_ref().map(|item| Path::new(&item.path)),
            config_dir,
        ) {
            Some(source) => source,
            None => continue,
        };
        let discovery = match source.discover() {
            Ok(discovery) => discovery,
            Err(_) => continue,
        };
        candidates.extend(discovery.candidates);
        source_by_id.insert(record.id.as_str(), (record, source));
    }
    for stored in &stored_approvals {
        let Some((record, source)) = source_by_id.get(stored.source_id.as_str()) else {
            continue;
        };
        let automatic = stored.automatic
            && record.automatic
            && if record.scope == "project" {
                project_settings.automatic
            } else {
                settings.automatic_enabled
            };
        let scope = source.identity().scope();
        let approval = SkillApproval::from_persisted_parts(
            source,
            PersistedSkillApproval {
                scope,
                canonical_root: Path::new(&record.canonical_root),
                root_dev: record.root_dev.parse().map_err(|_| invalid_store())?,
                root_ino: record.root_ino.parse().map_err(|_| invalid_store())?,
                name: &stored.name,
                approved_sha256: &stored.approved_sha256,
                source_label: &stored.source_label,
                enabled: stored.enabled,
                automatic,
            },
        )
        .map_err(|_| invalid_store())?;
        approvals.push(approval);
    }
    let mut explicit = Vec::new();
    for pin in &pins {
        let selected = approvals
            .iter()
            .filter(|approval| approval.enabled())
            .find(|approval| {
                let selection = approval.selection();
                let identity = selection.source();
                selection.name() == pin.name
                    && selection.sha256() == pin.approved_sha256
                    && approval.source_label() == pin.source_label
                    && identity.canonical_root() == Path::new(&pin.canonical_root)
                    && identity.device().to_string() == pin.root_dev
                    && identity.inode().to_string() == pin.root_ino
                    && matches!(
                        (identity.scope(), pin.scope.as_str()),
                        (SourceScope::Project, "project")
                            | (SourceScope::VegaGlobal, "vega_global")
                            | (SourceScope::Imported, "imported")
                    )
            })
            .ok_or_else(unavailable)?;
        explicit.push(selected.selection());
    }
    let auto_enabled = (project_settings.enabled && project_settings.automatic)
        || (settings.global_enabled && settings.automatic_enabled);
    let catalog =
        SkillCatalog::freeze(candidates, &approvals, auto_enabled).map_err(|_| invalid_store())?;
    if catalog.model_catalog().is_empty() && explicit.is_empty() {
        return Ok(None);
    }
    let binding = RunBinding::for_catalog(
        run_id,
        thread_id,
        settings.consent_generation,
        settings.revocation_generation,
        &catalog,
    )
    .map_err(|_| invalid_store())?;
    let run = SkillRun::new_bound(catalog, true, binding).map_err(|_| invalid_store())?;
    Ok(Some(PreparedSkills { run, explicit }))
}
