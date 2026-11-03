//! UI-safe Skills settings commands and projections. Source paths are shown
//! only in Settings; provider/tool audit uses opaque labels instead.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillUiScope {
    Project,
    VegaGlobal,
    Imported,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SkillCandidateView {
    pub name: String,
    pub description: Option<String>,
    pub content_sha256: Option<String>,
    pub approved_sha256: Option<String>,
    pub size_bytes: Option<usize>,
    pub enabled: bool,
    pub automatic: bool,
    pub model_winner: bool,
    /// Stable, bounded code; never a raw path, YAML value, or file body.
    pub diagnostic: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SkillSourceView {
    pub id: String,
    pub scope: SkillUiScope,
    pub project_id: Option<String>,
    pub configured_root: PathBuf,
    pub canonical_root: PathBuf,
    pub source_label: String,
    pub enabled: bool,
    pub automatic: bool,
    pub diagnostic: Option<String>,
    pub candidates: Vec<SkillCandidateView>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SkillSettingsProjection {
    pub consent_generation: u64,
    pub revocation_generation: u64,
    pub global_enabled: bool,
    pub automatic_enabled: bool,
    pub selected_project_id: Option<String>,
    pub project_enabled: bool,
    pub project_automatic: bool,
    pub sources: Vec<SkillSourceView>,
}

/// A root is only associated after the user sees this exact canonical target
/// and confirms its ephemeral token. The token is not file authority.
#[derive(Clone, PartialEq, Eq)]
pub struct SkillRootPreview {
    pub token: String,
    pub source_label: String,
    pub scope: SkillUiScope,
    pub project_id: Option<String>,
    pub configured_root: PathBuf,
    pub canonical_root: PathBuf,
    pub candidates: Vec<SkillCandidateView>,
}

/// Full bounded SKILL.md text, delivered to Settings for explicit review.
/// Never log, persist in an audit row, or send to a provider before approval.
#[derive(Clone, PartialEq, Eq)]
pub struct SkillBodyPreview {
    pub token: String,
    pub source_id: String,
    pub name: String,
    pub description: String,
    pub content_sha256: String,
    pub body: String,
}

#[derive(Clone, PartialEq, Eq)]
pub enum SkillSettingsMutation {
    LinkRoot {
        preview_token: String,
    },
    ApproveSkill {
        preview_token: String,
    },
    SetGlobal {
        enabled: bool,
        automatic: bool,
    },
    SetProject {
        project_id: String,
        enabled: bool,
        automatic: bool,
    },
    SetSource {
        source_id: String,
        enabled: bool,
        automatic: bool,
    },
    SetSkill {
        source_id: String,
        name: String,
        enabled: bool,
        automatic: bool,
    },
    UnlinkSource {
        source_id: String,
    },
}

/// One live, already reviewed Skill the user may explicitly pin to a thread.
/// Private filesystem paths and Skill body bytes never enter this projection.
#[derive(Clone, PartialEq, Eq)]
pub struct SkillComposerCandidate {
    pub source_id: String,
    pub name: String,
    pub description: String,
    pub source_label: String,
    pub content_sha256: String,
    pub scope: SkillUiScope,
    pub selected: bool,
}

/// An exact stored identity remains visible even if its source becomes stale.
#[derive(Clone, PartialEq, Eq)]
pub struct SkillComposerPin {
    pub name: String,
    pub source_label: String,
    pub content_sha256: String,
    pub available: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SkillComposerProjection {
    pub thread_id: String,
    pub consent_generation: u64,
    pub candidates: Vec<SkillComposerCandidate>,
    pub pins: Vec<SkillComposerPin>,
}

/// A draft-only, content-free user choice. It is not Store authority: first
/// send must revalidate the root, exact bytes and approval inside a worker.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillSelectionIntent {
    pub source_id: String,
    pub name: String,
    pub content_sha256: String,
    pub expected_consent_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillMention {
    pub range: std::ops::Range<usize>,
    pub text: String,
    pub intent: SkillSelectionIntent,
}

impl SkillMention {
    pub fn selections(content: &str, mentions: &[Self]) -> Option<Vec<SkillSelectionIntent>> {
        let mut ranges = mentions
            .iter()
            .map(|mention| &mention.range)
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| range.start);
        if ranges.windows(2).any(|pair| pair[0].end > pair[1].start) {
            return None;
        }
        let mut selections = Vec::new();
        for mention in mentions {
            if mention.text.is_empty()
                || content.get(mention.range.clone()) != Some(mention.text.as_str())
            {
                return None;
            }
            if !selections.contains(&mention.intent) {
                selections.push(mention.intent.clone());
            }
        }
        (selections.len() <= vega_runtime::skills::MAX_RUN_ACTIVATIONS).then_some(selections)
    }
}

/// Content-free, already-durable activation displayed only for the current
/// assistant run. The label is generated by Vega and cannot contain a path.
#[derive(Clone, PartialEq, Eq)]
pub struct ActiveSkillView {
    pub name: String,
    pub source_label: String,
    pub content_sha256: String,
}

/// A user gesture only; the controller checks the current root and approval
/// again before touching the transactionally bound thread pin.
#[derive(Clone, PartialEq, Eq)]
pub enum SkillComposerMutation {
    Pin {
        source_id: String,
        name: String,
        content_sha256: String,
    },
    Unpin {
        name: String,
    },
    DisableFuture {
        name: String,
        source_label: String,
        content_sha256: String,
    },
}
