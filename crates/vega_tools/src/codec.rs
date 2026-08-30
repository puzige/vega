//! Strict, content-free wire codecs for S5 write/edit boundaries.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{MutationError, MutationErrorCode};
use crate::fence::validate_wire_path;
use crate::sha256::{Sha256, hex};

const FINGERPRINT_DOMAIN: &[u8] = b"vega.write-edit.fingerprint.v1\0";
const INVALID_INPUT_DOMAIN: &[u8] = b"vega.write-edit.invalid-input.v1\0";
const AUDIT_VERSION: &str = "write_edit_v1";
const INVALID_AUDIT_VERSION: &str = "write_edit_invalid_v1";
const METADATA_VERSION: &str = "preimage_v1";
const METADATA_KIND: &str = "created_new_file";
const CHECKPOINT_REF_PREFIX: &str = "preimage-v1";
const MAX_ID_BYTES: usize = 120;

/// Mutating tool names accepted by the strict codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationTool {
    Write,
    Edit,
}

impl MutationTool {
    /// Exact provider/store wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Edit => "edit",
        }
    }

    fn parse(value: &str) -> Result<Self, MutationError> {
        match value {
            "write" => Ok(Self::Write),
            "edit" => Ok(Self::Edit),
            _ => Err(codec_error()),
        }
    }
}

/// Validated project/thread/call checkpoint identifiers.
#[derive(Clone, PartialEq, Eq)]
pub struct CheckpointIds {
    project: EncodedId,
    thread: EncodedId,
    call: EncodedId,
}

impl CheckpointIds {
    /// Validate and collision-free encode the three raw identifiers.
    pub fn new(project: &str, thread: &str, call: &str) -> Result<Self, MutationError> {
        Ok(Self {
            project: EncodedId::new(project)?,
            thread: EncodedId::new(thread)?,
            call: EncodedId::new(call)?,
        })
    }

    pub(crate) fn project_component(&self) -> &str {
        &self.project.0
    }

    pub(crate) fn thread_component(&self) -> &str {
        &self.thread.0
    }

    pub(crate) fn call_component(&self) -> &str {
        &self.call.0
    }

    /// Opaque wire reference for this call checkpoint.
    pub fn checkpoint_ref(&self) -> CheckpointRef {
        CheckpointRef(format!(
            "{CHECKPOINT_REF_PREFIX}/{}/{}/{}",
            self.project.0, self.thread.0, self.call.0
        ))
    }
}

impl fmt::Debug for CheckpointIds {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CheckpointIds([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
struct EncodedId(String);

impl EncodedId {
    fn new(raw: &str) -> Result<Self, MutationError> {
        if raw.is_empty() || raw.len() > MAX_ID_BYTES {
            return Err(MutationError::new(MutationErrorCode::CheckpointIdInvalid));
        }
        let mut encoded = String::with_capacity(3 + raw.len().saturating_mul(2));
        encoded.push_str("id-");
        encoded.push_str(&hex(raw.as_bytes()));
        Ok(Self(encoded))
    }

    fn parse(encoded: &str) -> Result<Self, MutationError> {
        let Some(hex_part) = encoded.strip_prefix("id-") else {
            return Err(codec_error());
        };
        if hex_part.is_empty()
            || hex_part.len() % 2 != 0
            || !hex_part
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(codec_error());
        }
        let mut raw = Vec::with_capacity(hex_part.len() / 2);
        let (pairs, _) = hex_part.as_bytes().as_chunks::<2>();
        for pair in pairs {
            let high = decode_hex(pair[0]).ok_or_else(codec_error)?;
            let low = decode_hex(pair[1]).ok_or_else(codec_error)?;
            raw.push((high << 4) | low);
        }
        if raw.is_empty() || raw.len() > MAX_ID_BYTES || std::str::from_utf8(&raw).is_err() {
            return Err(codec_error());
        }
        Ok(Self(encoded.to_string()))
    }
}

fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Validated opaque checkpoint reference. Its inner value never contains an
/// absolute data-root path or raw identifier.
#[derive(Clone, PartialEq, Eq)]
pub struct CheckpointRef(String);

impl CheckpointRef {
    /// Strictly decode an opaque checkpoint reference.
    pub fn parse(value: &str) -> Result<Self, MutationError> {
        if value.starts_with('/') || value.contains("..") || value.contains('\\') {
            return Err(codec_error());
        }
        let segments: Vec<_> = value.split('/').collect();
        if segments.len() != 4 || segments[0] != CHECKPOINT_REF_PREFIX {
            return Err(codec_error());
        }
        EncodedId::parse(segments[1])?;
        EncodedId::parse(segments[2])?;
        EncodedId::parse(segments[3])?;
        Ok(Self(value.to_string()))
    }

    /// Exact content-free wire value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CheckpointRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CheckpointRef([OPAQUE])")
    }
}

/// Strict valid audit projection for a write or edit.
#[derive(Clone, PartialEq, Eq)]
pub enum WriteEditAudit {
    Write {
        path: String,
        content_bytes: u64,
        fingerprint_v1: String,
    },
    Edit {
        path: String,
        old_string_bytes: u64,
        new_string_bytes: u64,
        fingerprint_v1: String,
    },
}

impl WriteEditAudit {
    pub(crate) fn write(path: &str, content: &str) -> Result<Self, MutationError> {
        validate_wire_path(path).map_err(MutationError::new)?;
        Ok(Self::Write {
            path: path.to_string(),
            content_bytes: checked_len(content.as_bytes())?,
            fingerprint_v1: fingerprint(&[b"write", path.as_bytes(), content.as_bytes()])?,
        })
    }

