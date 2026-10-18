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
#[cfg(test)]
#[path = "grep/tests.rs"]
mod external_tests;

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
        #[cfg(test)]
        test_event(GrepTestEvent::BeforeFileRead, path, Some(&file));
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
            #[cfg(test)]
            test_event(GrepTestEvent::LineRead, path, Some(reader.get_ref()));
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

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum GrepTestEvent {
    DirectoryEnumerated,
    BeforeIgnoreRead,
    BeforeFileRead,
    LineRead,
}

#[cfg(test)]
type GrepTestHook = Box<dyn FnMut(GrepTestEvent, &Path, Option<&File>)>;

#[cfg(test)]
thread_local! {
    static GREP_TEST_HOOK: std::cell::RefCell<Option<GrepTestHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_event(event: GrepTestEvent, path: &Path, file: Option<&File>) {
    GREP_TEST_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(event, path, file);
        }
    });
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
    fn absolute_project_paths_match_relative_paths_and_explicit_external_targets() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.txt"), "needle\n").unwrap();
        fs::write(outside.path().join("secret.txt"), "needle\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
        let tools = Tools::new(dir.path()).unwrap();
        for relative in ["src", "src/a.txt"] {
            let absolute = tools.root().join(relative);
            assert_eq!(
                tools
                    .grep("needle", Some(absolute.to_str().unwrap()))
                    .unwrap()
                    .text,
                tools.grep("needle", Some(relative)).unwrap().text
            );
        }
        for path in [
            outside.path().canonicalize().unwrap(),
            tools.root().join("escape"),
        ] {
            assert_eq!(
                tools
                    .grep("needle", Some(path.to_str().unwrap()))
                    .unwrap()
                    .text,
                format!(
                    "{}:1:needle",
                    outside
                        .path()
                        .canonicalize()
                        .unwrap()
                        .join("secret.txt")
                        .display()
                )
            );
        }
        assert!(
            tools
                .grep("needle", Some(tools.root().join(".git").to_str().unwrap()))
                .is_err()
        );
    }

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
    fn subtree_search_honors_gitignore_from_project_root() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/ignored")).unwrap();
        fs::write(dir.path().join("src/visible.txt"), "needle\n").unwrap();
        fs::write(dir.path().join("src/ignored/secret.txt"), "needle\n").unwrap();
        fs::write(dir.path().join(".gitignore"), "src/ignored/\n").unwrap();

        let out = Tools::new(dir.path())
            .unwrap()
            .grep("needle", Some("src"))
            .unwrap();

        assert_eq!(out.text, "src/visible.txt:1:needle");
    }

    #[test]
    fn rejects_invalid_regex_and_missing_targets_but_accepts_explicit_external_aliases() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape-link")).unwrap();
        let tools = Tools::new(dir.path()).unwrap();

        assert!(matches!(
            tools.grep("[unclosed", None),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            tools.grep("secret", Some("../missing-issue426-target")),
            Err(ToolError::NotFound(_))
        ));
        assert_eq!(
            tools.grep("secret", Some("escape-link")).unwrap().text,
            format!(
                "{}:1:secret",
                outside
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("secret.txt")
                    .display()
            )
        );
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
