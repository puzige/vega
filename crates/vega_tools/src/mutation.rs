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

impl Tools {
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
        Ok(PreparedWrite {
            instance_id: self.instance_id,
            project_root: self.root.clone(),
            checkpoint_scope: context.scope_key().to_string(),
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
        Ok(PreparedEdit {
            instance_id: self.instance_id,
            project_root: self.root.clone(),
            checkpoint_scope: context.scope_key().to_string(),
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
mod tests {
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    use std::path::{Path, PathBuf};

    use tempfile::{TempDir, tempdir};

    use super::{FAIL_ATOMIC_REPLACE, PrepareMutationError};
    use crate::{
        CheckpointIds, CreatedNewFileMetadata, EditSuccessOutput, InvalidMutation,
        MutationErrorCode, MutationTool, ToolError, Tools, WriteSuccessOutput,
    };

    struct Fixture {
        project: TempDir,
        checkpoints: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                project: tempdir().unwrap(),
                checkpoints: tempdir().unwrap(),
            }
        }

        fn tools(&self, call: &str) -> Tools {
            Tools::new(self.project.path())
                .unwrap()
                .with_mutation_context(self.checkpoints.path(), "project", "thread", call)
                .unwrap()
        }

        fn call_root(&self, call: &str) -> PathBuf {
            let checkpoint_ref = CheckpointIds::new("project", "thread", call)
                .unwrap()
                .checkpoint_ref();
            checkpoint_ref
                .as_str()
                .split('/')
                .skip(1)
                .fold(self.checkpoints.path().to_path_buf(), |path, component| {
                    path.join(component)
                })
        }
    }

    fn invalid(error: PrepareMutationError) -> crate::InvalidMutation {
        match error {
            PrepareMutationError::Invalid(invalid) => invalid,
            PrepareMutationError::Internal(error) => panic!("unexpected internal error: {error}"),
        }
    }

    fn mutation_code(error: ToolError) -> MutationErrorCode {
        match error {
            ToolError::Mutation(error) => error.code(),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn unconfigured_mutation_is_internal_and_never_creates_validation_audit() {
        let fixture = Fixture::new();
        let tools = Tools::new(fixture.project.path()).unwrap();
        let write_raw = r#"{"path":"secret.txt","content":"token=do-not-log"}"#;
        let write_error = tools.prepare_write_json(write_raw).unwrap_err();
        assert!(write_error.invalid().is_none());
        assert!(matches!(
            &write_error,
            PrepareMutationError::Internal(error)
                if error.code() == MutationErrorCode::CheckpointUnavailable
        ));
        assert!(!write_error.to_string().contains("secret.txt"));
        assert!(!format!("{write_error:?}").contains("do-not-log"));

        let edit_raw =
            r#"{"path":"secret.txt","old_string":"old-secret","new_string":"new-secret"}"#;
        let edit_error = tools.prepare_edit_json(edit_raw).unwrap_err();
        assert!(edit_error.invalid().is_none());
        assert!(matches!(
            &edit_error,
            PrepareMutationError::Internal(error)
                if error.code() == MutationErrorCode::CheckpointUnavailable
        ));
        assert!(!edit_error.to_string().contains("old-secret"));
        assert!(!format!("{edit_error:?}").contains("new-secret"));
        assert!(!fixture.project.path().join("secret.txt").exists());
        assert_eq!(fs::read_dir(fixture.checkpoints.path()).unwrap().count(), 0);

        assert!(
            InvalidMutation::from_raw(
                MutationTool::Write,
                write_raw,
                MutationErrorCode::AtomicWriteFailed,
            )
            .is_err()
        );
    }

    #[test]
    fn checkpoint_root_equal_to_or_inside_project_is_rejected_without_mutation() {
        let fixture = Fixture::new();
        let target = fixture.project.path().join("target");
        fs::write(&target, "unchanged").unwrap();

        let equal_error = Tools::new(fixture.project.path())
            .unwrap()
            .with_mutation_context(fixture.project.path(), "project", "thread", "equal")
            .unwrap_err();
        assert_eq!(
            mutation_code(equal_error),
            MutationErrorCode::CheckpointUnavailable
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "unchanged");
        assert_eq!(fs::read_dir(fixture.project.path()).unwrap().count(), 1);

        let descendant = fixture.project.path().join("checkpoints");
        fs::create_dir(&descendant).unwrap();
        let descendant_error = Tools::new(fixture.project.path())
            .unwrap()
            .with_mutation_context(&descendant, "project", "thread", "descendant")
            .unwrap_err();
        assert_eq!(
            mutation_code(descendant_error),
            MutationErrorCode::CheckpointUnavailable
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "unchanged");
        assert_eq!(fs::read_dir(&descendant).unwrap().count(), 0);
    }

    #[test]
    fn write_new_file_records_exact_metadata_before_atomic_creation() {
        let fixture = Fixture::new();
        let tools = fixture.tools("new");
        let output = tools.write("metadata.json", "hello\n").unwrap();
        assert_eq!(
            fs::read(fixture.project.path().join("metadata.json")).unwrap(),
            b"hello\n"
        );
        let success = WriteSuccessOutput::from_json(&output.text).unwrap();
        assert_eq!(success.path, "metadata.json");
        assert_eq!(success.bytes_written, 6);

        let call_root = fixture.call_root("new");
        let metadata_text = fs::read_to_string(call_root.join("metadata.json")).unwrap();
        assert_eq!(
            metadata_text,
            r#"{"metadata_version":"preimage_v1","kind":"created_new_file","path":"metadata.json"}"#
        );
        assert_eq!(
            CreatedNewFileMetadata::from_json(&metadata_text)
                .unwrap()
                .path(),
            "metadata.json"
        );
        assert!(!call_root.join("files").exists());
        assert!(
            !output
                .text
                .contains(fixture.checkpoints.path().to_string_lossy().as_ref())
        );
    }

    #[test]
    fn write_overwrite_preserves_binary_preimage_and_permissions_without_metadata() {
        let fixture = Fixture::new();
        let target = fixture.project.path().join("src/data.bin");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"\xff\x00old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();

        let output = fixture
            .tools("overwrite")
            .write("src/data.bin", "new")
            .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o640);
        let call_root = fixture.call_root("overwrite");
        assert_eq!(
            fs::read(call_root.join("files/src/data.bin")).unwrap(),
            b"\xff\x00old"
        );
        assert!(!call_root.join("metadata.json").exists());
        assert_eq!(
            WriteSuccessOutput::from_json(&output.text)
                .unwrap()
                .bytes_written,
            3
        );
    }

    #[test]
    fn edit_unique_match_handles_non_utf8_and_records_original_bytes() {
        let fixture = Fixture::new();
        let target = fixture.project.path().join("binary.dat");
        fs::write(&target, b"\xffOLD\xfe").unwrap();
        let output = fixture
            .tools("binary-edit")
            .edit("binary.dat", "OLD", "NEW")
            .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"\xffNEW\xfe");
        assert_eq!(
            fs::read(fixture.call_root("binary-edit").join("files/binary.dat")).unwrap(),
            b"\xffOLD\xfe"
        );
        let success = EditSuccessOutput::from_json(&output.text).unwrap();
        assert_eq!(success.replacements, 1);
        assert_eq!(success.bytes_written, 5);
    }

    #[test]
    fn edit_zero_and_overlapping_multiple_matches_create_no_checkpoint_or_change() {
        for (call, content, old, expected) in [
            (
                "zero",
                "prefix secret suffix",
                "absent",
                MutationErrorCode::EditNoMatch,
            ),
            (
                "multiple",
                "aaa secret context",
                "aa",
                MutationErrorCode::EditMultipleMatches,
            ),
        ] {
            let fixture = Fixture::new();
            let target = fixture.project.path().join("file.txt");
            fs::write(&target, content).unwrap();
            let error = fixture
                .tools(call)
                .edit("file.txt", old, "replacement")
                .unwrap_err();
            assert_eq!(mutation_code(error), expected);
            assert_eq!(fs::read_to_string(&target).unwrap(), content);
            assert!(!fixture.call_root(call).exists());
        }
    }

    #[test]
    fn edit_failure_context_is_bounded_and_absent_from_display_and_debug() {
        let fixture = Fixture::new();
        let body = format!("{}secret-old{}", "x".repeat(2_000), "y".repeat(2_000));
        fs::write(fixture.project.path().join("file.txt"), &body).unwrap();
        let error = fixture
            .tools("context")
            .edit("file.txt", "missing-secret", "new-secret")
            .unwrap_err();
        let ToolError::Mutation(error) = error else {
            panic!("expected mutation error");
        };
        let context = error.edit_context().unwrap().expose();
        assert!(context.chars().count() <= 530);
        assert!(!error.to_string().contains(context));
        assert!(!error.to_string().contains("missing-secret"));
        assert!(!format!("{error:?}").contains(context));
        assert!(!format!("{error:?}").contains("new-secret"));
    }

    #[test]
    fn empty_old_string_is_invalid_and_never_checkpoints() {
        let fixture = Fixture::new();
        fs::write(fixture.project.path().join("a"), "body").unwrap();
        let invalid = invalid(
            fixture
                .tools("empty")
                .prepare_edit_json(r#"{"path":"a","old_string":"","new_string":"x"}"#)
                .unwrap_err(),
        );
        assert_eq!(invalid.code(), MutationErrorCode::EditEmptyOldString);
        assert!(!fixture.call_root("empty").exists());
    }

    #[test]
    fn strict_input_validation_codes_are_stable_and_raw_is_absent() {
        let fixture = Fixture::new();
        let tools = fixture.tools("invalids");
        let cases = [
            ("{secret", MutationErrorCode::MalformedJson),
            ("[]", MutationErrorCode::InputNotObject),
            (r#"{"content":"secret"}"#, MutationErrorCode::MissingPath),
            (
                r#"{"path":1,"content":"secret"}"#,
                MutationErrorCode::WrongPathType,
            ),
            (r#"{"path":"a"}"#, MutationErrorCode::MissingContent),
            (
                r#"{"path":"a","content":1}"#,
                MutationErrorCode::WrongContentType,
            ),
            (
                r#"{"path":"a","content":"secret","extra":true}"#,
                MutationErrorCode::UnexpectedField,
            ),
        ];
        for (raw, code) in cases {
            let invalid = invalid(tools.prepare_write_json(raw).unwrap_err());
            assert_eq!(invalid.code(), code);
            let projection = invalid.audit().to_json().unwrap();
            assert!(!projection.contains("secret"));
            assert!(!invalid.tool_result().contains("secret"));
        }
    }

    #[test]
    fn edit_field_validation_codes_cover_every_missing_and_wrong_type() {
        let fixture = Fixture::new();
        fs::write(fixture.project.path().join("a"), "old").unwrap();
        let tools = fixture.tools("edit-invalids");
        let cases = [
            (
                r#"{"old_string":"old","new_string":"new"}"#,
                MutationErrorCode::MissingPath,
            ),
            (
                r#"{"path":1,"old_string":"old","new_string":"new"}"#,
                MutationErrorCode::WrongPathType,
            ),
            (
                r#"{"path":"a","new_string":"new"}"#,
                MutationErrorCode::MissingOldString,
            ),
            (
                r#"{"path":"a","old_string":1,"new_string":"new"}"#,
                MutationErrorCode::WrongOldStringType,
            ),
            (
                r#"{"path":"a","old_string":"old"}"#,
                MutationErrorCode::MissingNewString,
            ),
            (
                r#"{"path":"a","old_string":"old","new_string":1}"#,
                MutationErrorCode::WrongNewStringType,
            ),
            (
                r#"{"path":"a","old_string":"old","new_string":"new","extra":"secret"}"#,
                MutationErrorCode::UnexpectedField,
            ),
        ];
        for (raw, code) in cases {
            let invalid = invalid(tools.prepare_edit_json(raw).unwrap_err());
            assert_eq!(invalid.code(), code);
            assert!(!invalid.audit().to_json().unwrap().contains("secret"));
        }
    }

    #[test]
    fn mutation_fence_rejects_every_escape_and_special_file_shape() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.project.path().join("dir")).unwrap();
        fs::write(fixture.project.path().join("real"), "body").unwrap();
        fs::hard_link(
            fixture.project.path().join("real"),
            fixture.project.path().join("hard"),
        )
        .unwrap();
        symlink("real", fixture.project.path().join("inside-link")).unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("outside"), "outside").unwrap();
        symlink(
            outside.path().join("outside"),
            fixture.project.path().join("outside-link"),
        )
        .unwrap();
        fs::create_dir_all(fixture.project.path().join(".git/hooks")).unwrap();
        fs::write(fixture.project.path().join(".git/hooks/pre-commit"), "hook").unwrap();

        let cases = [
            ("/absolute", MutationErrorCode::PathAbsolute),
            ("../parent", MutationErrorCode::PathParent),
            ("", MutationErrorCode::PathRoot),
            ("missing/child", MutationErrorCode::ParentNotFound),
            ("inside-link", MutationErrorCode::PathSymlink),
            ("outside-link", MutationErrorCode::PathSymlink),
            ("hard", MutationErrorCode::PathHardlink),
            ("dir", MutationErrorCode::PathNotFile),
            (".git/hooks/pre-commit", MutationErrorCode::PathGit),
        ];
        for (index, (path, code)) in cases.into_iter().enumerate() {
            let tools = fixture.tools(&format!("fence-{index}"));
            let raw = serde_json::json!({"path": path, "content": "secret"}).to_string();
            let invalid = invalid(tools.prepare_write_json(&raw).unwrap_err());
            assert_eq!(invalid.code(), code, "{path}");
            let projection: serde_json::Value =
                serde_json::from_str(&invalid.audit().to_json().unwrap()).unwrap();
            assert!(projection.get("path").is_none());
            assert!(projection.get("content").is_none());
        }
        assert_eq!(
            fs::read_to_string(outside.path().join("outside")).unwrap(),
            "outside"
        );
    }

    #[test]
    fn worktree_real_gitdir_and_hooks_are_read_only() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.project.path().join("control/hooks")).unwrap();
        fs::write(
            fixture.project.path().join("control/hooks/pre-commit"),
            "hook",
        )
        .unwrap();
        fs::write(fixture.project.path().join(".git"), "gitdir: control\n").unwrap();
        let tools = Tools::new(fixture.project.path())
            .unwrap()
            .with_mutation_context(fixture.checkpoints.path(), "project", "thread", "gitdir")
            .unwrap();
        let invalid = invalid(
            tools
                .prepare_write_json(r#"{"path":"control/hooks/pre-commit","content":"replace"}"#)
                .unwrap_err(),
        );
        assert_eq!(invalid.code(), MutationErrorCode::PathGit);
        assert_eq!(
            fs::read_to_string(fixture.project.path().join("control/hooks/pre-commit")).unwrap(),
            "hook"
        );
    }

    #[test]
    fn gitdir_added_after_prepare_is_rechecked_before_checkpoint() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.project.path().join("control/hooks")).unwrap();
        let tools = fixture.tools("late-gitdir");
        let prepared = tools
            .prepare_write_json(r#"{"path":"control/hooks/pre-commit","content":"replace"}"#)
            .unwrap();
        fs::write(fixture.project.path().join(".git"), "gitdir: control\n").unwrap();
        let error = tools.execute_write(prepared).unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::PathGit);
        assert!(!fixture.call_root("late-gitdir").exists());
        assert!(
            !fixture
                .project
                .path()
                .join("control/hooks/pre-commit")
                .exists()
        );
    }