    pub(crate) fn edit(path: &str, old: &str, new: &str) -> Result<Self, MutationError> {
        validate_wire_path(path).map_err(MutationError::new)?;
        Ok(Self::Edit {
            path: path.to_string(),
            old_string_bytes: checked_len(old.as_bytes())?,
            new_string_bytes: checked_len(new.as_bytes())?,
            fingerprint_v1: fingerprint(&[
                b"edit",
                path.as_bytes(),
                old.as_bytes(),
                new.as_bytes(),
            ])?,
        })
    }

    /// Mutating tool represented by this projection.
    pub fn tool(&self) -> MutationTool {
        match self {
            Self::Write { .. } => MutationTool::Write,
            Self::Edit { .. } => MutationTool::Edit,
        }
    }

    /// Normalized project-relative path.
    pub fn path(&self) -> &str {
        match self {
            Self::Write { path, .. } | Self::Edit { path, .. } => path,
        }
    }

    /// Strict JSON representation.
    pub fn to_json(&self) -> Result<String, MutationError> {
        match self {
            Self::Write {
                path,
                content_bytes,
                fingerprint_v1,
            } => encode_json(&WriteAuditWire {
                audit_version: AUDIT_VERSION,
                tool: "write",
                path,
                content_bytes: *content_bytes,
                fingerprint_v1,
            }),
            Self::Edit {
                path,
                old_string_bytes,
                new_string_bytes,
                fingerprint_v1,
            } => encode_json(&EditAuditWire {
                audit_version: AUDIT_VERSION,
                tool: "edit",
                path,
                old_string_bytes: *old_string_bytes,
                new_string_bytes: *new_string_bytes,
                fingerprint_v1,
            }),
        }
    }

    /// Strictly decode either valid write/edit projection.
    pub fn from_json(json: &str) -> Result<Self, MutationError> {
        let selector: AuditSelector = decode_json(json)?;
        if selector.audit_version != AUDIT_VERSION {
            return Err(codec_error());
        }
        match MutationTool::parse(&selector.tool)? {
            MutationTool::Write => {
                let wire: WriteAuditOwned = decode_json(json)?;
                if wire.audit_version != AUDIT_VERSION || wire.tool != "write" {
                    return Err(codec_error());
                }
                validate_wire_path(&wire.path).map_err(MutationError::new)?;
                validate_hash(&wire.fingerprint_v1)?;
                Ok(Self::Write {
                    path: wire.path,
                    content_bytes: wire.content_bytes,
                    fingerprint_v1: wire.fingerprint_v1,
                })
            }
            MutationTool::Edit => {
                let wire: EditAuditOwned = decode_json(json)?;
                if wire.audit_version != AUDIT_VERSION || wire.tool != "edit" {
                    return Err(codec_error());
                }
                validate_wire_path(&wire.path).map_err(MutationError::new)?;
                validate_hash(&wire.fingerprint_v1)?;
                Ok(Self::Edit {
                    path: wire.path,
                    old_string_bytes: wire.old_string_bytes,
                    new_string_bytes: wire.new_string_bytes,
                    fingerprint_v1: wire.fingerprint_v1,
                })
            }
        }
    }
}

impl fmt::Debug for WriteEditAudit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WriteEditAudit")
            .field("tool", &self.tool())
            .field("path", &self.path())
            .field("content", &"[ABSENT]")
            .finish()
    }
}

