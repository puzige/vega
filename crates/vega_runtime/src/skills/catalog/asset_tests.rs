use super::*;
use crate::skills::asset_probe;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, symlink};
use std::rc::Rc;
use tempfile::TempDir;

fn prepared(body: &str, bound: bool) -> (TempDir, SkillRun, SkillSource, RunBinding) {
    let root = tempfile::tempdir().unwrap();
    let skill = root.path().join(".agents/skills/reviewer");
    fs::create_dir_all(skill.join("assets")).unwrap();
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: reviewer\ndescription: Review code changes.\n---\n{body}"),
    )
    .unwrap();
    let source = SkillSource::project_approved(root.path()).unwrap().unwrap();
    let candidate = source.discover().unwrap().candidates.remove(0);
    let approval = SkillApproval::reviewed(&candidate, "project-one", true, true).unwrap();
    let catalog = SkillCatalog::freeze(vec![candidate], &[approval], true).unwrap();
    let binding = RunBinding::for_catalog("run-one", "thread-one", 7, 11, &catalog).unwrap();
    let run = if bound {
        SkillRun::new_bound(catalog, true, binding.clone()).unwrap()
    } else {
        SkillRun::new(catalog, true)
    };
    (root, run, source, binding)
}

fn write_asset(root: &TempDir, path: &str, bytes: &[u8]) -> std::path::PathBuf {
    let skill = root.path().join(".agents/skills/reviewer");
    let mut directory = fs::File::open(&skill).unwrap();
    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        let name = std::ffi::CString::new(component).unwrap();
        if components.peek().is_some() {
            // SAFETY: the owned directory descriptor and NUL-terminated name remain live.
            if unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                assert_eq!(
                    std::io::Error::last_os_error().kind(),
                    std::io::ErrorKind::AlreadyExists
                );
            }
            // SAFETY: the owned directory descriptor and NUL-terminated name remain live.
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            assert!(fd >= 0, "{:?}", std::io::Error::last_os_error());
            // SAFETY: openat returned an owned valid descriptor that is transferred once.
            directory = unsafe { fs::File::from_raw_fd(fd) };
        } else {
            // SAFETY: the owned directory descriptor and NUL-terminated name remain live.
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_TRUNC
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            assert!(fd >= 0, "{:?}", std::io::Error::last_os_error());
            // SAFETY: openat returned an owned valid descriptor that is transferred once.
            let mut file = unsafe { fs::File::from_raw_fd(fd) };
            file.write_all(bytes).unwrap();
        }
    }
    skill.join(path)
}

fn returned(output: &str) -> Value {
    serde_json::from_str(
        output
            .strip_prefix("[Lower-trust Skill asset metadata]\n")
            .unwrap(),
    )
    .unwrap()
}

fn restore_payload(value: &Value, binding: &RunBinding) -> Result<SkillRun, SkillError> {
    let bytes = serde_json::to_vec(value).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    SkillRun::restore_snapshot(&bytes, binding, &hash)
}

fn long_path(index: usize, length: usize, controls: bool) -> String {
    let mut path = format!("assets/d{index:03}/");
    while length - path.len() > 240 {
        path.push_str(&if controls {
            "\u{0001}".repeat(239)
        } else {
            "a".repeat(239)
        });
        path.push('/');
    }
    path.push_str(&if controls {
        "\u{0001}".repeat(length - path.len())
    } else {
        "b".repeat(length - path.len())
    });
    path
}

