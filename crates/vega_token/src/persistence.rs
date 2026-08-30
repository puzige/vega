use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{PricingCatalog, PricingError, catalog::MAX_FILE_BYTES};

const TEMP_ATTEMPTS: u64 = 16;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Result of opening a catalog, including whether built-ins were seeded.
#[derive(Clone, PartialEq, Eq)]
pub struct CatalogLoad {
    pub catalog: PricingCatalog,
    pub seeded: bool,
}

impl std::fmt::Debug for CatalogLoad {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CatalogLoad")
            .field("seeded", &self.seeded)
            .field("model_count", &self.catalog.specs().len())
            .finish()
    }
}

/// Loads and validates a catalog from one explicit path.
pub fn load_catalog(path: &Path) -> Result<PricingCatalog, PricingError> {
    match snapshot_target(path)? {
        TargetSnapshot::Existing { bytes, .. } => PricingCatalog::decode(&bytes),
        TargetSnapshot::Missing => Err(PricingError::io("open")),
    }
}

/// Loads an existing catalog or atomically seeds the built-in five-model catalog.
pub fn load_or_seed_catalog(path: &Path) -> Result<CatalogLoad, PricingError> {
    match fs::symlink_metadata(path) {
        Ok(_) => load_catalog(path).map(|catalog| CatalogLoad {
            catalog,
            seeded: false,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let catalog = PricingCatalog::built_in()?;
            match save_catalog_atomic(path, &catalog) {
                Ok(()) => Ok(CatalogLoad {
                    catalog: load_catalog(path)?,
                    seeded: true,
                }),
                Err(PricingError::SaveTargetChanged) => Ok(CatalogLoad {
                    catalog: load_catalog(path)?,
                    seeded: false,
                }),
                Err(error) => Err(error),
            }
        }
        Err(_) => Err(PricingError::io("open")),
    }
}

/// Atomically replaces one explicit pricing file after complete validation.
///
/// A successful rename is the commit point. A later directory-sync failure
/// returns [`PricingError::CommittedDurabilityUnknown`], meaning the new bytes
/// may already be visible and callers must not blindly retry.
pub fn save_catalog_atomic(path: &Path, catalog: &PricingCatalog) -> Result<(), PricingError> {
    save_catalog_atomic_inner(path, catalog, SaveFault::None)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveFault {
    None,
    #[cfg(test)]
    BeforeRename,
    #[cfg(test)]
    ChangeTargetBeforeRecheck,
    #[cfg(test)]
    AfterRename,
    #[cfg(test)]
    TempCollisionExhaustion,
    #[cfg(test)]
    Write,
    #[cfg(test)]
    Flush,
    #[cfg(test)]
    FileSync,
    #[cfg(test)]
    Rename,
    #[cfg(test)]
    DirectoryOpen,
    #[cfg(test)]
    DirectorySync,
}

fn save_catalog_atomic_inner(
    path: &Path,
    catalog: &PricingCatalog,
    _fault: SaveFault,
) -> Result<(), PricingError> {
    let bytes = catalog.encode()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(PricingError::InvalidSchema { field: "save_path" })?;
    let file_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(PricingError::InvalidSchema { field: "save_path" })?;
    let target_snapshot = snapshot_target(path)?;
    if let TargetSnapshot::Existing { bytes, .. } = &target_snapshot {
        PricingCatalog::decode(bytes)?;
    }

    let (mut file, temp_path) = create_temp(parent, file_name, _fault)?;
    let mut cleanup = TempCleanup::new(temp_path);
    let precommit = (|| -> Result<(), PricingError> {
        #[cfg(test)]
        if _fault == SaveFault::Write {
            file.write_all(&bytes[..bytes.len() / 2])
                .map_err(|_| PricingError::io("write"))?;
            return Err(PricingError::io("write"));
        }
        file.write_all(&bytes)
            .map_err(|_| PricingError::io("write"))?;
        #[cfg(test)]
        if _fault == SaveFault::Flush {
            return Err(PricingError::io("flush"));
        }
        file.flush().map_err(|_| PricingError::io("flush"))?;
        #[cfg(test)]
        if _fault == SaveFault::FileSync {
            return Err(PricingError::io("file_sync"));
        }
        file.sync_all().map_err(|_| PricingError::io("file_sync"))?;
        drop(file);
        #[cfg(test)]
        if _fault == SaveFault::ChangeTargetBeforeRecheck {
            fs::write(path, b"concurrent winner")
                .map_err(|_| PricingError::io("injected_target_change"))?;
        }
        if snapshot_target(path)? != target_snapshot {
            return Err(PricingError::SaveTargetChanged);
        }
        #[cfg(test)]
        if _fault == SaveFault::BeforeRename {
            return Err(PricingError::io("injected_precommit"));
        }
        #[cfg(test)]
        if _fault == SaveFault::Rename {
            return Err(PricingError::io("rename"));
        }
        fs::rename(cleanup.path()?, path).map_err(|_| PricingError::io("rename"))?;
        cleanup.disarm();
        Ok(())
    })();
    precommit?;

    #[cfg(test)]
    if matches!(_fault, SaveFault::AfterRename | SaveFault::DirectoryOpen) {
        return Err(PricingError::CommittedDurabilityUnknown);
    }
    let directory = File::open(parent).map_err(|_| PricingError::CommittedDurabilityUnknown)?;
    #[cfg(test)]
    if _fault == SaveFault::DirectorySync {
        return Err(PricingError::CommittedDurabilityUnknown);
    }
    directory
        .sync_all()
        .map_err(|_| PricingError::CommittedDurabilityUnknown)
}

#[derive(Debug, PartialEq, Eq)]
enum TargetSnapshot {
    Missing,
    Existing {
        identity: FileIdentity,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    modified_seconds: i64,
    #[cfg(unix)]
    modified_nanoseconds: i64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

fn snapshot_target(path: &Path) -> Result<TargetSnapshot, PricingError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TargetSnapshot::Missing);
        }
        Err(_) => return Err(PricingError::io("target_metadata")),
    };
    if !metadata.file_type().is_file() {
        return Err(PricingError::UnsafeSaveTarget);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(PricingError::UnsafeSaveTarget);
        }
    }
    let mut file = File::open(path).map_err(|_| PricingError::SaveTargetChanged)?;
    let opened_before = file
        .metadata()
        .map_err(|_| PricingError::SaveTargetChanged)?;
    validate_opened_identity(&metadata, &opened_before)?;
    let mut bytes = Vec::with_capacity((opened_before.len() as usize).min(MAX_FILE_BYTES));
    Read::take(&mut file, (MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| PricingError::io("target_read"))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(PricingError::FileTooLarge);
    }
    let opened_after = file
        .metadata()
        .map_err(|_| PricingError::SaveTargetChanged)?;
    if file_identity(&opened_before) != file_identity(&opened_after)
        || opened_after.len() != bytes.len() as u64
    {
        return Err(PricingError::SaveTargetChanged);
    }
    Ok(TargetSnapshot::Existing {
        identity: file_identity(&opened_after),
        bytes,
    })
}

