//! Headless Agent Skills source and format boundary (Issue #74).

mod frontmatter;
mod source;

pub use frontmatter::{MAX_FRONTMATTER_BYTES, SkillDocument, SkillMetadata, parse_skill_md};
pub use source::{
    Discovery, MAX_SKILL_BYTES, SkillCandidate, SkillDiagnostic, SkillError, SkillSource,
    SourceKind, deduplicate_sources, resolve_precedence,
};

#[cfg(test)]
mod tests;
