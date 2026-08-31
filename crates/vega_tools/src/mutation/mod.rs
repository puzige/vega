//! Prepared, checkpointed, same-directory-atomic write and edit tools.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
use std::cell::Cell;

use serde_json::{Map, Value};

use crate::codec::{
    EditSuccessOutput, InvalidWriteEditAudit, MutationTool, WriteEditAudit, WriteSuccessOutput,
};
use crate::error::{MutationError, MutationErrorCode, ToolError};
use crate::fence::{MutationTarget, discover_git_dir, resolve_mutation_target};
use crate::{ToolOutput, Tools};

const MAX_EDIT_CONTEXT_CHARS: usize = 512;
const CONTEXT_SIDE_BYTES: usize = 96;
const TEMP_ATTEMPTS: usize = 16;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    static FAIL_ATOMIC_REPLACE: Cell<bool> = const { Cell::new(false) };
}

/// A content-free invalid-input result ready for terminal rejection/audit.
#[derive(Clone, PartialEq, Eq)]
pub struct InvalidMutation {
    audit: InvalidWriteEditAudit,
    tool_result: String,
}

impl InvalidMutation {
    /// Build the strict terminal projection for a validation failure that
    /// occurs outside path parsing (notably invalid checkpoint identifiers).
    pub fn from_raw(
        tool: MutationTool,
        raw_input: &str,
        code: MutationErrorCode,
    ) -> Result<Self, MutationError> {
        Ok(Self {
            audit: InvalidWriteEditAudit::new(tool, raw_input, code)?,
            tool_result: format!(
                "Tool error: invalid {} input ({})",
                tool.as_str(),
                code.as_str()
            ),
        })
    }

    /// Strict content-free input projection.
    pub fn audit(&self) -> &InvalidWriteEditAudit {
        &self.audit
    }

    /// Stable observable result; never contains the raw input or path.
    pub fn tool_result(&self) -> &str {
        &self.tool_result
    }

    /// Stable validation code.
    pub fn code(&self) -> MutationErrorCode {
        self.audit.validation_error_code()
    }
}

impl fmt::Debug for InvalidMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InvalidMutation")
            .field("code", &self.code())
            .field("raw_input", &"[ABSENT]")
            .finish()
    }
}

impl fmt::Display for InvalidMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.tool_result)
    }
}

impl std::error::Error for InvalidMutation {}

/// Preparation either yields a terminal invalid projection or an internal
/// content-free codec error. The latter is unreachable for ordinary sized
/// in-memory strings but remains fail closed.
#[derive(Debug)]
pub enum PrepareMutationError {
    Invalid(InvalidMutation),
    Internal(MutationError),
}

impl PrepareMutationError {
    /// Return the terminal invalid projection when validation failed.
    pub fn invalid(&self) -> Option<&InvalidMutation> {
        match self {
            Self::Invalid(invalid) => Some(invalid),
            Self::Internal(_) => None,
        }
    }
}

impl fmt::Display for PrepareMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(invalid) => invalid.fmt(formatter),
            Self::Internal(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PrepareMutationError {}

/// Parsed and fenced write input bound to one `Tools` instance and mutation
/// scope. Its body is intentionally absent from `Debug`.
pub struct PreparedWrite {
    instance_id: u64,
    project_root: PathBuf,
    checkpoint_scope: String,
    path: String,
    content: String,
    audit: WriteEditAudit,
}

impl PreparedWrite {
    pub fn audit(&self) -> &WriteEditAudit {
        &self.audit
    }

    pub fn normalized_path(&self) -> &str {
        &self.path
    }
}

impl fmt::Debug for PreparedWrite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedWrite")
            .field("path", &self.path)
            .field("content", &"[REDACTED]")
            .finish()
    }
}

/// Parsed and fenced edit input bound to one `Tools` instance and mutation
/// scope. Old/new bodies are intentionally absent from `Debug`.
pub struct PreparedEdit {
    instance_id: u64,
    project_root: PathBuf,
    checkpoint_scope: String,
    path: String,
    old_string: String,
    new_string: String,
    audit: WriteEditAudit,
}

impl PreparedEdit {
    pub fn audit(&self) -> &WriteEditAudit {
        &self.audit
    }

    pub fn normalized_path(&self) -> &str {
        &self.path
    }
}

impl fmt::Debug for PreparedEdit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedEdit")
            .field("path", &self.path)
            .field("old_string", &"[REDACTED]")
            .field("new_string", &"[REDACTED]")
            .finish()
    }
}

