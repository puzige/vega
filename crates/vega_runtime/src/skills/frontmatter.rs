//! Bounded Agent Skills frontmatter decoding with no include or interpolation.

use super::source::{MAX_SKILL_BYTES, SkillError, valid_name};
use serde::Deserialize;
use serde_saphyr::granit_parser::{Event, Parser, Tag};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy};
use std::collections::BTreeMap;

/// Maximum byte size of YAML frontmatter, excluding the two fences.
pub const MAX_FRONTMATTER_BYTES: usize = 8 * 1024;
const MAX_YAML_EVENTS: usize = 2048;
const MAX_YAML_NODES: usize = 1024;
const MAX_YAML_DEPTH: usize = 16;
const MAX_YAML_ALIASES: usize = 16;

/// Validated Agent Skills metadata. `allowed_tools` is inert display data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub allowed_tools: Option<String>,
}

/// One validated SKILL.md snapshot; the caller supplies consent and run state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillDocument {
    pub metadata: SkillMetadata,
    pub body: String,
}

#[derive(Deserialize)]
struct RawMetadata {
    name: String,
    description: String,
    license: Option<String>,
    compatibility: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<String>,
}

/// Parse a bounded complete SKILL.md; reject non-core tags before Serde can
/// erase their provenance. No resolver or property map is ever supplied.
pub fn parse_skill_md(bytes: &[u8], directory_name: &str) -> Result<SkillDocument, SkillError> {
    if bytes.len() > MAX_SKILL_BYTES {
        return Err(SkillError::TooLarge);
    }
    if bytes.contains(&0) {
        return Err(SkillError::InvalidUtf8);
    }
    if !valid_name(directory_name) {
        return Err(SkillError::InvalidName);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SkillError::InvalidUtf8)?;
    let (yaml, body) = split_frontmatter(text)?;
    check_yaml_events(yaml)?;

    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_documents: 1,
            max_events: MAX_YAML_EVENTS,
            max_nodes: MAX_YAML_NODES,
            max_total_scalar_bytes: MAX_FRONTMATTER_BYTES,
            max_total_comment_bytes: MAX_FRONTMATTER_BYTES,
            max_depth: MAX_YAML_DEPTH,
            max_aliases: MAX_YAML_ALIASES,
            max_anchors: MAX_YAML_ALIASES,
            max_merge_keys: 0,
            max_inclusion_depth: 0,
        },
        alias_limits: serde_saphyr::alias_limits! {
            max_total_replayed_events: 4096,
            max_replay_stack_depth: MAX_YAML_DEPTH,
            max_alias_expansions_per_anchor: MAX_YAML_ALIASES,
        },
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        strict_booleans: true,
        with_snippet: false,
    };
    let parsed: RawMetadata = serde_saphyr::from_str_with_options(yaml, options)
        .map_err(|_| SkillError::MalformedYaml)?;
    if !valid_name(&parsed.name) || parsed.name != directory_name {
        return Err(SkillError::InvalidName);
    }
    if parsed.description.trim().is_empty() || parsed.description.chars().count() > 1024 {
        return Err(SkillError::InvalidFormat);
    }
    if parsed
        .compatibility
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.chars().count() > 500)
    {
        return Err(SkillError::InvalidFormat);
    }
    Ok(SkillDocument {
        metadata: SkillMetadata {
            name: parsed.name,
            description: parsed.description,
            license: parsed.license,
            compatibility: parsed.compatibility,
            metadata: parsed.metadata,
            allowed_tools: parsed.allowed_tools,
        },
        body: body.to_string(),
    })
}

fn split_frontmatter(text: &str) -> Result<(&str, &str), SkillError> {
    let opening = if text.starts_with("---\r\n") {
        5
    } else if text.starts_with("---\n") {
        4
    } else {
        return Err(SkillError::InvalidFormat);
    };
    let mut offset = opening;
    for line in text[opening..].split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if content == "---" {
            if offset - opening > MAX_FRONTMATTER_BYTES {
                return Err(SkillError::TooLarge);
            }
            return Ok((&text[opening..offset], &text[offset + line.len()..]));
        }
        if content == "..." {
            return Err(SkillError::InvalidFormat);
        }
        offset += line.len();
        if offset - opening > MAX_FRONTMATTER_BYTES {
            return Err(SkillError::TooLarge);
        }
    }
    Err(SkillError::InvalidFormat)
}

fn check_yaml_events(yaml: &str) -> Result<(), SkillError> {
    let mut documents = 0;
    let mut events = 0;
    let mut aliases = 0;
    let mut anchors = 0;
    for event in Parser::new_from_str(yaml) {
        let (event, _) = event.map_err(|_| SkillError::MalformedYaml)?;
        events += 1;
        if events > MAX_YAML_EVENTS {
            return Err(SkillError::TooLarge);
        }
        match event {
            Event::DocumentStart(_, _) => {
                documents += 1;
                if documents > 1 {
                    return Err(SkillError::InvalidFormat);
                }
            }
            Event::Alias(_) => {
                aliases += 1;
                if aliases > MAX_YAML_ALIASES {
                    return Err(SkillError::TooLarge);
                }
            }
            Event::Scalar(_, _, anchor, tag)
            | Event::SequenceStart(_, anchor, tag)
            | Event::MappingStart(_, anchor, tag) => {
                if anchor != 0 {
                    anchors += 1;
                    if anchors > MAX_YAML_ALIASES {
                        return Err(SkillError::TooLarge);
                    }
                }
                if let Some(tag) = tag.as_deref() {
                    allow_core_tag(tag)?;
                }
            }
            _ => {}
        }
    }
    if documents != 1 {
        return Err(SkillError::InvalidFormat);
    }
    Ok(())
}

fn allow_core_tag(tag: &Tag) -> Result<(), SkillError> {
    match tag.core_suffix() {
        Some("str" | "int" | "float" | "bool" | "null" | "map" | "seq") => Ok(()),
        _ => Err(SkillError::ForbiddenTag),
    }
}
