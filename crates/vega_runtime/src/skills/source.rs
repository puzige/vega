//! Exact-root discovery and descriptor-relative reads. No ambient home scan.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

/// Maximum UTF-8 size of one `SKILL.md` file.
pub const MAX_SKILL_BYTES: usize = 128 * 1024;
pub(super) const MAX_REFERENCE_BYTES: usize = 32 * 1024;
const MAX_CANDIDATES_PER_ROOT: usize = 128;
pub(super) const MAX_RESOURCE_PATH_BYTES: usize = 1024;

/// The caller-controlled source tier; imported roots must have prior UI consent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    Project,
    VegaGlobal,
    Imported { order: usize },
}

/// Scope only; UI priority is deliberately not part of a consent identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SourceScope {
    Project,
    VegaGlobal,
    Imported,
}

/// Durable identity of an approved source, without its UI ordering or alias.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceIdentity {
    scope: SourceScope,
    canonical_root: PathBuf,
    root_dev: u64,
    root_ino: u64,
}

impl SourceIdentity {
    pub fn scope(&self) -> SourceScope {
        self.scope
    }

    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    pub fn device(&self) -> u64 {
        self.root_dev
    }

    pub fn inode(&self) -> u64 {
        self.root_ino
    }
}

impl SourceKind {
    pub(super) fn priority(self) -> (u8, usize) {
        match self {
            Self::Project => (0, 0),
            Self::VegaGlobal => (1, 0),
            Self::Imported { order } => (2, order),
        }
    }
}

/// Content-safe failures. Paths and file bytes are intentionally omitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillError {
    RootChanged,
    UnsafePath,
    NotRegular,
    Hardlink,
    TooLarge,
    TooManyCandidates,
    InvalidUtf8,
    InvalidName,
    InvalidFormat,
    MalformedYaml,
    ForbiddenTag,
    Stale,
    NotActivated,
    OverBudget,
    AggregateLimit,
    Cancelled,
    Io,
}

impl SkillError {
    /// Stable failure code suitable for a bounded UI diagnostic.
    pub fn code(self) -> &'static str {
        match self {
            Self::RootChanged => "root_changed",
            Self::UnsafePath => "unsafe_path",
            Self::NotRegular => "not_regular",
            Self::Hardlink => "hardlink",
            Self::TooLarge => "too_large",
            Self::TooManyCandidates => "too_many_candidates",
            Self::InvalidUtf8 => "invalid_utf8",
            Self::InvalidName => "invalid_name",
            Self::InvalidFormat => "invalid_format",
            Self::MalformedYaml => "malformed_yaml",
            Self::ForbiddenTag => "forbidden_tag",
            Self::Stale => "stale",
            Self::NotActivated => "not_activated",
            Self::OverBudget => "over_budget",
            Self::AggregateLimit => "aggregate_limit",
            Self::Cancelled => "cancelled",
            Self::Io => "io",
        }
    }
}

/// A bound exact root. The configured path may itself be a symlink only for
/// a UI-approved external import; all paths beneath the canonical root are
/// opened without following symlinks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSource {
    kind: SourceKind,
    configured_root: PathBuf,
    canonical_root: PathBuf,
    root_dev: u64,
    root_ino: u64,
}

/// A bounded candidate with only its name, source identity and content hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCandidate {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub sha256: String,
    pub size_bytes: usize,
}

/// A discovery failure for one potential immediate child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillDiagnostic {
    pub name: String,
    pub error: SkillError,
}

/// Discovery never loads references, assets or scripts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    pub candidates: Vec<SkillCandidate>,
    pub diagnostics: Vec<SkillDiagnostic>,
}

impl SkillSource {
    /// Bind the exact selected project's `.agents/skills` root after UI consent.
    pub fn project_approved(project_root: &Path) -> Result<Option<Self>, SkillError> {
        let project = project_root.canonicalize().map_err(|_| SkillError::Io)?;
        let agents = project.join(".agents");
        if !is_plain_directory_or_missing(&agents)? {
            return Ok(None);
        }
        let root = agents.join("skills");
        if !is_plain_directory_or_missing(&root)? {
            return Ok(None);
        }
        Self::bind(SourceKind::Project, &root).map(Some)
    }

