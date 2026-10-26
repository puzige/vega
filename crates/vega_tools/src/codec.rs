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
        expected_written_bytes: Option<u64>,
        fingerprint_v1: String,
    },
    Edit {
        path: String,
        old_string_bytes: u64,
        new_string_bytes: u64,
        edit_count: Option<u64>,
        fingerprint_v1: String,
    },
}

impl WriteEditAudit {
    pub(crate) fn write(path: &str, content: &str) -> Result<Self, MutationError> {
        validate_wire_path(path).map_err(MutationError::new)?;
        Ok(Self::Write {
            path: path.to_string(),
            content_bytes: checked_len(content.as_bytes())?,
            expected_written_bytes: None,
            fingerprint_v1: fingerprint(&[b"write", path.as_bytes(), content.as_bytes()])?,
        })
    }

    pub(crate) fn write_encoded(
        path: &str,
        content: &str,
        encoded_bytes: usize,
    ) -> Result<Self, MutationError> {
        let mut audit = Self::write(path, content)?;
        if encoded_bytes != content.len() {
            let expected = u64::try_from(encoded_bytes).map_err(|_| codec_error())?;
            if let Self::Write {
                expected_written_bytes,
                fingerprint_v1,
                ..
            } = &mut audit
            {
                *expected_written_bytes = Some(expected);
                *fingerprint_v1 = fingerprint(&[
                    b"write:encoded",
                    path.as_bytes(),
                    content.as_bytes(),
                    &expected.to_be_bytes(),
                ])?;
            }
        }
        Ok(audit)
    }

    pub(crate) fn edit_with_replace_all(
        path: &str,
        old: &str,
        new: &str,
        replace_all: bool,
    ) -> Result<Self, MutationError> {
        validate_wire_path(path).map_err(MutationError::new)?;
        Ok(Self::Edit {
            path: path.to_string(),
            old_string_bytes: checked_len(old.as_bytes())?,
            new_string_bytes: checked_len(new.as_bytes())?,
            edit_count: None,
            fingerprint_v1: fingerprint(&[
                if replace_all {
                    b"edit:replace_all"
                } else {
                    b"edit"
                },
                path.as_bytes(),
                old.as_bytes(),
                new.as_bytes(),
            ])?,
        })
    }

    pub(crate) fn edit_batch(path: &str, edits: &[(&str, &str)]) -> Result<Self, MutationError> {
        if let [(old, new)] = edits {
            return Self::edit_with_replace_all(path, old, new, false);
        }
        validate_wire_path(path).map_err(MutationError::new)?;
        let count = u64::try_from(edits.len()).map_err(|_| codec_error())?;
        if count < 2 {
            return Err(codec_error());
        }
        let mut old_bytes = 0u64;
        let mut new_bytes = 0u64;
        let count_bytes = count.to_be_bytes();
        let mut fields: Vec<&[u8]> = vec![b"edit:batch:v1", path.as_bytes(), &count_bytes];
        for (old, new) in edits {
            old_bytes = old_bytes
                .checked_add(checked_len(old.as_bytes())?)
                .ok_or_else(codec_error)?;
            new_bytes = new_bytes
                .checked_add(checked_len(new.as_bytes())?)
                .ok_or_else(codec_error)?;
            fields.push(old.as_bytes());
            fields.push(new.as_bytes());
        }
        Ok(Self::Edit {
            path: path.to_string(),
            old_string_bytes: old_bytes,
            new_string_bytes: new_bytes,
            edit_count: Some(count),
            fingerprint_v1: fingerprint(&fields)?,
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
                expected_written_bytes,
                fingerprint_v1,
            } => encode_json(&WriteAuditWire {
                audit_version: AUDIT_VERSION,
                tool: "write",
                path,
                content_bytes: *content_bytes,
                expected_written_bytes: *expected_written_bytes,
                fingerprint_v1,
            }),
            Self::Edit {
                path,
                old_string_bytes,
                new_string_bytes,
                edit_count,
                fingerprint_v1,
            } => encode_json(&EditAuditWire {
                audit_version: AUDIT_VERSION,
                tool: "edit",
                path,
                old_string_bytes: *old_string_bytes,
                new_string_bytes: *new_string_bytes,
                edit_count: *edit_count,
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
                    expected_written_bytes: wire.expected_written_bytes,
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
                if wire.edit_count.is_some_and(|count| count < 2) {
                    return Err(codec_error());
                }
                Ok(Self::Edit {
                    path: wire.path,
                    old_string_bytes: wire.old_string_bytes,
                    new_string_bytes: wire.new_string_bytes,
                    edit_count: wire.edit_count,
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
        let Some(code) = MutationErrorCode::parse_code(&wire.validation_error_code) else {
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
        if self.replacements == 0 {
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
        if wire.replacements == 0 {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_written_bytes: Option<u64>,
    fingerprint_v1: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteAuditOwned {
    audit_version: String,
    tool: String,
    path: String,
    content_bytes: u64,
    #[serde(default, deserialize_with = "optional_u64")]
    expected_written_bytes: Option<u64>,
    fingerprint_v1: String,
}

fn optional_u64<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    u64::deserialize(deserializer).map(Some)
}

#[derive(Serialize)]
struct EditAuditWire<'a> {
    audit_version: &'static str,
    tool: &'static str,
    path: &'a str,
    old_string_bytes: u64,
    new_string_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    edit_count: Option<u64>,
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
    #[serde(default, deserialize_with = "optional_u64")]
    edit_count: Option<u64>,
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
