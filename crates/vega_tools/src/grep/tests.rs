use std::cell::{Cell, RefCell};
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use regex::Regex;
use tempfile::{TempDir, tempdir};
use tokio_util::sync::CancellationToken;

use super::{CredentialGuard, GREP_TEST_HOOK, GrepSearch, GrepTestEvent};
use crate::{MAX_RESULTS, RESULT_TRUNCATION_MARKER, ToolCredentialPolicy, ToolError, Tools};

struct Fixture {
    owned: TempDir,
    project: PathBuf,
    external: PathBuf,
    protected: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let owned = tempdir().unwrap();
        let root = owned.path().canonicalize().unwrap();
        let project = root.join("project");
        let external = root.join("external");
        let protected = root.join("credentials");
        for path in [&project, &external, &protected] {
            fs::create_dir(path).unwrap();
        }
        Self {
            owned,
            project,
            external,
            protected,
        }
    }

    fn tools(&self) -> Tools {
        Tools::new(&self.project).unwrap()
    }

    fn protected_tools(&self) -> Tools {
        self.tools().with_credential_policy(policy(&self.protected))
    }

    fn external_input(&self) -> &str {
        self.external.to_str().unwrap()
    }
}

fn policy(path: &Path) -> ToolCredentialPolicy {
    ToolCredentialPolicy::new(path.to_path_buf(), Vec::new(), Arc::new(|_| unreachable!())).unwrap()
}

struct HookReset;

impl Drop for HookReset {
    fn drop(&mut self) {
        GREP_TEST_HOOK.with(|slot| *slot.borrow_mut() = None);
    }
}

fn hook(mut callback: impl FnMut(GrepTestEvent, &Path) + 'static) -> HookReset {
    fd_hook(move |event, path, _| callback(event, path))
}

fn fd_hook(callback: impl FnMut(GrepTestEvent, &Path, Option<&fs::File>) + 'static) -> HookReset {
    GREP_TEST_HOOK.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(callback));
    });
    HookReset
}

fn identity(metadata: &fs::Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

#[test]
fn issue426_external_file_has_canonical_locator_and_can_be_read_independently() {
    let fixture = Fixture::new();
    let file = fixture.external.join("note.txt");
    fs::write(&file, "first\nneedle\nlast\n").unwrap();
    let canonical = file.canonicalize().unwrap();
    let tools = fixture.tools();
    for input in [file.to_str().unwrap(), "../external/note.txt"] {
        let prepared = tools.prepare_grep("needle", Some(input)).unwrap();
        assert_eq!(prepared.path(), canonical);
        assert!(!prepared.is_directory());
        let output = tools
            .execute_grep(prepared, &CancellationToken::new())
            .unwrap();
        assert_eq!(output.text, format!("{}:2:needle", canonical.display()));
        assert!(!output.truncated);
    }
    assert_eq!(
        tools
            .read(canonical.to_str().unwrap(), Some(2), Some(1))
            .unwrap()
            .text,
        "2 | needle"
    );
}

#[test]
fn issue426_default_and_internal_paths_keep_relative_scope_and_skip_descendant_links() {
    let fixture = Fixture::new();
    fs::write(fixture.project.join("safe.txt"), "needle\n").unwrap();
    fs::write(
        fixture.external.join("outside.txt"),
        "needle OUTSIDE_CANARY\n",
    )
    .unwrap();
    symlink(&fixture.external, fixture.project.join("escape")).unwrap();
    let tools = fixture.tools();
    for input in [
        None,
        Some(""),
        Some("."),
        Some(tools.root().to_str().unwrap()),
    ] {
        assert_eq!(
            tools.grep("needle", input).unwrap().text,
            "safe.txt:1:needle"
        );
    }
    for input in [
        "safe.txt".to_string(),
        tools.root().join("safe.txt").to_str().unwrap().to_string(),
    ] {
        assert_eq!(
            tools.grep("needle", Some(&input)).unwrap().text,
            "safe.txt:1:needle"
        );
    }
}

#[test]
fn issue426_external_subtree_sorts_and_filters_binary_hidden_and_ignored_files() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.external.join("nested/ignored")).unwrap();
    for name in [
        "a.txt",
        "z.txt",
        "nested/b.txt",
        "nested/ignored/secret.txt",
        "omit.txt",
        ".hidden",
    ] {
        fs::write(fixture.external.join(name), "needle\n").unwrap();
    }
    fs::write(fixture.external.join("data.bin"), b"needle\0binary\n").unwrap();
    fs::write(
        fixture.external.join(".gitignore"),
        "omit.txt\nnested/ignored/\n",
    )
    .unwrap();
    fs::write(fixture.project.join("a.txt"), "needle SIBLING_CANARY\n").unwrap();
    let root = fixture.external.canonicalize().unwrap();
    let output = fixture
        .tools()
        .grep("needle", Some(fixture.external_input()))
        .unwrap();
    assert_eq!(
        output.text,
        ["a.txt", "nested/b.txt", "z.txt"]
            .map(|name| format!("{}:1:needle", root.join(name).display()))
            .join("\n")
    );
    assert!(!output.truncated);
}