fn validate_opened_identity(
    path_metadata: &fs::Metadata,
    opened_metadata: &fs::Metadata,
) -> Result<(), PricingError> {
    if !opened_metadata.file_type().is_file() {
        return Err(PricingError::UnsafeSaveTarget);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened_metadata.nlink() > 1
            || path_metadata.dev() != opened_metadata.dev()
            || path_metadata.ino() != opened_metadata.ino()
        {
            return Err(PricingError::SaveTargetChanged);
        }
    }
    Ok(())
}

fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        FileIdentity {
            len: metadata.len(),
            device: metadata.dev(),
            inode: metadata.ino(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
    #[cfg(not(unix))]
    {
        FileIdentity {
            len: metadata.len(),
        }
    }
}

fn create_temp(
    parent: &Path,
    file_name: &std::ffi::OsStr,
    _fault: SaveFault,
) -> Result<(File, PathBuf), PricingError> {
    let process_id = std::process::id();
    for _ in 0..TEMP_ATTEMPTS {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(format!(".vega-{process_id}-{sequence}.tmp"));
        let temp_path = parent.join(temp_name);
        #[cfg(test)]
        let opened = if _fault == SaveFault::TempCollisionExhaustion {
            TEMP_COLLISION_ATTEMPTS.with(|count| count.set(count.get() + 1));
            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
        } else {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
        };
        #[cfg(not(test))]
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path);
        match opened {
            Ok(file) => return Ok((file, temp_path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(PricingError::io("temp_create")),
        }
    }
    Err(PricingError::io("temp_create"))
}

#[cfg(test)]
thread_local! {
    static TEMP_COLLISION_ATTEMPTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_temp_collision_attempts() {
    TEMP_COLLISION_ATTEMPTS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn temp_collision_attempts() -> u64 {
    TEMP_COLLISION_ATTEMPTS.with(std::cell::Cell::get)
}

struct TempCleanup {
    path: Option<PathBuf>,
}

impl TempCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn path(&self) -> Result<&Path, PricingError> {
        self.path.as_deref().ok_or(PricingError::io("temp_state"))
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
pub(crate) fn save_with_precommit_failure(
    path: &Path,
    catalog: &PricingCatalog,
) -> Result<(), PricingError> {
    save_catalog_atomic_inner(path, catalog, SaveFault::BeforeRename)
}

#[cfg(test)]
pub(crate) fn save_with_postcommit_failure(
    path: &Path,
    catalog: &PricingCatalog,
) -> Result<(), PricingError> {
    save_catalog_atomic_inner(path, catalog, SaveFault::AfterRename)
}

#[cfg(test)]
pub(crate) fn save_with_concurrent_target_change(
    path: &Path,
    catalog: &PricingCatalog,
) -> Result<(), PricingError> {
    save_catalog_atomic_inner(path, catalog, SaveFault::ChangeTargetBeforeRecheck)
}

#[cfg(test)]
pub(crate) fn save_with_fault(
    path: &Path,
    catalog: &PricingCatalog,
    fault: SaveFault,
) -> Result<(), PricingError> {
    save_catalog_atomic_inner(path, catalog, fault)
}
