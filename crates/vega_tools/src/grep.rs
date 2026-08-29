//! `grep` tool: regex matches over project files with `.gitignore`
//! filtering, binary skipping, and a 500-entry cap (tech-spec §4.4,
//! S4-T21).

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
        let start = resolve_in_root(&self.root, input_path)?;
        let mut matches = Vec::new();

        for walked in walker(&start) {
            let entry = walked.map_err(|error| ToolError::Traversal(error.to_string()))?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let display = relative_display(&self.root, entry.path())?;
            append_file_matches(entry.path(), &display, &regex, &mut matches)?;
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
) -> Result<(), ToolError> {
    let mut file = File::open(path)?;
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{MAX_RESULTS, RESULT_TRUNCATION_MARKER, ToolError, Tools};

    #[test]
    fn finds_numbered_matches_and_honors_gitignore() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("ignored")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            "safe\n// TODO: first\n// TODO: second\n",
        )
        .unwrap();
        fs::write(dir.path().join("ignored/secret.rs"), "// TODO: hidden\n").unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();

        let out = Tools::new(dir.path()).unwrap().grep("TODO", None).unwrap();
        assert_eq!(
            out.text,
            "src/lib.rs:2:// TODO: first\nsrc/lib.rs:3:// TODO: second"
        );
        assert!(!out.truncated);
    }

    #[test]
    fn searches_a_fenced_subtree_and_skips_binary_files() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/nested")).unwrap();
        fs::write(dir.path().join("src/a.txt"), "needle\n").unwrap();
        fs::write(dir.path().join("src/nested/b.txt"), "needle\n").unwrap();
        fs::write(dir.path().join("src/data.bin"), b"needle\0more\n").unwrap();
        fs::write(dir.path().join("outside.txt"), "needle\n").unwrap();

        let out = Tools::new(dir.path())
            .unwrap()
            .grep("needle", Some("src"))
            .unwrap();
        assert_eq!(out.text, "src/a.txt:1:needle\nsrc/nested/b.txt:1:needle");

        let out = Tools::new(dir.path())
            .unwrap()
            .grep("needle", Some("src/a.txt"))
            .unwrap();
        assert_eq!(out.text, "src/a.txt:1:needle");
    }

    #[test]
    fn rejects_invalid_regex_and_all_path_escape_shapes() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape-link")).unwrap();
        let tools = Tools::new(dir.path()).unwrap();

        assert!(matches!(
            tools.grep("[unclosed", None),
            Err(ToolError::InvalidInput(_))
        ));
        for path in ["../outside", "/etc", "escape-link"] {
            assert!(matches!(
                tools.grep("secret", Some(path)),
                Err(ToolError::PathEscape(_))
            ));
        }
    }

    #[test]
    fn caps_matches_at_five_hundred_entries() {
        let dir = tempdir().unwrap();
        let content: String = (0..=MAX_RESULTS)
            .map(|index| format!("match {index}\n"))
            .collect();
        fs::write(dir.path().join("many.txt"), content).unwrap();

        let out = Tools::new(dir.path()).unwrap().grep("match", None).unwrap();
        assert!(out.truncated);
        let lines: Vec<_> = out.text.lines().collect();
        assert_eq!(lines.len(), MAX_RESULTS + 1);
        assert_eq!(lines.last().copied(), Some(RESULT_TRUNCATION_MARKER));
        assert!(!out.text.contains("many.txt:501:match 500"));
    }
}