#[test]
fn issue426_external_parent_search_keeps_absolute_locators_for_project_descendants() {
    let fixture = Fixture::new();
    fs::write(fixture.project.join("body.txt"), "needle\n").unwrap();
    let parent = fixture.owned.path().canonicalize().unwrap();
    let output = fixture
        .tools()
        .grep("needle", Some(parent.to_str().unwrap()))
        .unwrap();
    assert_eq!(
        output.text,
        format!("{}:1:needle", fixture.project.join("body.txt").display())
    );
}

#[test]
fn issue426_external_ignore_is_tree_local_and_does_not_follow_git_pointers() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.external.join("nested")).unwrap();
    fs::create_dir_all(fixture.protected.join("info")).unwrap();
    fs::write(fixture.owned.path().join(".gitignore"), "external/a.txt\n").unwrap();
    fs::write(fixture.owned.path().join(".ignore"), "external/b.txt\n").unwrap();
    fs::write(fixture.protected.join("info/exclude"), "*.txt\n").unwrap();
    fs::write(
        fixture.external.join(".git"),
        format!("gitdir: {}\n", fixture.protected.display()),
    )
    .unwrap();
    fs::write(fixture.external.join(".gitignore"), "*.skip\n!keep.skip\n").unwrap();
    fs::write(fixture.external.join(".ignore"), "override.txt\n").unwrap();
    fs::write(
        fixture.external.join("nested/.gitignore"),
        "!override.txt\n",
    )
    .unwrap();
    fs::write(fixture.external.join("nested/.ignore"), "!visible.skip\n").unwrap();
    for name in [
        "a.txt",
        "b.txt",
        "drop.skip",
        "keep.skip",
        "nested/override.txt",
        "nested/visible.skip",
    ] {
        fs::write(fixture.external.join(name), "needle\n").unwrap();
    }
    let reads = Rc::new(RefCell::new(Vec::new()));
    let observed = reads.clone();
    let _hook = hook(move |event, path| {
        if event == GrepTestEvent::BeforeIgnoreRead {
            observed.borrow_mut().push(path.to_path_buf());
        }
    });
    let output = fixture
        .protected_tools()
        .grep("needle", Some(fixture.external_input()))
        .unwrap();
    assert!(output.text.contains("/a.txt:1:needle"));
    assert!(output.text.contains("/b.txt:1:needle"));
    assert!(output.text.contains("/keep.skip:1:needle"));
    assert!(output.text.contains("/nested/visible.skip:1:needle"));
    assert!(!output.text.contains("drop.skip"));
    assert!(!output.text.contains("override.txt"));
    assert!(!reads.borrow().is_empty());
    for path in reads.borrow().iter() {
        assert!(path.starts_with(&fixture.external));
        assert!(matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some(".gitignore" | ".ignore")
        ));
    }
}

