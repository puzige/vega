//! Settings-owned Skill discovery and consent. The UI sends choices to this
//! controller; it never reads SQLite or trusts a previously scanned file.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use vega_runtime::skills::{SkillSource, SourceIdentity, parse_skill_md, resolve_precedence};
use vega_store::Store;
use vega_store::skills::{
    self, NewSkillApproval, NewSkillSource, SkillSourceRecord, SkillStoreError,
};

use crate::types::{
    SkillBodyPreview, SkillCandidateView, SkillRootPreview, SkillSettingsMutation,
    SkillSettingsProjection, SkillSourceView, SkillUiScope,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SkillSettingsError {
    #[error("Skill settings storage unavailable")]
    Store,
    #[error("Skill source unavailable")]
    NotFound,
    #[error("Skill source or review changed")]
    Stale,
    #[error("Skill source is unsafe or invalid")]
    Invalid,
    #[error("Skill preview required")]
    PreviewRequired,
    #[error("Skill selection limit reached")]
    SelectionLimit,
}

impl SkillSettingsError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Store => "storage_failed",
            Self::NotFound => "not_found",
            Self::Stale => "changed_review_required",
            Self::Invalid => "invalid_source",
            Self::PreviewRequired => "preview_required",
            Self::SelectionLimit => "selection_limit",
        }
    }
}

impl From<SkillStoreError> for SkillSettingsError {
    fn from(error: SkillStoreError) -> Self {
        match error {
            SkillStoreError::Stale | SkillStoreError::DigestMismatch => Self::Stale,
            SkillStoreError::NotFound => Self::NotFound,
            SkillStoreError::InvalidInput | SkillStoreError::TooLarge => Self::Invalid,
            SkillStoreError::Sql(_) | SkillStoreError::GenerationOverflow => Self::Store,
        }
    }
}

struct RootTicket {
    token: String,
    source_id: String,
    consent_generation: u64,
    scope: SkillUiScope,
    configured_root: PathBuf,
    identity: SourceIdentity,
}

struct SkillTicket {
    token: String,
    consent_generation: u64,
    source_id: String,
    name: String,
    sha256: String,
}

#[derive(Default)]
struct PreviewState {
    active: bool,
    epoch: u64,
    root: Option<RootTicket>,
    skill: Option<SkillTicket>,
}

/// One Settings session owns ephemeral preview receipts. Reopening Settings
/// invalidates them; Store CAS and a live SHA read are still mandatory.
#[derive(Clone)]
pub struct SkillSettingsService {
    database_path: PathBuf,
    config_root: PathBuf,
    selected_project_id: Option<String>,
    previews: Arc<Mutex<PreviewState>>,
}

impl SkillSettingsService {
    pub fn new(
        database_path: PathBuf,
        config_root: PathBuf,
        selected_project_id: Option<String>,
    ) -> Self {
        Self {
            database_path,
            config_root,
            selected_project_id,
            previews: Arc::new(Mutex::new(PreviewState {
                active: true,
                ..PreviewState::default()
            })),
        }
    }

    /// Leaving the Skills page revokes its ephemeral root/body receipts.
    /// Reentering the page may still use this Settings session after reload.
    pub fn clear_previews(&self) {
        if let Ok(mut previews) = self.previews.lock() {
            previews.epoch = previews.epoch.wrapping_add(1);
            previews.root = None;
            previews.skill = None;
        }
    }

    /// Closing Settings retires every clone of this session, including a
    /// worker that may still be finishing a native folder-picker request.
    pub fn close_session(&self) {
        if let Ok(mut previews) = self.previews.lock() {
            previews.active = false;
            previews.epoch = previews.epoch.wrapping_add(1);
            previews.root = None;
            previews.skill = None;
        }
    }

    fn store(&self) -> Result<Store, SkillSettingsError> {
        let store = Store::open(&self.database_path).map_err(|_| SkillSettingsError::Store)?;
        store.migrate().map_err(|_| SkillSettingsError::Store)?;
        Ok(store)
    }

    fn preview_epoch(&self) -> Result<u64, SkillSettingsError> {
        let previews = self
            .previews
            .lock()
            .map_err(|_| SkillSettingsError::Store)?;
        if !previews.active {
            return Err(SkillSettingsError::Stale);
        }
        Ok(previews.epoch)
    }

