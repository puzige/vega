//! Bounded `@file` context injection (A2-12, S8-T47).
//!
//! The composer may reference project files with `@relative/path` tokens.
//! This module is the sole authority for turning those tokens into injected
//! context, with every bound fail-closed:
//!
//! - **确定性文件序**: the candidate index walks the project root through the
//!   shared gitignore-aware walker ([`crate::tools`]) and is sorted
//!   lexicographically; fuzzy filtering preserves that order.
//! - **ignore 规则 / repo root 边界**: the index only walks ignored-respecting
//!   entries, and every resolved reference goes through the path fence
//!   ([`resolve_in_root`]) — `..` traversal, absolute injection, and
//!   symlink escapes are [`ToolError::PathEscape`].
//! - **symlink**: symlinked entries never enter the index, and a referenced
//!   path whose final component is a symlink is rejected.
//! - **non-UTF8**: identical to the read tool's normative semantics — a NUL
//!   probe rejects binary files ([`ToolError::BinaryFile`]); other decodable
//!   text degrades lossily.
//! - **数量/字节上限**: distinct references per message, per-file bytes, and
//!   total injected bytes are hard caps; any violation fails the whole
//!   resolution so the caller can refuse the run with **zero provider
//!   requests**.
//!
//! The module is headless and reuses the existing dependency set (the
//! `ignore` crate walker); no new dependency is introduced.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::ToolError;
use crate::fence::resolve_in_root;

/// NUL-probe window shared with the read/grep tools (8 KiB head).
const BINARY_PROBE_LEN: u64 = 8192;

/// Hard cap on the selector index: at most this many project-relative file
/// paths are offered to the UI (deterministic lexicographic truncation).
pub const REFERENCE_INDEX_LIMIT: usize = 512;

/// Hard cap on distinct `@path` references resolved per submitted message.
pub const REFERENCE_MAX_FILES: usize = 8;

/// Hard cap on one referenced file's size (bytes) eligible for injection.
pub const REFERENCE_MAX_FILE_BYTES: u64 = 16 * 1024;