#[test]
fn issue426_prepare_does_not_read_body_ignore_or_probe_binary() {
    let fixture = Fixture::new();
    fs::write(fixture.external.join("body.txt"), b"needle\0BODY_CANARY").unwrap();
    fs::write(fixture.external.join(".ignore"), "body.txt\n").unwrap();
    let events = Rc::new(Cell::new(0));
    let observed = events.clone();
    let _hook = hook(move |_, _| observed.set(observed.get() + 1));
    let tools = fixture.tools();
    let directory = tools
        .prepare_grep("needle", Some(fixture.external_input()))
        .unwrap();
    assert!(directory.is_directory());
    let file = tools
        .prepare_grep(
            "needle",
            Some(fixture.external.join("body.txt").to_str().unwrap()),
        )
        .unwrap();
    assert!(!file.is_directory());
    assert_eq!(events.get(), 0);
}

#[test]
fn issue426_alias_retarget_does_not_redirect_prepared_execution() {
    let fixture = Fixture::new();
    let original = fixture.external.join("original.txt");
    let alias = fixture.project.join("alias");
    fs::write(&original, "needle ORIGINAL\n").unwrap();
    fs::write(
        fixture.external.join("other.txt"),
        "needle REDIRECT_CANARY\n",
    )
    .unwrap();
    symlink(&original, &alias).unwrap();
    let tools = fixture.tools();
    let prepared = tools.prepare_grep("needle", Some("alias")).unwrap();
    fs::remove_file(&alias).unwrap();
    symlink(fixture.external.join("other.txt"), &alias).unwrap();
    let output = tools
        .execute_grep(prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(
        output.text,
        format!(
            "{}:1:needle ORIGINAL",
            original.canonicalize().unwrap().display()
        )
    );
    assert!(!output.text.contains("REDIRECT_CANARY"));
}

#[test]
fn issue426_directory_alias_retarget_keeps_the_frozen_tree() {
    let fixture = Fixture::new();
    let alias = fixture.project.join("alias");
    fs::write(fixture.external.join("body.txt"), "needle ORIGINAL\n").unwrap();
    fs::write(
        fixture.protected.join("body.txt"),
        "needle REDIRECT_CANARY\n",
    )
    .unwrap();
    symlink(&fixture.external, &alias).unwrap();
    let tools = fixture.tools();
    let prepared = tools.prepare_grep("needle", Some("alias")).unwrap();
    assert!(prepared.is_directory());
    fs::remove_file(&alias).unwrap();
    symlink(&fixture.protected, &alias).unwrap();
    let output = tools
        .execute_grep(prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(
        output.text,
        format!(
            "{}:1:needle ORIGINAL",
            fixture.external.join("body.txt").display()
        )
    );
}

#[test]
fn issue426_prepared_file_rejects_inode_type_deletion_and_symlink_changes() {
    for replacement in ["file", "directory", "deleted", "symlink"] {
        let fixture = Fixture::new();
        let target = fixture.external.join("target");
        fs::write(&target, "needle ORIGINAL\n").unwrap();
        fs::write(
            fixture.external.join("canary"),
            "needle REPLACEMENT_CANARY\n",
        )
        .unwrap();
        let tools = fixture.tools();
        let prepared = tools
            .prepare_grep("needle", Some(target.to_str().unwrap()))
            .unwrap();
        fs::rename(&target, fixture.owned.path().join("saved")).unwrap();
        match replacement {
            "file" => fs::write(&target, "needle REPLACEMENT_CANARY\n").unwrap(),
            "directory" => fs::create_dir(&target).unwrap(),
            "symlink" => symlink(fixture.external.join("canary"), &target).unwrap(),
            "deleted" => {}
            _ => unreachable!(),
        }
        assert!(
            tools
                .execute_grep(prepared, &CancellationToken::new())
                .is_err(),
            "{replacement}"
        );
    }
}

#[test]
fn issue426_prepared_directory_and_ancestor_replacements_fail_closed() {
    for replacement in [
        "ancestor",
        "ancestor-link",
        "directory",
        "file",
        "deleted",
        "symlink",
    ] {
        let fixture = Fixture::new();
        let target = fixture.external.join("branch");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("safe.txt"), "needle ORIGINAL\n").unwrap();
        let tools = fixture.tools();
        let prepared = tools
            .prepare_grep("needle", Some(target.to_str().unwrap()))
            .unwrap();
        if matches!(replacement, "ancestor" | "ancestor-link") {
            let saved = fixture.owned.path().join("saved-parent");
            fs::rename(&fixture.external, &saved).unwrap();
            if replacement == "ancestor-link" {
                symlink(&saved, &fixture.external).unwrap();
            } else {
                fs::create_dir(&fixture.external).unwrap();
                fs::rename(saved.join("branch"), &target).unwrap();
            }
        } else {
            fs::rename(&target, fixture.owned.path().join("saved-target")).unwrap();
            match replacement {
                "directory" => {
                    fs::create_dir(&target).unwrap();
                    fs::write(target.join("safe.txt"), "needle REPLACEMENT_CANARY\n").unwrap();
                }
                "file" => fs::write(&target, "needle REPLACEMENT_CANARY\n").unwrap(),
                "symlink" => symlink(&fixture.protected, &target).unwrap(),
                "deleted" => {}
                _ => unreachable!(),
            }
        }
        assert!(
            tools
                .execute_grep(prepared, &CancellationToken::new())
                .is_err(),
            "{replacement}"
        );
    }
}

#[test]
fn issue426_same_inode_content_update_is_allowed() {
    let fixture = Fixture::new();
    let target = fixture.external.join("note.txt");
    fs::write(&target, "needle before\n").unwrap();
    let tools = fixture.tools();
    let prepared = tools
        .prepare_grep("needle", Some(target.to_str().unwrap()))
        .unwrap();
    fs::write(&target, "needle after\n").unwrap();
    assert!(
        tools
            .execute_grep(prepared, &CancellationToken::new())
            .unwrap()
            .text
            .ends_with(":1:needle after")
    );
}

#[test]
fn issue426_descendant_replaced_after_enumeration_cannot_redirect_ignore_or_body() {
    for replace_open_child in [false, true] {
        let fixture = Fixture::new();
        let branch = fixture.external.join("branch");
        fs::create_dir(&branch).unwrap();
        fs::write(branch.join("body.txt"), "needle ORIGINAL\n").unwrap();
        fs::write(branch.join(".ignore"), "not-present\n").unwrap();
        fs::write(
            fixture.protected.join("body.txt"),
            "needle OUTSIDE_CANARY\n",
        )
        .unwrap();
        fs::write(
            fixture.protected.join(".ignore"),
            "body.txt\nOUTSIDE_IGNORE_CANARY\n",
        )
        .unwrap();
        let original_body = identity(&fs::metadata(branch.join("body.txt")).unwrap());
        let original_ignore = identity(&fs::metadata(branch.join(".ignore")).unwrap());
        let trigger = if replace_open_child {
            branch.clone()
        } else {
            fixture.external.clone()
        };
        let saved = fixture.owned.path().join("saved-branch");
        let outside = fixture.protected.clone();
        let changed = Rc::new(Cell::new(false));
        let observed = changed.clone();
        let reads = Rc::new(Cell::new(0));
        let observed_reads = reads.clone();
        let _hook = fd_hook(move |event, path, file| {
            if event == GrepTestEvent::DirectoryEnumerated
                && path == trigger
                && !observed.replace(true)
            {
                fs::rename(&branch, &saved).unwrap();
                symlink(&outside, &branch).unwrap();
            }
            let expected = match event {
                GrepTestEvent::BeforeIgnoreRead => Some(original_ignore),
                GrepTestEvent::BeforeFileRead => Some(original_body),
                _ => None,
            };
            if let Some(expected) = expected {
                assert_eq!(identity(&file.unwrap().metadata().unwrap()), expected);
                observed_reads.set(observed_reads.get() + 1);
            }
        });
        let result = fixture
            .tools()
            .grep("needle", Some(fixture.external_input()));
        assert!(changed.get());
        if replace_open_child {
            let output = result.unwrap();
            assert_eq!(
                output.text,
                format!(
                    "{}:1:needle ORIGINAL",
                    fixture.external.join("branch/body.txt").display()
                )
            );
            assert_eq!(reads.get(), 2);
        } else {
            assert!(result.is_err());
            assert_eq!(reads.get(), 0);
        }
    }
}

#[test]
fn issue426_external_ignore_symlink_and_hardlink_aliases_are_not_read() {
    for hardlink in [false, true] {
        let fixture = Fixture::new();
        let secret = fixture.protected.join("secret-ignore");
        fs::write(&secret, "visible.txt\n").unwrap();
        fs::write(fixture.external.join("visible.txt"), "needle\n").unwrap();
        if hardlink {
            fs::hard_link(&secret, fixture.external.join(".ignore")).unwrap();
        } else {
            symlink(&secret, fixture.external.join(".ignore")).unwrap();
        }
        let output = fixture
            .protected_tools()
            .grep("needle", Some(fixture.external_input()))
            .unwrap();
        assert!(output.text.ends_with("/visible.txt:1:needle"));
    }
}

#[test]
fn issue426_credential_targets_overlap_and_hardlinks_are_rejected() {
    let fixture = Fixture::new();
    let secret = fixture.protected.join("secret.txt");
    fs::write(&secret, "CREDENTIAL_CANARY\n").unwrap();
    symlink(&fixture.protected, fixture.project.join("alias")).unwrap();
    fs::hard_link(&secret, fixture.external.join("linked.txt")).unwrap();
    let tools = fixture.protected_tools();
    for target in [
        &fixture.protected,
        &secret,
        fixture.owned.path(),
        &fixture.project.join("alias"),
        &fixture.external.join("linked.txt"),
    ] {
        assert!(
            tools
                .prepare_grep("CANARY", Some(target.to_str().unwrap()))
                .is_err()
        );
    }
    assert!(tools.read(secret.to_str().unwrap(), None, None).is_err());
}

#[test]
fn issue426_project_credential_subtree_does_not_hide_safe_results() {
    let fixture = Fixture::new();
    let protected = fixture.project.join("config");
    fs::create_dir(&protected).unwrap();
    fs::write(protected.join("secret.txt"), "needle CREDENTIAL_CANARY\n").unwrap();
    fs::write(protected.join(".ignore"), "needle IGNORE_CANARY\n").unwrap();
    fs::write(fixture.project.join("safe.txt"), "needle\n").unwrap();
    let tools = fixture.tools().with_credential_policy(policy(&protected));
    for input in [
        None,
        Some(""),
        Some("."),
        Some(tools.root().to_str().unwrap()),
    ] {
        assert_eq!(
            tools.grep("needle", input).unwrap().text,
            "safe.txt:1:needle"
        );
    }
    for target in [&protected, &protected.join("secret.txt")] {
        assert!(
            tools
                .grep("needle", Some(target.to_str().unwrap()))
                .is_err()
        );
    }
    assert!(
        tools
            .read(protected.join("secret.txt").to_str().unwrap(), None, None)
            .is_err()
    );
}

#[test]
fn issue426_renamed_credential_directory_is_filtered_by_object_identity() {
    let fixture = Fixture::new();
    let protected = fixture.project.join("config");
    fs::create_dir(&protected).unwrap();
    fs::write(protected.join("secret.txt"), "needle CREDENTIAL_CANARY\n").unwrap();
    fs::write(fixture.project.join("safe.txt"), "needle\n").unwrap();
    let tools = fixture.tools().with_credential_policy(policy(&protected));
    fs::rename(&protected, fixture.project.join("renamed-config")).unwrap();
    fs::create_dir(&protected).unwrap();
    assert_eq!(
        tools.grep("needle", None).unwrap().text,
        "safe.txt:1:needle"
    );
}

#[test]
fn issue426_directory_moved_to_credential_identity_after_prepare_is_not_entered() {
    let fixture = Fixture::new();
    let branch = fixture.external.join("branch");
    fs::create_dir(&branch).unwrap();
    fs::write(
        fixture.protected.join("secret.txt"),
        "needle CREDENTIAL_CANARY\n",
    )
    .unwrap();
    fs::write(fixture.external.join("safe.txt"), "needle\n").unwrap();
    let tools = fixture.protected_tools();
    let prepared = tools
        .prepare_grep("needle", Some(fixture.external_input()))
        .unwrap();
    let saved = fixture.owned.path().join("saved");
    fs::rename(&branch, saved).unwrap();
    fs::rename(&fixture.protected, &branch).unwrap();
    let output = tools
        .execute_grep(prepared, &CancellationToken::new())
        .unwrap();
    assert!(output.text.ends_with("/safe.txt:1:needle"));
    assert!(!output.text.contains("CREDENTIAL_CANARY"));
    assert!(!output.text.contains("branch"));
}

#[test]
fn issue426_credential_root_retarget_protects_both_old_and_new_objects() {
    let fixture = Fixture::new();
    let tools = fixture.protected_tools();
    let moved = fixture.project.join("old-credentials");
    fs::rename(&fixture.protected, &moved).unwrap();
    fs::write(moved.join("secret.txt"), "needle OLD_CREDENTIAL_CANARY\n").unwrap();
    fs::write(
        fixture.external.join("secret.txt"),
        "needle NEW_CREDENTIAL_CANARY\n",
    )
    .unwrap();
    symlink(&fixture.external, &fixture.protected).unwrap();
    fs::write(fixture.project.join("safe.txt"), "needle\n").unwrap();
    assert!(
        tools
            .prepare_grep("needle", Some(fixture.external_input()))
            .is_err()
    );
    assert!(
        tools
            .prepare_grep("needle", Some(moved.to_str().unwrap()))
            .is_err()
    );
    assert_eq!(
        tools.grep("needle", None).unwrap().text,
        "safe.txt:1:needle"
    );
}

#[test]
fn issue426_prepare_retains_credential_identity_across_multiple_replacements() {
    let fixture = Fixture::new();
    let tools = fixture.protected_tools();
    fs::rename(
        &fixture.protected,
        fixture.owned.path().join("original-credentials"),
    )
    .unwrap();
    fs::create_dir(&fixture.protected).unwrap();
    fs::write(
        fixture.protected.join("secret.txt"),
        "needle CREDENTIAL_CANARY\n",
    )
    .unwrap();
    fs::write(fixture.external.join("safe.txt"), "needle\n").unwrap();
    let prepared = tools
        .prepare_grep("needle", Some(fixture.external_input()))
        .unwrap();
    fs::rename(
        &fixture.protected,
        fixture.external.join("moved-credentials"),
    )
    .unwrap();
    fs::create_dir(&fixture.protected).unwrap();
    let output = tools
        .execute_grep(prepared, &CancellationToken::new())
        .unwrap();
    assert!(output.text.ends_with("/safe.txt:1:needle"));
    assert!(!output.text.contains("CREDENTIAL_CANARY"));
    assert!(!output.text.contains("moved-credentials"));
}

#[test]
fn issue426_descriptor_hardlink_recheck_precedes_body_and_ignore_reads() {
    for ignore in [false, true] {
        let fixture = Fixture::new();
        fs::write(fixture.external.join("body.txt"), "needle\n").unwrap();
        fs::write(fixture.external.join(".ignore"), "body.txt\n").unwrap();
        let linked = fixture.protected.join("linked");
        let linked_event = if ignore {
            GrepTestEvent::BeforeIgnoreRead
        } else {
            GrepTestEvent::BeforeFileRead
        };
        let changed = Rc::new(Cell::new(false));
        let observed = changed.clone();
        let _hook = hook(move |event, path| {
            if event == linked_event && !observed.replace(true) {
                fs::hard_link(path, &linked).unwrap();
            }
        });
        let tools = fixture.protected_tools();
        let input = if ignore {
            fixture.external.clone()
        } else {
            fixture.external.join("body.txt")
        };
        assert!(tools.grep("needle", Some(input.to_str().unwrap())).is_err());
        assert!(changed.get());
    }
}

#[test]
fn issue426_default_search_inside_credential_root_is_rejected() {
    let fixture = Fixture::new();
    let nested = fixture.protected.join("nested");
    fs::create_dir(&nested).unwrap();
    for root in [&fixture.protected, &nested] {
        let tools = Tools::new(root)
            .unwrap()
            .with_credential_policy(policy(&fixture.protected));
        for input in [
            None,
            Some(""),
            Some("."),
            Some(tools.root().to_str().unwrap()),
        ] {
            assert!(tools.grep("needle", input).is_err());
        }
    }
}

#[test]
fn issue426_missing_dangling_git_and_special_targets_fail_without_reading() {
    let fixture = Fixture::new();
    let fifo = fixture.external.join("fifo");
    let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: name is a valid NUL-terminated path in this test's owned temporary directory.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let socket = fixture.external.join("socket");
    let _listener = UnixListener::bind(&socket).unwrap();
    symlink(
        fixture.external.join("missing"),
        fixture.external.join("dangling"),
    )
    .unwrap();
    fs::create_dir(fixture.external.join(".git")).unwrap();
    fs::write(fixture.external.join(".git/config"), "GIT_CANARY").unwrap();
    let tools = fixture.tools();
    for name in [
        "missing",
        "dangling",
        "fifo",
        "socket",
        ".git/config",
        ".GIT/config",
    ] {
        assert!(
            tools
                .prepare_grep(
                    "CANARY",
                    Some(fixture.external.join(name).to_str().unwrap())
                )
                .is_err(),
            "{name}"
        );
    }
    assert!(tools.prepare_grep("CANARY", Some("/dev/null")).is_err());
    assert!(
        tools
            .grep("CANARY", Some(fixture.external_input()))
            .unwrap()
            .text
            .is_empty()
    );
}

#[test]
fn issue426_invalid_regex_does_not_echo_sensitive_input() {
    let fixture = Fixture::new();
    let result = fixture
        .tools()
        .prepare_grep("[SENSITIVE_REGEX_CANARY", None);
    let Err(error) = result else {
        panic!("invalid regex accepted")
    };
    assert!(matches!(error, ToolError::InvalidInput(_)));
    assert!(!error.to_string().contains("SENSITIVE_REGEX_CANARY"));
    assert!(!format!("{error:?}").contains("SENSITIVE_REGEX_CANARY"));
}

#[test]
fn issue426_external_non_utf8_locators_are_rejected_without_lossy_paths() {
    let fixture = Fixture::new();
    let readable = fixture.external.join("bad-\u{fffd}.txt");
    fs::write(&readable, "needle NON_UTF8_CONTENT_CANARY\n").unwrap();
    let name = std::ffi::OsString::from_vec(b"bad-\xff.txt".to_vec());
    let path = fixture.external.join(name);
    assert!(path.to_str().is_none());
    let tools = fixture.tools();
    let regex = Regex::new("needle").unwrap();
    let cancel = CancellationToken::new();
    let guard = CredentialGuard::new(&tools);
    let mut search = GrepSearch {
        root: tools.root(),
        regex: &regex,
        matches: Vec::new(),
        cancel: &cancel,
        guard: &guard,
        external: true,
    };
    let reads = Rc::new(Cell::new(0));
    let observed = reads.clone();
    let _hook = hook(move |_, _| observed.set(observed.get() + 1));
    let error = search
        .append(fs::File::open(&readable).unwrap(), &path)
        .unwrap_err();
    assert!(matches!(error, ToolError::InvalidInput(_)));
    assert_eq!(
        error.to_string(),
        "invalid input: external grep paths must be valid UTF-8"
    );
    assert!(!error.to_string().contains('\u{fffd}'));
    for message in [error.to_string(), format!("{error:?}")] {
        assert!(!message.contains('\u{fffd}'));
        assert!(!message.contains("bad-"));
        assert!(!message.contains("NON_UTF8_CONTENT_CANARY"));
    }
    assert!(search.matches.is_empty());
    assert_eq!(reads.get(), 0);
}

#[test]
fn issue426_external_utf8_locators_preserve_paths_through_fd_traversal() {
    let fixture = Fixture::new();
    let file = fixture.external.join("bad-\u{fffd}.txt");
    fs::write(&file, "first\nneedle UTF8_CONTENT_CANARY\n").unwrap();
    let canonical = file.canonicalize().unwrap();
    let expected_descriptors = [
        identity(&fs::metadata(&fixture.external).unwrap()),
        identity(&fs::metadata(&file).unwrap()),
    ];
    let descriptors = Rc::new(RefCell::new(Vec::new()));
    let observed = descriptors.clone();
    let _hook = fd_hook(move |event, _, file| {
        if matches!(
            event,
            GrepTestEvent::DirectoryEnumerated | GrepTestEvent::BeforeFileRead
        ) {
            observed
                .borrow_mut()
                .push(identity(&file.unwrap().metadata().unwrap()));
        }
    });
    let output = fixture
        .tools()
        .grep("needle", Some(fixture.external_input()))
        .unwrap();
    assert_eq!(
        output.text,
        format!("{}:2:needle UTF8_CONTENT_CANARY", canonical.display())
    );
    assert!(!output.truncated);
    assert_eq!(descriptors.borrow().as_slice(), &expected_descriptors);
}

#[test]
fn issue426_caps_after_the_501st_match_and_does_not_continue_reading_lines() {
    let fixture = Fixture::new();
    let body: String = (0..MAX_RESULTS + 100)
        .map(|index| format!("needle {index}\n"))
        .collect();
    fs::write(fixture.external.join("many.txt"), body).unwrap();
    let count = Rc::new(Cell::new(0));
    let observed = count.clone();
    let _hook = hook(move |event, _| {
        if event == GrepTestEvent::LineRead {
            observed.set(observed.get() + 1);
        }
    });
    let output = fixture
        .tools()
        .grep("needle", Some(fixture.external_input()))
        .unwrap();
    assert!(output.truncated);
    assert_eq!(output.text.lines().count(), MAX_RESULTS + 1);
    assert!(output.text.ends_with(RESULT_TRUNCATION_MARKER));
    assert!(!output.text.contains(":501:needle 500"));
    assert_eq!(count.get(), MAX_RESULTS + 1);
}

#[test]
fn issue426_cancellation_before_worker_and_during_scan_never_returns_success() {
    let fixture = Fixture::new();
    fs::write(fixture.external.join("many.txt"), "needle\n".repeat(1000)).unwrap();
    let tools = fixture.tools();
    let events = Rc::new(Cell::new(0));
    let observed = events.clone();
    let first_hook = hook(move |_, _| observed.set(observed.get() + 1));
    let cancelled = CancellationToken::new();
    let prepared = tools
        .prepare_grep("needle", Some(fixture.external_input()))
        .unwrap();
    cancelled.cancel();
    assert!(tools.execute_grep(prepared, &cancelled).is_err());
    assert_eq!(events.get(), 0);
    drop(first_hook);
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let _hook = hook(move |event, _| {
        if event == GrepTestEvent::LineRead {
            signal.cancel();
        }
    });
    let prepared = tools
        .prepare_grep("needle", Some(fixture.external_input()))
        .unwrap();
    assert!(tools.execute_grep(prepared, &cancel).is_err());
}

#[test]
fn issue426_grep_does_not_record_read_state_or_expand_glob_permissions() {
    let fixture = Fixture::new();
    let file = fixture.external.join("note.txt");
    fs::write(&file, "needle\n").unwrap();
    let checkpoints = fixture.owned.path().join("checkpoints");
    fs::create_dir(&checkpoints).unwrap();
    let tools = fixture
        .tools()
        .with_mutation_context(&checkpoints, "project", "thread", "call")
        .unwrap();
    assert!(
        !tools
            .grep("needle", Some(file.to_str().unwrap()))
            .unwrap()
            .text
            .is_empty()
    );
    assert!(
        tools
            .reads
            .validate(&file.canonicalize().unwrap(), b"needle\n")
            .is_err()
    );
    let raw = serde_json::json!({"path":file.to_str().unwrap(),"content":"changed"}).to_string();
    match tools.prepare_write_json(&raw) {
        Err(crate::PrepareMutationError::Invalid(invalid)) => {
            assert_eq!(invalid.code(), crate::MutationErrorCode::FileNotRead);
        }
        _ => panic!("grep must not authorize write without read"),
    }
    assert_eq!(fs::read(&file).unwrap(), b"needle\n");
    assert!(fs::read_dir(checkpoints).unwrap().next().is_none());
    let absolute_pattern = fixture.external.join("*");
    for pattern in ["../external/*", absolute_pattern.to_str().unwrap()] {
        assert!(matches!(tools.glob(pattern), Err(ToolError::PathEscape(_))));
    }
    assert_eq!(tools.root(), fixture.project.canonicalize().unwrap());
}