#[test]
fn issue87_s14_discovery_and_metadata_never_use_asset_body_reader() {
    let (root, mut run, source, _) = prepared("PRIVATE GUIDANCE", true);
    let first = write_asset(&root, "assets/pixel.png", b"\0\xffPRIVATE ASSET BODY");
    let second = write_asset(&root, "assets/unrequested.bin", b"UNREQUESTED ASSET BODY");
    let first_inode = fs::metadata(&first).unwrap();
    let second_inode = fs::metadata(&second).unwrap();
    let reads = Rc::new(RefCell::new(BTreeMap::new()));
    let observed = reads.clone();
    asset_probe::observe_body_reads(
        move |dev, ino| {
            *observed.borrow_mut().entry((dev, ino)).or_insert(0usize) += 1;
        },
        || {
            assert_eq!(source.discover().unwrap().candidates.len(), 1);
            assert!(!run.model_catalog().contains("unrequested"));
            assert_eq!(
                run.load_model("reviewer", |_| true).receipt.status,
                "loaded"
            );
            assert!(reads.borrow().values().sum::<usize>() >= 4);
            let result = run
                .read_resource("reviewer", "assets/pixel.png", |_| true)
                .unwrap();
            assert_eq!(
                returned(&result),
                json!({"name":"reviewer","path":"assets/pixel.png","type":"unknown","size_bytes":b"\0\xffPRIVATE ASSET BODY".len(),"lower_trust":true})
            );
            assert!(!result.contains("PRIVATE ASSET BODY"));
            let snapshot = run.export_snapshot().unwrap();
            let bytes = String::from_utf8_lossy(snapshot.bytes());
            assert!(!bytes.contains("PRIVATE ASSET BODY"));
            assert!(!bytes.contains("UNREQUESTED ASSET BODY"));
            assert!(!bytes.contains("unrequested.bin"));
        },
    );
    assert_eq!(
        reads
            .borrow()
            .get(&(first_inode.dev(), first_inode.ino()))
            .copied()
            .unwrap_or(0),
        0
    );
    assert_eq!(
        reads
            .borrow()
            .get(&(second_inode.dev(), second_inode.ino()))
            .copied()
            .unwrap_or(0),
        0
    );
    assert_eq!(fs::read(first).unwrap(), b"\0\xffPRIVATE ASSET BODY");
}

#[test]
fn issue87_s14_metadata_accepts_empty_sparse_and_maximum_path_without_text_limit() {
    let (root, mut run, source, _) = prepared("Guide", false);
    write_asset(&root, "assets/empty", b"");
    let sparse = write_asset(&root, "assets/large.bin", b"");
    fs::OpenOptions::new()
        .write(true)
        .open(&sparse)
        .unwrap()
        .set_len(8 * 1024 * 1024 * 1024)
        .unwrap();
    let maximum = long_path(1, MAX_RESOURCE_PATH_BYTES, false);
    write_asset(&root, &maximum, b"x");
    assert_eq!(
        source.inspect_asset_metadata("reviewer", "assets/empty"),
        Ok(0)
    );
    assert_eq!(
        source.inspect_asset_metadata("reviewer", "assets/large.bin"),
        Ok(8 * 1024 * 1024 * 1024)
    );
    assert_eq!(source.inspect_asset_metadata("reviewer", &maximum), Ok(1));
    assert_eq!(
        run.load_model("reviewer", |_| true).receipt.status,
        "loaded"
    );
    assert_eq!(
        returned(
            &run.read_resource("reviewer", "assets/large.bin", |_| true)
                .unwrap()
        )["size_bytes"],
        8 * 1024 * 1024 * 1024u64
    );
    assert_eq!(run.export_snapshot().err(), Some(SkillError::InvalidFormat));
}

#[test]
fn issue87_s14_metadata_uses_only_the_approved_source_scope() {
    for imported in [false, true] {
        let config = tempfile::tempdir().unwrap();
        let skill = config.path().join("skills/reviewer");
        fs::create_dir_all(skill.join("assets")).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: reviewer\ndescription: Review code changes.\n---\nGuide",
        )
        .unwrap();
        fs::write(skill.join("assets/safe"), b"\xffbody").unwrap();
        let source = if imported {
            SkillSource::imported_approved(&config.path().join("skills"), 2).unwrap()
        } else {
            SkillSource::vega_global(config.path()).unwrap().unwrap()
        };
        let candidate = source.discover().unwrap().candidates.remove(0);
        let approval = SkillApproval::reviewed(&candidate, "approved-one", true, true).unwrap();
        let catalog = SkillCatalog::freeze(vec![candidate], &[approval], true).unwrap();
        let binding = RunBinding::for_catalog("run-scope", "thread-one", 7, 11, &catalog).unwrap();
        let mut run = SkillRun::new_bound(catalog, true, binding).unwrap();
        assert_eq!(
            run.load_model("reviewer", |_| true).receipt.status,
            "loaded"
        );
        let output = run
            .read_resource("reviewer", "assets/safe", |_| true)
            .unwrap();
        assert_eq!(returned(&output)["size_bytes"], 5);
        assert!(!output.contains(config.path().to_str().unwrap()));
        assert!(run.export_snapshot().is_ok());
    }
}

