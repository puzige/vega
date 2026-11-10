use super::{SkillSettingsError, now_ms, source_label};
use vega_store::Store;
use vega_store::skills::{self, NewSkillApproval, NewSkillSource, SkillSourceRecord};

pub(super) struct SkillApprovalInstall {
    pub source_id: String,
    pub name: String,
    pub sha256: String,
    pub target_binding: Option<String>,
    pub enabled: bool,
    pub automatic: bool,
}

pub(super) enum SkillSettingsWrite {
    Unchanged,
    Global {
        enabled: bool,
        automatic: bool,
    },
    Project {
        project_id: String,
        enabled: bool,
        automatic: bool,
    },
    Source {
        source_id: String,
        enabled: bool,
        automatic: bool,
    },
    Unlink {
        source_id: String,
    },
    Link(SkillSourceRecord),
    Approval(SkillApprovalInstall),
}

pub(super) struct SaveSkillSettings {
    pub previews: std::sync::Arc<std::sync::Mutex<super::PreviewState>>,
    pub preview_epoch: u64,
    pub expected_generation: u64,
    pub command: SkillSettingsWrite,
}

impl crate::database_writer::Sealed for SaveSkillSettings {}

impl crate::database_writer::DatabaseCommand for SaveSkillSettings {
    type Output = Result<(), SkillSettingsError>;

    fn apply(self, store: &Store) -> Self::Output {
        let previews = self
            .previews
            .lock()
            .map_err(|_| SkillSettingsError::Store)?;
        if !previews.active || previews.epoch != self.preview_epoch {
            return Err(SkillSettingsError::Stale);
        }
        let generation = self.expected_generation;
        match self.command {
            SkillSettingsWrite::Unchanged => {
                if skills::read_settings(store.conn())?.consent_generation != generation {
                    return Err(SkillSettingsError::Stale);
                }
            }
            SkillSettingsWrite::Global { enabled, automatic } => {
                skills::set_global_settings(store.conn(), generation, enabled, automatic)?;
            }
            SkillSettingsWrite::Project {
                project_id,
                enabled,
                automatic,
            } => {
                skills::set_project_settings(
                    store.conn(),
                    generation,
                    &project_id,
                    enabled,
                    automatic,
                )?;
            }
            SkillSettingsWrite::Source {
                source_id,
                enabled,
                automatic,
            } => {
                skills::set_source_preferences(
                    store.conn(),
                    generation,
                    &source_id,
                    enabled,
                    automatic,
                )?;
            }
            SkillSettingsWrite::Unlink { source_id } => {
                skills::unlink_source(store.conn(), generation, &source_id)?;
            }
            SkillSettingsWrite::Link(row) => {
                skills::link_source(
                    store.conn(),
                    generation,
                    NewSkillSource {
                        id: &row.id,
                        scope: &row.scope,
                        project_id: row.project_id.as_deref(),
                        configured_root: &row.configured_root,
                        canonical_root: &row.canonical_root,
                        root_dev: &row.root_dev,
                        root_ino: &row.root_ino,
                        import_order: row.import_order,
                        enabled: row.enabled,
                        automatic: row.automatic,
                        created_at: row.created_at,
                    },
                )?;
            }
            SkillSettingsWrite::Approval(row) => {
                skills::approve_skill(
                    store.conn(),
                    generation,
                    NewSkillApproval {
                        source_id: &row.source_id,
                        name: &row.name,
                        approved_sha256: &row.sha256,
                        target_binding: row.target_binding.as_deref(),
                        source_label: &source_label(&row.source_id),
                        enabled: row.enabled,
                        automatic: row.automatic,
                        reviewed_at: now_ms(),
                    },
                )?;
            }
        }
        Ok(())
    }
}
