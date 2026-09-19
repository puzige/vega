//! Headless Agent Skills source and format boundary (Issue #74).

mod catalog;
mod frontmatter;
mod source;

pub use catalog::{
    ActivationAudit, ActivationOrigin, ActivationOutcome, ActivationReceipt, ActiveSkillSummary,
    BatchPolicy, CatalogExclusion, FrozenReference, LOAD_SKILL_TOOL_NAME, MAX_CATALOG_BYTES,
    MAX_RUN_ACTIVATIONS, MAX_RUN_REFERENCE_BYTES, MAX_RUN_SNAPSHOT_BYTES, PersistedSkillApproval,
    READ_SKILL_RESOURCE_TOOL_NAME, RunBinding, SkillApproval, SkillCatalog, SkillRun,
    SkillRunSnapshot, SkillSelection, classify_tool_batch,
};
pub use frontmatter::{MAX_FRONTMATTER_BYTES, SkillDocument, SkillMetadata, parse_skill_md};
pub use source::{
    Discovery, MAX_SKILL_BYTES, SkillCandidate, SkillDiagnostic, SkillError, SkillSource,
    SourceIdentity, SourceKind, SourceScope, deduplicate_sources, resolve_precedence,
};

#[cfg(test)]
mod catalog_tests;

#[cfg(test)]
mod snapshot_tests;

#[cfg(test)]
mod tests;