/// Strict, content-free projection for malformed or fence-invalid input.
#[derive(Clone, PartialEq, Eq)]
pub struct InvalidWriteEditAudit {
    tool: MutationTool,
    raw_input_bytes: u64,
    raw_input_sha256: String,
    validation_error_code: MutationErrorCode,
}

impl InvalidWriteEditAudit {
    pub(crate) fn new(
        tool: MutationTool,
        raw_input: &str,
        code: MutationErrorCode,
    ) -> Result<Self, MutationError> {
        if !code.is_invalid_input_code() {
            return Err(codec_error());
        }
        Ok(Self {
            tool,
            raw_input_bytes: checked_len(raw_input.as_bytes())?,
            raw_input_sha256: invalid_input_hash(raw_input.as_bytes())?,
            validation_error_code: code,
        })
    }

    /// Stable validation code.
    pub fn validation_error_code(&self) -> MutationErrorCode {
        self.validation_error_code
    }

    /// Tool associated with the invalid raw input.
    pub fn tool(&self) -> MutationTool {
        self.tool
    }

    /// Strict JSON representation without raw input.
    pub fn to_json(&self) -> Result<String, MutationError> {
        encode_json(&InvalidAuditWire {
            audit_version: INVALID_AUDIT_VERSION,
            tool: self.tool.as_str(),
            raw_input_bytes: self.raw_input_bytes,
            raw_input_sha256: &self.raw_input_sha256,
            validation_error_code: self.validation_error_code.as_str(),
        })
    }

    /// Strictly decode an invalid input projection.
    pub fn from_json(json: &str) -> Result<Self, MutationError> {
        let wire: InvalidAuditOwned = decode_json(json)?;
        if wire.audit_version != INVALID_AUDIT_VERSION {
            return Err(codec_error());
        }
        let tool = MutationTool::parse(&wire.tool)?;
        validate_hash(&wire.raw_input_sha256)?;
        let Some(code) = MutationErrorCode::from_str(&wire.validation_error_code) else {
            return Err(codec_error());
        };
        if !code.is_invalid_input_code() {
            return Err(codec_error());
        }
        Ok(Self {
            tool,
            raw_input_bytes: wire.raw_input_bytes,
            raw_input_sha256: wire.raw_input_sha256,
            validation_error_code: code,
        })
    }
}

impl fmt::Debug for InvalidWriteEditAudit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InvalidWriteEditAudit")
            .field("tool", &self.tool)
            .field("raw_input_bytes", &self.raw_input_bytes)
            .field("raw_input", &"[ABSENT]")
            .field("validation_error_code", &self.validation_error_code)
            .finish()
    }
}

/// Strict metadata written only for a target that did not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedNewFileMetadata {
    path: String,
}

