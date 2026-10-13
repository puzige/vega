use sha2::{Digest, Sha256};

pub const NAME: &str = "vega-cua";
pub const VERSION: u32 = 3;
const DOCUMENT: &str = include_str!("vega-cua/SKILL.md");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltinSkillActivation {
    pub name: String,
    pub version: u32,
    pub content_sha256: String,
}

pub(crate) fn activation() -> BuiltinSkillActivation {
    BuiltinSkillActivation {
        name: NAME.into(),
        version: VERSION,
        content_sha256: format!("{:x}", Sha256::digest(DOCUMENT.as_bytes())),
    }
}

pub(crate) fn system_prompt(base: &str) -> String {
    format!("{base}\n\n[Builtin Vega desktop skill: {NAME}; version={VERSION}]\n{DOCUMENT}")
}
