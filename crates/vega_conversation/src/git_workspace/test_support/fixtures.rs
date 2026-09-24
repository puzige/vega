use crate::types::GitWorkspaceErrorCode;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::os::unix::process::ExitStatusExt;
use std::sync::{OnceLock, Weak};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tempfile::TempDir;

struct Replay {
    root: PathBuf,
    entries: Vec<Value>,
    phase: usize,
}

type ReplayRegistry = Mutex<HashMap<PathBuf, Weak<Mutex<Replay>>>>;

fn registry() -> &'static ReplayRegistry {
    static REGISTRY: OnceLock<ReplayRegistry> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

pub struct GitCommandFixture {
    root: TempDir,
    _guard: crate::GitTestCommandGuard,
    _replay: Arc<Mutex<Replay>>,
}

impl GitCommandFixture {
    pub fn for_current_test(cases_json: &str, label: &str) -> Self {
        let root = tempfile::tempdir().expect(label);
        let canonical = root.path().canonicalize().unwrap();
        let current = std::thread::current();
        let name = current.name().expect("named test").replace("::", "-");
        let cases: Value = serde_json::from_str(cases_json).expect("Git fixture cases");
        thread_local! { static COUNTS: std::cell::RefCell<HashMap<String, usize>> = std::cell::RefCell::new(HashMap::new()); }
        let index = COUNTS.with(|counts| {
            let mut counts = counts.borrow_mut();
            let index = counts.entry(name.clone()).or_default();
            let result = *index;
            *index += 1;
            result
        });
        let case = cases
            .get(&name)
            .and_then(|roots| roots.get(index))
            .unwrap_or_else(|| panic!("missing Git capture for {name} root {index}"));
        let replay = Arc::new(Mutex::new(Replay {
            root: canonical.clone(),
            entries: case.as_array().unwrap().clone(),
            phase: 0,
        }));
        let callback = replay.clone();
        let guard = crate::register_git_test_executor(
            &canonical,
            Arc::new(move |command, input, _limit| {
                let args = command
                    .get_args()
                    .map(|arg| arg.to_str().expect("UTF-8 captured argument").to_owned())
                    .collect::<Vec<_>>();
                let env = command
                    .get_envs()
                    .filter(|(key, _)| {
                        ["GIT_INDEX_FILE", "GIT_DIR", "GIT_WORK_TREE"]
                            .iter()
                            .any(|name| *key == std::ffi::OsStr::new(name))
                    })
                    .map(|(k, v)| {
                        (
                            k.to_str()
                                .expect("UTF-8 captured environment key")
                                .to_owned(),
                            v.map(|v| {
                                v.to_str()
                                    .expect("UTF-8 captured environment value")
                                    .to_owned()
                            }),
                        )
                    })
                    .collect::<Vec<_>>();
                let mut replay = callback.lock().unwrap();
                let entry = replay.execute(false, &args, input, &env);
                if let Some(code) = entry.get("error").and_then(Value::as_str) {
                    let code = match code {
                        "GitFailed" => GitWorkspaceErrorCode::GitFailed,
                        "Cancelled" => GitWorkspaceErrorCode::Cancelled,
                        "NotRepository" => GitWorkspaceErrorCode::NotRepository,
                        "OutputTooLarge" => GitWorkspaceErrorCode::OutputTooLarge,
                        _ => panic!("uncaptured error code {code}"),
                    };
                    return Err(crate::types::GitWorkspaceError::for_test(code));
                }
                Ok((
                    replay
                        .expand(entry["stdout"].as_str().unwrap_or_default())
                        .into_bytes(),
                    entry["overflow"].as_bool().unwrap_or(false),
                ))
            }),
        )
        .unwrap();
        registry()
            .lock()
            .unwrap()
            .insert(canonical, Arc::downgrade(&replay));
        Self {
            root,
            _guard: guard,
            _replay: replay,
        }
    }
    pub fn path(&self) -> &std::path::Path {
        self.root.path()
    }
}