/// Hard cap on the total injected reference payload (bytes) per message.
pub const REFERENCE_MAX_TOTAL_BYTES: u64 = 48 * 1024;

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
/// (they cannot be addressed by a text token), sorts lexicographically, and
/// truncates at `limit`.
pub fn bounded_file_index(root: &Path, limit: usize) -> Result<Vec<String>, ToolError> {
    let root = root.canonicalize().map_err(|error| ToolError::Io(error))?;
    let mut entries: Vec<String> = Vec::new();
    for entry in crate::tools::walker(&root) {
        let entry = entry.map_err(|error| ToolError::Traversal(error.to_string()))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            // Directories and (non-followed) symlink entries are skipped.
            continue;
        }
        let relative = match entry.path().strip_prefix(&root) {
            Ok(relative) => relative,
            Err(_) => {
                return Err(ToolError::PathEscape(
                    entry.path().to_string_lossy().into_owned(),
                ));
            }
        };
        let Some(text) = relative.to_str() else {
            continue;
        };
        entries.push(text.to_string());
        if entries.len() >= limit {
            break;
        }
    }
    entries.sort();
    Ok(entries)
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
            {
                if !tokens.iter().any(|token| token == path) {
                    tokens.push(path.to_string());
                }
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
    let root = root.canonicalize().map_err(|error| ToolError::Io(error))?;
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
        let bytes = read_bounded(&canonical)?;
        if bytes.len() as u64 > max_file_bytes {
            return Err(ToolError::TooManyResults {
                limit: max_file_bytes as usize,
            });
        }
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
fn read_bounded(canonical: &PathBuf) -> Result<Vec<u8>, ToolError> {
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
    file.read_to_end(&mut rest)?;
    head.extend_from_slice(&rest);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(path: &Path, text: &str) {
        fs::write(path, text).expect("test fixture write");
    }

    #[test]
    fn index_is_deterministic_honors_ignore_and_skips_symlinks() {
        let dir = tempdir().expect("index fixture");
        write(&dir.path().join("b.txt"), "b");
        write(&dir.path().join("a.txt"), "a");
        fs::create_dir_all(dir.path().join("ignored")).expect("dir");
        write(&dir.path().join("ignored/secret.txt"), "hidden");
        write(&dir.path().join(".gitignore"), "ignored/\n");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("a.txt"), dir.path().join("link.txt"))
            .expect("symlink fixture");
        write(&dir.path().join("nested_z.txt"), "z");

        let entries = bounded_file_index(dir.path(), REFERENCE_INDEX_LIMIT).expect("index");
        // Hidden entries (`.gitignore`) are excluded by the shared walker's
        // hidden-file rule, alongside gitignore'd trees and symlinks.
        assert_eq!(
            entries,
            vec![
                "a.txt".to_string(),
                "b.txt".to_string(),
                "nested_z.txt".to_string()
            ],
            "deterministic sort, gitignore honored, symlinks skipped"
        );

        let fuzzy = fuzzy_filter(
            &entries.iter().map(String::as_str).collect::<Vec<_>>(),
            "at",
            REFERENCE_INDEX_LIMIT,
        );
        assert_eq!(
            fuzzy,
            vec!["a.txt"],
            "subsequence match stays deterministic"
        );
        assert_eq!(
            entries.len(),
            bounded_file_index(dir.path(), 512).expect("index").len()
        );
    }

    #[test]
    fn resolution_injects_bounded_content_and_failures_are_fail_closed() {
        let dir = tempdir().expect("resolve fixture");
        write(&dir.path().join("lib.rs"), "fn main() {}\n");
        write(&dir.path().join("big.bin"), "ok");
        fs::create_dir_all(dir.path().join("sub")).expect("dir");
        write(&dir.path().join("sub/inner.txt"), "inner");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("lib.rs"), dir.path().join("alias.rs"))
            .expect("symlink fixture");
        fs::write(dir.path().join("nul.bin"), [b'a', 0, b'b']).expect("binary fixture");
        for index in 0..REFERENCE_MAX_FILES + 1 {
            write(&dir.path().join(format!("f{index}.txt")), "x");
        }
        let many = (0..REFERENCE_MAX_FILES + 1)
            .map(|index| format!("@f{index}.txt"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(matches!(
            resolve_bounded_references(dir.path(), &many, 8, 1024, 4096),
            Err(ToolError::TooManyResults { .. })
        ));

        // Main path: two refs, first-occurrence order, deduplicated.
        let refs = resolve_bounded_references(
            dir.path(),
            "看 @lib.rs 然后 @sub/inner.txt 再提 @lib.rs",
            REFERENCE_MAX_FILES,
            REFERENCE_MAX_FILE_BYTES,
            REFERENCE_MAX_TOTAL_BYTES,
        )
        .expect("bounded refs");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].path, "lib.rs");
        assert!(refs[0].content.starts_with("fn main()"));
        assert_eq!(refs[1].content, "inner");

        // Fail-closed matrix: escape / symlink / binary / oversize / count.
        assert!(matches!(
            resolve_bounded_references(dir.path(), "@../escape.txt", 8, 1024, 4096),
            Err(ToolError::PathEscape(_))
        ));
        assert!(matches!(
            resolve_bounded_references(dir.path(), "@alias.rs", 8, 1024, 4096),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            resolve_bounded_references(dir.path(), "@nul.bin", 8, 1024, 4096),
            Err(ToolError::BinaryFile(_))
        ));
        assert!(matches!(
            resolve_bounded_references(dir.path(), "@lib.rs", 8, 4, 4096),
            Err(ToolError::TooManyResults { .. })
        ));
        assert!(matches!(
            resolve_bounded_references(dir.path(), "@lib.rs", 8, 4, 4096),
            Err(ToolError::TooManyResults { .. })
        ));
        // No tokens → zero refs (nothing injected).
        assert!(
            resolve_bounded_references(dir.path(), "plain", 8, 1024, 4096)
                .expect("no refs")
                .is_empty()
        );

        let block = render_reference_block(&refs);
        assert!(block.starts_with("[@lib.rs]\nfn main()"));
        assert!(block.contains("[/sub/inner.txt]"));
    }
}
