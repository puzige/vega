//! Explicit Composer pinning uses the same reviewed sources as Settings.
//! It does not grant file, script or MCP authority.

use super::*;
use crate::types::{
    SkillComposerCandidate, SkillComposerMutation, SkillComposerPin, SkillComposerProjection,
    SkillSelectionIntent,
};

impl SkillSettingsService {
    /// First-send retry is idempotent after an earlier worker committed its
    /// exact pin but its UI acknowledgement was cancelled/lost. An existing
    /// pin is accepted only if the current approval and file still project as
    /// the same reviewed source/name/SHA; otherwise the transaction CAS below
    /// remains authoritative and fails closed.
    pub fn ensure_composer_pin(
        &self,
        thread_id: &str,
        intent: &SkillSelectionIntent,
    ) -> Result<(), SkillSettingsError> {
        let projection = self.composer_projection(thread_id)?;
        if projection.candidates.iter().any(|candidate| {
            candidate.selected
                && candidate.source_id == intent.source_id
                && candidate.name == intent.name
                && candidate.content_sha256 == intent.content_sha256
        }) {
            return Ok(());
        }
        self.apply_composer(
            thread_id,
            intent.expected_consent_generation,
            SkillComposerMutation::Pin {
                source_id: intent.source_id.clone(),
                name: intent.name.clone(),
                content_sha256: intent.content_sha256.clone(),
            },
        )
    }

    fn composer_thread(
        &self,
        store: &Store,
        thread_id: &str,
    ) -> Result<vega_store::threads::ThreadRow, SkillSettingsError> {
        let thread = vega_store::threads::find(store.conn(), thread_id)
            .map_err(|_| SkillSettingsError::Store)?
            .ok_or(SkillSettingsError::NotFound)?;
        if thread.project_id != self.selected_project_id.as_deref().unwrap_or("") {
            return Err(SkillSettingsError::NotFound);
        }
        Ok(thread)
    }

    /// List only current, enabled, hash-reviewed choices. Existing pins stay
    /// visible when unavailable so the user can remove them deliberately.
    pub fn composer_projection(
        &self,
        thread_id: &str,
    ) -> Result<SkillComposerProjection, SkillSettingsError> {
        self.composer_projection_inner(thread_id, false)
    }

    /// A new-task draft has no Store thread row yet. This read-only catalog
    /// does not create one; its choice remains a UI intent until first send.
    pub fn composer_draft_projection(
        &self,
        draft_thread_id: &str,
    ) -> Result<SkillComposerProjection, SkillSettingsError> {
        self.composer_projection_inner(draft_thread_id, true)
    }

