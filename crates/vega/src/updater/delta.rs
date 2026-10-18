use std::collections::{BTreeMap, HashSet};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{UpdateResult, failure, network, signature};

pub(super) const MANIFEST_NAME: &str = "Vega-delta.json";
pub(super) const SIGNATURE_NAME: &str = "Vega-delta.json.sig";
pub(super) const MANIFEST_LIMIT: u64 = 1024 * 1024;
const EXPANDED_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const DOMAIN: &[u8] = b"Vega file delta manifest v1\n";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    schema: u32,
    pub version: String,
    bundle_id: String,
    target: String,
    full_sha256: String,
    pub files: Vec<TargetFile>,
    pub patches: Vec<Patch>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub mode: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
    pub from_version: String,
    pub archive: String,
    pub size: u64,
    pub sha256: String,
    pub files: Vec<String>,
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && path.is_ascii()
        && !path
            .bytes()
            .any(|b| b < 32 || b == 127 || b == b'\\' || b == b':')
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
        && path.starts_with("Contents/")
}

impl Manifest {
    fn validate(&self, full: &signature::SignedManifest) -> UpdateResult<()> {
        if self.schema != 1
            || self.bundle_id != "ai.vega"
            || self.target != "aarch64-apple-darwin"
            || self.version != full.version
            || self.full_sha256 != full.sha256
            || (self.files.is_empty() && !self.patches.is_empty())
            || self.files.len() > 4096
            || self.patches.len() > 5
        {
            return Err(failure("增量清单身份或边界无效"));
        }
        let mut paths = HashSet::new();
        let mut spelling = BTreeMap::new();
        let mut total = 0_u64;
        for file in &self.files {
            if !valid_path(&file.path)
                || !valid_hash(&file.sha256)
                || !matches!(file.mode, 0o644 | 0o755)
                || !paths.insert(file.path.to_ascii_lowercase())
            {
                return Err(failure("增量文件路径、权限或摘要无效"));
            }
            for end in file
                .path
                .match_indices('/')
                .map(|(index, _)| index)
                .chain(std::iter::once(file.path.len()))
            {
                let prefix = &file.path[..end];
                if spelling
                    .insert(prefix.to_ascii_lowercase(), prefix)
                    .is_some_and(|old| old != prefix)
                {
                    return Err(failure("增量目录大小写冲突"));
                }
            }
            total = total
                .checked_add(file.size)
                .filter(|n| *n <= EXPANDED_LIMIT)
                .ok_or_else(|| failure("增量解压大小超出限制"))?;
        }
        for path in &paths {
            for (index, _) in path.match_indices('/') {
                if paths.contains(&path[..index]) {
                    return Err(failure("增量文件与目录路径冲突"));
                }
            }
        }
        if !self.files.is_empty()
            && (!self
                .files
                .iter()
                .any(|f| f.path == "Contents/MacOS/vega" && f.mode == 0o755 && f.size > 0)
                || !self
                    .files
                    .iter()
                    .any(|f| f.path == "Contents/Info.plist" && f.size > 0))
        {
            return Err(failure("增量清单缺少应用身份"));
        }
        let exact: HashSet<_> = self.files.iter().map(|f| &f.path).collect();
        let mut versions = HashSet::new();
        for patch in &self.patches {
            if network::Version::parse(&patch.from_version)?
                >= network::Version::parse(&self.version)?
                || !versions.insert(&patch.from_version)
                || patch.archive
                    != format!("Vega-delta-{}-to-{}.zip", patch.from_version, self.version)
                || patch.size == 0
                || patch.size >= full.size
                || patch.size > network::DOWNLOAD_LIMIT
                || !valid_hash(&patch.sha256)
                || patch.files.len() > self.files.len()
            {
                return Err(failure("增量补丁描述无效"));
            }
            let mut changed = HashSet::new();
            for path in &patch.files {
                if !exact.contains(path) || !changed.insert(path) {
                    return Err(failure("增量补丁文件清单无效"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn patch(&self, current: &str) -> UpdateResult<&Patch> {
        self.patches
            .iter()
            .find(|p| p.from_version == current)
            .ok_or_else(|| failure("当前版本没有增量补丁"))
    }
}

pub(super) fn verify(
    bytes: &[u8],
    signed: &[u8],
    full: &signature::SignedManifest,
) -> UpdateResult<Manifest> {
    if bytes.is_empty()
        || bytes.len() as u64 > MANIFEST_LIMIT
        || signed.len() as u64 > signature::SIGNATURE_LIMIT
    {
        return Err(failure("增量签名大小无效"));
    }
    signature::verify_signature(bytes, signed, DOMAIN)?;
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| failure("增量清单格式无效"))?;
    manifest.validate(full)?;
    Ok(manifest)
}

pub(super) fn load(staging: &Path, full: &signature::SignedManifest) -> UpdateResult<Manifest> {
    verify(
        &signature::bounded_file(&staging.join(MANIFEST_NAME), MANIFEST_LIMIT)?,
        &signature::bounded_file(&staging.join(SIGNATURE_NAME), signature::SIGNATURE_LIMIT)?,
        full,
    )
}

fn open_child(parent: &File, name: &std::ffi::OsStr, directory: bool) -> UpdateResult<File> {
    let name = CString::new(name.as_bytes()).map_err(|_| failure("增量路径无效"))?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | libc::O_CLOEXEC
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent owns a live directory descriptor; name is NUL terminated and flags require no mode argument.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful openat returned a new owned descriptor, transferred exactly once to File.
    let file = unsafe { File::from_raw_fd(fd) };
    if (directory && !file.metadata()?.is_dir()) || (!directory && !file.metadata()?.is_file()) {
        return Err(failure("增量来源不是普通文件或目录"));
    }
    Ok(file)
}

fn source_root(path: &Path) -> UpdateResult<File> {
    if !path.is_absolute() {
        return Err(failure("增量来源路径必须绝对"));
    }
    let mut directory = File::open("/")?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => directory = open_child(&directory, name, true)?,
            _ => return Err(failure("增量来源路径无效")),
        }
    }
    Ok(directory)
}

fn source_file(root: &File, path: &str) -> UpdateResult<File> {
    let mut directory = root.try_clone()?;
    let parts: Vec<_> = path.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        let file = open_child(
            &directory,
            std::ffi::OsStr::new(part),
            index + 1 < parts.len(),
        )?;
        directory = file;
    }
    Ok(directory)
}

fn verified_copy(
    reader: &mut impl Read,
    target: &TargetFile,
    output: &mut impl Write,
) -> UpdateResult<()> {
    let mut hash = Sha256::new();
    let mut remaining = target.size;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let limit = remaining.min(buffer.len() as u64) as usize;
        let count = reader.read(&mut buffer[..limit])?;
        if count == 0 {
            return Err(failure("增量文件长度不足"));
        }
        output.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
        remaining -= count as u64;
    }
    if reader.read(&mut buffer[..1])? != 0 || format!("{:x}", hash.finalize()) != target.sha256 {
        return Err(failure("增量文件长度或摘要不匹配"));
    }
    Ok(())
}

pub(super) fn validate_reuse(
    manifest: &Manifest,
    patch: &Patch,
    source: &Path,
) -> UpdateResult<()> {
    let root = source_root(source)?;
    let changed: HashSet<_> = patch.files.iter().collect();
    for target in &manifest.files {
        if !changed.contains(&target.path) {
            let mut file = source_file(&root, &target.path)?;
            if file.metadata()?.len() != target.size {
                return Err(failure("复用文件大小变化"));
            }
            verified_copy(&mut file, target, &mut std::io::sink())?;
        }
    }
    Ok(())
}

pub(super) fn reconstruct(
    staging: &Path,
    source: &Path,
    destination: &Path,
    full: &signature::SignedManifest,
    current: &str,
) -> UpdateResult<PathBuf> {
    let manifest = load(staging, full)?;
    let patch = manifest.patch(current)?;
    let archive = signature::bounded_file(&staging.join(&patch.archive), patch.size)?;
    if archive.len() as u64 != patch.size
        || format!("{:x}", Sha256::digest(&archive)) != patch.sha256
    {
        return Err(failure("增量压缩包长度或摘要不匹配"));
    }
    reconstruct_archive(&manifest, patch, archive, source, destination)
}

fn reconstruct_archive(
    manifest: &Manifest,
    patch: &Patch,
    bytes: Vec<u8>,
    source: &Path,
    destination: &Path,
) -> UpdateResult<PathBuf> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() != patch.files.len() {
        return Err(failure("增量压缩包条目不匹配"));
    }
    let targets: BTreeMap<_, _> = manifest
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let changed: HashSet<_> = patch.files.iter().map(String::as_str).collect();
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        let target = targets
            .get(name.as_str())
            .ok_or_else(|| failure("补丁包含未授权文件"))?;
        if !changed.contains(name.as_str())
            || entries.insert(name, index).is_some()
            || entry.is_dir()
            || entry.size() != target.size
            || entry
                .unix_mode()
                .is_none_or(|mode| mode != (0o100000 | target.mode))
        {
            return Err(failure("补丁文件结构或权限无效"));
        }
    }
    let root = source_root(source)?;
    let candidate = destination.join("Vega.app");
    std::fs::create_dir(&candidate)?;
    std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o755))?;
    for target in &manifest.files {
        let path = candidate.join(&target.path);
        let parent = path.parent().ok_or_else(|| failure("目标文件路径无效"))?;
        std::fs::create_dir_all(parent)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        if let Some(index) = entries.get(&target.path) {
            verified_copy(&mut archive.by_index(*index)?, target, &mut output)?;
        } else {
            let mut input = source_file(&root, &target.path)?;
            if input.metadata()?.len() != target.size {
                return Err(failure("复用文件大小变化"));
            }
            verified_copy(&mut input, target, &mut output)?;
        }
        output.set_permissions(std::fs::Permissions::from_mode(target.mode))?;
        output.sync_all()?;
        let mut directory = parent;
        while directory != candidate {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))?;
            directory = directory
                .parent()
                .ok_or_else(|| failure("目标目录路径无效"))?;
        }
    }
    verify_tree(&manifest.files, &candidate)?;
    Ok(candidate)
}

