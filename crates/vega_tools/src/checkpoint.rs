//! Single-file preimage checkpoint writer. This is not the Phase 2 rollback
//! system: every call creates one immutable call directory and never reuses or
//! overwrites existing call data.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::codec::{CheckpointIds, CheckpointRef, CreatedNewFileMetadata};
use crate::error::{MutationError, MutationErrorCode};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(crate) struct MutationContext {
    checkpoint_root: PathBuf,
    ids: CheckpointIds,
    checkpoint_ref: CheckpointRef,
}

impl MutationContext {
    pub(crate) fn new(
        checkpoint_root: PathBuf,
        project_root: &Path,
        ids: CheckpointIds,
        git_dir: Option<&Path>,
    ) -> Result<Self, MutationError> {
        let metadata = fs::symlink_metadata(&checkpoint_root)
            .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
        if metadata.file_type().is_symlink() {
            return Err(mutation_error(MutationErrorCode::CheckpointSymlink));
        }
        if !metadata.is_dir() {
            return Err(mutation_error(MutationErrorCode::CheckpointUnavailable));
        }
        let canonical = checkpoint_root
            .canonicalize()
            .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
        if git_dir.is_some_and(|directory| canonical.starts_with(directory)) {
            return Err(mutation_error(MutationErrorCode::PathGit));
        }
        if canonical.starts_with(project_root) {
            return Err(mutation_error(MutationErrorCode::CheckpointUnavailable));
        }
        let checkpoint_ref = ids.checkpoint_ref();
        Ok(Self {
            checkpoint_root: canonical,
            ids,
            checkpoint_ref,
        })
    }

    pub(crate) fn scope_key(&self) -> &str {
        self.checkpoint_ref.as_str()
    }

    pub(crate) fn validate_git_boundary(
        &self,
        git_dir: Option<&Path>,
    ) -> Result<(), MutationError> {
        if git_dir.is_some_and(|directory| self.checkpoint_root.starts_with(directory)) {
            Err(mutation_error(MutationErrorCode::PathGit))
        } else {
            Ok(())
        }
    }

    pub(crate) fn checkpoint(
        &self,
        relative: &Path,
        display: &str,
        preimage: Option<&[u8]>,
    ) -> Result<CheckpointRef, MutationError> {
        self.validate_root()?;
        let mut created_dirs = Vec::new();
        let call_root = match self.create_call_root(&mut created_dirs) {
            Ok(path) => path,
            Err(error) => {
                cleanup_empty_dirs(&created_dirs);
                return Err(error);
            }
        };

        let artifact_result = if let Some(bytes) = preimage {
            self.write_preimage(&call_root, relative, bytes, &mut created_dirs)
        } else {
            self.write_created_metadata(&call_root, display)
        };
        if let Err(error) = artifact_result {
            cleanup_empty_dirs(&created_dirs);
            return Err(error);
        }

        self.validate_root()?;
        validate_safe_directory(&call_root)?;
        Ok(self.checkpoint_ref.clone())
    }

    fn validate_root(&self) -> Result<(), MutationError> {
        validate_safe_directory(&self.checkpoint_root)
    }

    fn create_call_root(&self, created: &mut Vec<PathBuf>) -> Result<PathBuf, MutationError> {
        let project = self.checkpoint_root.join(self.ids.project_component());
        ensure_directory(&project, created)?;
        let thread = project.join(self.ids.thread_component());
        ensure_directory(&thread, created)?;
        let call = thread.join(self.ids.call_component());
        match fs::create_dir(&call) {
            Ok(()) => created.push(call.clone()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(mutation_error(MutationErrorCode::CheckpointExists));
            }
            Err(_) => {
                return Err(mutation_error(MutationErrorCode::CheckpointUnavailable));
            }
        }
        validate_safe_directory(&call)?;
        Ok(call)
    }

    fn write_preimage(
        &self,
        call_root: &Path,
        relative: &Path,
        bytes: &[u8],
        created: &mut Vec<PathBuf>,
    ) -> Result<(), MutationError> {
        if call_root.join("metadata.json").exists() {
            return Err(mutation_error(MutationErrorCode::CheckpointMetadataInvalid));
        }
        let files = call_root.join("files");
        ensure_directory(&files, created)?;
        let Some(parent_relative) = relative.parent() else {
            return Err(mutation_error(MutationErrorCode::CodecInvalid));
        };
        let mut parent = files.clone();
        for component in parent_relative.components() {
            parent.push(component.as_os_str());
            ensure_directory(&parent, created)?;
        }
        let destination = files.join(relative);
        atomic_write_new(&destination, bytes)?;
        validate_safe_file(&destination)
    }

    fn write_created_metadata(&self, call_root: &Path, display: &str) -> Result<(), MutationError> {
        if call_root.join("files").exists() {
            return Err(mutation_error(MutationErrorCode::CheckpointMetadataInvalid));
        }
        let metadata = CreatedNewFileMetadata::new(display)?;
        let json = metadata.to_json()?;
        let destination = call_root.join("metadata.json");
        atomic_write_new(&destination, json.as_bytes())?;
        validate_safe_file(&destination)
    }
}

impl std::fmt::Debug for MutationContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MutationContext([REDACTED])")
    }
}

fn ensure_directory(path: &Path, created: &mut Vec<PathBuf>) -> Result<(), MutationError> {
    match fs::create_dir(path) {
        Ok(()) => created.push(path.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => {
            return Err(mutation_error(MutationErrorCode::CheckpointUnavailable));
        }
    }
    validate_safe_directory(path)
}

fn validate_safe_directory(path: &Path) -> Result<(), MutationError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
    if metadata.file_type().is_symlink() {
        return Err(mutation_error(MutationErrorCode::CheckpointSymlink));
    }
    if !metadata.is_dir() || metadata.nlink() == 0 {
        return Err(mutation_error(MutationErrorCode::CheckpointUnavailable));
    }
    Ok(())
}

fn validate_safe_file(path: &Path) -> Result<(), MutationError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
    if metadata.file_type().is_symlink() {
        return Err(mutation_error(MutationErrorCode::CheckpointSymlink));
    }
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(mutation_error(MutationErrorCode::CheckpointUnavailable));
    }
    Ok(())
}

fn atomic_write_new(destination: &Path, bytes: &[u8]) -> Result<(), MutationError> {
    let Some(parent) = destination.parent() else {
        return Err(mutation_error(MutationErrorCode::CheckpointUnavailable));
    };
    validate_safe_directory(parent)?;

    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".vega-checkpoint-{}-{sequence}.tmp",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
    let operation = (|| -> Result<(), MutationError> {
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
        file.write_all(bytes)
            .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
        file.sync_all()
            .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
        fs::rename(&temp, destination)
            .map_err(|_| mutation_error(MutationErrorCode::CheckpointUnavailable))?;
        Ok(())
    })();
    if operation.is_err() {
        let _ = fs::remove_file(&temp);
    }
    operation
}

fn cleanup_empty_dirs(created: &[PathBuf]) {
    for path in created.iter().rev() {
        let _ = fs::remove_dir(path);
    }
}

fn mutation_error(code: MutationErrorCode) -> MutationError {
    MutationError::new(code)
}