    /// Read exactly linked roots for this Settings project's scope; an
    /// unimported home directory is never scanned.
    pub fn projection(&self) -> Result<SkillSettingsProjection, SkillSettingsError> {
        let store = self.store()?;
        let settings = skills::read_settings(store.conn())?;
        let project_settings = self
            .selected_project_id
            .as_deref()
            .map(|id| skills::read_project_settings(store.conn(), id))
            .transpose()?
            .unwrap_or(skills::ProjectSkillSettings {
                enabled: false,
                automatic: false,
            });
        let rows =
            skills::list_sources_for_project(store.conn(), self.selected_project_id.as_deref())?;
        let approvals = skills::list_approved_skills(store.conn())?
            .into_iter()
            .map(|approval| {
                (
                    (approval.source_id.clone(), approval.name.clone()),
                    approval,
                )
            })
            .collect::<HashMap<_, _>>();
        let mut sources = Vec::with_capacity(rows.len());
        let mut identities = Vec::with_capacity(rows.len());
        let mut eligible = Vec::new();
        for row in &rows {
            let scope = parse_scope(&row.scope)?;
            let label = source_label(&row.id);
            let mut view = SkillSourceView {
                id: row.id.clone(),
                scope,
                project_id: row.project_id.clone(),
                configured_root: PathBuf::from(&row.configured_root),
                canonical_root: PathBuf::from(&row.canonical_root),
                source_label: label,
                enabled: row.enabled,
                automatic: row.automatic,
                diagnostic: None,
                candidates: Vec::new(),
            };
            let source = match self.rebind(&store, row) {
                Ok(source) => source,
                Err(error) => {
                    view.diagnostic = Some(error.code().to_owned());
                    sources.push(view);
                    identities.push(None);
                    continue;
                }
            };
            identities.push(Some(source.identity()));
            let discovery = match source.discover() {
                Ok(discovery) => discovery,
                Err(error) => {
                    view.diagnostic = Some(error.code().to_owned());
                    sources.push(view);
                    continue;
                }
            };
            for diagnostic in discovery.diagnostics {
                let approval = approvals.get(&(row.id.clone(), diagnostic.name.clone()));
                view.candidates.push(SkillCandidateView {
                    name: diagnostic.name,
                    description: None,
                    content_sha256: None,
                    approved_sha256: approval.map(|item| item.approved_sha256.clone()),
                    size_bytes: None,
                    enabled: approval.is_some_and(|item| item.enabled),
                    automatic: approval.is_some_and(|item| item.automatic),
                    model_winner: false,
                    diagnostic: Some(diagnostic.error.code().to_owned()),
                });
            }
            for candidate in discovery.candidates {
                let approval = approvals.get(&(row.id.clone(), candidate.name.clone()));
                let current = approval.is_some_and(|item| item.approved_sha256 == candidate.sha256);
                let scope_enabled = if scope == SkillUiScope::Project {
                    project_settings.enabled
                } else {
                    settings.global_enabled
                };
                let scope_auto = if scope == SkillUiScope::Project {
                    project_settings.automatic
                } else {
                    settings.automatic_enabled
                };
                if current
                    && row.enabled
                    && row.automatic
                    && scope_enabled
                    && scope_auto
                    && approval.is_some_and(|item| item.enabled && item.automatic)
                {
                    eligible.push(candidate.clone());
                }
                view.candidates.push(SkillCandidateView {
                    name: candidate.name,
                    description: Some(candidate.description),
                    content_sha256: Some(candidate.sha256.clone()),
                    approved_sha256: approval.map(|item| item.approved_sha256.clone()),
                    size_bytes: Some(candidate.size_bytes),
                    enabled: approval.is_some_and(|item| item.enabled),
                    automatic: approval.is_some_and(|item| item.automatic),
                    model_winner: false,
                    diagnostic: if approval.is_none() {
                        Some("review_required".into())
                    } else if !current {
                        Some("changed_review_required".into())
                    } else {
                        None
                    },
                });
            }
            view.candidates.sort_by(|a, b| a.name.cmp(&b.name));
            sources.push(view);
        }
        let winners = resolve_precedence(eligible)
            .into_iter()
            .map(|candidate| {
                (
                    candidate.source.identity(),
                    candidate.name,
                    candidate.sha256,
                )
            })
            .collect::<BTreeSet<_>>();
        for (identity, source) in identities.into_iter().zip(sources.iter_mut()) {
            let Some(identity) = identity else { continue };
            for candidate in &mut source.candidates {
                candidate.model_winner = candidate.content_sha256.as_ref().is_some_and(|sha| {
                    winners.contains(&(identity.clone(), candidate.name.clone(), sha.clone()))
                });
            }
        }
        Ok(SkillSettingsProjection {
            consent_generation: settings.consent_generation,
            revocation_generation: settings.revocation_generation,
            global_enabled: settings.global_enabled,
            automatic_enabled: settings.automatic_enabled,
            selected_project_id: self.selected_project_id.clone(),
            project_enabled: project_settings.enabled,
            project_automatic: project_settings.automatic,
            sources,
        })
    }

