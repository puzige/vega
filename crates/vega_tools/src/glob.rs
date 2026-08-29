//! `glob` tool: project-root-relative file matching with `.gitignore`
//! filtering and a 500-entry output cap (tech-spec §4.4, S4-T21).

use std::path::{Component, Path};

use ignore::overrides::OverrideBuilder;

use crate::Tools;
use crate::error::ToolError;
use crate::output::{MAX_RESULTS, ToolOutput, capped_results};
use crate::tools::{relative_display, walker};

impl Tools {
    /// Return project-relative files matching `pattern`.
    ///
    /// Matching uses the `ignore` crate's gitignore-style glob engine. The
    /// traversal honors project ignore files and never follows symlinks.
    /// At most 500 result entries are returned; an explicit marker and
    /// [`ToolOutput::truncated`] report overflow.
    pub fn glob(&self, pattern: &str) -> Result<ToolOutput, ToolError> {
        validate_pattern_path(pattern)?;
        let matcher = build_matcher(&self.root, pattern)?;
        let mut matches = Vec::new();

        for walked in walker(&self.root) {
            let entry = walked.map_err(|error| ToolError::Traversal(error.to_string()))?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if !matcher.matched(entry.path(), false).is_whitelist() {
                continue;
            }
            matches.push(relative_display(&self.root, entry.path())?);
            if matches.len() > MAX_RESULTS {
                break;
            }
        }

        Ok(capped_results(matches))
    }
}

fn validate_pattern_path(pattern: &str) -> Result<(), ToolError> {
    if pattern.is_empty() {
        return Err(ToolError::InvalidInput(
            "glob pattern must not be empty".to_string(),
        ));
    }
    let path = Path::new(pattern);
    if path.is_absolute()
        || path.has_root()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ToolError::PathEscape(pattern.to_string()));
    }
    Ok(())
}

fn build_matcher(root: &Path, pattern: &str) -> Result<ignore::overrides::Override, ToolError> {
    let mut builder = OverrideBuilder::new(root);
    builder
        .add(pattern)
        .map_err(|error| ToolError::InvalidInput(format!("invalid glob pattern: {error}")))?;
    builder
        .build()
        .map_err(|error| ToolError::InvalidInput(format!("invalid glob pattern: {error}")))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{MAX_RESULTS, RESULT_TRUNCATION_MARKER, ToolError, Tools};

    #[test]
    fn matches_files_recursively_and_honors_gitignore() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/nested")).unwrap();
        fs::create_dir_all(dir.path().join("ignored")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
        fs::write(dir.path().join("src/nested/mod.rs"), "pub mod nested;\n").unwrap();
        fs::write(dir.path().join("src/readme.md"), "nope\n").unwrap();
        fs::write(dir.path().join("ignored/secret.rs"), "secret\n").unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();

        let out = Tools::new(dir.path()).unwrap().glob("**/*.rs").unwrap();
        assert_eq!(out.text, "src/lib.rs\nsrc/nested/mod.rs");
        assert!(!out.truncated);
    }

    #[test]
    fn rejects_invalid_and_escaping_patterns() {
        let dir = tempdir().unwrap();
        let tools = Tools::new(dir.path()).unwrap();
        assert!(matches!(tools.glob(""), Err(ToolError::InvalidInput(_))));
        assert!(matches!(
            tools.glob("[unclosed"),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            tools.glob("../*.rs"),
            Err(ToolError::PathEscape(_))
        ));
        assert!(matches!(
            tools.glob("/tmp/*.rs"),
            Err(ToolError::PathEscape(_))
        ));
    }

    #[test]
    fn caps_results_at_five_hundred_entries() {
        let dir = tempdir().unwrap();
        for index in 0..=MAX_RESULTS {
            fs::write(dir.path().join(format!("file-{index:03}.txt")), "x").unwrap();
        }

        let out = Tools::new(dir.path()).unwrap().glob("*.txt").unwrap();
        assert!(out.truncated);
        let lines: Vec<_> = out.text.lines().collect();
        assert_eq!(lines.len(), MAX_RESULTS + 1);
        assert_eq!(lines.last().copied(), Some(RESULT_TRUNCATION_MARKER));
        assert!(!out.text.contains("file-500.txt"));
    }
}
