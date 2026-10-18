use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::ToolError;
use crate::fence::resolve_in_root;

/// NUL-probe window shared with the read/grep tools (8 KiB head).
const BINARY_PROBE_LEN: u64 = 8192;

/// Hard cap on the selector index: at most this many project-relative file
/// paths are offered to the UI (deterministic lexicographic truncation).
pub const REFERENCE_INDEX_LIMIT: usize = 512;

/// Per-entry UTF-8 path budget for the ephemeral selector index.
pub const REFERENCE_INDEX_PATH_BYTES: usize = 4096;

/// Retained path bytes for one successful index snapshot.
pub const REFERENCE_INDEX_RETAINED_BYTES: usize = 2 * 1024 * 1024;

/// Maximum number of walker items inspected by one index job.
pub const REFERENCE_INDEX_VISITED_ITEMS: usize = 8192;

/// Cooperative wall budget for one index job. A filesystem syscall already in
/// progress is allowed to finish; callers fence its result if it is stale.
pub const REFERENCE_INDEX_WALL_BUDGET: Duration = Duration::from_secs(2);

/// Hard cap on distinct `@path` references resolved per submitted message.
pub const REFERENCE_MAX_FILES: usize = 8;

/// Hard cap on one referenced file's size (bytes) eligible for injection.
pub const REFERENCE_MAX_FILE_BYTES: u64 = 16 * 1024;

/// Hard cap on the total injected reference payload (bytes) per message.
pub const REFERENCE_MAX_TOTAL_BYTES: u64 = 48 * 1024;

/// Content-free bounded index failures. The app layer maps these codes to its
/// UI projection without exposing paths, errno or file contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIndexErrorCode {
    ProjectUnavailable,
    Cancelled,
    DeadlineExceeded,
    VisitedLimitExceeded,
    RetainedBytesExceeded,
    Traversal,
    InvalidPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIndexError {
    code: FileIndexErrorCode,
}

impl FileIndexError {
    const fn new(code: FileIndexErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> FileIndexErrorCode {
        self.code
    }
}

impl std::fmt::Display for FileIndexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "file index failed ({:?})", self.code)
    }
}

impl std::error::Error for FileIndexError {}

/// One resolved `@file` reference: the project-relative path as referenced
/// and its lossily-decoded text content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReference {
    pub path: String,
    pub content: String,
}

/// Walks the project root into a deterministic, bounded candidate index of
/// project-relative file paths. Ignores `.gitignore`/`.ignore`/hidden rules
/// via the shared walker, never follows symlinks, skips non-UTF-8 names
/// (they cannot be addressed by a text token). Truncation happens in walker
/// order at `limit` entries; the kept set is then sorted lexicographically.
pub fn bounded_file_index(root: &Path, limit: usize) -> Result<Vec<String>, ToolError> {
    bounded_file_index_with_budget(root, limit, || false).map_err(|error| match error.code() {
        FileIndexErrorCode::ProjectUnavailable => {
            ToolError::InvalidInput("project root is unavailable".into())
        }
        FileIndexErrorCode::Cancelled => ToolError::InvalidInput("file index cancelled".into()),
        FileIndexErrorCode::DeadlineExceeded => ToolError::TooManyResults {
            limit: REFERENCE_INDEX_VISITED_ITEMS,
        },
        FileIndexErrorCode::VisitedLimitExceeded => ToolError::TooManyResults {
            limit: REFERENCE_INDEX_VISITED_ITEMS,
        },
        FileIndexErrorCode::RetainedBytesExceeded => ToolError::TooManyResults {
            limit: REFERENCE_INDEX_RETAINED_BYTES,
        },
        FileIndexErrorCode::Traversal => ToolError::Traversal("bounded index failed".into()),
        FileIndexErrorCode::InvalidPath => ToolError::InvalidInput("invalid indexed path".into()),
    })
}