    /// Bind `<vega_store::paths::config_dir()>/skills`, if present.
    /// The caller supplies the actual store-resolved config directory.
    pub fn vega_global(config_dir: &Path) -> Result<Option<Self>, SkillError> {
        let config = match config_dir.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(SkillError::Io),
        };
        let root = config.join("skills");
        if !is_plain_directory_or_missing(&root)? {
            return Ok(None);
        }
        Self::bind(SourceKind::VegaGlobal, &root).map(Some)
    }

    /// Bind one exact root selected and approved through Vega's UI.
    /// This function does not grant consent; callers must enforce it first.
    pub fn imported_approved(root: &Path, order: usize) -> Result<Self, SkillError> {
        Self::bind(SourceKind::Imported { order }, root)
    }

    fn bind(kind: SourceKind, root: &Path) -> Result<Self, SkillError> {
        if !root.is_absolute() {
            return Err(SkillError::UnsafePath);
        }
        let canonical_root = root.canonicalize().map_err(|_| SkillError::Io)?;
        let metadata = fs::metadata(&canonical_root).map_err(|_| SkillError::Io)?;
        if !metadata.is_dir() {
            return Err(SkillError::NotRegular);
        }
        Ok(Self {
            kind,
            configured_root: root.to_path_buf(),
            canonical_root,
            root_dev: metadata.dev(),
            root_ino: metadata.ino(),
        })
    }

    /// Return the source tier without exposing a private absolute path.
    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// Identity for UI consent persistence; reordering imports does not change it.
    pub fn identity(&self) -> SourceIdentity {
        SourceIdentity {
            scope: match self.kind {
                SourceKind::Project => SourceScope::Project,
                SourceKind::VegaGlobal => SourceScope::VegaGlobal,
                SourceKind::Imported { .. } => SourceScope::Imported,
            },
            canonical_root: self.canonical_root.clone(),
            root_dev: self.root_dev,
            root_ino: self.root_ino,
        }
    }

    /// Return the approved canonical target for Settings provenance display.
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    /// Validate stored shape without reading a source. An unactivated Skill
    /// still passes the live descriptor/hash fence on its later load.
    pub(super) fn snapshot_shape_valid(&self) -> bool {
        self.configured_root.is_absolute()
            && self.canonical_root.is_absolute()
            && self.configured_root.as_os_str().as_bytes().len() <= 4096
            && self.canonical_root.as_os_str().as_bytes().len() <= 4096
            && !self.configured_root.as_os_str().as_bytes().contains(&0)
            && !self.canonical_root.as_os_str().as_bytes().contains(&0)
    }

    fn ensure_current(&self) -> Result<File, SkillError> {
        let current = self
            .configured_root
            .canonicalize()
            .map_err(|_| SkillError::RootChanged)?;
        if current != self.canonical_root {
            return Err(SkillError::RootChanged);
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&self.canonical_root)
            .map_err(|_| SkillError::RootChanged)?;
        let metadata = file.metadata().map_err(|_| SkillError::Io)?;
        if !metadata.is_dir() || metadata.dev() != self.root_dev || metadata.ino() != self.root_ino
        {
            return Err(SkillError::RootChanged);
        }
        Ok(file)
    }

    /// Discover only immediate `<root>/<name>/SKILL.md` candidates.
    pub fn discover(&self) -> Result<Discovery, SkillError> {
        let _root = self.ensure_current()?;
        let mut discovered = Discovery::default();
        let mut candidate_count = 0;
        for child in fs::read_dir(&self.canonical_root).map_err(|_| SkillError::Io)? {
            let child = child.map_err(|_| SkillError::Io)?;
            let name = child.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !valid_name(name) {
                continue;
            }
            let entry = fs::symlink_metadata(child.path()).map_err(|_| SkillError::Io)?;
            if entry.file_type().is_symlink() {
                if discovered.diagnostics.len() >= MAX_CANDIDATES_PER_ROOT {
                    return Err(SkillError::TooManyCandidates);
                }
                discovered.diagnostics.push(SkillDiagnostic {
                    name: name.into(),
                    error: SkillError::UnsafePath,
                });
                continue;
            }
            if !entry.is_dir() {
                continue;
            }
            let skill_file = child.path().join("SKILL.md");
            match fs::symlink_metadata(skill_file) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(SkillError::Io),
                Ok(_) => {}
            }
            candidate_count += 1;
            if candidate_count > MAX_CANDIDATES_PER_ROOT {
                return Err(SkillError::TooManyCandidates);
            }
            match self.read_skill_md(name) {
                Ok(bytes) => match super::frontmatter::parse_skill_md(&bytes, name) {
                    Ok(document) => discovered.candidates.push(SkillCandidate {
                        name: name.into(),
                        description: document.metadata.description,
                        source: self.clone(),
                        sha256: sha256(&bytes),
                        size_bytes: bytes.len(),
                    }),
                    Err(error) => discovered.diagnostics.push(SkillDiagnostic {
                        name: name.into(),
                        error,
                    }),
                },
                Err(error) => discovered.diagnostics.push(SkillDiagnostic {
                    name: name.into(),
                    error,
                }),
            }
            if discovered.diagnostics.len() > MAX_CANDIDATES_PER_ROOT {
                return Err(SkillError::TooManyCandidates);
            }
        }
        let _root = self.ensure_current()?;
        discovered
            .candidates
            .sort_by(|left, right| left.name.cmp(&right.name));
        discovered
            .diagnostics
            .sort_by(|left, right| left.name.cmp(&right.name));
        Ok(discovered)
    }

    /// Re-read and verify a discovered candidate before activation.
    pub fn load_candidate(&self, candidate: &SkillCandidate) -> Result<Vec<u8>, SkillError> {
        if candidate.source != *self {
            return Err(SkillError::Stale);
        }
        let bytes = self.read_skill_md(&candidate.name)?;
        if bytes.len() != candidate.size_bytes || sha256(&bytes) != candidate.sha256 {
            return Err(SkillError::Stale);
        }
        Ok(bytes)
    }

    /// Read a bounded `SKILL.md` file from this approved root.
    pub fn read_skill_md(&self, name: &str) -> Result<Vec<u8>, SkillError> {
        if !valid_name(name) {
            return Err(SkillError::InvalidName);
        }
        let bytes =
            self.read_relative(Path::new(name).join("SKILL.md").as_path(), MAX_SKILL_BYTES)?;
        std::str::from_utf8(&bytes).map_err(|_| SkillError::InvalidUtf8)?;
        Ok(bytes)
    }

    /// Read one on-demand UTF-8 reference. This never executes scripts.
    pub fn read_reference(&self, name: &str, path: &str) -> Result<String, SkillError> {
        if !valid_name(name) || !valid_reference_path(path) {
            return Err(SkillError::UnsafePath);
        }
        let bytes = self.read_relative(&Path::new(name).join(path), MAX_REFERENCE_BYTES)?;
        if bytes.contains(&0) {
            return Err(SkillError::InvalidUtf8);
        }
        String::from_utf8(bytes).map_err(|_| SkillError::InvalidUtf8)
    }

    fn read_relative(&self, relative: &Path, limit: usize) -> Result<Vec<u8>, SkillError> {
        let components = normal_components(relative)?;
        let root = self.ensure_current()?;
        let mut opened = open_chain(&root, &components)?;
        let before = opened.file.metadata().map_err(|_| SkillError::Io)?;
        if !before.is_file() {
            return Err(SkillError::NotRegular);
        }
        if before.nlink() != 1 {
            return Err(SkillError::Hardlink);
        }
        if before.len() > limit as u64 {
            return Err(SkillError::TooLarge);
        }
        let first = read_limited(&mut opened.file, limit)?;
        opened
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|_| SkillError::Io)?;
        let second = read_limited(&mut opened.file, limit)?;
        let after = opened.file.metadata().map_err(|_| SkillError::Io)?;
        if first != second || !same_file_state(&before, &after) {
            return Err(SkillError::Stale);
        }
        let reopened = open_chain(&root, &components)?;
        if opened.directories.len() != reopened.directories.len() {
            return Err(SkillError::Stale);
        }
        for (before_dir, after_dir) in opened.directories.iter().zip(&reopened.directories) {
            if !same_identity(
                &before_dir.metadata().map_err(|_| SkillError::Io)?,
                &after_dir.metadata().map_err(|_| SkillError::Io)?,
            ) {
                return Err(SkillError::Stale);
            }
        }
        if !same_identity(
            &before,
            &reopened.file.metadata().map_err(|_| SkillError::Io)?,
        ) {
            return Err(SkillError::Stale);
        }
        let _root = self.ensure_current()?;
        Ok(first)
    }
}

