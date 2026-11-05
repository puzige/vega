use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
use yaml_edit::{Mapping, YamlFile, YamlKind, YamlNode};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
static EDIT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("configuration check failed ({stage:?}, {kind:?})")]
pub struct ConfigurationFailure {
    pub stage: ConfigurationFailureStage,
    pub kind: io::ErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigurationFailureStage {
    Poisoned,
    Directory,
    LockOpen,
    LockValidation,
    Read,
    Parse,
}

impl ConfigurationFailureStage {
    fn wrap(self, error: io::Error) -> io::Error {
        io::Error::new(
            error.kind(),
            ConfigurationFailure {
                stage: self,
                kind: error.kind(),
            },
        )
    }
}

pub fn edit_lock_poisoned() -> bool {
    EDIT_LOCK.is_poisoned()
}

pub fn failure(error: &io::Error) -> Option<ConfigurationFailure> {
    error
        .get_ref()
        .and_then(|error| error.downcast_ref::<ConfigurationFailure>())
        .copied()
}

pub struct Edit {
    path: PathBuf,
    bytes: std::cell::RefCell<Option<Vec<u8>>>,
    _file: File,
    _guard: MutexGuard<'static, ()>,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "configuration is invalid or unsupported",
    )
}

fn conflict() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "configuration changed; reload before saving",
    )
}

fn options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        merge_keys: serde_saphyr::MergeKeyPolicy::Error,
        with_snippet: false,
        budget: serde_saphyr::budget! {
            max_depth: 64,
            max_nodes: 100_000,
            max_documents: 1,
            max_aliases: 0,
            max_anchors: 0,
            max_merge_keys: 0,
        },
    }
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    let value = value(bytes)?;
    serde_json::from_value(value).map_err(|_| invalid())
}

#[cfg(test)]
mod configuration_lock_tests;

fn value(bytes: &[u8]) -> io::Result<Value> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let value: Value =
        serde_saphyr::from_str_with_options(text, options()).map_err(|_| invalid())?;
    if value.get("schema_version").and_then(Value::as_u64) != Some(1) || !value.is_object() {
        return Err(invalid());
    }
    let file = YamlFile::from_str(text).map_err(|_| invalid())?;
    if file.documents().count() != 1 || file.directives().next().is_some() {
        return Err(invalid());
    }
    let mapping = file
        .document()
        .and_then(|d| d.as_mapping())
        .ok_or_else(invalid)?;
    validate_mapping(&mapping, 0)?;
    let _: crate::config::AppConfig =
        serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if let Some(reasoning) = value.get("reasoning") {
        let reasoning: crate::reasoning::ReasoningConfig =
            serde_json::from_value(reasoning.clone()).map_err(|_| invalid())?;
        reasoning.validate().map_err(|_| invalid())?;
    }
    if let Some(models) = value.get("model_capabilities") {
        let models: crate::model_capabilities::ModelCapabilityConfig =
            serde_json::from_value(models.clone()).map_err(|_| invalid())?;
        models.validate().map_err(|_| invalid())?;
    }
    Ok(value)
}

fn validate_mapping(mapping: &Mapping, depth: usize) -> io::Result<()> {
    for (key, value) in mapping.iter() {
        if key.as_scalar().is_none() {
            return Err(invalid());
        }
        validate_node(&key, depth + 1)?;
        validate_node(&value, depth + 1)?;
    }
    Ok(())
}

fn validate_node(node: &YamlNode, depth: usize) -> io::Result<()> {
    if depth > 64 {
        return Err(invalid());
    }
    match node.kind() {
        YamlKind::Alias | YamlKind::Tagged(_) => Err(invalid()),
        _ => {
            if let Some(mapping) = node.as_mapping() {
                validate_mapping(mapping, depth + 1)?;
            } else if let Some(sequence) = node.as_sequence() {
                for value in sequence.values() {
                    validate_node(&value, depth + 1)?;
                }
            }
            Ok(())
        }
    }
}

pub fn section<T: DeserializeOwned + Default>(bytes: &[u8], name: &str) -> io::Result<T> {
    let value = value(bytes)?;
    match value.get(name) {
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| invalid()),
        None => Ok(T::default()),
    }
}

pub fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    read_checked(path).map_err(|error| ConfigurationFailureStage::Read.wrap(error))
}