pub fn bounded_file_index_with_budget(
    root: &Path,
    limit: usize,
    is_cancelled: impl Fn() -> bool,
) -> Result<Vec<String>, FileIndexError> {
    let deadline = Instant::now() + REFERENCE_INDEX_WALL_BUDGET;
    bounded_file_index_until(root, limit, is_cancelled, deadline)
}

fn bounded_file_index_until(
    root: &Path,
    limit: usize,
    is_cancelled: impl Fn() -> bool,
    deadline: Instant,
) -> Result<Vec<String>, FileIndexError> {
    bounded_file_search_until(root, "", limit, is_cancelled, deadline)
}

/// Query-aware bounded filename search. Nonmatching entries do not consume
/// retained-result slots; cancellation, visited-item and wall budgets are shared
/// with the existing reference index. The reference index contract is unchanged.
pub fn bounded_file_search(
    root: &Path,
    query: &str,
    limit: usize,
    is_cancelled: impl Fn() -> bool,
) -> Result<Vec<String>, FileIndexError> {
    let query: String = query.chars().take(256).collect::<String>().to_lowercase();
    bounded_file_search_until(
        root,
        &query,
        limit,
        is_cancelled,
        Instant::now() + REFERENCE_INDEX_WALL_BUDGET,
    )
}

fn bounded_file_search_until(
    root: &Path,
    query: &str,
    limit: usize,
    is_cancelled: impl Fn() -> bool,
    deadline: Instant,
) -> Result<Vec<String>, FileIndexError> {
    check_index_budget(&is_cancelled, deadline, 0)?;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let canonical_root = root.canonicalize();
    check_index_budget(&is_cancelled, deadline, 0)?;
    let root =
        canonical_root.map_err(|_| FileIndexError::new(FileIndexErrorCode::ProjectUnavailable))?;
    if !root.is_dir() {
        return Err(FileIndexError::new(FileIndexErrorCode::ProjectUnavailable));
    }
    let limit = limit.min(REFERENCE_INDEX_LIMIT);
    check_index_budget(&is_cancelled, deadline, 0)?;

    let mut walker = crate::tools::walker(&root);
    let mut entries: Vec<String> = Vec::new();
    let mut retained_bytes = 0usize;
    let mut visited_items = 0usize;
    loop {
        // The serial iterator preserves the shared walker's deterministic
        // filename order, so the 512-entry truncation keeps the same set as
        // the existing index. `next` itself may contain an uninterruptible
        // filesystem operation; all checks around it are cooperative.
        check_index_budget(&is_cancelled, deadline, visited_items)?;
        let Some(entry) = walker.next() else {
            break;
        };
        visited_items = visited_items.saturating_add(1);
        check_index_budget(&is_cancelled, deadline, visited_items)?;
        let entry = entry.map_err(|_| FileIndexError::new(FileIndexErrorCode::Traversal))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            // Directories and (non-followed) symlink entries are skipped.
            continue;
        }
        let relative = match entry.path().strip_prefix(&root) {
            Ok(relative) => relative,
            Err(_) => return Err(FileIndexError::new(FileIndexErrorCode::InvalidPath)),
        };
        let Some(text) = relative.to_str() else {
            continue;
        };
        if !query.is_empty() && !text.to_lowercase().contains(query) {
            continue;
        }
        let path_bytes = text.len();
        if path_bytes > REFERENCE_INDEX_PATH_BYTES {
            continue;
        }
        let new_retained = retained_bytes.saturating_add(path_bytes);
        if new_retained > REFERENCE_INDEX_RETAINED_BYTES {
            return Err(FileIndexError::new(
                FileIndexErrorCode::RetainedBytesExceeded,
            ));
        }
        retained_bytes = new_retained;
        entries.push(text.to_string());
        if entries.len() >= limit {
            break;
        }
        check_index_budget(&is_cancelled, deadline, visited_items)?;
    }
    check_index_budget(&is_cancelled, deadline, visited_items)?;
    entries.sort();
    check_index_budget(&is_cancelled, deadline, visited_items)?;
    Ok(entries)
}