/// Select one winner per name after the caller has filtered out disabled or
/// unconsented candidates. An unapproved project candidate cannot shadow an
/// approved global candidate because it must not be passed to this function.
pub fn resolve_precedence(mut candidates: Vec<SkillCandidate>) -> Vec<SkillCandidate> {
    candidates.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(
                left.source
                    .kind
                    .priority()
                    .cmp(&right.source.kind.priority()),
            )
            .then(
                left.source
                    .canonical_root
                    .as_os_str()
                    .as_bytes()
                    .cmp(right.source.canonical_root.as_os_str().as_bytes()),
            )
    });
    candidates.dedup_by(|left, right| left.name == right.name);
    candidates
}

/// Collapse UI-imported aliases of the same canonical directory before
/// scanning. The earliest UI order wins; project/global scopes stay distinct
/// because their consent is independent even if paths happen to coincide.
pub fn deduplicate_sources(mut sources: Vec<SkillSource>) -> Vec<SkillSource> {
    sources.sort_by(|left, right| {
        left.kind.priority().cmp(&right.kind.priority()).then(
            left.canonical_root
                .as_os_str()
                .as_bytes()
                .cmp(right.canonical_root.as_os_str().as_bytes()),
        )
    });
    let mut seen_imports = BTreeSet::new();
    sources.retain(|source| {
        !matches!(source.kind, SourceKind::Imported { .. })
            || seen_imports.insert((source.root_dev, source.root_ino))
    });
    sources
}