#[test]
fn issue87_s14_asset_path_type_link_and_root_fences_are_preserved() {
    let (root, mut run, source, _) = prepared("Guide", false);
    write_asset(&root, "assets/safe", b"x");
    for path in [
        "/assets/safe",
        "assets/../SKILL.md",
        "assets/./safe",
        "assets//safe",
        "assets/",
        "assets",
        "references/safe",
        "assets-prefix/safe",
        "assets/\0safe",
    ] {
        assert_eq!(
            source.inspect_asset_metadata("reviewer", path),
            Err(SkillError::UnsafePath),
            "{path:?}"
        );
    }
    assert_eq!(
        source.inspect_asset_metadata(
            "reviewer",
            &long_path(0, MAX_RESOURCE_PATH_BYTES + 1, false)
        ),
        Err(SkillError::UnsafePath)
    );
    assert_eq!(
        source.inspect_asset_metadata("../reviewer", "assets/safe"),
        Err(SkillError::UnsafePath)
    );
    let directory = root.path().join(".agents/skills/reviewer/assets/directory");
    fs::create_dir(&directory).unwrap();
    assert_eq!(
        source.inspect_asset_metadata("reviewer", "assets/directory"),
        Err(SkillError::NotRegular)
    );
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), b"OUTSIDE BODY").unwrap();
    let assets = directory.parent().unwrap();
    symlink(assets.join("safe"), assets.join("inside-link")).unwrap();
    symlink(outside.path().join("secret"), assets.join("outside-link")).unwrap();
    symlink(outside.path(), assets.join("nested-link")).unwrap();
    for path in [
        "assets/inside-link",
        "assets/outside-link",
        "assets/nested-link/secret",
    ] {
        assert_eq!(
            source.inspect_asset_metadata("reviewer", path),
            Err(SkillError::UnsafePath)
        );
    }
    fs::hard_link(outside.path().join("secret"), assets.join("shared")).unwrap();
    assert_eq!(
        source.inspect_asset_metadata("reviewer", "assets/shared"),
        Err(SkillError::Hardlink)
    );
    assert_eq!(
        run.read_resource("reviewer", "assets/safe", |_| true),
        Err(SkillError::NotActivated)
    );
    run.cancel();
    assert_eq!(
        run.read_resource("reviewer", "assets/safe", |_| true),
        Err(SkillError::Cancelled)
    );
    let imported_alias = outside.path().join("alias");
    symlink(source.canonical_root(), &imported_alias).unwrap();
    let imported = SkillSource::imported_approved(&imported_alias, 0).unwrap();
    assert_eq!(
        imported.inspect_asset_metadata("reviewer", "assets/safe"),
        Ok(1)
    );
    fs::remove_file(&imported_alias).unwrap();
    symlink(outside.path(), &imported_alias).unwrap();
    assert_eq!(
        imported.inspect_asset_metadata("reviewer", "assets/safe"),
        Err(SkillError::RootChanged)
    );
    let fifo = assets.join("fifo");
    let fifo_name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: the owned fixture CString is NUL-terminated and live for this call.
    assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
    assert_eq!(
        source.inspect_asset_metadata("reviewer", "assets/fifo"),
        Err(SkillError::NotRegular)
    );
}

