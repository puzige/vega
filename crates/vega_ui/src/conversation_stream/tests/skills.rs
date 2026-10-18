use super::*;
use vega_conversation::types::{ActiveSkillView, SkillComposerCandidate, SkillUiScope};

fn projection(thread_id: &str) -> SkillComposerProjection {
    SkillComposerProjection {
        thread_id: thread_id.into(),
        consent_generation: 9,
        candidates: vec![SkillComposerCandidate {
            source_id: "reviewed-source".into(),
            name: "reviewer".into(),
            description: "Review changes".into(),
            source_label: "project:reviewed".into(),
            content_sha256: "a".repeat(64),
            scope: SkillUiScope::Project,
            selected: false,
        }],
        pins: Vec::new(),
    }
}