fn is_plain_directory_or_missing(path: &Path) -> Result<bool, SkillError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(SkillError::UnsafePath),
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(SkillError::NotRegular),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(SkillError::Io),
    }
}

pub(super) fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
        && !bytes.windows(2).any(|window| window == b"--")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

pub(super) fn valid_reference_path(path: &str) -> bool {
    if path.len() > MAX_RESOURCE_PATH_BYTES {
        return false;
    }
    match normal_components(Path::new(path)) {
        Ok(components) => components.len() >= 2 && components[0] == OsStr::new("references"),
        Err(_) => false,
    }
}

fn normal_components(path: &Path) -> Result<Vec<OsString>, SkillError> {
    // `Path::components` normalizes some interior `.` segments away. Reject
    // them from the original spelling so callers cannot smuggle a different
    // textual path into a frozen resource identity.
    if path
        .as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|part| part == b"." || part.is_empty())
    {
        return Err(SkillError::UnsafePath);
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) if !value.as_bytes().contains(&0) => {
                components.push(value.to_os_string());
            }
            _ => return Err(SkillError::UnsafePath),
        }
    }
    if components.is_empty() {
        return Err(SkillError::UnsafePath);
    }
    Ok(components)
}

struct OpenedChain {
    directories: Vec<File>,
    file: File,
}

fn open_chain(root: &File, components: &[OsString]) -> Result<OpenedChain, SkillError> {
    let mut directories = Vec::new();
    for component in &components[..components.len() - 1] {
        let parent = directories.last().unwrap_or(root);
        let directory = open_child(parent, component, true)?;
        if !directory.metadata().map_err(|_| SkillError::Io)?.is_dir() {
            return Err(SkillError::NotRegular);
        }
        directories.push(directory);
    }
    let parent = directories.last().unwrap_or(root);
    let file = open_child(parent, &components[components.len() - 1], false)?;
    Ok(OpenedChain { directories, file })
}

fn open_child(parent: &File, name: &OsStr, directory: bool) -> Result<File, SkillError> {
    let component = CString::new(name.as_bytes()).map_err(|_| SkillError::UnsafePath)?;
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent is a live owned directory fd, component is NUL-free, and
    // a successful openat returns a new fd whose ownership is transferred once.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), component.as_ptr(), flags) };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return Err(match error.raw_os_error() {
            Some(libc::ELOOP) => SkillError::UnsafePath,
            Some(libc::ENOENT) => SkillError::Stale,
            Some(libc::ENOTDIR) if child_is_symlink(parent, &component) => SkillError::UnsafePath,
            Some(libc::ENOTDIR) => SkillError::NotRegular,
            _ => SkillError::Io,
        });
    }
    // SAFETY: fd was returned by openat above and is not owned elsewhere.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn child_is_symlink(parent: &File, component: &CString) -> bool {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: stat points to writable storage and the name is NUL-terminated.
    let status = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            component.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if status != 0 {
        return false;
    }
    // SAFETY: fstatat succeeded and initialized the full stat structure.
    let stat = unsafe { stat.assume_init() };
    stat.st_mode & libc::S_IFMT == libc::S_IFLNK
}

fn read_limited(file: &mut File, limit: usize) -> Result<Vec<u8>, SkillError> {
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SkillError::Io)?;
    if bytes.len() > limit {
        return Err(SkillError::TooLarge);
    }
    Ok(bytes)
}

fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn same_file_state(left: &Metadata, right: &Metadata) -> bool {
    same_identity(left, right)
        && left.len() == right.len()
        && left.nlink() == right.nlink()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
