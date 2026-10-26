use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::ToolError;

use super::descriptor::{self, CredentialGuard};
use super::{GrepSearch, check_cancelled, invalid};

struct IgnoreLayer {
    ignore: Option<Gitignore>,
    gitignore: Option<Gitignore>,
}

impl IgnoreLayer {
    fn matched(&self, path: &Path, is_directory: bool, ignore: bool) -> Option<bool> {
        let matcher = if ignore {
            &self.ignore
        } else {
            &self.gitignore
        };
        let matched = matcher.as_ref()?.matched(path, is_directory);
        if matched.is_none() {
            None
        } else {
            Some(matched.is_ignore())
        }
    }
}

fn ignored(layers: &[IgnoreLayer], path: &Path, is_directory: bool) -> bool {
    for ignore in [true, false] {
        for layer in layers.iter().rev() {
            if let Some(ignored) = layer.matched(path, is_directory, ignore) {
                return ignored;
            }
        }
    }
    path.file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
}

fn load_ignore(file: File, path: &Path, search: &GrepSearch<'_>) -> Result<Gitignore, ToolError> {
    check_cancelled(search.cancel)?;
    if search.guard.blocks_file(&file)? {
        return Err(invalid("grep ignore configuration is protected"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("invalid grep ignore path"))?;
    let mut builder = GitignoreBuilder::new(parent);
    for line in BufReader::new(file).lines() {
        check_cancelled(search.cancel)?;
        let line = line.map_err(|_| invalid("unable to read grep ignore configuration"))?;
        builder
            .add_line(Some(path.to_path_buf()), &line)
            .map_err(|_| invalid("invalid grep ignore configuration"))?;
    }
    check_cancelled(search.cancel)?;
    builder
        .build()
        .map_err(|_| invalid("invalid grep ignore configuration"))
}

pub(super) fn search_directory(
    directory: &File,
    path: &Path,
    guard: &CredentialGuard,
    search: &mut GrepSearch<'_>,
) -> Result<(), ToolError> {
    visit(directory, path, guard, &mut Vec::new(), search)
}

fn visit(
    directory: &File,
    path: &Path,
    guard: &CredentialGuard,
    layers: &mut Vec<IgnoreLayer>,
    search: &mut GrepSearch<'_>,
) -> Result<(), ToolError> {
    check_cancelled(search.cancel)?;
    let entries = descriptor::entries(directory, search.cancel)?;
    check_cancelled(search.cancel)?;
    let mut layer = IgnoreLayer {
        ignore: None,
        gitignore: None,
    };
    for entry in &entries {
        check_cancelled(search.cancel)?;
        let matcher = match entry.name.to_str() {
            Some(".ignore") => &mut layer.ignore,
            Some(".gitignore") => &mut layer.gitignore,
            _ => continue,
        };
        if !entry.identity.is_file() || entry.is_protected(guard) {
            continue;
        }
        let child = path.join(&entry.name);
        if guard.blocks_path(&child) {
            continue;
        }
        let file = descriptor::open_child(directory, entry)?;
        if guard.blocks_file(&file)? {
            continue;
        }
        *matcher = Some(load_ignore(file, &child, search)?);
    }
    layers.push(layer);
    for entry in entries {
        check_cancelled(search.cancel)?;
        if search.is_full() {
            break;
        }
        if (!entry.identity.is_file() && !entry.identity.is_directory())
            || entry.is_protected(guard)
            || entry.name.as_encoded_bytes().eq_ignore_ascii_case(b".git")
        {
            continue;
        }
        let child = path.join(&entry.name);
        if guard.blocks_path(&child) || ignored(layers, &child, entry.identity.is_directory()) {
            continue;
        }
        let file = descriptor::open_child(directory, &entry)?;
        if guard.blocks_file(&file)? {
            continue;
        }
        if entry.identity.is_directory() {
            visit(&file, &child, guard, layers, search)?;
        } else {
            search.append(file, &child)?;
        }
    }
    layers.pop();
    check_cancelled(search.cancel)
}