fn verify_tree(files: &[TargetFile], root: &Path) -> UpdateResult<()> {
    let descriptor = source_root(root)?;
    let mut found = HashSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if metadata.is_dir() {
                if metadata.permissions().mode() & 0o7777 != 0o755 {
                    return Err(failure("候选目录权限变化"));
                }
                pending.push(entry.path());
            } else if metadata.is_file() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)?
                    .to_str()
                    .ok_or_else(|| failure("候选路径无效"))?
                    .to_string();
                found.insert(relative);
            } else {
                return Err(failure("候选包含特殊文件"));
            }
        }
    }
    if found.len() != files.len() {
        return Err(failure("候选文件集合变化"));
    }
    for target in files {
        if !found.contains(&target.path) {
            return Err(failure("候选文件缺失"));
        }
        let mut file = source_file(&descriptor, &target.path)?;
        if file.metadata()?.permissions().mode() & 0o7777 != target.mode {
            return Err(failure("候选文件权限变化"));
        }
        verified_copy(&mut file, target, &mut std::io::sink())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn target(path: &str, bytes: &[u8], mode: u32) -> TargetFile {
        TargetFile {
            path: path.into(),
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            mode,
        }
    }

    fn fixture() -> (Manifest, signature::SignedManifest, Vec<u8>) {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "Contents/MacOS/vega",
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(b"new binary").unwrap();
        zip.start_file(
            "Contents/new",
            zip::write::SimpleFileOptions::default().unix_permissions(0o644),
        )
        .unwrap();
        zip.write_all(b"new resource").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let full: signature::SignedManifest = serde_json::from_value(json!({
            "schema": 1, "version": "1.0.2", "bundle_id": "ai.vega", "target": "aarch64-apple-darwin",
            "archive": network::ASSET, "size": 100000, "sha256": "a".repeat(64)
        })).unwrap();
        let manifest = Manifest {
            schema: 1,
            version: "1.0.2".into(),
            bundle_id: "ai.vega".into(),
            target: "aarch64-apple-darwin".into(),
            full_sha256: full.sha256.clone(),
            files: vec![
                target("Contents/MacOS/vega", b"new binary", 0o755),
                target("Contents/Info.plist", b"plist", 0o644),
                target("Contents/reused", b"same resource", 0o755),
                target("Contents/new", b"new resource", 0o644),
            ],
            patches: vec![Patch {
                from_version: "1.0.0".into(),
                archive: "Vega-delta-1.0.0-to-1.0.2.zip".into(),
                size: archive.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&archive)),
                files: vec!["Contents/MacOS/vega".into(), "Contents/new".into()],
            }],
        };
        (manifest, full, archive)
    }

    fn source(root: &Path) -> PathBuf {
        let source = root.join("source.app");
        std::fs::create_dir_all(source.join("Contents/MacOS")).unwrap();
        for (path, bytes) in [
            ("Contents/MacOS/vega", "old binary"),
            ("Contents/Info.plist", "plist"),
            ("Contents/reused", "same resource"),
            ("Contents/deleted", "old resource"),
        ] {
            std::fs::write(source.join(path), bytes).unwrap();
        }
        source.canonicalize().unwrap()
    }

    #[test]
    fn reconstructs_direct_patch_adds_deletes_and_changes_permissions() {
        let (manifest, full, bytes) = fixture();
        manifest.validate(&full).unwrap();
        assert_eq!(manifest.patch("1.0.0").unwrap().from_version, "1.0.0");
        assert!(manifest.patch("1.0.1").is_err());
        let root = tempfile::tempdir().unwrap();
        let source = source(root.path());
        validate_reuse(&manifest, &manifest.patches[0], &source).unwrap();
        let output = tempfile::tempdir().unwrap();
        let candidate = reconstruct_archive(
            &manifest,
            &manifest.patches[0],
            bytes,
            &source,
            &output.path().canonicalize().unwrap(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(candidate.join("Contents/MacOS/vega")).unwrap(),
            b"new binary"
        );
        assert_eq!(
            std::fs::read(candidate.join("Contents/new")).unwrap(),
            b"new resource"
        );
        assert!(!candidate.join("Contents/deleted").exists());
        assert_eq!(
            std::fs::metadata(candidate.join("Contents/reused"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o755
        );
        assert_eq!(
            std::fs::read(source.join("Contents/MacOS/vega")).unwrap(),
            b"old binary"
        );
    }

    #[test]
    fn rejects_modified_sources_and_symlink_ancestors_and_leaves_old_app() {
        let (manifest, _, bytes) = fixture();
        let root = tempfile::tempdir().unwrap();
        let source = source(root.path());
        validate_reuse(&manifest, &manifest.patches[0], &source).unwrap();
        std::fs::write(source.join("Contents/reused"), b"evil resource").unwrap();
        assert!(validate_reuse(&manifest, &manifest.patches[0], &source).is_err());
        let output = tempfile::tempdir().unwrap();
        assert!(
            reconstruct_archive(
                &manifest,
                &manifest.patches[0],
                bytes,
                &source,
                output.path()
            )
            .is_err()
        );
        std::fs::remove_file(source.join("Contents/reused")).unwrap();
        std::os::unix::fs::symlink(
            source.join("Contents/Info.plist"),
            source.join("Contents/reused"),
        )
        .unwrap();
        assert!(validate_reuse(&manifest, &manifest.patches[0], &source).is_err());
        std::fs::rename(source.join("Contents"), source.join("real")).unwrap();
        std::os::unix::fs::symlink(source.join("real"), source.join("Contents")).unwrap();
        assert!(validate_reuse(&manifest, &manifest.patches[0], &source).is_err());
        assert_eq!(
            std::fs::read(source.join("real/MacOS/vega")).unwrap(),
            b"old binary"
        );
    }

    #[test]
    fn manifest_rejects_unsafe_paths_collisions_modes_limits_and_binding() {
        for path in [
            "/Contents/evil",
            "Contents/../evil",
            "Contents//evil",
            "Contents/./evil",
            "Contents/evil\\x",
            "Contents/evil\0",
            "Contents/é",
            "Contents/a:b",
        ] {
            let (mut manifest, full, _) = fixture();
            manifest.files.push(target(path, b"x", 0o644));
            assert!(manifest.validate(&full).is_err(), "{path:?}");
        }
        for paths in [
            ["Contents/reused", "Contents/reused/a"],
            ["Contents/A/a", "Contents/a/b"],
            ["Contents/X", "Contents/x"],
        ] {
            let (mut manifest, full, _) = fixture();
            for path in paths {
                manifest.files.push(target(path, b"x", 0o644));
            }
            assert!(manifest.validate(&full).is_err());
        }
        let (mut manifest, full, _) = fixture();
        manifest.files[0].mode = 0o4755;
        assert!(manifest.validate(&full).is_err());
        manifest.files[0].mode = 0o755;
        manifest.files[0].size = EXPANDED_LIMIT;
        assert!(manifest.validate(&full).is_err());
        manifest.files[0].size = 10;
        manifest.full_sha256 = "b".repeat(64);
        assert!(manifest.validate(&full).is_err());
    }

    #[test]
    fn rejects_unlisted_symlink_wrong_mode_and_corrupted_patch_entries() {
        for variant in ["extra", "symlink", "mode", "hash"] {
            let (manifest, _, _) = fixture();
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            if variant == "symlink" {
                zip.add_symlink("Contents/MacOS/vega", "/outside", options)
                    .unwrap();
            } else {
                zip.start_file(
                    "Contents/MacOS/vega",
                    if variant == "mode" {
                        options.unix_permissions(0o644)
                    } else {
                        options
                    },
                )
                .unwrap();
                zip.write_all(if variant == "hash" {
                    b"bad binary"
                } else {
                    b"new binary"
                })
                .unwrap();
            }
            let second = match variant {
                "extra" => "Contents/extra",
                _ => "Contents/new",
            };
            zip.start_file(second, options.unix_permissions(0o644))
                .unwrap();
            zip.write_all(b"new resource").unwrap();
            let bytes = zip.finish().unwrap().into_inner();
            let root = tempfile::tempdir().unwrap();
            let source = source(root.path());
            let output = tempfile::tempdir().unwrap();
            assert!(
                reconstruct_archive(
                    &manifest,
                    &manifest.patches[0],
                    bytes,
                    &source,
                    output.path()
                )
                .is_err(),
                "{variant}"
            );
        }
    }

    #[test]
    fn changed_candidate_is_detected_and_empty_candidates_allow_full_only() {
        let (mut manifest, full, bytes) = fixture();
        let root = tempfile::tempdir().unwrap();
        let source = source(root.path());
        let output = tempfile::tempdir().unwrap();
        let candidate = reconstruct_archive(
            &manifest,
            &manifest.patches[0],
            bytes,
            &source,
            &output.path().canonicalize().unwrap(),
        )
        .unwrap();
        std::fs::write(candidate.join("Contents/new"), b"tampered").unwrap();
        assert!(verify_tree(&manifest.files, &candidate).is_err());
        manifest.files.clear();
        assert!(manifest.validate(&full).is_err());
        manifest.patches.clear();
        manifest.validate(&full).unwrap();
        assert!(manifest.patch("1.0.0").is_err());
    }
    #[test]
    fn duplicate_zip_names_and_unknown_manifest_fields_are_rejected() {
        let (manifest, _, _) = fixture();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["Contents/MacOS/vega", "Contents/MacOS/fake"] {
            zip.start_file(
                name,
                zip::write::SimpleFileOptions::default().unix_permissions(0o755),
            )
            .unwrap();
            zip.write_all(b"new binary").unwrap();
        }
        let mut bytes = zip.finish().unwrap().into_inner();
        let needle = b"Contents/MacOS/fake";
        for index in 0..bytes.len() - needle.len() {
            if &bytes[index..index + needle.len()] == needle {
                bytes[index..index + needle.len()].copy_from_slice(b"Contents/MacOS/vega");
            }
        }
        let root = tempfile::tempdir().unwrap();
        let source = source(root.path());
        let output = tempfile::tempdir().unwrap();
        assert!(
            reconstruct_archive(
                &manifest,
                &manifest.patches[0],
                bytes,
                &source,
                output.path()
            )
            .is_err()
        );
        assert!(serde_json::from_value::<TargetFile>(json!({"path": "Contents/a", "size": 0, "mode": 420, "sha256": "a".repeat(64), "extra": true})).is_err());
    }

    #[test]
    fn unsigned_and_oversized_delta_metadata_is_rejected() {
        let (_, full, _) = fixture();
        assert!(verify(b"{}", b"invalid", &full).is_err());
        assert!(verify(&vec![b' '; MANIFEST_LIMIT as usize + 1], b"invalid", &full).is_err());
        assert!(
            verify(
                b"{}",
                &vec![b' '; signature::SIGNATURE_LIMIT as usize + 1],
                &full
            )
            .is_err()
        );
    }
}
