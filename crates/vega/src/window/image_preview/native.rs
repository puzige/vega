use gpui_kit::EntityId;
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU8, Ordering},
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use vega_conversation::types::ImageAttachment;

#[derive(Clone)]
pub(super) struct ImageApplication {
    pub(super) label: String,
    pub(super) path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImageOpenError {
    Cancelled,
    Limit,
    File,
    Applications,
    Launch,
    TimedOut,
}

impl ImageOpenError {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Cancelled => "打开已取消",
            Self::Limit => "临时图片已达上限，请保存到本地后打开",
            Self::File => "无法准备图片，请保存到本地后打开",
            Self::Applications => "无法查询打开方式，请保存到本地后打开",
            Self::Launch => "打开失败，请重试或保存到本地",
            Self::TimedOut => "打开请求超时，图片仍保留供应用读取",
        }
    }
}

const UNOPENED: u8 = 0;
const SPAWNING: u8 = 1;
const PINNED: u8 = 2;
const RETIRED: u8 = 3;

pub(super) struct ExportFile {
    directory: Mutex<Option<TempDir>>,
    pub(super) path: PathBuf,
    state: AtomicU8,
}

impl Drop for ExportFile {
    fn drop(&mut self) {
        if let Ok(directory) = self.directory.get_mut()
            && let Some(directory) = directory.take()
        {
            std::thread::spawn(move || drop(directory));
        }
    }
}

struct Reservation {
    id: u64,
    source: EntityId,
    bytes: usize,
    file: Option<Arc<ExportFile>>,
}

#[derive(Default)]
struct Cache {
    closing: bool,
    inflight: usize,
    #[cfg(test)]
    materialize_gate: Option<Arc<std::sync::Barrier>>,
    cleanup: Vec<std::thread::JoinHandle<()>>,
    next: u64,
    entries: Vec<Reservation>,
}

#[derive(Default)]
struct CacheState {
    state: Mutex<Cache>,
    changed: Condvar,
}

#[derive(Clone, Default)]
pub(super) struct ImageExportCache(Arc<CacheState>);

struct Materializing(ImageExportCache);
impl Drop for Materializing {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.0.state.lock() {
            state.inflight = state.inflight.saturating_sub(1);
            self.0.0.changed.notify_all();
        }
    }
}

struct SessionImageCache(ImageExportCache);
impl gpui_kit::Global for SessionImageCache {}

impl ImageExportCache {
    pub(super) fn for_app(cx: &mut gpui_kit::App) -> Self {
        if !cx.has_global::<SessionImageCache>() {
            let cache = Self::default();
            cx.set_global(SessionImageCache(cache.clone()));
            cx.on_app_quit(move |cx| {
                let cache = cache.clone();
                cx.background_executor().spawn(async move {
                    cache.shutdown();
                })
            })
            .detach();
        }
        cx.global::<SessionImageCache>().0.clone()
    }

    fn shutdown(&self) {
        let (entries, cleanup) = {
            let Ok(mut cache) = self.0.state.lock() else {
                return;
            };
            cache.closing = true;
            while cache.inflight > 0 {
                let Ok(next) = self.0.changed.wait(cache) else {
                    return;
                };
                cache = next;
            }
            (
                std::mem::take(&mut cache.entries),
                std::mem::take(&mut cache.cleanup),
            )
        };
        for worker in cleanup {
            let _ = worker.join();
        }
        for entry in entries {
            if let Some(file) = entry.file
                && let Ok(mut directory) = file.directory.lock()
            {
                drop(directory.take());
            }
        }
    }