fn files(root: &std::path::Path) -> BTreeMap<String, Value> {
    fn visit(root: &std::path::Path, dir: &std::path::Path, output: &mut BTreeMap<String, Value>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if name == ".git"
                || name
                    .to_str()
                    .expect("UTF-8 captured filename")
                    .contains(".db")
            {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                visit(root, &path, output);
            } else if metadata.is_file() && metadata.len() < 65536 {
                output.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .expect("UTF-8 captured relative path")
                        .to_owned(),
                    match String::from_utf8(fs::read(path).unwrap()) {
                        Ok(text) => Value::String(text),
                        Err(error) => serde_json::json!(error.into_bytes()),
                    },
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

fn large_files(root: &std::path::Path) -> BTreeMap<String, u64> {
    fn visit(root: &std::path::Path, dir: &std::path::Path, output: &mut BTreeMap<String, u64>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if name == ".git" || name.to_str().unwrap().contains(".db") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                visit(root, &path, output);
            } else if metadata.is_file() && metadata.len() >= 65536 {
                output.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                    metadata.len(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

impl Replay {
    fn normalize(&self, value: &str) -> String {
        value
            .replace(self.root.to_str().unwrap(), "<ROOT>")
            .replace(
                self.root.to_str().unwrap().trim_start_matches("/private"),
                "<ROOT>",
            )
    }
    fn expand(&self, value: &str) -> String {
        value.replace("<ROOT>", self.root.to_str().unwrap())
    }
    fn execute(
        &mut self,
        fixture: bool,
        args: &[String],
        input: Option<&[u8]>,
        env: &[(String, Option<String>)],
    ) -> Value {
        let args = args
            .iter()
            .map(|arg| self.normalize(arg))
            .collect::<Vec<_>>();
        let input = input
            .map(|bytes| self.normalize(std::str::from_utf8(bytes).expect("UTF-8 captured stdin")));
        let env = env
            .iter()
            .map(|(k, v)| (k.clone(), v.as_ref().map(|v| self.normalize(v))))
            .collect::<Vec<_>>();
        let actual_files = files(&self.root);
        let actual_large_files = large_files(&self.root);
        let entry = self.entries.iter().find(|entry| {
            entry["phase"].as_u64() == Some(self.phase as u64)
                && entry["fixture"].as_bool().unwrap_or(false) == fixture
                && entry["args"] == serde_json::json!(args)
                && (fixture || (entry["input"] == serde_json::json!(input) && entry["env"] == serde_json::json!(env) && entry["files"] == serde_json::json!(actual_files) && entry.get("large_files").is_none_or(|expected| *expected == serde_json::json!(actual_large_files))))
        }).unwrap_or_else(|| panic!("unrecorded fixture Git command phase={} fixture={fixture} args={args:?} input={input:?} env={env:?} files={actual_files:?}", self.phase)).clone();
        if let Some(paths) = entry["remove_files"].as_array() {
            for path in paths {
                fs::remove_file(self.root.join(path.as_str().unwrap())).unwrap();
            }
        }
        if let Some(files) = entry["write_files"].as_object() {
            for (path, content) in files {
                fs::write(self.root.join(path), content.as_str().unwrap()).unwrap();
            }
        }
        if entry["advance"].as_bool().unwrap_or(false) {
            self.phase += 1;
        }
        entry
    }
}

pub struct FixtureGitCommand {
    root: PathBuf,
    args: Vec<String>,
}

pub fn fixture_git_command(root: &std::path::Path, args: &[&str]) -> FixtureGitCommand {
    FixtureGitCommand {
        root: root.canonicalize().unwrap(),
        args: args.iter().map(|s| s.to_string()).collect(),
    }
}

impl FixtureGitCommand {
    pub fn output(&self) -> std::io::Result<std::process::Output> {
        let replay = registry()
            .lock()
            .unwrap()
            .get(&self.root)
            .and_then(Weak::upgrade)
            .expect("registered Git fixture");
        let mut replay = replay.lock().unwrap();
        let entry = replay.execute(true, &self.args, None, &[]);
        if self.args.first().is_some_and(|arg| arg == "init") {
            fs::create_dir_all(self.root.join(".git"))?;
        }
        Ok(std::process::Output {
            status: std::process::ExitStatus::from_raw(if entry["success"].as_bool().unwrap() {
                0
            } else {
                256
            }),
            stdout: replay
                .expand(entry["stdout"].as_str().unwrap_or_default())
                .into_bytes(),
            stderr: Vec::new(),
        })
    }
    pub fn status(&self) -> std::io::Result<std::process::ExitStatus> {
        self.output().map(|out| out.status)
    }
}

impl Drop for GitCommandFixture {
    fn drop(&mut self) {
        registry()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(
                &self
                    ._replay
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .root,
            );
    }
}