    #[test]
    fn checkpoint_root_cannot_be_injected_as_a_symlink_after_configuration() {
        let fixture = Fixture::new();
        let configured_root = fixture.checkpoints.path().join("configured");
        fs::create_dir(&configured_root).unwrap();
        let tools = Tools::new(fixture.project.path())
            .unwrap()
            .with_mutation_context(&configured_root, "project", "thread", "symlink")
            .unwrap();
        let prepared = tools
            .prepare_write_json(r#"{"path":"target","content":"new"}"#)
            .unwrap();
        let moved = fixture.checkpoints.path().join("moved");
        fs::rename(&configured_root, &moved).unwrap();
        let attacker = tempdir().unwrap();
        symlink(attacker.path(), &configured_root).unwrap();
        let error = tools.execute_write(prepared).unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::CheckpointSymlink);
        assert!(!fixture.project.path().join("target").exists());
        assert_eq!(fs::read_dir(attacker.path()).unwrap().count(), 0);
    }

    #[test]
    fn existing_checkpoint_call_data_is_never_overwritten() {
        let fixture = Fixture::new();
        let call_root = fixture.call_root("existing");
        fs::create_dir_all(&call_root).unwrap();
        fs::write(call_root.join("sentinel"), "keep").unwrap();
        let target = fixture.project.path().join("target");
        fs::write(&target, "old").unwrap();
        let error = fixture
            .tools("existing")
            .write("target", "new")
            .unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::CheckpointExists);
        assert_eq!(fs::read_to_string(&target).unwrap(), "old");
        assert_eq!(
            fs::read_to_string(call_root.join("sentinel")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn post_checkpoint_revalidation_prevents_overwriting_a_changed_target() {
        let fixture = Fixture::new();
        let target = fixture.project.path().join("target");
        fs::write(&target, "old").unwrap();
        let tools = fixture.tools("changed");
        let prepared = tools
            .prepare_write_json(r#"{"path":"target","content":"requested"}"#)
            .unwrap();
        let hook_target = target.clone();
        let hook = || fs::write(&hook_target, "concurrent").unwrap();
        let error = tools
            .execute_write_inner(prepared, Some(&hook))
            .unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::TargetChanged);
        assert_eq!(fs::read_to_string(&target).unwrap(), "concurrent");
        assert_eq!(
            fs::read(fixture.call_root("changed").join("files/target")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn post_checkpoint_symlink_swap_cannot_escape_project() {
        let fixture = Fixture::new();
        let target = fixture.project.path().join("target");
        fs::write(&target, "old").unwrap();
        let outside = tempdir().unwrap();
        let outside_target = outside.path().join("outside");
        fs::write(&outside_target, "outside").unwrap();
        let tools = fixture.tools("swap");
        let prepared = tools
            .prepare_write_json(r#"{"path":"target","content":"requested"}"#)
            .unwrap();
        let hook_target = target.clone();
        let hook_outside = outside_target.clone();
        let hook = || {
            fs::remove_file(&hook_target).unwrap();
            symlink(&hook_outside, &hook_target).unwrap();
        };
        let error = tools
            .execute_write_inner(prepared, Some(&hook))
            .unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::TargetChanged);
        assert_eq!(fs::read_to_string(&outside_target).unwrap(), "outside");
    }

    #[test]
    fn atomic_failure_leaves_target_byte_identical_and_no_temp_file() {
        let fixture = Fixture::new();
        let target = fixture.project.path().join("target");
        fs::write(&target, b"old\0bytes").unwrap();
        let tools = fixture.tools("atomic-fail");
        let prepared = tools
            .prepare_write_json(r#"{"path":"target","content":"new"}"#)
            .unwrap();
        FAIL_ATOMIC_REPLACE.set(true);
        let result = tools.execute_write(prepared);
        FAIL_ATOMIC_REPLACE.set(false);
        assert_eq!(
            mutation_code(result.unwrap_err()),
            MutationErrorCode::AtomicWriteFailed
        );
        assert_eq!(fs::read(&target).unwrap(), b"old\0bytes");
        let leaked_temp = fs::read_dir(fixture.project.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vega-write-")
            });
        assert!(!leaked_temp);
    }

    #[test]
    fn new_file_metadata_precedes_target_and_survives_atomic_target_failure() {
        let fixture = Fixture::new();
        let tools = fixture.tools("new-atomic-fail");
        let prepared = tools
            .prepare_write_json(r#"{"path":"new-file","content":"new"}"#)
            .unwrap();
        FAIL_ATOMIC_REPLACE.set(true);
        let result = tools.execute_write(prepared);
        FAIL_ATOMIC_REPLACE.set(false);
        assert_eq!(
            mutation_code(result.unwrap_err()),
            MutationErrorCode::AtomicWriteFailed
        );
        assert!(!fixture.project.path().join("new-file").exists());
        assert_eq!(
            fs::read_to_string(fixture.call_root("new-atomic-fail").join("metadata.json")).unwrap(),
            r#"{"metadata_version":"preimage_v1","kind":"created_new_file","path":"new-file"}"#
        );
    }

    #[test]
    fn prepared_mutation_cannot_cross_tools_or_projects() {
        let first = Fixture::new();
        let second = Fixture::new();
        let first_tools = first.tools("scope");
        let prepared = first_tools
            .prepare_write_json(r#"{"path":"target","content":"secret"}"#)
            .unwrap();
        let error = second.tools("scope").execute_write(prepared).unwrap_err();
        assert_eq!(
            mutation_code(error),
            MutationErrorCode::PreparedScopeMismatch
        );
        assert!(!first.project.path().join("target").exists());
        assert!(!second.project.path().join("target").exists());
    }

    #[test]
    fn configured_checkpoint_root_may_not_be_a_symlink_or_gitdir() {
        let fixture = Fixture::new();
        let real = fixture.checkpoints.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = fixture.checkpoints.path().join("link");
        symlink(&real, &link).unwrap();
        let error = Tools::new(fixture.project.path())
            .unwrap()
            .with_mutation_context(&link, "p", "t", "c")
            .unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::CheckpointSymlink);

        let git_project = tempdir().unwrap();
        fs::create_dir_all(git_project.path().join(".git")).unwrap();
        let error = Tools::new(git_project.path())
            .unwrap()
            .with_mutation_context(git_project.path().join(".git"), "p", "t", "c")
            .unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::PathGit);

        let worktree = tempdir().unwrap();
        let actual_gitdir = tempdir().unwrap();
        fs::write(
            worktree.path().join(".git"),
            format!("gitdir: {}\n", actual_gitdir.path().display()),
        )
        .unwrap();
        let error = Tools::new(worktree.path())
            .unwrap()
            .with_mutation_context(actual_gitdir.path(), "p", "t", "c")
            .unwrap_err();
        assert_eq!(mutation_code(error), MutationErrorCode::PathGit);
    }

    #[test]
    fn normalized_path_and_exact_success_json_are_stable() {
        let fixture = Fixture::new();
        let output = fixture.tools("normalize").write("./nested", "abc").unwrap();
        let success = WriteSuccessOutput::from_json(&output.text).unwrap();
        assert_eq!(success.path, "nested");
        assert_eq!(
            output.text,
            format!(
                r#"{{"path":"nested","bytes_written":3,"checkpoint_ref":"{}"}}"#,
                success.checkpoint_ref.as_str()
            )
        );
    }

    #[test]
    fn checkpoint_user_control_names_remain_under_files_namespace() {
        let fixture = Fixture::new();
        let path = fixture.project.path().join("nested/metadata.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "old").unwrap();
        fixture
            .tools("reserved")
            .write("nested/metadata.json", "new")
            .unwrap();
        let call_root = fixture.call_root("reserved");
        assert_eq!(
            fs::read_to_string(call_root.join("files/nested/metadata.json")).unwrap(),
            "old"
        );
        assert!(!call_root.join("metadata.json").exists());
    }

    #[test]
    fn read_behavior_still_follows_internal_symlink_while_write_rejects_it() {
        let fixture = Fixture::new();
        fs::write(fixture.project.path().join("real"), "inside\n").unwrap();
        symlink("real", fixture.project.path().join("alias")).unwrap();
        let tools = fixture.tools("read-write-symlink");
        assert_eq!(tools.read("alias", None, None).unwrap().text, "1 | inside");
        let raw = r#"{"path":"alias","content":"new"}"#;
        assert_eq!(
            invalid(tools.prepare_write_json(raw).unwrap_err()).code(),
            MutationErrorCode::PathSymlink
        );
        assert_eq!(
            fs::read_to_string(fixture.project.path().join("real")).unwrap(),
            "inside\n"
        );
    }

    #[test]
    fn checkpoint_file_is_single_link_and_private() {
        let fixture = Fixture::new();
        fs::write(fixture.project.path().join("target"), "old").unwrap();
        fixture.tools("private").write("target", "new").unwrap();
        let checkpoint = fixture.call_root("private").join("files/target");
        let metadata = fs::metadata(checkpoint).unwrap();
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(metadata.mode() & 0o777, 0o600);
    }

    #[test]
    fn parent_directory_is_never_created_implicitly() {
        let fixture = Fixture::new();
        let tools = fixture.tools("parent");
        let invalid = invalid(
            tools
                .prepare_write_json(r#"{"path":"missing/file","content":"x"}"#)
                .unwrap_err(),
        );
        assert_eq!(invalid.code(), MutationErrorCode::ParentNotFound);
        assert!(!fixture.project.path().join("missing").exists());
        assert!(!fixture.call_root("parent").exists());
    }

    #[test]
    fn checkpoint_ids_cover_one_and_120_byte_boundaries_in_real_layout() {
        for (call, valid) in [("x".to_string(), true), ("x".repeat(120), true)] {
            let fixture = Fixture::new();
            let tools = Tools::new(fixture.project.path())
                .unwrap()
                .with_mutation_context(fixture.checkpoints.path(), "p", "t", &call);
            assert_eq!(tools.is_ok(), valid);
        }
        let fixture = Fixture::new();
        for invalid_id in ["".to_string(), "x".repeat(121)] {
            let error = Tools::new(fixture.project.path())
                .unwrap()
                .with_mutation_context(fixture.checkpoints.path(), "p", "t", &invalid_id)
                .unwrap_err();
            assert_eq!(mutation_code(error), MutationErrorCode::CheckpointIdInvalid);

            let raw = r#"{"path":"secret-path","content":"secret-body"}"#;
            let invalid = InvalidMutation::from_raw(
                MutationTool::Write,
                raw,
                MutationErrorCode::CheckpointIdInvalid,
            )
            .unwrap();
            assert_eq!(invalid.code(), MutationErrorCode::CheckpointIdInvalid);
            assert!(!invalid.audit().to_json().unwrap().contains("secret-path"));
            assert!(!invalid.audit().to_json().unwrap().contains("secret-body"));
        }
    }

    #[test]
    fn checkpoint_layout_contains_only_expected_paths() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.project.path().join("a/b")).unwrap();
        fs::write(fixture.project.path().join("a/b/file"), "old").unwrap();
        fixture.tools("layout").write("a/b/file", "new").unwrap();
        let call_root = fixture.call_root("layout");
        assert!(call_root.join("files/a/b/file").is_file());
        assert!(!call_root.join("metadata.json").exists());
        let entries = walk_relative(&call_root);
        assert_eq!(entries, ["files", "files/a", "files/a/b", "files/a/b/file"]);
    }

    fn walk_relative(root: &Path) -> Vec<String> {
        fn visit(root: &Path, path: &Path, output: &mut Vec<String>) {
            let mut entries: Vec<_> = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            entries.sort();
            for entry in entries {
                output.push(
                    entry
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
                if entry.is_dir() {
                    visit(root, &entry, output);
                }
            }
        }
        let mut output = Vec::new();
        visit(root, root, &mut output);
        output
    }
}