impl CreatedNewFileMetadata {
    pub(crate) fn new(path: &str) -> Result<Self, MutationError> {
        validate_wire_path(path).map_err(MutationError::new)?;
        Ok(Self {
            path: path.to_string(),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn to_json(&self) -> Result<String, MutationError> {
        encode_json(&MetadataWire {
            metadata_version: METADATA_VERSION,
            kind: METADATA_KIND,
            path: &self.path,
        })
    }

    pub fn from_json(json: &str) -> Result<Self, MutationError> {
        let wire: MetadataOwned = decode_json(json)?;
        if wire.metadata_version != METADATA_VERSION || wire.kind != METADATA_KIND {
            return Err(codec_error());
        }
        validate_wire_path(&wire.path).map_err(MutationError::new)?;
        Ok(Self { path: wire.path })
    }
}

/// Strict success projection for write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteSuccessOutput {
    pub path: String,
    pub bytes_written: u64,
    pub checkpoint_ref: CheckpointRef,
}

impl WriteSuccessOutput {
    pub fn to_json(&self) -> Result<String, MutationError> {
        validate_wire_path(&self.path).map_err(MutationError::new)?;
        CheckpointRef::parse(self.checkpoint_ref.as_str())?;
        encode_json(&WriteSuccessWire {
            path: &self.path,
            bytes_written: self.bytes_written,
            checkpoint_ref: self.checkpoint_ref.as_str(),
        })
    }

    pub fn from_json(json: &str) -> Result<Self, MutationError> {
        let wire: WriteSuccessOwned = decode_json(json)?;
        validate_wire_path(&wire.path).map_err(MutationError::new)?;
        Ok(Self {
            path: wire.path,
            bytes_written: wire.bytes_written,
            checkpoint_ref: CheckpointRef::parse(&wire.checkpoint_ref)?,
        })
    }
}

/// Strict success projection for edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSuccessOutput {
    pub path: String,
    pub bytes_written: u64,
    pub replacements: u64,
    pub checkpoint_ref: CheckpointRef,
}

impl EditSuccessOutput {
    pub fn to_json(&self) -> Result<String, MutationError> {
        validate_wire_path(&self.path).map_err(MutationError::new)?;
        CheckpointRef::parse(self.checkpoint_ref.as_str())?;
        if self.replacements != 1 {
            return Err(codec_error());
        }
        encode_json(&EditSuccessWire {
            path: &self.path,
            bytes_written: self.bytes_written,
            replacements: self.replacements,
            checkpoint_ref: self.checkpoint_ref.as_str(),
        })
    }

    pub fn from_json(json: &str) -> Result<Self, MutationError> {
        let wire: EditSuccessOwned = decode_json(json)?;
        validate_wire_path(&wire.path).map_err(MutationError::new)?;
        if wire.replacements != 1 {
            return Err(codec_error());
        }
        Ok(Self {
            path: wire.path,
            bytes_written: wire.bytes_written,
            replacements: wire.replacements,
            checkpoint_ref: CheckpointRef::parse(&wire.checkpoint_ref)?,
        })
    }
}

fn fingerprint(fields: &[&[u8]]) -> Result<String, MutationError> {
    let mut hash = Sha256::new();
    hash.update(FINGERPRINT_DOMAIN);
    for field in fields {
        let len = checked_len(field)?;
        hash.update(&len.to_be_bytes());
        hash.update(field);
    }
    Ok(hex(&hash.finalize()))
}

fn invalid_input_hash(raw: &[u8]) -> Result<String, MutationError> {
    let mut hash = Sha256::new();
    hash.update(INVALID_INPUT_DOMAIN);
    let len = checked_len(raw)?;
    hash.update(&len.to_be_bytes());
    hash.update(raw);
    Ok(hex(&hash.finalize()))
}

fn checked_len(value: &[u8]) -> Result<u64, MutationError> {
    u64::try_from(value.len()).map_err(|_| codec_error())
}

fn validate_hash(value: &str) -> Result<(), MutationError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(codec_error())
    }
}

fn encode_json<T: Serialize>(value: &T) -> Result<String, MutationError> {
    serde_json::to_string(value).map_err(|_| codec_error())
}

fn decode_json<'a, T: Deserialize<'a>>(value: &'a str) -> Result<T, MutationError> {
    serde_json::from_str(value).map_err(|_| codec_error())
}

fn codec_error() -> MutationError {
    MutationError::new(MutationErrorCode::CodecInvalid)
}

#[derive(Deserialize)]
struct AuditSelector {
    audit_version: String,
    tool: String,
}

#[derive(Serialize)]
struct WriteAuditWire<'a> {
    audit_version: &'static str,
    tool: &'static str,
    path: &'a str,
    content_bytes: u64,
    fingerprint_v1: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteAuditOwned {
    audit_version: String,
    tool: String,
    path: String,
    content_bytes: u64,
    fingerprint_v1: String,
}

#[derive(Serialize)]
struct EditAuditWire<'a> {
    audit_version: &'static str,
    tool: &'static str,
    path: &'a str,
    old_string_bytes: u64,
    new_string_bytes: u64,
    fingerprint_v1: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditAuditOwned {
    audit_version: String,
    tool: String,
    path: String,
    old_string_bytes: u64,
    new_string_bytes: u64,
    fingerprint_v1: String,
}