struct AuditedWriteInput {
    path: String,
    content: String,
    audit: WriteEditAudit,
}

struct AuditedEditInput {
    path: String,
    old_string: String,
    new_string: String,
    audit: WriteEditAudit,
}

impl Tools {
    /// Validate and fence raw write input into a content-free audit projection.
    /// This never creates an executable mutation capability or checkpoint.
    pub fn audit_write_json(
        &self,
        raw_input: &str,
    ) -> Result<WriteEditAudit, PrepareMutationError> {
        Ok(self.parse_write_input(raw_input)?.audit)
    }

    /// Validate and fence raw edit input into a content-free audit projection.
    /// This never creates an executable mutation capability or checkpoint.
    pub fn audit_edit_json(&self, raw_input: &str) -> Result<WriteEditAudit, PrepareMutationError> {
        Ok(self.parse_edit_input(raw_input)?.audit)
    }

    /// Parse strict raw provider JSON, normalize/fence its path, and create a
    /// content-free write audit projection. No filesystem mutation occurs.
    pub fn prepare_write_json(
        &self,
        raw_input: &str,
    ) -> Result<PreparedWrite, PrepareMutationError> {
        let context = self.mutation.as_ref().ok_or_else(|| {
            PrepareMutationError::Internal(MutationError::new(
                MutationErrorCode::CheckpointUnavailable,
            ))
        })?;
        let input = self.parse_write_input(raw_input)?;
        Ok(PreparedWrite {
            instance_id: self.instance_id,
            project_root: self.root.clone(),
            checkpoint_scope: context.scope_key().to_string(),
            path: input.path,
            content: input.content,
            audit: input.audit,
        })
    }

    fn parse_write_input(
        &self,
        raw_input: &str,
    ) -> Result<AuditedWriteInput, PrepareMutationError> {
        let values = parse_object(raw_input, MutationTool::Write)?;
        let path = required_string(
            &values,
            "path",
            MutationErrorCode::MissingPath,
            MutationErrorCode::WrongPathType,
            raw_input,
            MutationTool::Write,
        )?;
        let content = required_string(
            &values,
            "content",
            MutationErrorCode::MissingContent,
            MutationErrorCode::WrongContentType,
            raw_input,
            MutationTool::Write,
        )?;
        reject_extra_fields(
            &values,
            &["path", "content"],
            raw_input,
            MutationTool::Write,
        )?;
        let git_dir = discover_git_dir(&self.root)
            .map_err(|code| invalid_error(MutationTool::Write, raw_input, code))?;
        let target = resolve_mutation_target(&self.root, git_dir.as_deref(), &path, false)
            .map_err(|code| invalid_error(MutationTool::Write, raw_input, code))?;
        let audit = WriteEditAudit::write(&target.display, &content)
            .map_err(PrepareMutationError::Internal)?;
        Ok(AuditedWriteInput {
            path: target.display,
            content,
            audit,
        })
    }

    /// Parse strict raw provider JSON, normalize/fence its existing path, and
    /// create a content-free edit audit projection. No filesystem mutation
    /// or checkpoint occurs.
    pub fn prepare_edit_json(&self, raw_input: &str) -> Result<PreparedEdit, PrepareMutationError> {
        let context = self.mutation.as_ref().ok_or_else(|| {
            PrepareMutationError::Internal(MutationError::new(
                MutationErrorCode::CheckpointUnavailable,
            ))
        })?;
        let input = self.parse_edit_input(raw_input)?;
        Ok(PreparedEdit {
            instance_id: self.instance_id,
            project_root: self.root.clone(),
            checkpoint_scope: context.scope_key().to_string(),
            path: input.path,
            old_string: input.old_string,
            new_string: input.new_string,
            audit: input.audit,
        })
    }