    /// Preview the selected project's exact root, if one exists.
    pub fn preview_project_root(&self) -> Result<SkillRootPreview, SkillSettingsError> {
        let epoch = self.preview_epoch()?;
        let store = self.store()?;
        let project_id = self
            .selected_project_id
            .as_deref()
            .ok_or(SkillSettingsError::NotFound)?;
        let project = vega_store::projects::find(store.conn(), project_id)
            .map_err(|_| SkillSettingsError::Store)?
            .ok_or(SkillSettingsError::NotFound)?;
        let canonical_project = Path::new(&project.path)
            .canonicalize()
            .map_err(|_| SkillSettingsError::NotFound)?;
        let source = SkillSource::project_approved(&canonical_project)
            .map_err(|_| SkillSettingsError::Invalid)?
            .ok_or(SkillSettingsError::NotFound)?;
        self.preview_root(
            &store,
            SkillUiScope::Project,
            canonical_project.join(".agents/skills"),
            source,
            epoch,
        )
    }

    /// Preview Vega's own config-root Skills, never a reconstructed home path.
    pub fn preview_vega_global_root(&self) -> Result<SkillRootPreview, SkillSettingsError> {
        let epoch = self.preview_epoch()?;
        let store = self.store()?;
        let source = SkillSource::vega_global(&self.config_root)
            .map_err(|_| SkillSettingsError::Invalid)?
            .ok_or(SkillSettingsError::NotFound)?;
        self.preview_root(
            &store,
            SkillUiScope::VegaGlobal,
            self.config_root.join("skills"),
            source,
            epoch,
        )
    }

    /// The caller supplies only the native folder picker's exact returned
    /// directory. Nothing under the user's home is searched implicitly.
    pub fn preview_imported_root(
        &self,
        picked: &Path,
    ) -> Result<SkillRootPreview, SkillSettingsError> {
        let epoch = self.preview_epoch()?;
        let store = self.store()?;
        let source =
            SkillSource::imported_approved(picked, 0).map_err(|_| SkillSettingsError::Invalid)?;
        self.preview_root(
            &store,
            SkillUiScope::Imported,
            picked.to_path_buf(),
            source,
            epoch,
        )
    }