    pub(super) fn materialize(
        &self,
        source: EntityId,
        image: &ImageAttachment,
        cancel: &CancellationToken,
    ) -> Result<Arc<ExportFile>, ImageOpenError> {
        let candidates = self
            .0
            .state
            .lock()
            .map_err(|_| ImageOpenError::File)?
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.source == source)
            .filter_map(|entry| entry.file.clone())
            .collect::<Vec<_>>();
        for candidate in candidates {
            if cancel.is_cancelled() {
                return Err(ImageOpenError::Cancelled);
            }
            if matches!(candidate.state.load(Ordering::SeqCst), UNOPENED | PINNED)
                && original_bytes(&candidate.path, image.bytes())
            {
                return Ok(candidate);
            }
        }
        let id = {
            let mut cache = self.0.state.lock().map_err(|_| ImageOpenError::File)?;
            if cache.closing {
                return Err(ImageOpenError::Cancelled);
            }
            if cache.entries.len() >= 32
                || cache
                    .entries
                    .iter()
                    .map(|entry| entry.bytes)
                    .sum::<usize>()
                    .saturating_add(image.bytes().len())
                    > 128 * 1024 * 1024
            {
                return Err(ImageOpenError::Limit);
            }
            cache.next = cache.next.checked_add(1).ok_or(ImageOpenError::Limit)?;
            let id = cache.next;
            cache.entries.push(Reservation {
                id,
                source,
                bytes: image.bytes().len(),
                file: None,
            });
            cache.inflight += 1;
            id
        };
        let _materializing = Materializing(self.clone());
        #[cfg(test)]
        {
            let gate = self
                .0
                .state
                .lock()
                .ok()
                .and_then(|cache| cache.materialize_gate.clone());
            if let Some(gate) = gate {
                gate.wait();
                gate.wait();
            }
        }
        let result = (|| {
            if cancel.is_cancelled() {
                return Err(ImageOpenError::Cancelled);
            }
            let directory = tempfile::Builder::new()
                .prefix("vega-image-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()
                .map_err(|_| ImageOpenError::File)?;
            let extension = match image.mime_type() {
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                _ => "png",
            };
            let path = directory.path().join(format!("image.{extension}"));
            vega_conversation::attachments::save_screenshot(image, &path)
                .map_err(|_| ImageOpenError::File)?;
            if cancel.is_cancelled() {
                return Err(ImageOpenError::Cancelled);
            }
            Ok(Arc::new(ExportFile {
                directory: Mutex::new(Some(directory)),
                path,
                state: AtomicU8::new(UNOPENED),
            }))
        })();
        let mut cache = self.0.state.lock().map_err(|_| ImageOpenError::File)?;
        match result {
            Ok(file) => {
                if cache.closing || !cache.entries.iter().any(|entry| entry.id == id) {
                    cache.entries.retain(|entry| entry.id != id);
                    drop(cache);
                    if let Ok(mut directory) = file.directory.lock() {
                        drop(directory.take());
                    }
                    return Err(ImageOpenError::Cancelled);
                }
                let Some(entry) = cache.entries.iter_mut().find(|entry| entry.id == id) else {
                    return Err(ImageOpenError::Cancelled);
                };
                entry.file = Some(file.clone());
                Ok(file)
            }
            Err(error) => {
                cache.entries.retain(|entry| entry.id != id);
                Err(error)
            }
        }
    }

    pub(super) fn release_failed(&self, file: &Arc<ExportFile>) {
        let Ok(mut cache) = self.0.state.lock() else {
            return;
        };
        if file.state.load(Ordering::SeqCst) != RETIRED {
            return;
        }
        cache.entries.retain(|entry| {
            entry
                .file
                .as_ref()
                .is_none_or(|candidate| !Arc::ptr_eq(candidate, file))
        });
        let file = file.clone();
        cache.cleanup.push(std::thread::spawn(move || {
            if let Ok(mut directory) = file.directory.lock() {
                drop(directory.take());
            }
        }));
    }

    pub(super) fn release_unopened(&self, source: EntityId) {
        let Ok(mut cache) = self.0.state.lock() else {
            return;
        };
        let mut retired = Vec::new();
        let mut kept = Vec::new();
        for entry in cache.entries.drain(..) {
            if entry.source == source
                && entry.file.as_ref().is_none_or(|file| {
                    file.state
                        .compare_exchange(UNOPENED, RETIRED, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                })
            {
                retired.push(entry);
            } else {
                kept.push(entry);
            }
        }
        cache.entries = kept;
        if !retired.is_empty() {
            cache.cleanup.push(std::thread::spawn(move || {
                for entry in retired {
                    if let Some(file) = entry.file
                        && let Ok(mut directory) = file.directory.lock()
                    {
                        drop(directory.take());
                    }
                }
            }));
        }
    }
}

fn original_bytes(path: &Path, bytes: &[u8]) -> bool {
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == bytes.len() as u64)
    {
        return false;
    }
    let mut content = Vec::new();
    file.take(bytes.len() as u64 + 1)
        .read_to_end(&mut content)
        .is_ok()
        && content == bytes
}

pub(super) trait ImageProcess: Send {
    fn poll(&mut self) -> Result<Option<bool>, ImageOpenError>;
    fn cancel(&mut self);
}