#[test]
fn issue87_s14_metadata_recheck_detects_owned_file_parent_and_root_races() {
    for scenario in 0..5 {
        let (root, mut run, _source, _) = prepared("Guide", true);
        let file = write_asset(&root, "assets/safe", b"old");
        assert_eq!(
            run.load_model("reviewer", |_| true).receipt.status,
            "loaded"
        );
        let before = run.export_snapshot().unwrap();
        let changed = file.clone();
        let source_root = root.path().join(".agents/skills");
        let error = asset_probe::before_asset_recheck(
            move || match scenario {
                0 => fs::write(&changed, b"new length").unwrap(),
                1 => {
                    fs::rename(&changed, changed.with_extension("old")).unwrap();
                    fs::write(&changed, b"new").unwrap();
                }
                2 => {
                    let parent = changed.parent().unwrap();
                    fs::rename(parent, parent.with_extension("old")).unwrap();
                    fs::create_dir(parent).unwrap();
                    fs::write(&changed, b"new").unwrap();
                }
                3 => {
                    fs::rename(&source_root, source_root.with_extension("old")).unwrap();
                    fs::create_dir(&source_root).unwrap();
                }
                _ => fs::hard_link(&changed, changed.with_extension("shared")).unwrap(),
            },
            || run.read_resource("reviewer", "assets/safe", |_| true),
        );
        assert!(
            matches!(error, Err(SkillError::Stale | SkillError::RootChanged)),
            "{scenario}: {error:?}"
        );
        assert!(run.assets.is_empty());
        assert_eq!(run.asset_metadata_bytes, 0);
        assert_eq!(run.export_snapshot().unwrap(), before);
    }
}

#[test]
fn issue87_s14_cached_metadata_and_restore_are_frozen_but_cancel_still_wins() {
    let (root, mut run, _source, binding) = prepared("Guide", true);
    let file = write_asset(&root, "assets/safe", b"old");
    assert_eq!(
        run.load_model("reviewer", |_| true).receipt.status,
        "loaded"
    );
    let original = run
        .read_resource("reviewer", "assets/safe", |_| true)
        .unwrap();
    let snapshot = run.export_snapshot().unwrap();
    for scenario in 0..3 {
        match scenario {
            0 => fs::write(&file, b"new and longer").unwrap(),
            1 => fs::remove_file(&file).unwrap(),
            _ => symlink(root.path().join("missing"), &file).unwrap(),
        }
        assert_eq!(
            run.read_resource("reviewer", "assets/safe", |_| true)
                .unwrap(),
            original
        );
        assert_eq!(run.export_snapshot().unwrap(), snapshot);
        let fresh_binding =
            RunBinding::for_catalog("run-two", "thread-one", 7, 11, &run.catalog).unwrap();
        let mut fresh = SkillRun::new_bound(run.catalog.clone(), true, fresh_binding).unwrap();
        assert_eq!(
            fresh.load_model("reviewer", |_| true).receipt.status,
            "loaded"
        );
        let first = fresh.read_resource("reviewer", "assets/safe", |_| true);
        match scenario {
            0 => assert_eq!(
                returned(&first.unwrap())["size_bytes"],
                b"new and longer".len()
            ),
            1 => assert_eq!(first, Err(SkillError::Stale)),
            _ => assert_eq!(first, Err(SkillError::UnsafePath)),
        }
    }
    fs::remove_dir_all(root.path().join(".agents")).unwrap();
    assert_eq!(
        run.read_resource("reviewer", "assets/safe", |_| true)
            .unwrap(),
        original
    );
    let mut restored =
        SkillRun::restore_snapshot(snapshot.bytes(), &binding, snapshot.sha256()).unwrap();
    assert_eq!(
        restored
            .read_resource("reviewer", "assets/safe", |_| true)
            .unwrap(),
        original
    );
    assert_eq!(
        restored.read_resource("reviewer", "assets/safe", |_| false),
        Err(SkillError::OverBudget)
    );
    assert_eq!(restored.export_snapshot().unwrap(), snapshot);
    restored.cancel();
    assert_eq!(
        restored.read_resource("reviewer", "assets/safe", |_| true),
        Err(SkillError::Cancelled)
    );
}