fn read_checked(path: &Path) -> io::Result<Option<Vec<u8>>> {
    validate_parents(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    Ok(Some(bytes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigurationLockBusy {
    #[error("configuration file lock is busy")]
    File,
    #[error("configuration edit lock is busy")]
    Edit,
    #[error("configuration registry lock is busy")]
    Registry,
}

impl ConfigurationLockBusy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Edit => "edit",
            Self::Registry => "registry",
        }
    }

    pub fn into_io(self) -> io::Error {
        io::Error::new(io::ErrorKind::WouldBlock, self)
    }
}

pub fn lock_busy(error: &io::Error) -> Option<ConfigurationLockBusy> {
    error
        .get_ref()
        .and_then(|error| error.downcast_ref::<ConfigurationLockBusy>())
        .copied()
}

pub fn begin(path: &Path) -> io::Result<Edit> {
    let guard = EDIT_LOCK.lock().map_err(|_| {
        ConfigurationFailureStage::Poisoned.wrap(io::Error::other("configuration unavailable"))
    })?;
    begin_with_guard(path, guard)
}

pub fn try_begin(path: &Path) -> io::Result<Edit> {
    let guard = match EDIT_LOCK.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => {
            return Err(ConfigurationLockBusy::Edit.into_io());
        }
        Err(std::sync::TryLockError::Poisoned(_)) => {
            return Err(ConfigurationFailureStage::Poisoned
                .wrap(io::Error::other("configuration unavailable")));
        }
    };
    begin_with_guard(path, guard)
}

