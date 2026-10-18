use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use regex::Regex;
use tokio_util::sync::CancellationToken;

use crate::Tools;
use crate::error::{MutationError, ToolError};
use crate::output::{MAX_RESULTS, ToolOutput, capped_results};
use crate::tools::relative_display;

mod descriptor;
mod external;

use descriptor::{CredentialGuard, Identity};

const BINARY_PROBE_LEN: u64 = 8192;

pub struct PreparedGrep {
    path: PathBuf,
    file: File,
    chain: Vec<Identity>,
    regex: Regex,
    directory: bool,
    external: bool,
    instance_id: u64,
    credential_guard: CredentialGuard,
}

impl PreparedGrep {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_directory(&self) -> bool {
        self.directory
    }
}

impl Tools {
    pub fn prepare_grep(
        &self,
        pattern: &str,
        path: Option<&str>,
    ) -> Result<PreparedGrep, ToolError> {
        let regex = Regex::new(pattern).map_err(|_| invalid("invalid regex"))?;
        let input = path.unwrap_or("");
        let path = if input.is_empty() {
            self.root.clone()
        } else {
            crate::fence::resolve_file_path(&self.root, input).map_err(MutationError::new)?
        };
        if contains_git_component(&path) {
            return Err(invalid("grep cannot access Git control paths"));
        }
        let external = !path.starts_with(&self.root);
        if external && path.to_str().is_none() {
            return Err(invalid("external grep paths must be valid UTF-8"));
        }
        let guard = CredentialGuard::new(self);
        let target = descriptor::capture(&path, &guard).map_err(|error| match error {
            ToolError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ToolError::NotFound("grep target does not exist".into())
            }
            other => other,
        })?;
        let directory = target.file.metadata()?.is_dir();
        if external && directory && guard.overlaps(&path) {
            return Err(invalid("grep directory overlaps protected credentials"));
        }
        Ok(PreparedGrep {
            path,
            file: target.file,
            chain: target.chain,
            regex,
            directory,
            external,
            instance_id: self.instance_id,
            credential_guard: guard,
        })
    }

    pub fn execute_grep(
        &self,
        prepared: PreparedGrep,
        cancel: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        check_cancelled(cancel)?;
        if prepared.instance_id != self.instance_id {
            return Err(invalid("prepared grep belongs to another tool instance"));
        }
        let guard = prepared.credential_guard.merged(CredentialGuard::new(self));
        descriptor::verify(&prepared.path, &prepared.file, &prepared.chain, &guard)?;
        if prepared.external && prepared.directory && guard.overlaps(&prepared.path) {
            return Err(invalid("grep directory overlaps protected credentials"));
        }
        check_cancelled(cancel)?;
        let mut search = GrepSearch {
            root: &self.root,
            regex: &prepared.regex,
            matches: Vec::new(),
            cancel,
            guard: &guard,
            external: prepared.external,
        };
        if !prepared.directory {
            search.append(prepared.file, &prepared.path)?;
        } else if prepared.external {
            external::search_directory(&prepared.file, &prepared.path, &guard, &mut search)?;
        } else {
            search_project(&prepared, &guard, &mut search)?;
        }
        check_cancelled(cancel)?;
        Ok(capped_results(search.matches))
    }

    pub fn grep(&self, pattern: &str, path: Option<&str>) -> Result<ToolOutput, ToolError> {
        self.execute_grep(self.prepare_grep(pattern, path)?, &CancellationToken::new())
    }
}

fn search_project(
    prepared: &PreparedGrep,
    guard: &CredentialGuard,
    search: &mut GrepSearch<'_>,
) -> Result<(), ToolError> {
    let mut builder = ignore::WalkBuilder::new(&prepared.path);
    let filter_guard = guard.clone();
    builder
        .require_git(false)
        .follow_links(false)
        .sort_by_file_name(|left, right| left.cmp(right))
        .filter_entry(move |entry| filter_guard.allows_project_entry(entry.path()));
    for walked in builder.build() {
        check_cancelled(search.cancel)?;
        let entry = walked.map_err(|_| invalid("unable to traverse grep directory"))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file())
            || !guard.allows_project_entry(entry.path())
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&prepared.path)
            .map_err(|_| invalid("grep entry escaped its directory"))?;
        if let Some(file) = descriptor::open_relative(&prepared.file, relative, guard)? {
            search.append(file, entry.path())?;
        }
        if search.is_full() {
            break;
        }
    }
    check_cancelled(search.cancel)
}

struct GrepSearch<'a> {
    root: &'a Path,
    regex: &'a Regex,
    matches: Vec<String>,
    cancel: &'a CancellationToken,
    guard: &'a CredentialGuard,
    external: bool,
}

impl GrepSearch<'_> {
    fn is_full(&self) -> bool {
        self.matches.len() > MAX_RESULTS
    }

    fn append(&mut self, mut file: File, path: &Path) -> Result<(), ToolError> {
        check_cancelled(self.cancel)?;
        let display = if !self.external {
            relative_display(self.root, path)?
        } else {
            path.to_str()
                .ok_or_else(|| invalid("external grep paths must be valid UTF-8"))?
                .to_string()
        };
        check_cancelled(self.cancel)?;
        if self.guard.blocks_file(&file)? {
            return Err(invalid("grep file is protected"));
        }
        if binary_probe(&mut file)? {
            return check_cancelled(self.cancel);
        }
        let mut reader = BufReader::new(file);
        let mut bytes = Vec::new();
        let mut line_number = 0usize;
        while !self.is_full() {
            check_cancelled(self.cancel)?;
            bytes.clear();
            if reader.read_until(b'\n', &mut bytes)? == 0 {
                break;
            }
            check_cancelled(self.cancel)?;
            line_number += 1;
            trim_line_ending(&mut bytes);
            let line = String::from_utf8_lossy(&bytes);
            if self.regex.is_match(&line) {
                self.matches.push(format!("{display}:{line_number}:{line}"));
            }
        }
        check_cancelled(self.cancel)
    }
}

fn contains_git_component(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .as_encoded_bytes()
            .eq_ignore_ascii_case(b".git")
    })
}

fn invalid(message: &str) -> ToolError {
    ToolError::InvalidInput(message.to_string())
}

fn check_cancelled(cancel: &CancellationToken) -> Result<(), ToolError> {
    if cancel.is_cancelled() {
        Err(invalid("grep cancelled"))
    } else {
        Ok(())
    }
}

fn binary_probe(file: &mut File) -> Result<bool, ToolError> {
    let mut head = Vec::new();
    file.by_ref()
        .take(BINARY_PROBE_LEN)
        .read_to_end(&mut head)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(head.contains(&0))
}

fn trim_line_ending(bytes: &mut Vec<u8>) {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
}