#[derive(Serialize)]
struct OriginalV1<'a> {
    version: u32,
    binding: &'a RunBinding,
    catalog: &'a SkillCatalog,
    direct_user: bool,
    active: &'a Vec<ActiveSkill>,
    references: Vec<&'a FrozenReference>,
    reference_bytes: usize,
    cancelled: bool,
}

fn original_v1(run: &SkillRun) -> Vec<u8> {
    serde_json::to_vec(&OriginalV1 {
        version: 1,
        binding: run.binding.as_ref().unwrap(),
        catalog: &run.catalog,
        direct_user: run.direct_user,
        active: &run.active,
        references: run.references.values().collect(),
        reference_bytes: run.reference_bytes,
        cancelled: run.cancelled,
    })
    .unwrap()
}

#[test]
fn issue87_s14_snapshot_v1_bytes_and_v1_to_v2_are_compatible() {
    let (root, mut run, _source, binding) = prepared("Guide", true);
    assert_eq!(run.export_snapshot().unwrap().bytes(), original_v1(&run));
    assert_eq!(
        run.load_model("reviewer", |_| true).receipt.status,
        "loaded"
    );
    assert_eq!(run.export_snapshot().unwrap().bytes(), original_v1(&run));
    write_asset(&root, "references/note", b"ORIGINAL REFERENCE");
    run.read_reference("reviewer", "references/note", |_| true)
        .unwrap();
    let snapshot = run.export_snapshot().unwrap();
    assert_eq!(snapshot.bytes(), original_v1(&run));
    let mut restored =
        SkillRun::restore_snapshot(snapshot.bytes(), &binding, snapshot.sha256()).unwrap();
    assert_eq!(restored.export_snapshot().unwrap(), snapshot);
    write_asset(&root, "assets/safe", b"\xffPRIVATE BINARY");
    restored
        .read_resource("reviewer", "assets/safe", |_| true)
        .unwrap();
    let v2 = restored.export_snapshot().unwrap();
    let mut payload: Value = serde_json::from_slice(v2.bytes()).unwrap();
    assert_eq!(payload["version"], 2);
    assert_eq!(payload["assets"].as_array().unwrap().len(), 1);
    payload["assets"] = json!([]);
    let empty = restore_payload(&payload, &binding).unwrap();
    assert_eq!(empty.export_snapshot().unwrap(), snapshot);
    fs::remove_dir_all(root.path().join(".agents")).unwrap();
    let mut no_source =
        SkillRun::restore_snapshot(snapshot.bytes(), &binding, snapshot.sha256()).unwrap();
    assert_eq!(no_source.export_snapshot().unwrap(), snapshot);
    assert_eq!(
        no_source
            .read_reference("reviewer", "references/note", |_| true)
            .unwrap()
            .text,
        "ORIGINAL REFERENCE"
    );
}

