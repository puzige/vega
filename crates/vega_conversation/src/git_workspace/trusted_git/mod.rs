//! Canonical two-stage commit assistance.
//!
//! This module is the only T34 boundary allowed to stage or commit. Raw Git
//! paths, refs, object ids, status/tree bytes, patch summaries and messages
//! never leave the service as debuggable values.

use super::*;
use crate::types::{
    CommitChecklist, CommitCompletion, CommitDraft, CommitErrorCode, CommitOutcome,
    CommitPrepareCompletion, CommitSelection, CommitSelectionKind, IndexSnapshotId, PreparedCommit,
    PreparedCommitId,
};
use futures::StreamExt as _;
use std::collections::HashSet;
use vega_runtime::{ChatMessage, ChatRequest, ChatRole, Provider, ProviderEvent, StopReason};

const SUMMARY_LIMIT: usize = 256 * 1024;
const SUMMARY_MARKER: &[u8] = b"\n[vega-summary truncated=true]\n";
const MESSAGE_LIMIT: usize = 32 * 1024;
const DRAFT_TIMEOUT: Duration = Duration::from_secs(60);
const SYSTEM_PROMPT: &str = "Generate one concise Git commit message for the exact staged diff. Return only the commit message text. Do not call tools.";
const USER_PREFIX: &str = "Generate the commit message for the staged diff below.\ntruncated=";

#[derive(Clone, PartialEq, Eq)]
struct HeadAuthority {
    unborn: bool,
    oid: Vec<u8>,
    short: Vec<u8>,
    full_ref: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StageEntry {
    mode: Vec<u8>,
    oid: Vec<u8>,
    path: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TreeEntry {
    mode: Vec<u8>,
    object_type: Vec<u8>,
    oid: Vec<u8>,
    path: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusShape {
    Ordinary,
    Rename,
    Copy,
    Untracked,
}

#[derive(Clone, PartialEq, Eq)]
struct StatusRecord {
    shape: StatusShape,
    x: u8,
    y: u8,
    sub: Vec<u8>,
    head_mode: Vec<u8>,
    index_mode: Vec<u8>,
    worktree_mode: Vec<u8>,
    head_oid: Vec<u8>,
    index_oid: Vec<u8>,
    path: Vec<u8>,
    previous: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Eq)]
struct IndexAuthority {
    head: HeadAuthority,
    status_raw: Vec<u8>,
    stage_raw: Vec<u8>,
    tree_raw: Vec<u8>,
    records: Vec<StatusRecord>,
    stages: Vec<StageEntry>,
    tree: Vec<TreeEntry>,
    workspace_generation: u64,
}

#[derive(Clone)]
struct ChecklistRow {
    public: CommitSelection,
    closure: Vec<Vec<u8>>,
    record: StatusRecord,
    optional_kind: CommitSelectionKind,
    worktree_mode: Option<Vec<u8>>,
}

#[derive(Clone)]
struct StoredChecklist {
    id: IndexSnapshotId,
    authority: IndexAuthority,
    optional: Vec<ChecklistRow>,
}

struct StoredPrepared {
    id: PreparedCommitId,
    authority: IndexAuthority,
    summary: String,
    summary_truncated: bool,
}

#[derive(Default)]
struct CommitState {
    next_generation: u64,
    next_slot: u64,
    checklist: Option<StoredChecklist>,
    prepared: Option<StoredPrepared>,
    mutation_active: bool,
}

/// Route-owned headless commit service. Controller routing and the shared
/// trusted-action token remain app responsibilities; this service enforces
/// repository and single-use Git authority.
pub struct TrustedGitService {
    root: PathBuf,
    root_identity: RootIdentity,
    instance_nonce: u64,
    workspace: Arc<GitWorkspaceService>,
    state: Arc<Mutex<CommitState>>,
    #[cfg(test)]
    mutation_executable: Option<PathBuf>,
    #[cfg(test)]
    mutation_timeout: Duration,
    #[cfg(test)]
    read_executable: Option<PathBuf>,
}

mod authority;
mod parsing;
mod service;

#[cfg(test)]
mod tests;

pub(crate) use authority::*;
pub(crate) use parsing::*;
