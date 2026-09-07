//! Single-worker, bounded, descriptor-fenced read-only HEAD metadata service.
use crate::types::{
    ProjectBranchCompletion, ProjectBranchRow, ProjectBranchState, ProjectBranchTarget,
};
use std::{
    ffi::CString,
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const FILE_LIMIT: u64 = 4096;
const BATCH_LIMIT: usize = 128;
const BATCH_BUDGET: Duration = Duration::from_millis(250);

struct Request {
    generation: u64,
    targets: Vec<ProjectBranchTarget>,
}
#[derive(Default)]
struct Mailbox {
    request: Option<Request>,
    completion: Option<ProjectBranchCompletion>,
}
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
    cancel: CancellationToken,
    generation: AtomicU64,
}

/// Owns one worker and two bounded mailbox slots. Dropping cancels without UI IO.
pub struct ProjectBranchService {
    shared: Arc<Shared>,
}
impl ProjectBranchService {
    /// Starts a single metadata worker; thread creation failure is recoverable.
    pub fn new() -> io::Result<Self> {
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox::default()),
            wake: Condvar::new(),
            cancel: CancellationToken::new(),
            generation: AtomicU64::new(0),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("vega-branch-metadata".into())
            .spawn(move || run(worker))?;
        Ok(Self { shared })
    }

    /// Invalidates in-flight and pending results, retaining no old labels.
    pub fn invalidate(&self, generation: u64) {
        self.shared.generation.store(generation, Ordering::Release);
        let mut state = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        state.request = None;
        state.completion = None;
    }

    /// Replaces the pending batch. Callers pass only registered project projections.
    pub fn request(&self, generation: u64, mut targets: Vec<ProjectBranchTarget>) {
        targets.truncate(BATCH_LIMIT);
        self.shared.generation.store(generation, Ordering::Release);
        let mut state = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        state.completion = None;
        state.request = Some(Request {
            generation,
            targets,
        });
        self.shared.wake.notify_one();
    }

    /// Takes the newest completion without waiting for filesystem work.
    pub fn take_completion(&self) -> Option<ProjectBranchCompletion> {
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .completion
            .take()
    }
}
impl Drop for ProjectBranchService {
    fn drop(&mut self) {
        // Serialize with the wait predicate to avoid a lost shutdown wakeup.
        let _state = self
            .shared
            .mailbox
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        self.shared.cancel.cancel();
        self.shared.wake.notify_one();
    }
}
fn run(shared: Arc<Shared>) {
    loop {
        let request = {
            let mut state = shared.mailbox.lock().unwrap_or_else(|p| p.into_inner());
            while state.request.is_none() && !shared.cancel.is_cancelled() {
                state = shared.wake.wait(state).unwrap_or_else(|p| p.into_inner());
            }
            if shared.cancel.is_cancelled() {
                return;
            }
            state.request.take()
        };
        let Some(request) = request else { continue };
        let deadline = Instant::now() + BATCH_BUDGET;
        let current = || {
            !shared.cancel.is_cancelled()
                && shared.generation.load(Ordering::Acquire) == request.generation
        };
        let mut rows = Vec::with_capacity(request.targets.len());
        for target in request.targets {
            if !current() {
                break;
            }
            let state = if Instant::now() >= deadline {
                ProjectBranchState::Unknown
            } else {
                read_branch(Path::new(&target.registered_path), &|| {
                    current() && Instant::now() < deadline
                })
                .unwrap_or(ProjectBranchState::Unknown)
            };
            rows.push(ProjectBranchRow { target, state });
        }
        let mut state = shared.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        if current() && state.request.is_none() {
            state.completion = Some(ProjectBranchCompletion {
                generation: request.generation,
                rows,
            });
        }
    }
}
fn invalid() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}
fn check(alive: &impl Fn() -> bool) -> io::Result<()> {
    if alive() {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::Interrupted))
    }
}
fn child(parent: &File, name: &std::ffi::OsStr, directory: bool) -> io::Result<File> {
    let name = CString::new(name.as_bytes()).map_err(|_| invalid())?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | libc::O_CLOEXEC
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent and NUL-terminated single component live across openat.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returns a newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn directory(path: &Path, alive: &impl Fn() -> bool) -> io::Result<File> {
    if !path.is_absolute() || path.as_os_str().len() > FILE_LIMIT as usize {
        return Err(invalid());
    }
    let mut dir = File::open("/")?;
    for (index, component) in path.components().enumerate() {
        check(alive)?;
        if index > 128 {
            return Err(invalid());
        }
        match component {
            Component::RootDir => {}
            Component::Normal(name) => dir = child(&dir, name, true)?,
            _ => return Err(invalid()),
        }
    }
    Ok(dir)
}
fn text(parent: &File, name: &str, alive: &impl Fn() -> bool) -> io::Result<String> {
    check(alive)?;
    let file = child(parent, name.as_ref(), false)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > FILE_LIMIT {
        return Err(invalid());
    }
    let mut value = String::new();
    file.take(FILE_LIMIT + 1).read_to_string(&mut value)?;
    check(alive)?;
    if value.len() > FILE_LIMIT as usize {
        return Err(invalid());
    }
    Ok(value)
}
fn resolve(base: &Path, raw: &str) -> io::Result<PathBuf> {
    if raw.is_empty() || raw.len() > FILE_LIMIT as usize || raw.chars().any(char::is_control) {
        return Err(invalid());
    }
    let path = Path::new(raw);
    let mut result = if path.is_absolute() {
        PathBuf::from("/")
    } else {
        base.to_path_buf()
    };
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return Err(invalid());
                }
            }
            Component::Normal(part) => result.push(part),
            _ => return Err(invalid()),
        }
    }
    Ok(result)
}
fn read_branch(root: &Path, alive: &impl Fn() -> bool) -> io::Result<ProjectBranchState> {
    let root_dir = directory(root, alive)?;
    check(alive)?;
    let git = match child(&root_dir, ".git".as_ref(), true) {
        Ok(git) => git,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ProjectBranchState::NonGit);
        }
        Err(_) => {
            let pointer = text(&root_dir, ".git", alive)?;
            let raw = pointer
                .strip_prefix("gitdir: ")
                .ok_or_else(invalid)?
                .trim_end_matches('\n');
            let target = resolve(root, raw)?;
            let worktrees = target.parent().ok_or_else(invalid)?;
            let owner_git = worktrees.parent().ok_or_else(invalid)?;
            if worktrees.file_name() != Some("worktrees".as_ref())
                || owner_git.file_name() != Some(".git".as_ref())
            {
                return Err(invalid());
            }
            let git = directory(&target, alive)?;
            let backlink = text(&git, "gitdir", alive)?;
            if resolve(&target, backlink.trim_end_matches('\n'))? != root.join(".git") {
                return Err(invalid());
            }
            git
        }
    };
    parse_head(&text(&git, "HEAD", alive)?)
}
fn parse_head(raw: &str) -> io::Result<ProjectBranchState> {
    let head = raw.strip_suffix('\n').unwrap_or(raw);
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        if branch.is_empty()
            || branch.len() > 1024
            || branch.starts_with('/')
            || branch.ends_with('/')
            || branch.ends_with('.')
            || branch.contains("..")
            || branch.contains("@{")
            || branch
                .chars()
                .any(|c| c.is_control() || " ~^:?*[\\".contains(c))
            || branch
                .split('/')
                .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
        {
            return Err(invalid());
        }
        return Ok(ProjectBranchState::Branch(branch.to_string()));
    }
    if matches!(head.len(), 40 | 64) && head.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(ProjectBranchState::Detached);
    }
    Err(invalid())
}

#[cfg(test)]
mod tests;