fn check_index_budget(
    is_cancelled: &impl Fn() -> bool,
    deadline: Instant,
    visited_items: usize,
) -> Result<(), FileIndexError> {
    if is_cancelled() {
        return Err(FileIndexError::new(FileIndexErrorCode::Cancelled));
    }
    if Instant::now() >= deadline {
        return Err(FileIndexError::new(FileIndexErrorCode::DeadlineExceeded));
    }
    if visited_items > REFERENCE_INDEX_VISITED_ITEMS {
        return Err(FileIndexError::new(
            FileIndexErrorCode::VisitedLimitExceeded,
        ));
    }
    Ok(())
}

/// Case-insensitive subsequence fuzzy match over the bounded index. The
/// candidate order is the index order (deterministic; no scoring sort), and
/// results are capped at `limit`. An empty query matches everything.
pub fn fuzzy_filter<'a>(entries: &[&'a str], query: &str, limit: usize) -> Vec<&'a str> {
    let query = query.to_lowercase();
    let query: Vec<char> = query.chars().collect();
    entries
        .iter()
        .filter(|entry| {
            if query.is_empty() {
                return true;
            }
            let candidate: Vec<char> = entry.to_lowercase().chars().collect();
            is_subsequence(&query, &candidate)
        })
        .take(limit)
        .copied()
        .collect()
}

fn is_subsequence(query: &[char], candidate: &[char]) -> bool {
    let mut cursor = 0usize;
    for expected in query {
        loop {
            match candidate.get(cursor) {
                Some(found) if found == expected => {
                    cursor += 1;
                    break;
                }
                Some(_) => cursor += 1,
                None => return false,
            }
        }
    }
    true
}

/// Extracts the distinct `@path` tokens from a message in first-occurrence
/// order. A token starts at an `@` that is preceded by the text start or
/// whitespace and ends at the next whitespace/newline. Tokens after the cap
/// are a fail-closed error at the [`resolve_bounded_references`] level, not
/// silently dropped.
fn reference_tokens(content: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let bytes = content.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'@' && (index == 0 || bytes[index - 1].is_ascii_whitespace()) {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            if end > start
                && let Ok(path) = std::str::from_utf8(&bytes[start..end])
                && !tokens.iter().any(|token| token == path)
            {
                tokens.push(path.to_string());
            }
            index = end;
        } else {
            index += 1;
        }
    }
    tokens
}

/// Resolves every `@path` token in `content` against the canonical project
/// root under the fail-closed bounds. Any escape, symlink, binary probe hit,
/// oversize file, or cap violation fails the whole call; the caller must
/// then refuse the run before any provider request is built.
pub fn resolve_bounded_references(
    root: &Path,
    content: &str,
    max_files: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
) -> Result<Vec<ResolvedReference>, ToolError> {
    let root = root.canonicalize().map_err(ToolError::Io)?;
    let tokens = reference_tokens(content);
    if tokens.len() > max_files {
        return Err(ToolError::TooManyResults { limit: max_files });
    }
    let mut resolved = Vec::with_capacity(tokens.len());
    let mut total: u64 = 0;
    for token in &tokens {
        let canonical = resolve_in_root(&root, token)?;
        reject_symlink_target(&root, token, &canonical)?;
        if canonical.is_dir() {
            return Err(ToolError::InvalidInput(format!("{token} is a directory")));
        }
        let bytes = read_bounded(&canonical, max_file_bytes)?;
        total = total.saturating_add(bytes.len() as u64);
        if total > max_total_bytes {
            return Err(ToolError::TooManyResults {
                limit: max_total_bytes as usize,
            });
        }
        resolved.push(ResolvedReference {
            path: token.clone(),
            content: String::from_utf8_lossy(&bytes).into_owned(),
        });
    }
    Ok(resolved)
}