    fn parse_edit_input(&self, raw_input: &str) -> Result<AuditedEditInput, PrepareMutationError> {
        let values = parse_object(raw_input, MutationTool::Edit)?;
        let path = required_string(
            &values,
            "path",
            MutationErrorCode::MissingPath,
            MutationErrorCode::WrongPathType,
            raw_input,
            MutationTool::Edit,
        )?;
        let old_string = required_string(
            &values,
            "old_string",
            MutationErrorCode::MissingOldString,
            MutationErrorCode::WrongOldStringType,
            raw_input,
            MutationTool::Edit,
        )?;
        let new_string = required_string(
            &values,
            "new_string",
            MutationErrorCode::MissingNewString,
            MutationErrorCode::WrongNewStringType,
            raw_input,
            MutationTool::Edit,
        )?;
        reject_extra_fields(
            &values,
            &["path", "old_string", "new_string"],
            raw_input,
            MutationTool::Edit,
        )?;
        if old_string.is_empty() {
            return Err(invalid_error(
                MutationTool::Edit,
                raw_input,
                MutationErrorCode::EditEmptyOldString,
            ));
        }
        let git_dir = discover_git_dir(&self.root)
            .map_err(|code| invalid_error(MutationTool::Edit, raw_input, code))?;
        let target = resolve_mutation_target(&self.root, git_dir.as_deref(), &path, true)
            .map_err(|code| invalid_error(MutationTool::Edit, raw_input, code))?;
        let audit = WriteEditAudit::edit(&target.display, &old_string, &new_string)
            .map_err(PrepareMutationError::Internal)?;
        Ok(AuditedEditInput {
            path: target.display,
            old_string,
            new_string,
            audit,
        })
    }

    /// Execute a previously prepared write after checking that it belongs to
    /// this exact tool instance and checkpoint scope.
    pub fn execute_write(&self, prepared: PreparedWrite) -> Result<ToolOutput, ToolError> {
        self.execute_write_inner(prepared, None)
    }

    /// Execute a previously prepared edit after checking that it belongs to
    /// this exact tool instance and checkpoint scope.
    pub fn execute_edit(&self, prepared: PreparedEdit) -> Result<ToolOutput, ToolError> {
        self.execute_edit_inner(prepared, None)
    }

    #[cfg(test)]
    fn write(&self, path: &str, content: &str) -> Result<ToolOutput, ToolError> {
        let raw = serde_json::to_string(&WriteInput { path, content })
            .map_err(|_| MutationError::new(MutationErrorCode::CodecInvalid))?;
        let prepared = self
            .prepare_write_json(&raw)
            .map_err(prepare_to_tool_error)?;
        self.execute_write(prepared)
    }

    #[cfg(test)]
    fn edit(
        &self,
        path: &str,
        old_string: &str,
        new_string: &str,
    ) -> Result<ToolOutput, ToolError> {
        let raw = serde_json::to_string(&EditInput {
            path,
            old_string,
            new_string,
        })
        .map_err(|_| MutationError::new(MutationErrorCode::CodecInvalid))?;
        let prepared = self
            .prepare_edit_json(&raw)
            .map_err(prepare_to_tool_error)?;
        self.execute_edit(prepared)
    }

    fn execute_write_inner(
        &self,
        prepared: PreparedWrite,
        after_checkpoint: Option<&dyn Fn()>,
    ) -> Result<ToolOutput, ToolError> {
        let context = self.validate_prepared_scope(
            prepared.instance_id,
            &prepared.project_root,
            &prepared.checkpoint_scope,
        )?;
        let git_dir = current_git_dir(self)?;
        context.validate_git_boundary(git_dir.as_deref())?;
        let target = resolve_mutation_target(&self.root, git_dir.as_deref(), &prepared.path, false)
            .map_err(mutation_error)?;
        let preimage = target
            .metadata
            .as_ref()
            .map(|_| fs::read(&target.absolute))
            .transpose()
            .map_err(|_| mutation_error(MutationErrorCode::FilesystemError))?;
        let permissions = target
            .metadata
            .as_ref()
            .map(|metadata| metadata.permissions());
        let checkpoint_ref =
            context.checkpoint(&target.relative, &target.display, preimage.as_deref())?;
        if let Some(hook) = after_checkpoint {
            hook();
        }
        let current_git_dir = current_git_dir(self)?;
        context.validate_git_boundary(current_git_dir.as_deref())?;
        revalidate_target(self, &target, preimage.as_deref())?;
        atomic_replace(&target.absolute, prepared.content.as_bytes(), permissions)?;
        let bytes_written = u64::try_from(prepared.content.len())
            .map_err(|_| mutation_error(MutationErrorCode::CodecInvalid))?;
        let success = WriteSuccessOutput {
            path: target.display,
            bytes_written,
            checkpoint_ref,
        };
        Ok(ToolOutput::clean(success.to_json()?))
    }