pub(super) trait ImageSystem: Send + Sync {
    fn applications(&self, path: &Path) -> Result<Vec<ImageApplication>, ImageOpenError>;
    fn spawn(&self, args: &[std::ffi::OsString]) -> Result<Box<dyn ImageProcess>, ImageOpenError>;
}

pub(super) struct NativeImageSystem;

impl ImageSystem for NativeImageSystem {
    fn applications(&self, path: &Path) -> Result<Vec<ImageApplication>, ImageOpenError> {
        #[cfg(target_os = "macos")]
        {
            #[cfg(test)]
            {
                let _ = macos::applications
                    as fn(&Path) -> Result<Vec<ImageApplication>, ImageOpenError>;
                let _ = path;
                Err(ImageOpenError::Applications)
            }
            #[cfg(not(test))]
            {
                macos::applications(path)
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            Err(ImageOpenError::Applications)
        }
    }
    fn spawn(&self, args: &[std::ffi::OsString]) -> Result<Box<dyn ImageProcess>, ImageOpenError> {
        #[cfg(test)]
        {
            let _ = args;
            Err(ImageOpenError::Launch)
        }
        #[cfg(not(test))]
        {
            let child = std::process::Command::new("/usr/bin/open")
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|_| ImageOpenError::Launch)?;
            Ok(Box::new(NativeImageProcess(child)))
        }
    }
}

#[cfg(not(test))]
struct NativeImageProcess(std::process::Child);

#[cfg(not(test))]
impl ImageProcess for NativeImageProcess {
    fn poll(&mut self) -> Result<Option<bool>, ImageOpenError> {
        self.0
            .try_wait()
            .map(|status| status.map(|status| status.success()))
            .map_err(|_| ImageOpenError::Launch)
    }
    fn cancel(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) fn launch(
    system: &dyn ImageSystem,
    application: &ImageApplication,
    file: &ExportFile,
    cancel: &CancellationToken,
    timeout: std::time::Duration,
) -> Result<(), ImageOpenError> {
    if cancel.is_cancelled() {
        file.state
            .compare_exchange(UNOPENED, RETIRED, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
        return Err(ImageOpenError::Cancelled);
    }
    if file.state.load(Ordering::SeqCst) != PINNED
        && file
            .state
            .compare_exchange(UNOPENED, SPAWNING, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return Err(ImageOpenError::Cancelled);
    }
    let args = [
        std::ffi::OsString::from("-a"),
        application.path.as_os_str().to_owned(),
        std::ffi::OsString::from("--"),
        file.path.as_os_str().to_owned(),
    ];
    let mut child = match system.spawn(&args) {
        Ok(child) => {
            file.state.store(PINNED, Ordering::SeqCst);
            child
        }
        Err(error) => {
            file.state
                .compare_exchange(SPAWNING, RETIRED, Ordering::SeqCst, Ordering::SeqCst)
                .ok();
            return Err(error);
        }
    };
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.poll() {
            Ok(Some(true)) => return Ok(()),
            Ok(Some(false)) => return Err(ImageOpenError::Launch),
            Err(error) => {
                child.cancel();
                return Err(error);
            }
            Ok(None) => {}
        }
        if cancel.is_cancelled() || std::time::Instant::now() >= deadline {
            child.cancel();
            return Err(if cancel.is_cancelled() {
                ImageOpenError::Cancelled
            } else {
                ImageOpenError::TimedOut
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn image() -> ImageAttachment {
        ImageAttachment::from_bytes(
            include_bytes!(
                "../../../../vega_ui/src/conversation_stream/tests/fixtures/screenshot-tiny.png"
            )
            .to_vec(),
        )
        .unwrap()
    }

    struct Process;
    impl ImageProcess for Process {
        fn poll(&mut self) -> Result<Option<bool>, ImageOpenError> {
            Ok(None)
        }
        fn cancel(&mut self) {}
    }

    struct System(Mutex<Vec<Vec<OsString>>>);
    impl ImageSystem for System {
        fn applications(&self, _: &Path) -> Result<Vec<ImageApplication>, ImageOpenError> {
            Ok(Vec::new())
        }
        fn spawn(&self, args: &[OsString]) -> Result<Box<dyn ImageProcess>, ImageOpenError> {
            self.0.lock().unwrap().push(args.to_vec());
            Ok(Box::new(Process))
        }
    }
}