    fn preview_root(
        &self,
        store: &Store,
        scope: SkillUiScope,
        configured_root: PathBuf,
        source: SkillSource,
        epoch: u64,
    ) -> Result<SkillRootPreview, SkillSettingsError> {
        let discovery = source.discover().map_err(|_| SkillSettingsError::Invalid)?;
        let generation = skills::read_settings(store.conn())?.consent_generation;
        let token = ulid::Ulid::generate().to_string();
        let identity = source.identity();
        let source_id = skills::list_sources(store.conn())?
            .into_iter()
            .find(|row| {
                parse_scope(&row.scope).ok() == Some(scope)
                    && persisted_identity_matches(row, &identity)
            })
            .map(|row| row.id)
            .unwrap_or_else(|| ulid::Ulid::generate().to_string());
        let candidates = discovery
            .candidates
            .into_iter()
            .map(|candidate| SkillCandidateView {
                name: candidate.name,
                description: Some(candidate.description),
                content_sha256: Some(candidate.sha256),
                approved_sha256: None,
                size_bytes: Some(candidate.size_bytes),
                enabled: false,
                automatic: false,
                model_winner: false,
                diagnostic: None,
            })
            .chain(
                discovery
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| SkillCandidateView {
                        name: diagnostic.name,
                        description: None,
                        content_sha256: None,
                        approved_sha256: None,
                        size_bytes: None,
                        enabled: false,
                        automatic: false,
                        model_winner: false,
                        diagnostic: Some(diagnostic.error.code().to_owned()),
                    }),
            )
            .collect();
        let mut previews = self
            .previews
            .lock()
            .map_err(|_| SkillSettingsError::Store)?;
        if !previews.active || previews.epoch != epoch {
            return Err(SkillSettingsError::Stale);
        }
        previews.root = Some(RootTicket {
            token: token.clone(),
            source_id: source_id.clone(),
            consent_generation: generation,
            scope,
            configured_root: configured_root.clone(),
            identity,
        });
        Ok(SkillRootPreview {
            token,
            source_label: source_label(&source_id),
            scope,
            project_id: (scope == SkillUiScope::Project)
                .then(|| self.selected_project_id.clone())
                .flatten(),
            configured_root,
            canonical_root: source.canonical_root().to_path_buf(),
            candidates,
        })
    }

    /// Read one full bounded SKILL.md for the user's explicit review. The receipt
    /// can approve only these same bytes after another live descriptor read.
    pub fn preview_skill(
        &self,
        source_id: &str,
        name: &str,
    ) -> Result<SkillBodyPreview, SkillSettingsError> {
        let epoch = self.preview_epoch()?;
        let store = self.store()?;
        let row = self.find_scoped_source(&store, source_id)?;
        let source = self.rebind(&store, &row)?;
        let candidate = source
            .discover()
            .map_err(|_| SkillSettingsError::Invalid)?
            .candidates
            .into_iter()
            .find(|candidate| candidate.name == name)
            .ok_or(SkillSettingsError::NotFound)?;
        let bytes = source
            .load_candidate(&candidate)
            .map_err(|_| SkillSettingsError::Stale)?;
        let document = parse_skill_md(&bytes, name).map_err(|_| SkillSettingsError::Invalid)?;
        let reviewed_text = std::str::from_utf8(&bytes)
            .map_err(|_| SkillSettingsError::Invalid)?
            .to_owned();
        let generation = skills::read_settings(store.conn())?.consent_generation;
        let token = ulid::Ulid::generate().to_string();
        let mut previews = self
            .previews
            .lock()
            .map_err(|_| SkillSettingsError::Store)?;
        if !previews.active || previews.epoch != epoch {
            return Err(SkillSettingsError::Stale);
        }
        previews.skill = Some(SkillTicket {
            token: token.clone(),
            consent_generation: generation,
            source_id: source_id.to_owned(),
            name: name.to_owned(),
            sha256: candidate.sha256.clone(),
        });
        Ok(SkillBodyPreview {
            token,
            source_id: source_id.to_owned(),
            name: name.to_owned(),
            description: document.metadata.description,
            content_sha256: candidate.sha256,
            body: reviewed_text,
        })
    }

    /// Apply only an explicit Settings action. All rows use Store's
    /// transaction-local consent-generation CAS; preview actions additionally
    /// rebind root and re-read the exact reviewed Skill hash.
    pub fn apply(
        &self,
        expected_generation: u64,
        mutation: SkillSettingsMutation,
    ) -> Result<(), SkillSettingsError> {
        let store = self.store()?;
        let previews = self
            .previews
            .lock()
            .map_err(|_| SkillSettingsError::Store)?;
        if !previews.active {
            return Err(SkillSettingsError::Stale);
        }
        match mutation {
            SkillSettingsMutation::LinkRoot { preview_token } => {
                self.link_preview(&store, expected_generation, &preview_token, &previews)?;
            }
            SkillSettingsMutation::ApproveSkill { preview_token } => {
                self.approve_preview(&store, expected_generation, &preview_token, &previews)?;
            }
            SkillSettingsMutation::SetGlobal { enabled, automatic } => {
                skills::set_global_settings(store.conn(), expected_generation, enabled, automatic)?;
            }
            SkillSettingsMutation::SetProject {
                project_id,
                enabled,
                automatic,
            } => {
                if self.selected_project_id.as_deref() != Some(project_id.as_str()) {
                    return Err(SkillSettingsError::Invalid);
                }
                skills::set_project_settings(
                    store.conn(),
                    expected_generation,
                    &project_id,
                    enabled,
                    automatic,
                )?;
            }
            SkillSettingsMutation::SetSource {
                source_id,
                enabled,
                automatic,
            } => {
                let row = self.find_scoped_source(&store, &source_id)?;
                if enabled {
                    self.rebind(&store, &row)?;
                }
                skills::set_source_preferences(
                    store.conn(),
                    expected_generation,
                    &source_id,
                    enabled,
                    automatic,
                )?;
            }
            SkillSettingsMutation::SetSkill {
                source_id,
                name,
                enabled,
                automatic,
            } => {
                self.find_scoped_source(&store, &source_id)?;
                skills::set_approval_preferences(
                    store.conn(),
                    expected_generation,
                    &source_id,
                    &name,
                    enabled,
                    automatic,
                )?;
            }
            SkillSettingsMutation::UnlinkSource { source_id } => {
                self.find_scoped_source(&store, &source_id)?;
                skills::unlink_source(store.conn(), expected_generation, &source_id)?;
            }
        }
        Ok(())
    }

    fn link_preview(
        &self,
        store: &Store,
        expected_generation: u64,
        token: &str,
        previews: &PreviewState,
    ) -> Result<(), SkillSettingsError> {
        let ticket = previews
            .root
            .as_ref()
            .ok_or(SkillSettingsError::PreviewRequired)?;
        if ticket.token != token || ticket.consent_generation != expected_generation {
            return Err(SkillSettingsError::Stale);
        }
        if skills::read_settings(store.conn())?.consent_generation != expected_generation {
            return Err(SkillSettingsError::Stale);
        }
        let source = match ticket.scope {
            SkillUiScope::Project => {
                let project_id = self
                    .selected_project_id
                    .as_deref()
                    .ok_or(SkillSettingsError::Invalid)?;
                let project = vega_store::projects::find(store.conn(), project_id)
                    .map_err(|_| SkillSettingsError::Store)?
                    .ok_or(SkillSettingsError::NotFound)?;
                SkillSource::project_approved(Path::new(&project.path))
                    .map_err(|_| SkillSettingsError::Stale)?
                    .ok_or(SkillSettingsError::Stale)?
            }
            SkillUiScope::VegaGlobal => SkillSource::vega_global(&self.config_root)
                .map_err(|_| SkillSettingsError::Stale)?
                .ok_or(SkillSettingsError::Stale)?,
            SkillUiScope::Imported => SkillSource::imported_approved(&ticket.configured_root, 0)
                .map_err(|_| SkillSettingsError::Stale)?,
        };
        if source.identity() != ticket.identity {
            return Err(SkillSettingsError::Stale);
        }
        let rows = skills::list_sources(store.conn())?;
        if rows.iter().any(|row| {
            parse_scope(&row.scope).ok() == Some(ticket.scope)
                && persisted_identity_matches(row, &ticket.identity)
        }) {
            return Ok(());
        }
        let import_order = if ticket.scope == SkillUiScope::Imported {
            rows.iter()
                .filter(|row| row.scope == "imported")
                .map(|row| row.import_order)
                .max()
                .unwrap_or(-1)
                .checked_add(1)
                .ok_or(SkillSettingsError::Invalid)?
        } else {
            0
        };
        let id = ticket.source_id.clone();
        let configured_root = ticket
            .configured_root
            .to_str()
            .ok_or(SkillSettingsError::Invalid)?;
        let canonical_root = ticket
            .identity
            .canonical_root()
            .to_str()
            .ok_or(SkillSettingsError::Invalid)?;
        skills::link_source(
            store.conn(),
            expected_generation,
            NewSkillSource {
                id: &id,
                scope: scope_text(ticket.scope),
                project_id: (ticket.scope == SkillUiScope::Project)
                    .then_some(self.selected_project_id.as_deref())
                    .flatten(),
                configured_root,
                canonical_root,
                root_dev: &ticket.identity.device().to_string(),
                root_ino: &ticket.identity.inode().to_string(),
                import_order,
                enabled: false,
                automatic: false,
                created_at: now_ms(),
            },
        )?;
        Ok(())
    }

    fn approve_preview(
        &self,
        store: &Store,
        expected_generation: u64,
        token: &str,
        previews: &PreviewState,
    ) -> Result<(), SkillSettingsError> {
        let ticket = previews
            .skill
            .as_ref()
            .ok_or(SkillSettingsError::PreviewRequired)?;
        if ticket.token != token || ticket.consent_generation != expected_generation {
            return Err(SkillSettingsError::Stale);
        }
        let row = self.find_scoped_source(store, &ticket.source_id)?;
        let source = self.rebind(store, &row)?;
        let candidate = source
            .discover()
            .map_err(|_| SkillSettingsError::Stale)?
            .candidates
            .into_iter()
            .find(|candidate| candidate.name == ticket.name)
            .ok_or(SkillSettingsError::Stale)?;
        if candidate.sha256 != ticket.sha256 || source.load_candidate(&candidate).is_err() {
            return Err(SkillSettingsError::Stale);
        }
        skills::approve_skill(
            store.conn(),
            expected_generation,
            NewSkillApproval {
                source_id: &ticket.source_id,
                name: &ticket.name,
                approved_sha256: &ticket.sha256,
                source_label: &source_label(&ticket.source_id),
                enabled: true,
                automatic: false,
                reviewed_at: now_ms(),
            },
        )?;
        Ok(())
    }

    fn find_scoped_source(
        &self,
        store: &Store,
        id: &str,
    ) -> Result<SkillSourceRecord, SkillSettingsError> {
        skills::list_sources_for_project(store.conn(), self.selected_project_id.as_deref())?
            .into_iter()
            .find(|row| row.id == id)
            .ok_or(SkillSettingsError::NotFound)
    }

    fn rebind(
        &self,
        store: &Store,
        row: &SkillSourceRecord,
    ) -> Result<SkillSource, SkillSettingsError> {
        let source = match parse_scope(&row.scope)? {
            SkillUiScope::Project => {
                if row.project_id.as_deref() != self.selected_project_id.as_deref() {
                    return Err(SkillSettingsError::NotFound);
                }
                let project = vega_store::projects::find(
                    store.conn(),
                    row.project_id
                        .as_deref()
                        .ok_or(SkillSettingsError::Invalid)?,
                )
                .map_err(|_| SkillSettingsError::Store)?
                .ok_or(SkillSettingsError::NotFound)?;
                let canonical = Path::new(&project.path)
                    .canonicalize()
                    .map_err(|_| SkillSettingsError::Stale)?;
                if canonical.join(".agents/skills") != Path::new(&row.configured_root) {
                    return Err(SkillSettingsError::Stale);
                }
                SkillSource::project_approved(&canonical)
                    .map_err(|_| SkillSettingsError::Stale)?
                    .ok_or(SkillSettingsError::Stale)?
            }
            SkillUiScope::VegaGlobal => {
                if self.config_root.join("skills") != Path::new(&row.configured_root) {
                    return Err(SkillSettingsError::Stale);
                }
                SkillSource::vega_global(&self.config_root)
                    .map_err(|_| SkillSettingsError::Stale)?
                    .ok_or(SkillSettingsError::Stale)?
            }
            SkillUiScope::Imported => SkillSource::imported_approved(
                Path::new(&row.configured_root),
                usize::try_from(row.import_order).map_err(|_| SkillSettingsError::Invalid)?,
            )
            .map_err(|_| SkillSettingsError::Stale)?,
        };
        if !persisted_identity_matches(row, &source.identity()) {
            return Err(SkillSettingsError::Stale);
        }
        Ok(source)
    }
}