    fn execute_edit_inner(
        &self,
        prepared: PreparedEdit,
        after_checkpoint: Option<&dyn Fn()>,
    ) -> Result<ToolOutput, ToolError> {
        let context = self.validate_prepared_scope(
            prepared.instance_id,
            &prepared.project_root,
            &prepared.checkpoint_scope,
        )?;
        let git_dir = current_git_dir(self)?;
        context.validate_git_boundary(git_dir.as_deref())?;
        let target = resolve_mutation_target(&self.root, git_dir.as_deref(), &prepared.path, true)
            .map_err(mutation_error)?;
        let original = fs::read(&target.absolute)
            .map_err(|_| mutation_error(MutationErrorCode::FilesystemError))?;
        let matches = match_positions(&original, prepared.old_string.as_bytes());
        if matches.is_empty() {
            return Err(MutationError::with_context(
                MutationErrorCode::EditNoMatch,
                bounded_context(&original, &matches),
            )
            .into());
        }
        if matches.len() != 1 {
            return Err(MutationError::with_context(
                MutationErrorCode::EditMultipleMatches,
                bounded_context(&original, &matches),
            )
            .into());
        }
        let position = matches[0];
        let old_len = prepared.old_string.len();
        let mut replacement = Vec::with_capacity(
            original
                .len()
                .saturating_sub(old_len)
                .saturating_add(prepared.new_string.len()),
        );
        replacement.extend_from_slice(&original[..position]);
        replacement.extend_from_slice(prepared.new_string.as_bytes());
        replacement.extend_from_slice(&original[position + old_len..]);

        let permissions = target
            .metadata
            .as_ref()
            .map(|metadata| metadata.permissions());
        let checkpoint_ref =
            context.checkpoint(&target.relative, &target.display, Some(&original))?;
        if let Some(hook) = after_checkpoint {
            hook();
        }
        let current_git_dir = current_git_dir(self)?;
        context.validate_git_boundary(current_git_dir.as_deref())?;
        revalidate_target(self, &target, Some(&original))?;
        atomic_replace(&target.absolute, &replacement, permissions)?;
        let bytes_written = u64::try_from(replacement.len())
            .map_err(|_| mutation_error(MutationErrorCode::CodecInvalid))?;
        let success = EditSuccessOutput {
            path: target.display,
            bytes_written,
            replacements: 1,
            checkpoint_ref,
        };
        Ok(ToolOutput::clean(success.to_json()?))
    }

    fn validate_prepared_scope(
        &self,
        instance_id: u64,
        root: &Path,
        scope: &str,
    ) -> Result<&crate::checkpoint::MutationContext, ToolError> {
        let Some(context) = self.mutation.as_ref() else {
            return Err(mutation_error(MutationErrorCode::CheckpointUnavailable).into());
        };
        if self.instance_id != instance_id || self.root != root || context.scope_key() != scope {
            return Err(mutation_error(MutationErrorCode::PreparedScopeMismatch).into());
        }
        Ok(context)
    }
}

#[cfg(test)]
#[derive(serde::Serialize)]
struct WriteInput<'a> {
    path: &'a str,
    content: &'a str,
}

#[cfg(test)]
#[derive(serde::Serialize)]
struct EditInput<'a> {
    path: &'a str,
    old_string: &'a str,
    new_string: &'a str,
}

fn parse_object(
    raw_input: &str,
    tool: MutationTool,
) -> Result<Map<String, Value>, PrepareMutationError> {
    match serde_json::from_str::<Value>(raw_input) {
        Ok(Value::Object(values)) => Ok(values),
        Ok(_) => Err(invalid_error(
            tool,
            raw_input,
            MutationErrorCode::InputNotObject,
        )),
        Err(_) => Err(invalid_error(
            tool,
            raw_input,
            MutationErrorCode::MalformedJson,
        )),
    }
}

fn required_string(
    values: &Map<String, Value>,
    field: &str,
    missing: MutationErrorCode,
    wrong_type: MutationErrorCode,
    raw_input: &str,
    tool: MutationTool,
) -> Result<String, PrepareMutationError> {
    match values.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(invalid_error(tool, raw_input, wrong_type)),
        None => Err(invalid_error(tool, raw_input, missing)),
    }
}

fn reject_extra_fields(
    values: &Map<String, Value>,
    expected: &[&str],
    raw_input: &str,
    tool: MutationTool,
) -> Result<(), PrepareMutationError> {
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    if values.keys().any(|key| !expected.contains(key.as_str())) {
        Err(invalid_error(
            tool,
            raw_input,
            MutationErrorCode::UnexpectedField,
        ))
    } else {
        Ok(())
    }
}

