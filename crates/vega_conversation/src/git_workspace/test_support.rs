use super::*;
use std::sync::{OnceLock, Weak};

pub type GitTestCommandExecutor = Arc<
    dyn Fn(&Command, Option<&[u8]>, usize) -> Result<(Vec<u8>, bool), GitWorkspaceError>
        + Send
        + Sync,
>;

struct Executor(GitTestCommandExecutor);

impl GitCommandBackend for Executor {
    fn execute(
        &self,
        command: &Command,
        input: Option<&[u8]>,
        stdout_limit: usize,
    ) -> Result<Output, GitWorkspaceError> {
        let (stdout, overflow) = (self.0)(command, input, stdout_limit)?;
        if stdout.len() > stdout_limit && !overflow {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        Ok(Output { stdout, overflow })
    }
}

fn registry() -> &'static Mutex<HashMap<PathBuf, Weak<dyn GitCommandBackend>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Weak<dyn GitCommandBackend>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

pub struct GitTestCommandGuard {
    root: PathBuf,
    backend: Arc<dyn GitCommandBackend>,
}

pub fn register_git_test_executor(
    root: &Path,
    executor: GitTestCommandExecutor,
) -> Result<GitTestCommandGuard, GitWorkspaceError> {
    let root = fs::canonicalize(root).map_err(|_| error(GitWorkspaceErrorCode::InvalidRoot))?;
    let backend: Arc<dyn GitCommandBackend> = Arc::new(Executor(executor));
    let mut registry = registry()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert!(
        registry.get(&root).and_then(Weak::upgrade).is_none(),
        "duplicate test root"
    );
    registry.insert(root.clone(), Arc::downgrade(&backend));
    Ok(GitTestCommandGuard { root, backend })
}

impl Drop for GitTestCommandGuard {
    fn drop(&mut self) {
        let mut registry = registry()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if registry
            .get(&self.root)
            .and_then(Weak::upgrade)
            .is_some_and(|current| Arc::ptr_eq(&current, &self.backend))
        {
            registry.remove(&self.root);
        }
    }
}

struct UnregisteredRoot(PathBuf);

impl GitCommandBackend for UnregisteredRoot {
    fn execute(
        &self,
        _command: &Command,
        _input: Option<&[u8]>,
        _stdout_limit: usize,
    ) -> Result<Output, GitWorkspaceError> {
        panic!("unregistered Git command fixture: {}", self.0.display());
    }
}

pub(super) fn backend_for_root(root: &Path) -> Option<Arc<dyn GitCommandBackend>> {
    Some(
        registry()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(root)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| Arc::new(UnregisteredRoot(root.to_path_buf()))),
    )
}

mod fixtures;
pub use fixtures::{FixtureGitCommand, GitCommandFixture, fixture_git_command};