#[derive(Serialize)]
struct InvalidAuditWire<'a> {
    audit_version: &'static str,
    tool: &'a str,
    raw_input_bytes: u64,
    raw_input_sha256: &'a str,
    validation_error_code: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvalidAuditOwned {
    audit_version: String,
    tool: String,
    raw_input_bytes: u64,
    raw_input_sha256: String,
    validation_error_code: String,
}

#[derive(Serialize)]
struct MetadataWire<'a> {
    metadata_version: &'static str,
    kind: &'static str,
    path: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataOwned {
    metadata_version: String,
    kind: String,
    path: String,
}

#[derive(Serialize)]
struct WriteSuccessWire<'a> {
    path: &'a str,
    bytes_written: u64,
    checkpoint_ref: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteSuccessOwned {
    path: String,
    bytes_written: u64,
    checkpoint_ref: String,
}

#[derive(Serialize)]
struct EditSuccessWire<'a> {
    path: &'a str,
    bytes_written: u64,
    replacements: u64,
    checkpoint_ref: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditSuccessOwned {
    path: String,
    bytes_written: u64,
    replacements: u64,
    checkpoint_ref: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> CheckpointIds {
        CheckpointIds::new("project", "thread", "call").unwrap()
    }

    #[test]
    fn checkpoint_id_boundaries_and_unicode_are_collision_free() {
        assert!(CheckpointIds::new("", "t", "c").is_err());
        let max = "a".repeat(120);
        let refs = CheckpointIds::new(&max, "t", "c").unwrap();
        assert_eq!(refs.project_component().len(), 243);
        assert!(CheckpointIds::new(&"a".repeat(121), "t", "c").is_err());
        assert!(CheckpointIds::new(&"界".repeat(40), "t", "c").is_ok());
        assert!(CheckpointIds::new(&"界".repeat(41), "t", "c").is_err());
        assert_ne!(
            CheckpointIds::new("é", "t", "c")
                .unwrap()
                .project_component(),
            CheckpointIds::new("e", "t", "c")
                .unwrap()
                .project_component()
        );
    }

    #[test]
    fn checkpoint_ref_is_strict_and_opaque() {
        let checkpoint_ref = ids().checkpoint_ref();
        assert_eq!(
            CheckpointRef::parse(checkpoint_ref.as_str()).unwrap(),
            checkpoint_ref
        );
        for invalid in [
            "/preimage-v1/id-61/id-62/id-63",
            "preimage-v1/raw/id-62/id-63",
            "preimage-v1/id-6A/id-62/id-63",
            "preimage-v1/id-6/id-62/id-63",
            "preimage-v1/id-ff/id-62/id-63",
            "preimage-v1/id-61/id-62",
            "preimage-v1/id-61/id-62/id-63/extra",
            "wrong/id-61/id-62/id-63",
            "../preimage-v1/id-61/id-62/id-63",
        ] {
            assert!(CheckpointRef::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn valid_audits_roundtrip_without_bodies() {
        let write = WriteEditAudit::write("src/lib.rs", "super-secret").unwrap();
        let write_json = write.to_json().unwrap();
        assert!(!write_json.contains("super-secret"));
        assert_eq!(WriteEditAudit::from_json(&write_json).unwrap(), write);

        let edit = WriteEditAudit::edit("src/lib.rs", "old-secret", "new-secret").unwrap();
        let edit_json = edit.to_json().unwrap();
        assert!(!edit_json.contains("old-secret"));
        assert!(!edit_json.contains("new-secret"));
        assert_eq!(WriteEditAudit::from_json(&edit_json).unwrap(), edit);
    }

    #[test]
    fn fingerprint_is_domain_order_and_length_sensitive() {
        let a = WriteEditAudit::write("a/b", "c").unwrap();
        let b = WriteEditAudit::write("a", "bc").unwrap();
        let c = WriteEditAudit::edit("a/b", "", "c").unwrap();
        assert_ne!(a, b);
        assert_ne!(a.to_json().unwrap(), c.to_json().unwrap());

        let fixed_write = WriteEditAudit::write("src/lib.rs", "hello").unwrap();
        let fixed_edit = WriteEditAudit::edit("src/lib.rs", "old", "new").unwrap();
        let WriteEditAudit::Write { fingerprint_v1, .. } = fixed_write else {
            panic!("expected write audit");
        };
        let WriteEditAudit::Edit {
            fingerprint_v1: edit_fingerprint,
            ..
        } = fixed_edit
        else {
            panic!("expected edit audit");
        };
        assert_eq!(
            fingerprint_v1,
            "b6d0184e17548852c31283f2d837dd793c05bbb9c472eb247341d3f4fdc48a6a"
        );
        assert_eq!(
            WriteEditAudit::write("src/lib.rs", "hello")
                .unwrap()
                .to_json()
                .unwrap(),
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"src/lib.rs","content_bytes":5,"fingerprint_v1":"b6d0184e17548852c31283f2d837dd793c05bbb9c472eb247341d3f4fdc48a6a"}"#
        );
        assert_eq!(
            edit_fingerprint,
            "92011380b59838d6256152f088ef7e9e3ec03110f12be78200fe8271ea9ba3ac"
        );
    }

    #[test]
    fn invalid_audit_is_deterministic_and_content_free() {
        let raw = r#"{"path":"/secret","content":"token=very-secret"}"#;
        let audit =
            InvalidWriteEditAudit::new(MutationTool::Write, raw, MutationErrorCode::PathAbsolute)
                .unwrap();
        let json = audit.to_json().unwrap();
        assert!(!json.contains("/secret"));
        assert!(!json.contains("very-secret"));
        assert_eq!(InvalidWriteEditAudit::from_json(&json).unwrap(), audit);
        assert_eq!(audit.raw_input_bytes, 48);
        assert_eq!(
            audit.raw_input_sha256,
            "ea0c502a822d2607c11d7d0d605f903ddb663b8bbd5ef8925435a07634f28add"
        );
        assert_eq!(
            json,
            r#"{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":48,"raw_input_sha256":"ea0c502a822d2607c11d7d0d605f903ddb663b8bbd5ef8925435a07634f28add","validation_error_code":"path_absolute"}"#
        );
        assert_eq!(
            InvalidWriteEditAudit::new(MutationTool::Write, raw, MutationErrorCode::PathAbsolute)
                .unwrap(),
            audit
        );
    }

    #[test]
    fn metadata_and_success_outputs_are_exact_and_strict() {
        let metadata = CreatedNewFileMetadata::new("metadata.json").unwrap();
        assert_eq!(
            metadata.to_json().unwrap(),
            r#"{"metadata_version":"preimage_v1","kind":"created_new_file","path":"metadata.json"}"#
        );
        assert_eq!(
            CreatedNewFileMetadata::from_json(&metadata.to_json().unwrap()).unwrap(),
            metadata
        );

        let write = WriteSuccessOutput {
            path: "src/lib.rs".to_string(),
            bytes_written: 9,
            checkpoint_ref: ids().checkpoint_ref(),
        };
        assert_eq!(
            WriteSuccessOutput::from_json(&write.to_json().unwrap()).unwrap(),
            write
        );
        let edit = EditSuccessOutput {
            path: "src/lib.rs".to_string(),
            bytes_written: 10,
            replacements: 1,
            checkpoint_ref: ids().checkpoint_ref(),
        };
        assert_eq!(
            EditSuccessOutput::from_json(&edit.to_json().unwrap()).unwrap(),
            edit
        );
        assert_eq!(
            edit.to_json().unwrap(),
            format!(
                r#"{{"path":"src/lib.rs","bytes_written":10,"replacements":1,"checkpoint_ref":"{}"}}"#,
                ids().checkpoint_ref().as_str()
            )
        );
    }

    #[test]
    fn strict_codecs_reject_shape_number_constant_path_hash_and_ref_errors() {
        let valid_ref = ids().checkpoint_ref();
        let ref_value = valid_ref.as_str();
        let bad_audits = [
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":-1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":1.0,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":1e0,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":-0,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":18446744073709551616,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"wrong","tool":"write","path":"a","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"../a","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":1,"fingerprint_v1":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","extra":0}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","path":"b","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        ];
        for value in bad_audits {
            assert!(WriteEditAudit::from_json(value).is_err(), "{value}");
        }

        for value in [
            r#"{"metadata_version":"preimage_v1","kind":"created_new_file","path":"a","extra":1}"#,
            r#"{"metadata_version":"wrong","kind":"created_new_file","path":"a"}"#,
            r#"{"metadata_version":"preimage_v1","kind":"wrong","path":"a"}"#,
            r#"{"metadata_version":"preimage_v1","kind":"created_new_file","path":"./a"}"#,
        ] {
            assert!(CreatedNewFileMetadata::from_json(value).is_err(), "{value}");
        }

        let success_negatives = [
            format!(r#"{{"path":"a","bytes_written":-1,"checkpoint_ref":"{ref_value}"}}"#),
            format!(r#"{{"path":"a","bytes_written":1.0,"checkpoint_ref":"{ref_value}"}}"#),
            r#"{"path":"a","bytes_written":1,"checkpoint_ref":"raw"}"#.to_string(),
            format!(r#"{{"path":"a","bytes_written":1,"checkpoint_ref":"{ref_value}","extra":0}}"#),
            format!(
                r#"{{"path":"a","path":"b","bytes_written":1,"checkpoint_ref":"{ref_value}"}}"#
            ),
        ];
        for value in success_negatives {
            assert!(WriteSuccessOutput::from_json(&value).is_err(), "{value}");
        }
        for replacements in [0, 2] {
            let value = format!(
                r#"{{"path":"a","bytes_written":1,"replacements":{replacements},"checkpoint_ref":"{ref_value}"}}"#
            );
            assert!(EditSuccessOutput::from_json(&value).is_err());
        }
    }

    #[test]
    fn every_strict_schema_rejects_missing_extra_and_wrong_typed_fields() {
        let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let checkpoint_ref = ids().checkpoint_ref();
        let checkpoint_ref = checkpoint_ref.as_str();

        let write_audit_invalid = [
            format!(r#"{{"tool":"write","path":"a","content_bytes":1,"fingerprint_v1":"{hash}"}}"#),
            format!(r#"{{"audit_version":"write_edit_v1","path":"a","content_bytes":1,"fingerprint_v1":"{hash}"}}"#),
            format!(r#"{{"audit_version":"write_edit_v1","tool":"write","content_bytes":1,"fingerprint_v1":"{hash}"}}"#),
            format!(r#"{{"audit_version":"write_edit_v1","tool":"write","path":"a","fingerprint_v1":"{hash}"}}"#),
            r#"{"audit_version":1,"tool":"write","path":"a","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_string(),
            r#"{"audit_version":"write_edit_v1","tool":1,"path":"a","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_string(),
            r#"{"audit_version":"write_edit_v1","tool":"write","path":1,"content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_string(),
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":"1","fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_string(),
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a","content_bytes":1,"fingerprint_v1":1}"#.to_string(),
        ];
        for value in write_audit_invalid {
            assert!(WriteEditAudit::from_json(&value).is_err(), "{value}");
        }

        let edit_audit_invalid = [
            format!(r#"{{"tool":"edit","path":"a","old_string_bytes":1,"new_string_bytes":1,"fingerprint_v1":"{hash}"}}"#),
            format!(r#"{{"audit_version":"write_edit_v1","path":"a","old_string_bytes":1,"new_string_bytes":1,"fingerprint_v1":"{hash}"}}"#),
            format!(r#"{{"audit_version":"write_edit_v1","tool":"edit","old_string_bytes":1,"new_string_bytes":1,"fingerprint_v1":"{hash}"}}"#),
            format!(r#"{{"audit_version":"write_edit_v1","tool":"edit","path":"a","new_string_bytes":1,"fingerprint_v1":"{hash}"}}"#),
            format!(r#"{{"audit_version":"write_edit_v1","tool":"edit","path":"a","old_string_bytes":1,"fingerprint_v1":"{hash}"}}"#),
            r#"{"audit_version":"write_edit_v1","tool":"edit","path":"a","old_string_bytes":"1","new_string_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_string(),
            r#"{"audit_version":"write_edit_v1","tool":"edit","path":"a","old_string_bytes":1,"new_string_bytes":null,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.to_string(),
            format!(r#"{{"audit_version":"write_edit_v1","tool":"edit","path":"a","old_string_bytes":1,"new_string_bytes":1,"fingerprint_v1":"{hash}","extra":0}}"#),
        ];
        for value in edit_audit_invalid {
            assert!(WriteEditAudit::from_json(&value).is_err(), "{value}");
        }

        let invalid_audit_invalid = [
            format!(r#"{{"tool":"write","raw_input_bytes":1,"raw_input_sha256":"{hash}","validation_error_code":"malformed_json"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","raw_input_bytes":1,"raw_input_sha256":"{hash}","validation_error_code":"malformed_json"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_sha256":"{hash}","validation_error_code":"malformed_json"}}"#),
            r#"{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":1,"validation_error_code":"malformed_json"}"#.to_string(),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":1,"raw_input_sha256":"{hash}"}}"#),
            format!(r#"{{"audit_version":"wrong","tool":"write","raw_input_bytes":1,"raw_input_sha256":"{hash}","validation_error_code":"malformed_json"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"bash","raw_input_bytes":1,"raw_input_sha256":"{hash}","validation_error_code":"malformed_json"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":-1,"raw_input_sha256":"{hash}","validation_error_code":"malformed_json"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":1.0,"raw_input_sha256":"{hash}","validation_error_code":"malformed_json"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":18446744073709551616,"raw_input_sha256":"{hash}","validation_error_code":"malformed_json"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":1,"raw_input_sha256":"{hash}","validation_error_code":"unknown"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":1,"raw_input_sha256":"{hash}","validation_error_code":"atomic_write_failed"}}"#),
            format!(r#"{{"audit_version":"write_edit_invalid_v1","tool":"write","raw_input_bytes":1,"raw_input_sha256":"{hash}","validation_error_code":"malformed_json","extra":0}}"#),
        ];
        for value in invalid_audit_invalid {
            assert!(InvalidWriteEditAudit::from_json(&value).is_err(), "{value}");
        }

        let metadata_invalid = [
            r#"{"kind":"created_new_file","path":"a"}"#,
            r#"{"metadata_version":"preimage_v1","path":"a"}"#,
            r#"{"metadata_version":"preimage_v1","kind":"created_new_file"}"#,
            r#"{"metadata_version":1,"kind":"created_new_file","path":"a"}"#,
            r#"{"metadata_version":"preimage_v1","kind":1,"path":"a"}"#,
            r#"{"metadata_version":"preimage_v1","kind":"created_new_file","path":1}"#,
        ];
        for value in metadata_invalid {
            assert!(CreatedNewFileMetadata::from_json(value).is_err(), "{value}");
        }

        let write_success_invalid = [
            format!(r#"{{"bytes_written":1,"checkpoint_ref":"{checkpoint_ref}"}}"#),
            r#"{"path":"a","checkpoint_ref":"raw"}"#.to_string(),
            r#"{"path":"a","bytes_written":1}"#.to_string(),
            format!(r#"{{"path":1,"bytes_written":1,"checkpoint_ref":"{checkpoint_ref}"}}"#),
            format!(r#"{{"path":"a","bytes_written":"1","checkpoint_ref":"{checkpoint_ref}"}}"#),
            r#"{"path":"a","bytes_written":1,"checkpoint_ref":1}"#.to_string(),
        ];
        for value in write_success_invalid {
            assert!(WriteSuccessOutput::from_json(&value).is_err(), "{value}");
        }

        let edit_success_invalid = [
            format!(
                r#"{{"bytes_written":1,"replacements":1,"checkpoint_ref":"{checkpoint_ref}"}}"#
            ),
            format!(r#"{{"path":"a","replacements":1,"checkpoint_ref":"{checkpoint_ref}"}}"#),
            format!(r#"{{"path":"a","bytes_written":1,"checkpoint_ref":"{checkpoint_ref}"}}"#),
            r#"{"path":"a","bytes_written":1,"replacements":1}"#.to_string(),
            format!(
                r#"{{"path":"a","bytes_written":1,"replacements":"1","checkpoint_ref":"{checkpoint_ref}"}}"#
            ),
            format!(
                r#"{{"path":"a","bytes_written":1,"replacements":1.0,"checkpoint_ref":"{checkpoint_ref}"}}"#
            ),
            format!(
                r#"{{"path":"a","bytes_written":1,"replacements":1,"checkpoint_ref":"{checkpoint_ref}","extra":0}}"#
            ),
        ];
        for value in edit_success_invalid {
            assert!(EditSuccessOutput::from_json(&value).is_err(), "{value}");
        }
    }
}
