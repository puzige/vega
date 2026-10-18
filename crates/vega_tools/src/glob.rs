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
            if self.check_credential_path(entry.path()).is_err() {
                continue;
            }
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
