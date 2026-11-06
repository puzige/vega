//! Tool surface: one [`Tools`] instance bound to a canonical project root.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::checkpoint::MutationContext;
use crate::codec::CheckpointIds;
use crate::error::{MutationError, ToolError};
use crate::fence::discover_git_dir;

/// Conversation-owned read versions. Clones share state; defaults isolate conversations.
#[derive(Clone, Default)]
pub struct ReadState(Arc<Mutex<ReadObservations>>);

type ReadObservations = BTreeMap<PathBuf, ([u8; 32], u64)>;

fn observation_epochs() -> &'static Mutex<BTreeMap<PathBuf, u64>> {
    static EPOCHS: OnceLock<Mutex<BTreeMap<PathBuf, u64>>> = OnceLock::new();
    EPOCHS.get_or_init(Mutex::default)
}

impl ReadState {
    pub fn invalidate_workspace(workspace: &Path) -> Result<(), ToolError> {
        let workspace = workspace.canonicalize()?;
        let mut epochs = observation_epochs()
            .lock()
            .map_err(|_| MutationError::new(crate::MutationErrorCode::FilesystemError))?;
        let next = epochs
            .values()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| MutationError::new(crate::MutationErrorCode::FilesystemError))?;
        epochs.insert(workspace, next);
        Ok(())
    }

    pub(crate) fn observation_epoch(path: &Path) -> Result<u64, ToolError> {
        Ok(observation_epochs()
            .lock()
            .map_err(|_| MutationError::new(crate::MutationErrorCode::FilesystemError))?
            .iter()
            .filter(|(root, _)| path.starts_with(root))
            .map(|(_, epoch)| *epoch)
            .max()
            .unwrap_or(0))
    }

    pub(crate) fn record(&self, path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
        self.record_observation(path, bytes, Self::observation_epoch(path)?)
    }

    pub(crate) fn record_observation(
        &self,
        path: &Path,
        bytes: &[u8],
        epoch: u64,
    ) -> Result<(), ToolError> {
        self.0
            .lock()
            .map_err(|_| MutationError::new(crate::MutationErrorCode::FilesystemError))?
            .insert(path.to_path_buf(), (read_hash(bytes), epoch));
        Ok(())
    }
    pub(crate) fn validate(&self, path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
        let epoch = Self::observation_epoch(path)?;
        let state = self
            .0
            .lock()
            .map_err(|_| MutationError::new(crate::MutationErrorCode::FilesystemError))?;
        match state.get(path) {
            Some((previous, observed)) if *previous == read_hash(bytes) && *observed == epoch => {
                Ok(())
            }
            Some(_) => Err(MutationError::new(crate::MutationErrorCode::TargetChanged).into()),
            None => Err(MutationError::new(crate::MutationErrorCode::FileNotRead).into()),
        }
    }
}

fn read_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hash = crate::sha256::Sha256::new();
    hash.update(bytes);
    hash.finalize()
}

static TOOL_INSTANCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Read and explicitly configured mutation tools bound to one project root.
///
/// File paths accept absolute targets or resolve relative to the root. Runtime
/// permissions authorize external files; search/shell retain their own fences.
/// Clones share read versions; separate conversations need separate ReadState.
#[derive(Clone)]
pub struct Tools {
    pub(crate) root: PathBuf,
    pub(crate) mutation: Option<MutationContext>,
    pub(crate) instance_id: u64,
    pub(crate) reads: ReadState,
    pub(crate) credential_policy: Option<Arc<crate::credentials::ToolCredentialPolicy>>,
}

impl Tools {
    /// Bind the tools to `root`, which must exist and is canonicalized.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ToolError> {
        let path = root.into();
        let canonical = path.canonicalize().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                ToolError::NotFound(path.to_string_lossy().into_owned())
            }
            _ => ToolError::Io(e),
        })?;
        if !canonical.is_dir() {
            return Err(ToolError::InvalidInput(
                "project root must be a directory".to_string(),
            ));
        }
        Ok(Self {
            root: canonical,
            mutation: None,

            reads: ReadState::default(),
            credential_policy: None,
            instance_id: TOOL_INSTANCE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Explicitly bind this tool instance to one checkpoint call. `Tools::new`
    /// remains read-only; write/edit fail closed until this method succeeds.
    pub fn with_mutation_context(
        mut self,
        checkpoint_root: impl Into<PathBuf>,
        project_id: &str,
        thread_id: &str,
        call_id: &str,
    ) -> Result<Self, ToolError> {
        let git_dir = discover_git_dir(&self.root)
            .map_err(|code| ToolError::from(MutationError::new(code)))?;
        let ids = CheckpointIds::new(project_id, thread_id, call_id)?;
        self.mutation = Some(MutationContext::new(
            checkpoint_root.into(),
            &self.root,
            ids,
            git_dir.as_deref(),
        )?);
        Ok(self)
    }

    /// Share read versions across tool calls in exactly one conversation.
    pub fn with_read_state(mut self, state: ReadState) -> Self {
        self.reads = state;
        self
    }

    /// Clone the current conversation read state.
    pub fn read_state(&self) -> ReadState {
        self.reads.clone()
    }

    pub fn isolated(&self) -> Self {
        let mut tools = self.clone();
        tools.reads = ReadState::default();
        tools.mutation = None;
        tools.instance_id = TOOL_INSTANCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        tools
    }

    /// Resolve an absolute or project-relative file path without creating it.
    pub fn file_path(&self, path: &str) -> Result<PathBuf, ToolError> {
        let resolved =
            crate::fence::resolve_file_path(&self.root, path).map_err(MutationError::new)?;
        self.check_credential_path(&resolved)?;
        Ok(resolved)
    }

    /// The canonical project root all tool paths resolve against.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl fmt::Debug for Tools {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tools")
            .field("root", &self.root)
            .field("mutation", &self.mutation.as_ref().map(|_| "configured"))
            .finish()
    }
}

/// Shared deterministic directory walker. Standard filters remain enabled,
/// so `.gitignore`/`.ignore`/git excludes and hidden-file rules are honored.
/// `require_git(false)` makes project-local `.gitignore` work in tempdirs and
/// non-git projects too (tech-spec §4.4).
pub(crate) fn walker(start: &Path) -> ignore::Walk {
    let mut builder = ignore::WalkBuilder::new(start);
    builder
        .require_git(false)
        .follow_links(false)
        .sort_by_file_name(|left, right| left.cmp(right));
    builder.build()
}

/// Render a walker path relative to the canonical project root.
pub(crate) fn relative_display(root: &Path, path: &Path) -> Result<String, ToolError> {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().into_owned())
        .map_err(|_| ToolError::PathEscape(path.to_string_lossy().into_owned()))
}
