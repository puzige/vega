use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use regex::Regex;

use crate::Tools;
use crate::error::ToolError;
use crate::fence::resolve_in_root;
use crate::output::{MAX_RESULTS, ToolOutput, capped_results};
use crate::tools::{relative_display, walker};

const BINARY_PROBE_LEN: u64 = 8192;

impl Tools {
    /// Search files below `path` for lines matching the regular expression.
    ///
    /// `path` is project-root-relative (`None`/empty = whole project), fenced
    /// before traversal, and may name a single file or directory. Output is
    /// `file:line:content`. Ignored files and binary files are skipped. At
    /// most 500 match entries are returned.
    pub fn grep(&self, pattern: &str, path: Option<&str>) -> Result<ToolOutput, ToolError> {
        let regex = Regex::new(pattern)
            .map_err(|error| ToolError::InvalidInput(format!("invalid regex: {error}")))?;
        let input_path = path.unwrap_or("");
        let relative = if Path::new(input_path).is_absolute() {
            Path::new(input_path)
                .strip_prefix(&self.root)
                .map_err(|_| ToolError::PathEscape(input_path.to_owned()))?
                .to_str()
                .ok_or(ToolError::PathEscape(input_path.to_owned()))?
        } else {
            input_path
        };
        let start = resolve_in_root(&self.root, relative)?;
        let mut matches = Vec::new();

        for walked in walker(&start) {
            let entry = walked.map_err(|error| ToolError::Traversal(error.to_string()))?;
            if self.check_credential_path(entry.path()).is_err() {
                continue;
            }
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let display = relative_display(&self.root, entry.path())?;
            append_file_matches(
                entry.path(),
                &display,
                &regex,
                &mut matches,
                self.credential_policy.is_some(),
            )?;
            if matches.len() > MAX_RESULTS {
                break;
            }
        }

        Ok(capped_results(matches))
    }
}

fn append_file_matches(
    path: &Path,
    display: &str,
    regex: &Regex,
    matches: &mut Vec<String>,
    reject_hardlinks: bool,
) -> Result<(), ToolError> {
    let mut file = crate::text_file::open_regular(path, reject_hardlinks)?;
    if binary_probe(&mut file)? {
        return Ok(());
    }

    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    let mut line_number = 0usize;
    loop {
        bytes.clear();
        let read = reader.read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        line_number += 1;
        trim_line_ending(&mut bytes);
        let line = String::from_utf8_lossy(&bytes);
        if regex.is_match(&line) && matches.len() <= MAX_RESULTS {
            matches.push(format!("{display}:{line_number}:{line}"));
        }
    }
    Ok(())
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