fn invalid_error(
    tool: MutationTool,
    raw_input: &str,
    code: MutationErrorCode,
) -> PrepareMutationError {
    match InvalidMutation::from_raw(tool, raw_input, code) {
        Ok(invalid) => PrepareMutationError::Invalid(invalid),
        Err(error) => PrepareMutationError::Internal(error),
    }
}

#[cfg(test)]
fn prepare_to_tool_error(error: PrepareMutationError) -> ToolError {
    match error {
        PrepareMutationError::Invalid(invalid) => MutationError::new(invalid.code()).into(),
        PrepareMutationError::Internal(error) => error.into(),
    }
}

fn mutation_error(code: MutationErrorCode) -> MutationError {
    MutationError::new(code)
}

fn current_git_dir(tools: &Tools) -> Result<Option<PathBuf>, ToolError> {
    discover_git_dir(&tools.root).map_err(|code| mutation_error(code).into())
}

fn revalidate_target(
    tools: &Tools,
    initial: &MutationTarget,
    original: Option<&[u8]>,
) -> Result<(), ToolError> {
    let git_dir = current_git_dir(tools)?;
    let current = resolve_mutation_target(
        &tools.root,
        git_dir.as_deref(),
        &initial.display,
        initial.metadata.is_some(),
    )
    .map_err(|_| mutation_error(MutationErrorCode::TargetChanged))?;
    match (&initial.metadata, &current.metadata, original) {
        (None, None, None) => Ok(()),
        (Some(before), Some(after), Some(bytes))
            if before.dev() == after.dev() && before.ino() == after.ino() && after.nlink() == 1 =>
        {
            let current_bytes = fs::read(&current.absolute)
                .map_err(|_| mutation_error(MutationErrorCode::TargetChanged))?;
            if current_bytes == bytes {
                Ok(())
            } else {
                Err(mutation_error(MutationErrorCode::TargetChanged).into())
            }
        }
        _ => Err(mutation_error(MutationErrorCode::TargetChanged).into()),
    }
}

fn match_positions(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, window)| (window == needle).then_some(index))
        .take(2)
        .collect()
}

fn bounded_context(bytes: &[u8], matches: &[usize]) -> String {
    let sample = if let Some(position) = matches.first().copied() {
        let start = position.saturating_sub(CONTEXT_SIDE_BYTES);
        let end = bytes.len().min(position.saturating_add(CONTEXT_SIDE_BYTES));
        &bytes[start..end]
    } else if bytes.len() <= CONTEXT_SIDE_BYTES.saturating_mul(2) {
        bytes
    } else {
        &bytes[..CONTEXT_SIDE_BYTES.saturating_mul(2)]
    };
    let rendered = String::from_utf8_lossy(sample);
    let mut context = format!("matches={}; sample=", matches.len());
    context.extend(rendered.chars().take(MAX_EDIT_CONTEXT_CHARS));
    context
}

fn atomic_replace(
    destination: &Path,
    bytes: &[u8],
    permissions: Option<Permissions>,
) -> Result<(), ToolError> {
    #[cfg(test)]
    if FAIL_ATOMIC_REPLACE.get() {
        return Err(mutation_error(MutationErrorCode::AtomicWriteFailed).into());
    }
    let Some(parent) = destination.parent() else {
        return Err(mutation_error(MutationErrorCode::AtomicWriteFailed).into());
    };
    for _ in 0..TEMP_ATTEMPTS {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".vega-write-{}-{sequence}.tmp", std::process::id()));
        let file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                continue;
            }
            Err(_) => return Err(mutation_error(MutationErrorCode::AtomicWriteFailed).into()),
        };
        let operation = write_and_rename(file, &temp, destination, bytes, permissions.clone());
        if operation.is_err() {
            let _ = fs::remove_file(&temp);
        }
        return operation;
    }
    Err(mutation_error(MutationErrorCode::AtomicWriteFailed).into())
}

fn write_and_rename(
    mut file: fs::File,
    temp: &Path,
    destination: &Path,
    bytes: &[u8],
    permissions: Option<Permissions>,
) -> Result<(), ToolError> {
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)
            .map_err(|_| mutation_error(MutationErrorCode::AtomicWriteFailed))?;
    }
    file.write_all(bytes)
        .map_err(|_| mutation_error(MutationErrorCode::AtomicWriteFailed))?;
    file.sync_all()
        .map_err(|_| mutation_error(MutationErrorCode::AtomicWriteFailed))?;
    fs::rename(temp, destination)
        .map_err(|_| mutation_error(MutationErrorCode::AtomicWriteFailed))?;
    Ok(())
}

#[cfg(test)]
mod tests;