#[test]
fn issue87_s14_snapshot_rejects_malformed_assets_and_retains_all_old_guards() {
    let (root, mut run, _source, binding) = prepared("Guide", true);
    write_asset(&root, "assets/safe", b"x");
    run.load_model("reviewer", |_| true);
    run.read_resource("reviewer", "assets/safe", |_| true)
        .unwrap();
    let snapshot = run.export_snapshot().unwrap();
    let original: Value = serde_json::from_slice(snapshot.bytes()).unwrap();
    for scenario in 0..13 {
        let mut changed = original.clone();
        match scenario {
            0 => changed["version"] = json!(1),
            1 => {
                changed.as_object_mut().unwrap().remove("assets");
            }
            2 => changed["version"] = json!(3),
            3 => changed["extra"] = json!(true),
            4 => changed["assets"][0]["text"] = json!("PRIVATE BODY"),
            5 => changed["assets"][0]["path"] = json!("assets/../outside"),
            6 => changed["assets"][0]["name"] = json!("unactivated"),
            7 => changed["assets"][0]["type"] = json!("image/png"),
            8 => changed["assets"][0]["size_bytes"] = json!(-1),
            9 => changed["assets"][0]["lower_trust"] = json!(false),
            10 => changed["assets"]
                .as_array_mut()
                .unwrap()
                .push(original["assets"][0].clone()),
            11 => changed["assets"] = json!(vec![original["assets"][0].clone(); 129]),
            _ => changed["direct_user"] = json!(false),
        }
        assert!(
            restore_payload(&changed, &binding).is_err(),
            "scenario {scenario}"
        );
    }
    let encoded = String::from_utf8(snapshot.bytes().to_vec()).unwrap();
    let asset_start = encoded.find("\"assets\":[").unwrap();
    for (field, value) in [
        ("name", "\"reviewer\""),
        ("type", "\"unknown\""),
        ("size_bytes", "1"),
    ] {
        let member = format!("\"{field}\":{value}");
        let duplicated = format!(
            "{}{}",
            &encoded[..asset_start],
            encoded[asset_start..].replacen(&member, &format!("{member},{member}"), 1)
        );
        assert_ne!(duplicated, encoded);
        let digest = format!("{:x}", Sha256::digest(duplicated.as_bytes()));
        assert!(matches!(
            SkillRun::restore_snapshot(duplicated.as_bytes(), &binding, &digest),
            Err(SkillError::InvalidFormat)
        ));
    }
    let mut wrong = snapshot.bytes().to_vec();
    wrong[0] ^= 1;
    assert!(SkillRun::restore_snapshot(&wrong, &binding, snapshot.sha256()).is_err());
    let different =
        RunBinding::from_trusted_parts("other-run", "thread-one", 7, 11, binding.catalog_sha256())
            .unwrap();
    assert!(SkillRun::restore_snapshot(snapshot.bytes(), &different, snapshot.sha256()).is_err());
    let mut v1: Value = serde_json::from_slice(&original_v1(&run)).unwrap();
    v1["assets"] = json!([]);
    assert!(restore_payload(&v1, &binding).is_err());
}