/// Rejects a reference whose final component is a symlink (the bounded
/// matrix forbids symlinked sources; the fence already contained escapes).
fn reject_symlink_target(root: &Path, token: &str, canonical: &Path) -> Result<(), ToolError> {
    // The final path component must not be a symlink. `root` is canonical
    // here, so probing the pre-canonical join keeps the final component's
    // own type (the fence has already contained escapes); intermediate
    // in-root symlinks stay governed by the path fence, like the read tool.
    let _ = canonical;
    let metadata = std::fs::symlink_metadata(root.join(token)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ToolError::NotFound(token.to_string())
        } else {
            ToolError::Io(error)
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ToolError::InvalidInput(format!(
            "symlinked reference is not injectable: {token}"
        )));
    }
    Ok(())
}

/// Reads a referenced file with the read tool's normative semantics: a NUL
/// probe over the head rejects binary files; other text degrades lossily.
/// The per-file byte cap is enforced while reading (reads at most
/// `max_file_bytes + 1` past the probe, so an oversized file is rejected
/// before it can be fully buffered).
fn read_bounded(canonical: &PathBuf, max_file_bytes: u64) -> Result<Vec<u8>, ToolError> {
    let mut file = File::open(canonical)?;
    let mut head = Vec::new();
    file.by_ref()
        .take(BINARY_PROBE_LEN)
        .read_to_end(&mut head)?;
    if head.contains(&0) {
        return Err(ToolError::BinaryFile(
            canonical.to_string_lossy().into_owned(),
        ));
    }
    let mut rest = Vec::new();
    file.by_ref()
        .take(max_file_bytes + 1)
        .read_to_end(&mut rest)?;
    head.extend_from_slice(&rest);
    if head.len() as u64 > max_file_bytes {
        return Err(ToolError::TooManyResults {
            limit: max_file_bytes as usize,
        });
    }
    Ok(head)
}

/// Renders the deterministic injection block appended to the user message:
/// one labeled fenced section per reference, in first-occurrence order.
pub fn render_reference_block(refs: &[ResolvedReference]) -> String {
    let mut block = String::new();
    for reference in refs {
        if !block.is_empty() {
            block.push_str("\n\n");
        }
        block.push_str(&format!(
            "[@{}]\n{}\n[/{}]",
            reference.path, reference.content, reference.path
        ));
    }
    block
}

/// Read one bounded text file through the existing project fence. Reject
/// symlink components and changed descriptor identity before returning content.
pub fn preview_text_file(root: &Path, path: &str, max_bytes: u64) -> Result<String, ToolError> {
    use std::os::unix::fs::MetadataExt;
    let root = root.canonicalize()?;
    let canonical = resolve_in_root(&root, path)?;
    let mut walk = root.clone();
    for component in Path::new(path).components() {
        walk.push(component);
        if std::fs::symlink_metadata(&walk)?.file_type().is_symlink() {
            return Err(ToolError::PathEscape(path.into()));
        }
    }
    let before = std::fs::symlink_metadata(&canonical)?;
    if !before.is_file() {
        return Err(ToolError::InvalidInput("not a regular file".into()));
    }
    if before.len() > max_bytes {
        return Err(ToolError::TooManyResults {
            limit: max_bytes as usize,
        });
    }
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&canonical)?;
    let opened = file.metadata()?;
    if !opened.is_file() || before.dev() != opened.dev() || before.ino() != opened.ino() {
        return Err(ToolError::PathEscape(path.into()));
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(ToolError::TooManyResults {
            limit: max_bytes as usize,
        });
    }
    if bytes.contains(&0) {
        return Err(ToolError::BinaryFile(path.into()));
    }
    let after_path = resolve_in_root(&root, path)?;
    let after = std::fs::symlink_metadata(&after_path)?;
    if after_path != canonical
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || after.file_type().is_symlink()
    {
        return Err(ToolError::PathEscape(path.into()));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