fn persisted_identity_matches(row: &SkillSourceRecord, identity: &SourceIdentity) -> bool {
    parse_scope(&row.scope).ok().is_some_and(|scope| {
        matches!(
            (scope, identity.scope()),
            (
                SkillUiScope::Project,
                vega_runtime::skills::SourceScope::Project
            ) | (
                SkillUiScope::VegaGlobal,
                vega_runtime::skills::SourceScope::VegaGlobal
            ) | (
                SkillUiScope::Imported,
                vega_runtime::skills::SourceScope::Imported
            )
        )
    }) && Path::new(&row.canonical_root) == identity.canonical_root()
        && row.root_dev == identity.device().to_string()
        && row.root_ino == identity.inode().to_string()
}

fn parse_scope(scope: &str) -> Result<SkillUiScope, SkillSettingsError> {
    match scope {
        "project" => Ok(SkillUiScope::Project),
        "vega_global" => Ok(SkillUiScope::VegaGlobal),
        "imported" => Ok(SkillUiScope::Imported),
        _ => Err(SkillSettingsError::Invalid),
    }
}

fn scope_text(scope: SkillUiScope) -> &'static str {
    match scope {
        SkillUiScope::Project => "project",
        SkillUiScope::VegaGlobal => "vega_global",
        SkillUiScope::Imported => "imported",
    }
}

fn source_label(id: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(id.as_bytes()));
    format!("skill-{}", &digest[..16])
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;

mod composer;