#[test]
fn issue87_s14_asset_count_bytes_budget_failures_are_atomic_and_reference_caps_unchanged() {
    let (root, mut run, _source, binding) = prepared("Guide", true);
    run.load_model("reviewer", |_| true);
    let before = run.export_snapshot().unwrap();
    write_asset(&root, "assets/budget", b"x");
    assert_eq!(
        run.read_resource("reviewer", "assets/budget", |_| false),
        Err(SkillError::OverBudget)
    );
    assert_eq!(run.export_snapshot().unwrap(), before);
    for index in 0..128 {
        let path = format!("assets/empty-{index}");
        write_asset(&root, &path, b"");
        assert!(
            run.read_resource("reviewer", &path, |output| output.len() <= 8192)
                .is_ok()
        );
    }
    assert_eq!(run.assets.len(), 128);
    write_asset(&root, "assets/overflow", b"");
    let full = run.export_snapshot().unwrap();
    assert_eq!(
        run.read_resource("reviewer", "assets/overflow", |_| true),
        Err(SkillError::AggregateLimit)
    );
    assert_eq!(run.export_snapshot().unwrap(), full);
    assert!(
        run.read_resource("reviewer", "assets/empty-0", |_| true)
            .is_ok()
    );
    for index in 0..128 {
        let path = format!("references/empty-{index}");
        write_asset(&root, &path, b"");
        run.read_reference("reviewer", &path, |_| true).unwrap();
    }
    assert_eq!(run.references.len(), 128);
    assert!(
        SkillRun::restore_snapshot(
            run.export_snapshot().unwrap().bytes(),
            &binding,
            run.export_snapshot().unwrap().sha256()
        )
        .is_ok()
    );
    let (_unused, mut bytes_run, _, bytes_binding) = prepared("Guide", true);
    bytes_run.load_model("reviewer", |_| true);
    let mut payload: Value =
        serde_json::from_slice(bytes_run.export_snapshot().unwrap().bytes()).unwrap();
    payload["version"] = json!(2);
    let mut records = Vec::new();
    let mut used = 0;
    for index in 0..21 {
        let path = long_path(index, 1024, true);
        let value = json!({"name":"reviewer","path":path,"type":"unknown","size_bytes":0,"lower_trust":true});
        used += serde_json::to_vec(&value).unwrap().len();
        records.push(value);
    }
    let remaining = 128 * 1024 - used;
    let mut path = long_path(21, 512, true);
    let mut final_record =
        json!({"name":"reviewer","path":path,"type":"unknown","size_bytes":0,"lower_trust":true});
    while serde_json::to_vec(&final_record).unwrap().len() > remaining {
        let position = path.rfind('\u{0001}').unwrap();
        path.replace_range(position..position + 1, "a");
        final_record["path"] = json!(path);
    }
    while serde_json::to_vec(&final_record).unwrap().len() < remaining {
        path.push('a');
        final_record["path"] = json!(path);
    }
    assert!(path.len() <= 1024);
    records.push(final_record.clone());
    payload["assets"] = json!(records);
    let mut at_limit = restore_payload(&payload, &bytes_binding).unwrap();
    assert_eq!(at_limit.asset_metadata_bytes, 128 * 1024);
    write_asset(&_unused, "assets/overflow", b"x");
    let prior = at_limit.export_snapshot().unwrap();
    assert_eq!(
        at_limit.read_resource("reviewer", "assets/overflow", |_| true),
        Err(SkillError::AggregateLimit)
    );
    assert_eq!(at_limit.export_snapshot().unwrap(), prior);
    payload["assets"][21]["size_bytes"] = json!(10);
    assert!(matches!(
        restore_payload(&payload, &bytes_binding),
        Err(SkillError::AggregateLimit)
    ));
}

#[test]
fn issue87_s14_full_bound_snapshot_limit_rejects_before_inserting_metadata() {
    let (root, mut run, _source, binding) = prepared(&"b".repeat(127_000), true);
    run.load_model("reviewer", |_| true);
    for index in 0..127 {
        let path = long_path(index, 1020, false).replacen("assets/", "references/", 1);
        let text = if index < 3 {
            "\u{0001}".repeat(32 * 1024)
        } else {
            String::new()
        };
        write_asset(&root, &path, text.as_bytes());
        run.read_reference("reviewer", &path, |_| true).unwrap();
    }
    let baseline = run.export_snapshot().unwrap();
    let path = long_path(127, 1020, false).replacen("assets/", "references/", 1);
    write_asset(&root, &path, b"");
    run.read_reference("reviewer", &path, |_| true).unwrap();
    let empty_len = run.export_snapshot().unwrap().bytes().len();
    let desired = MAX_RUN_SNAPSHOT_BYTES - 32;
    let growth = desired - empty_len - 1;
    let text = format!(
        "{}{}",
        "\u{0001}".repeat(growth / 6),
        "x".repeat(growth % 6)
    );
    assert!(text.len() <= 32 * 1024);
    write_asset(&root, &path, text.as_bytes());
    let mut final_run =
        SkillRun::restore_snapshot(baseline.bytes(), &binding, baseline.sha256()).unwrap();
    final_run
        .read_reference("reviewer", &path, |_| true)
        .unwrap();
    let full = final_run.export_snapshot().unwrap();
    assert_eq!(full.bytes().len(), desired);
    write_asset(&root, "assets/overflow", b"x");
    assert_eq!(
        final_run.read_resource("reviewer", "assets/overflow", |_| true),
        Err(SkillError::TooLarge)
    );
    assert!(final_run.assets.is_empty());
    assert_eq!(final_run.asset_metadata_bytes, 0);
    assert_eq!(final_run.export_snapshot().unwrap(), full);
}