    fn composer_projection_inner(
        &self,
        thread_id: &str,
        draft: bool,
    ) -> Result<SkillComposerProjection, SkillSettingsError> {
        let store = self.store()?;
        if !draft {
            self.composer_thread(&store, thread_id)?;
        }
        let settings = self.projection()?;
        let pins = if draft {
            Vec::new()
        } else {
            skills::list_thread_pins(store.conn(), thread_id)?
        };
        if skills::read_settings(store.conn())?.consent_generation != settings.consent_generation {
            return Err(SkillSettingsError::Stale);
        }
        let mut candidates = Vec::new();
        for source in &settings.sources {
            let scope_enabled = match source.scope {
                SkillUiScope::Project => settings.project_enabled,
                SkillUiScope::VegaGlobal | SkillUiScope::Imported => settings.global_enabled,
            };
            if !scope_enabled || !source.enabled || source.diagnostic.is_some() {
                continue;
            }
            for candidate in &source.candidates {
                let (Some(description), Some(sha)) =
                    (&candidate.description, &candidate.content_sha256)
                else {
                    continue;
                };
                if !candidate.enabled || candidate.diagnostic.is_some() {
                    continue;
                }
                candidates.push(SkillComposerCandidate {
                    source_id: source.id.clone(),
                    name: candidate.name.clone(),
                    description: description.clone(),
                    source_label: source.source_label.clone(),
                    content_sha256: sha.clone(),
                    scope: source.scope,
                    selected: pins.iter().any(|pin| {
                        pin.name == candidate.name
                            && pin.source_label == source.source_label
                            && pin.approved_sha256 == *sha
                            && pin.canonical_root == source.canonical_root.to_string_lossy()
                    }),
                });
            }
        }
        candidates.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| a.source_label.cmp(&b.source_label))
                .then_with(|| a.source_id.cmp(&b.source_id))
        });
        let pins = pins
            .into_iter()
            .map(|pin| SkillComposerPin {
                available: candidates.iter().any(|candidate| {
                    candidate.selected
                        && candidate.name == pin.name
                        && candidate.source_label == pin.source_label
                        && candidate.content_sha256 == pin.approved_sha256
                }),
                name: pin.name,
                source_label: pin.source_label,
                content_sha256: pin.approved_sha256,
            })
            .collect();
        Ok(SkillComposerProjection {
            thread_id: thread_id.to_owned(),
            consent_generation: settings.consent_generation,
            candidates,
            pins,
        })
    }

    /// Each mutation is a user gesture plus a transaction-local Store CAS.
    /// A pin additionally performs a fresh descriptor-fenced file read.
    pub fn apply_composer(
        &self,
        thread_id: &str,
        expected_generation: u64,
        mutation: SkillComposerMutation,
    ) -> Result<(), SkillSettingsError> {
        let store = self.store()?;
        self.composer_thread(&store, thread_id)?;
        match mutation {
            SkillComposerMutation::Pin {
                source_id,
                name,
                content_sha256,
            } => {
                let row = self.find_scoped_source(&store, &source_id)?;
                let settings = skills::read_settings(store.conn())?;
                let enabled = if row.scope == "project" {
                    let project_id = row
                        .project_id
                        .as_deref()
                        .ok_or(SkillSettingsError::Invalid)?;
                    skills::read_project_settings(store.conn(), project_id)?.enabled
                } else {
                    settings.global_enabled
                };
                if !enabled || !row.enabled {
                    return Err(SkillSettingsError::Stale);
                }
                let approval = skills::list_approved_skills(store.conn())?
                    .into_iter()
                    .find(|item| item.source_id == source_id && item.name == name)
                    .ok_or(SkillSettingsError::Stale)?;
                if !approval.enabled || approval.approved_sha256 != content_sha256 {
                    return Err(SkillSettingsError::Stale);
                }
                let source = self.rebind(&store, &row)?;
                let candidate = source
                    .discover()
                    .map_err(|_| SkillSettingsError::Stale)?
                    .candidates
                    .into_iter()
                    .find(|candidate| candidate.name == name && candidate.sha256 == content_sha256)
                    .ok_or(SkillSettingsError::Stale)?;
                source
                    .load_candidate(&candidate)
                    .map_err(|_| SkillSettingsError::Stale)?;
                let pins = skills::list_thread_pins(store.conn(), thread_id)?;
                if pins.len() >= vega_runtime::skills::MAX_RUN_ACTIVATIONS
                    && !pins.iter().any(|pin| pin.name == name)
                {
                    return Err(SkillSettingsError::SelectionLimit);
                }
                skills::save_thread_pin(
                    store.conn(),
                    expected_generation,
                    skills::NewThreadSkillPin {
                        thread_id,
                        scope: &row.scope,
                        canonical_root: &row.canonical_root,
                        root_dev: &row.root_dev,
                        root_ino: &row.root_ino,
                        name: &name,
                        approved_sha256: &approval.approved_sha256,
                        source_label: &approval.source_label,
                        pinned_at: now_ms(),
                    },
                )?;
            }
            SkillComposerMutation::Unpin { name } => {
                skills::clear_thread_pin(store.conn(), expected_generation, thread_id, &name)?;
            }
            SkillComposerMutation::DisableFuture {
                name,
                source_label,
                content_sha256,
            } => {
                let sources = skills::list_sources_for_project(
                    store.conn(),
                    self.selected_project_id.as_deref(),
                )?;
                let approvals = skills::list_approved_skills(store.conn())?;
                let mut matching = approvals.into_iter().filter(|approval| {
                    approval.name == name
                        && approval.source_label == source_label
                        && approval.approved_sha256 == content_sha256
                        && sources.iter().any(|source| source.id == approval.source_id)
                });
                let approval = matching.next().ok_or(SkillSettingsError::Stale)?;
                if matching.next().is_some() {
                    return Err(SkillSettingsError::Invalid);
                }
                skills::set_approval_preferences(
                    store.conn(),
                    expected_generation,
                    &approval.source_id,
                    &name,
                    false,
                    false,
                )?;
            }
        }
        Ok(())
    }
}