fn begin_with_guard(path: &Path, guard: MutexGuard<'static, ()>) -> io::Result<Edit> {
    let parent = path
        .parent()
        .ok_or_else(|| ConfigurationFailureStage::Directory.wrap(invalid()))?;
    private_directory(parent).map_err(|error| ConfigurationFailureStage::Directory.wrap(error))?;
    let lock_path = parent.join(".config.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = options
        .open(lock_path)
        .map_err(|error| ConfigurationFailureStage::LockOpen.wrap(error))?;
    if !file
        .metadata()
        .map_err(|error| ConfigurationFailureStage::LockValidation.wrap(error))?
        .is_file()
    {
        return Err(ConfigurationFailureStage::LockValidation.wrap(invalid()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file
            .metadata()
            .map_err(|error| ConfigurationFailureStage::LockValidation.wrap(error))?;
        // SAFETY: geteuid has no arguments or memory preconditions.
        let uid = unsafe { libc::geteuid() };
        if metadata.uid() != uid || metadata.mode() & 0o077 != 0 || metadata.nlink() != 1 {
            return Err(ConfigurationFailureStage::LockValidation.wrap(invalid()));
        }
    }
    file.try_lock().map_err(lock_error)?;
    let bytes = read(path)?;
    if let Some(bytes) = bytes.as_deref() {
        value(bytes).map_err(|error| ConfigurationFailureStage::Parse.wrap(error))?;
    }
    Ok(Edit {
        path: path.to_path_buf(),
        bytes: std::cell::RefCell::new(bytes),
        _file: file,
        _guard: guard,
    })
}

fn lock_error(error: std::fs::TryLockError) -> io::Error {
    match error {
        std::fs::TryLockError::WouldBlock => ConfigurationLockBusy::File.into_io(),
        std::fs::TryLockError::Error(error) => error,
    }
}

fn validate_parents(path: &Path) -> io::Result<()> {
    for parent in path
        .ancestors()
        .skip(1)
        .filter(|p| !p.as_os_str().is_empty())
    {
        match std::fs::symlink_metadata(parent) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(invalid()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn private_directory(path: &Path) -> io::Result<()> {
    validate_parents(path)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
        && !parent.exists()
    {
        private_directory(parent)?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(invalid()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(path)
        }
        Err(error) => Err(error),
    }
}

impl Edit {
    pub fn check_current(&self) -> io::Result<()> {
        if read(&self.path)? != *self.bytes.borrow() {
            return Err(conflict());
        }
        Ok(())
    }

    pub fn bytes(&self) -> Option<Vec<u8>> {
        self.bytes.borrow().clone()
    }

    pub fn save_fields<T: Serialize>(&self, fields: &T) -> io::Result<Vec<u8>> {
        let candidate = serde_json::to_value(fields).map_err(|_| invalid())?;
        let candidate = candidate.as_object().ok_or_else(invalid)?;
        let expected = self.bytes.borrow().clone();
        if read(&self.path)? != expected {
            return Err(conflict());
        }
        let text = match expected.as_deref() {
            Some(bytes) => std::str::from_utf8(bytes).map_err(|_| invalid())?,
            None => "schema_version: 1\n",
        };
        let file = YamlFile::from_str(text).map_err(|_| invalid())?;
        let root = file
            .document()
            .and_then(|d| d.as_mapping())
            .ok_or_else(invalid)?;
        let before: Value =
            serde_saphyr::from_str_with_options(text, options()).map_err(|_| invalid())?;
        for (key, next) in candidate {
            if before.get(key) == Some(next) {
                continue;
            }
            patch_field(&root, key, before.get(key), next)?;
        }
        let bytes = file.to_string().into_bytes();
        value(&bytes)?;
        if expected.as_deref() == Some(bytes.as_slice()) {
            return Ok(bytes);
        }
        publish(&self.path, expected.as_deref(), &bytes)?;
        *self.bytes.borrow_mut() = Some(bytes.clone());
        Ok(bytes)
    }
}

fn node_for(value: &Value) -> io::Result<YamlNode> {
    let text = format!(
        "value: {}\n",
        serde_json::to_string(value).map_err(|_| invalid())?
    );
    YamlFile::from_str(&text)
        .map_err(|_| invalid())?
        .document()
        .and_then(|d| d.as_mapping())
        .and_then(|m| m.get("value"))
        .ok_or_else(invalid)
}

fn patch_field(mapping: &Mapping, key: &str, old: Option<&Value>, next: &Value) -> io::Result<()> {
    if let (Some(old), Some(next), Some(target)) = (
        old.and_then(Value::as_object),
        next.as_object(),
        mapping.get_mapping(key),
    ) {
        for (key, value) in next {
            if old.get(key) != Some(value) {
                patch_field(&target, key, old.get(key), value)?;
            }
        }
    } else if let (Some(old), Some(next), Some(target)) = (
        old.and_then(Value::as_array),
        next.as_array(),
        mapping.get_sequence(key),
    ) {
        let identity = ["id", "name", "model", "variable", "env"]
            .into_iter()
            .find(|field| {
                old.iter()
                    .chain(next)
                    .all(|value| value.get(*field).and_then(Value::as_str).is_some())
            });
        if let Some(identity) = identity {
            let identify = |value: &Value| -> io::Result<String> {
                let key = value
                    .get(identity)
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?;
                Ok(format!(
                    "{}\u{1f}{key}",
                    value
                        .get("provider")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                ))
            };
            let mut current_ids = old.iter().map(identify).collect::<io::Result<Vec<_>>>()?;
            let next_ids = next.iter().map(identify).collect::<io::Result<Vec<_>>>()?;
            for (index, desired) in next_ids.iter().enumerate() {
                if let Some(position) = current_ids.iter().position(|id| id == desired) {
                    if position != index {
                        let item = target.get(position).ok_or_else(invalid)?;
                        target.insert(index, item);
                        target.remove(position + usize::from(position >= index));
                        let id = current_ids.remove(position);
                        current_ids.insert(index, id);
                    }
                    let old = old
                        .iter()
                        .find(|value| identify(value).ok().as_ref() == Some(desired))
                        .ok_or_else(invalid)?;
                    let item = target
                        .get(index)
                        .and_then(|node| node.as_mapping().cloned())
                        .ok_or_else(invalid)?;
                    for (key, value) in next[index].as_object().ok_or_else(invalid)? {
                        if old.get(key) != Some(value) {
                            patch_field(&item, key, old.get(key), value)?;
                        }
                    }
                } else {
                    target.insert(index, node_for(&next[index])?);
                    current_ids.insert(index, desired.clone());
                }
            }
            while target.len() > next.len() {
                target.remove(target.len() - 1);
            }
            return Ok(());
        }
        mapping.set(key, node_for(&Value::Array(next.clone()))?);
    } else {
        mapping.set(key, node_for(next)?);
    }
    Ok(())
}

pub fn update_section<T: Serialize>(
    path: &Path,
    name: &str,
    value: &T,
    expected: Option<&[u8]>,
) -> io::Result<Vec<u8>> {
    let edit = begin(path)?;
    if expected.is_some_and(|expected| edit.bytes().as_deref() != Some(expected)) {
        return Err(conflict());
    }
    let mut fields = serde_json::Map::new();
    fields.insert(
        name.to_owned(),
        serde_json::to_value(value).map_err(|_| invalid())?,
    );
    edit.save_fields(&fields)
}

fn publish(path: &Path, expected: Option<&[u8]>, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(invalid)?;
    let temporary = parent.join(format!(".config-{}.tmp", ulid::Ulid::generate()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if read(path)?.as_deref() != expected {
            return Err(conflict());
        }
        std::fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
