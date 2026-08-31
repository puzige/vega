use super::*;
use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;
use std::sync::atomic::AtomicUsize;

#[path = "codec_topology.rs"]
mod codec_topology;
#[path = "commit_proof.rs"]
mod commit_proof;
#[path = "filter_gitlink.rs"]
mod filter_gitlink;
#[path = "runner_mutation.rs"]
mod runner_mutation;
#[path = "selection_noop.rs"]
mod selection_noop;
#[path = "selection_topology.rs"]
mod selection_topology;
#[path = "summary_draft.rs"]
mod summary_draft;

struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp repo");
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.name", "Vega Test"]);
        run_git(
            dir.path(),
            &["config", "user.email", "vega@example.invalid"],
        );
        fs::write(dir.path().join("tracked.txt"), "base\n").expect("write fixture");
        run_git(dir.path(), &["add", "tracked.txt"]);
        run_git(dir.path(), &["commit", "-qm", "base"]);
        Self { dir }
    }

    fn unborn() -> Self {
        let dir = tempfile::tempdir().expect("temp unborn repo");
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.name", "Vega Test"]);
        run_git(
            dir.path(),
            &["config", "user.email", "vega@example.invalid"],
        );
        Self { dir }
    }

    fn try_sha256() -> Result<Self, String> {
        let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
        let mut init = Command::new(GIT);
        init.current_dir(dir.path())
            .args(["init", "--object-format=sha256", "-q"]);
        scrub_git_environment(&mut init);
        let output = init.output().map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "git init --object-format=sha256 unsupported: status={:?}, stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        run_git(dir.path(), &["config", "user.name", "Vega Test"]);
        run_git(
            dir.path(),
            &["config", "user.email", "vega@example.invalid"],
        );
        fs::write(dir.path().join("tracked.txt"), "base\n").map_err(|error| error.to_string())?;
        run_git(dir.path(), &["add", "tracked.txt"]);
        run_git(dir.path(), &["commit", "-qm", "base"]);
        Ok(Self { dir })
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    async fn services(&self) -> (Arc<GitWorkspaceService>, TrustedGitService) {
        let workspace = Arc::new(GitWorkspaceService::new(self.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("workspace refresh");
        let trusted = TrustedGitService::new(self.path(), workspace.clone()).expect("trusted");
        (workspace, trusted)
    }
}

fn run_git(root: &Path, args: &[&str]) {
    let mut command = Command::new(GIT);
    command.current_dir(root).args(args);
    scrub_git_environment(&mut command);
    let status = command.status().expect("git fixture");
    assert!(status.success(), "git fixture failed: {args:?}");
}

fn run_git_output(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new(GIT);
    command.current_dir(root).args(args);
    scrub_git_environment(&mut command);
    let output = command.output().expect("git output fixture");
    assert!(output.status.success(), "git output failed: {args:?}");
    output.stdout
}

fn test_head(unborn: bool, width: usize) -> HeadAuthority {
    HeadAuthority {
        unborn,
        oid: vec![if unborn { b'0' } else { b'a' }; width],
        short: b"master".to_vec(),
        full_ref: b"refs/heads/master".to_vec(),
    }
}

fn status_prefix(head: &HeadAuthority) -> Vec<u8> {
    let mut bytes = b"# branch.oid ".to_vec();
    if head.unborn {
        bytes.extend_from_slice(b"(initial)");
    } else {
        bytes.extend_from_slice(&head.oid);
    }
    bytes.extend_from_slice(b"\0# branch.head ");
    bytes.extend_from_slice(&head.short);
    bytes.push(0);
    bytes
}

fn stage_record(mode: &[u8], oid: &[u8], path: &[u8]) -> Vec<u8> {
    let mut bytes = mode.to_vec();
    bytes.push(b' ');
    bytes.extend_from_slice(oid);
    bytes.extend_from_slice(b" 0\t");
    bytes.extend_from_slice(path);
    bytes.push(0);
    bytes
}

fn tree_record(mode: &[u8], object_type: &[u8], oid: &[u8], path: &[u8]) -> Vec<u8> {
    let mut bytes = mode.to_vec();
    bytes.push(b' ');
    bytes.extend_from_slice(object_type);
    bytes.push(b' ');
    bytes.extend_from_slice(oid);
    bytes.push(b'\t');
    bytes.extend_from_slice(path);
    bytes.push(0);
    bytes
}

fn status_rc_record(
    kind: u8,
    head_oid: &[u8],
    index_oid: &[u8],
    current: &[u8],
    previous: &[u8],
) -> Vec<u8> {
    let mut bytes = b"2 ".to_vec();
    bytes.push(kind);
    bytes.extend_from_slice(b". N... 100644 100644 100644 ");
    bytes.extend_from_slice(head_oid);
    bytes.push(b' ');
    bytes.extend_from_slice(index_oid);
    bytes.push(b' ');
    bytes.push(kind);
    bytes.extend_from_slice(b"100 ");
    bytes.extend_from_slice(current);
    bytes.push(0);
    bytes.extend_from_slice(previous);
    bytes.push(0);
    bytes
}

fn mutation_recorder() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("recorder tempdir");
    let script = dir.path().join("mutation-recorder.sh");
    let argv = dir.path().join("mutation-argv.bin");
    let input = dir.path().join("mutation-input.bin");
    let attempts = dir.path().join("mutation-attempts");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\nprintf x >> '{}'\n: > '{}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done\n/usr/bin/tee '{}' | /usr/bin/git \"$@\"\n",
            quote(&attempts),
            quote(&argv),
            quote(&argv),
            quote(&input),
        ),
    )
    .expect("recorder script");
    let mut permissions = fs::metadata(&script)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("script executable");
    (dir, script, argv, input)
}

fn blocking_mutation() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("blocking mutation tempdir");
    let script = dir.path().join("blocking-mutation.sh");
    let ready = dir.path().join("ready");
    let release = dir.path().join("release");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\n/usr/bin/git \"$@\"\n: > '{}'\nwhile [ ! -e '{}' ]; do /bin/sleep 0.01; done\n",
            quote(&ready),
            quote(&release),
        ),
    )
    .expect("blocking script");
    let mut permissions = fs::metadata(&script)
        .expect("blocking metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("blocking executable");
    (dir, script, ready, release)
}

fn blocking_before_mutation() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("pre-mutation tempdir");
    let script = dir.path().join("pre-mutation.sh");
    let ready = dir.path().join("ready");
    let release = dir.path().join("release");
    let attempts = dir.path().join("mutation-attempts");
    let argv = dir.path().join("mutation-argv.bin");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\nprintf x >> '{}'\n: > '{}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done\n: > '{}'\nwhile [ ! -e '{}' ]; do /bin/sleep 0.01; done\nexec /usr/bin/git \"$@\"\n",
            quote(&attempts),
            quote(&argv),
            quote(&argv),
            quote(&ready),
            quote(&release),
        ),
    )
    .expect("pre-mutation script");
    let mut permissions = fs::metadata(&script)
        .expect("pre-mutation metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("pre-mutation executable");
    (dir, script, ready, release)
}

fn fail_first_status_after_trigger(trigger: &Path) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("read fault tempdir");
    let script = dir.path().join("read-fault.sh");
    let failed = dir.path().join("failed-once");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\nis_status=0\nfor arg in \"$@\"; do [ \"$arg\" = status ] && is_status=1 || true; done\nif [ \"$is_status\" = 1 ] && [ -e '{}' ] && [ ! -e '{}' ]; then : > '{}'; exit 7; fi\nexec /usr/bin/git \"$@\"\n",
            quote(trigger),
            quote(&failed),
            quote(&failed),
        ),
    )
    .expect("read fault script");
    let mut permissions = fs::metadata(&script)
        .expect("read fault metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("read fault executable");
    (dir, script, failed)
}

fn scripted_mutation(body: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("scripted mutation tempdir");
    let script = dir.path().join("mutation.sh");
    let attempts = dir.path().join("attempts");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\nprintf x >> '{}'\n{}\n",
            quote(&attempts),
            body
        ),
    )
    .expect("scripted mutation");
    let mut permissions = fs::metadata(&script)
        .expect("scripted mutation metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("scripted mutation executable");
    (dir, script, attempts)
}

fn before_git_mutation(body: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("before-git fixture");
    let script = dir.path().join("before-git.sh");
    let attempts = dir.path().join("attempts");
    let argv = dir.path().join("argv");
    let input = dir.path().join("input");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\nprintf x >> '{}'\n: > '{}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done\n/usr/bin/tee '{}' >/dev/null\n{}\n",
            quote(&attempts),
            quote(&argv),
            quote(&argv),
            quote(&input),
            body
        ),
    )
    .expect("before-git script");
    let mut permissions = fs::metadata(&script)
        .expect("before-git metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("before-git executable");
    (dir, script, attempts, argv, input)
}

fn after_git_mutation(plan: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("after-git fixture");
    let script = dir.path().join("after-git.sh");
    let attempts = dir.path().join("attempts");
    let argv = dir.path().join("argv");
    let input = dir.path().join("input");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    let tail = match plan {
        "nonzero" => "exit 17".to_string(),
        "stdout-exact" => format!(
            "/usr/bin/python3 -c 'import sys; sys.stdout.buffer.write(b\"x\" * {})'",
            MUTATION_STDOUT_LIMIT
        ),
        "stdout-overflow" => format!(
            "/usr/bin/python3 -c 'import sys; sys.stdout.buffer.write(b\"x\" * {})'",
            MUTATION_STDOUT_LIMIT + 1
        ),
        "stderr-exact" => format!(
            "/usr/bin/python3 -c 'import sys; sys.stderr.buffer.write(b\"x\" * {})'",
            STDERR_LIMIT
        ),
        "stderr-overflow" => format!(
            "/usr/bin/python3 -c 'import sys; sys.stderr.buffer.write(b\"x\" * {})'",
            STDERR_LIMIT + 1
        ),
        "wait" => "/bin/sleep 30".to_string(),
        "inherited-pipe" => "/bin/sleep 30 & exit 0".to_string(),
        _ => panic!("unknown after-git plan"),
    };
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\nprintf x >> '{}'\n: > '{}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done\n/usr/bin/tee '{}' | /usr/bin/git \"$@\" >/dev/null\n{}\n",
            quote(&attempts),
            quote(&argv),
            quote(&argv),
            quote(&input),
            tail
        ),
    )
    .expect("after-git script");
    let mut permissions = fs::metadata(&script)
        .expect("after-git metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("after-git executable");
    (dir, script, attempts, argv, input)
}

fn proof_read_recorder(
    root: &Path,
    base_oid: &[u8],
    plan: &str,
) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("proof recorder tempdir");
    let script = dir.path().join("read-recorder.sh");
    let log = dir.path().join("read-argv.bin");
    let base = dir.path().join("base-oid");
    let attached_ref_file = dir.path().join("attached-ref");
    let status_count = dir.path().join("post-status-count");
    let root_backup = dir.path().join("root-backup");
    fs::write(&base, base_oid).expect("base oid");
    let attached_ref = run_git_output(root, &["symbolic-ref", "HEAD"]);
    fs::write(
        &attached_ref_file,
        attached_ref
            .strip_suffix(b"\n")
            .expect("attached ref newline"),
    )
    .expect("attached ref");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
set -eu
base=$(/bin/cat '{base}')
attached_ref=$(/bin/cat '{attached_ref_file}')
current=$(/usr/bin/git rev-parse --verify HEAD 2>/dev/null || true)
phase=pre
[ "$current" != "$base" ] && phase=post
printf '%s\0' "$phase" >> '{log}'
verb=
parent_arg=
for arg in "$@"; do
  printf '%s\0' "$arg" >> '{log}'
  case "$arg" in status|rev-parse|ls-tree|for-each-ref) verb="$arg" ;; esac
  case "$arg" in *'^@') parent_arg="$arg" ;; esac
done
printf '\n' >> '{log}'
if [ "$phase" = post ] && [ -n "$parent_arg" ]; then
  case '{plan}' in
zero-parent) exit 0 ;;
wrong-parent) printf '%s\n' "$current"; exit 0 ;;
two-parent) printf '%s\n%s\n' "$base" "$base"; exit 0 ;;
malformed-parent) printf 'not-an-oid\n'; exit 0 ;;
short-parent) printf '0123456789abcdef\n'; exit 0 ;;
mixed-parent) printf '%064d\n' 0; exit 0 ;;
object-missing)
  prefix=$(printf '%s' "$current" | /usr/bin/cut -c1-2)
  suffix=$(printf '%s' "$current" | /usr/bin/cut -c3-)
  object=$(/usr/bin/git rev-parse --git-path "objects/$prefix/$suffix")
  /bin/mv "$object" "$object.vega-test"
  status=0
  /usr/bin/git "$@" >/dev/null 2>&1 || status=$?
  /bin/mv "$object.vega-test" "$object"
  exit "$status"
  ;;
  esac
fi
if [ "$phase" = post ] && [ "$verb" = ls-tree ] && [ '{plan}' = tree-diff ]; then
  exec /usr/bin/git ls-tree -r -z --full-tree "$base"
fi
if [ "$phase" = post ] && [ "$verb" = status ]; then
  count=0
  [ -e '{status_count}' ] && count=$(/bin/cat '{status_count}')
  count=$((count + 1))
  printf '%s' "$count" > '{status_count}'
  if [ "$count" = 1 ] && [ '{plan}' = root-swap ]; then
/bin/mv '{root}' '{root_backup}'
/bin/mkdir '{root}'
  fi
  if [ "$count" = 2 ]; then
case '{plan}' in
  ref-moved) /usr/bin/git update-ref "$attached_ref" "$base" ;;
  ref-deleted) /usr/bin/git update-ref -d "$attached_ref" ;;
  ref-renamed) /usr/bin/git branch -m renamed-after-proof ;;
esac
  fi
fi
exec /usr/bin/git "$@"
"#,
            base = quote(&base),
            attached_ref_file = quote(&attached_ref_file),
            log = quote(&log),
            status_count = quote(&status_count),
            plan = plan,
            root = quote(root),
            root_backup = quote(&root_backup),
        ),
    )
    .expect("proof recorder script");
    let mut permissions = fs::metadata(&script)
        .expect("proof recorder metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("proof recorder executable");
    (dir, script, log)
}

fn blocking_summary_reader() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("summary reader tempdir");
    let script = dir.path().join("summary-reader.sh");
    let ready = dir.path().join("summary-drained");
    let release = dir.path().join("summary-release");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\nis_summary=false\nfor arg in \"$@\"; do [ \"$arg\" = --patch ] && is_summary=true; done\n/usr/bin/git \"$@\"\nstatus=$?\nif [ \"$is_summary\" = true ]; then : > '{}'; while [ ! -e '{}' ]; do /bin/sleep 0.01; done; fi\nexit \"$status\"\n",
            quote(&ready),
            quote(&release),
        ),
    )
    .expect("summary reader script");
    let mut permissions = fs::metadata(&script)
        .expect("summary reader metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("summary reader executable");
    (dir, script, ready, release)
}

fn read_invocations(path: &Path) -> Vec<Vec<Vec<u8>>> {
    fs::read(path)
        .expect("read invocation log")
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.split(|byte| *byte == 0)
                .filter(|field| !field.is_empty())
                .map(<[u8]>::to_vec)
                .collect()
        })
        .collect()
}

fn test_runner(root: &Path) -> Runner {
    let canonical = fs::canonicalize(root).expect("canonical test root");
    let metadata = fs::metadata(&canonical).expect("test root metadata");
    Runner::new(
        canonical,
        RootIdentity {
            dev: metadata.dev(),
            ino: metadata.ino(),
        },
        None,
    )
}

fn run_fake_mutation(
    runner: &Runner,
    verb: &'static str,
    executable: &Path,
    input: Arc<[u8]>,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<Output, GitWorkspaceError> {
    runner.run_trusted_mutation_with_executable_and_timeout(
        verb,
        &[],
        input,
        cancel,
        executable,
        timeout,
    )
}

fn mutation_error_code(result: Result<Output, GitWorkspaceError>) -> GitWorkspaceErrorCode {
    match result {
        Ok(_) => panic!("mutation unexpectedly succeeded"),
        Err(error) => error.code(),
    }
}

async fn wait_for_path(path: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("barrier ready");
}

fn expected_mutation_argv(verb: &[u8], tail: &[&[u8]]) -> Vec<u8> {
    let mut expected = Vec::new();
    for argument in PREFIX
        .iter()
        .map(|value| value.as_bytes())
        .chain([b"-c".as_slice(), b"core.hooksPath=/dev/null", verb])
        .chain(tail.iter().copied())
    {
        expected.extend_from_slice(argument);
        expected.push(0);
    }
    expected
}

fn assert_terminal_workspace(trusted: &TrustedGitService, terminal: &WorkspaceSnapshot) {
    let workspace = trusted
        .workspace
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert_eq!(workspace.snapshot.as_ref(), Some(terminal));
    assert!(workspace.active_mutation_owner.is_none());
    drop(workspace);
    let state = trusted
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert!(!state.mutation_active);
}

async fn staged_service_with_recorder() -> (
    Repo,
    tempfile::TempDir,
    TrustedGitService,
    PreparedCommit,
    PathBuf,
    PathBuf,
) {
    let repo = Repo::new();
    fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged fixture");
    run_git(repo.path(), &["add", "staged.txt"]);
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("staged refresh");
    let (recorder, script, argv, input) = mutation_recorder();
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
        .expect("trusted recorder");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("staged checklist");
    let prepared = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await
        .prepared
        .expect("staged prepared");
    assert!(!argv.exists(), "empty selection spawned add");
    (repo, recorder, trusted, prepared, argv, input)
}

async fn prepared_with_proof_plan(
    unborn: bool,
    plan: &str,
) -> (
    Repo,
    tempfile::TempDir,
    tempfile::TempDir,
    TrustedGitService,
    PreparedCommit,
    PathBuf,
    PathBuf,
    Vec<u8>,
) {
    let repo = if unborn { Repo::unborn() } else { Repo::new() };
    if unborn {
        fs::write(repo.path().join("first.txt"), "first\n").expect("unborn fixture");
    } else {
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged fixture");
        run_git(repo.path(), &["add", "staged.txt"]);
    }
    let base = if unborn {
        Vec::new()
    } else {
        run_git_output(repo.path(), &["rev-parse", "HEAD"])
            .strip_suffix(b"\n")
            .expect("base newline")
            .to_vec()
    };
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("proof workspace refresh");
    let (read_dir, read, read_log) = proof_read_recorder(repo.path(), &base, plan);
    let (mutation_dir, mutation, mutation_argv, _mutation_input) = mutation_recorder();
    let trusted =
        TrustedGitService::new_with_executables_for_test(repo.path(), workspace, mutation, read)
            .expect("trusted proof fixture");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("proof checklist");
    let selected = if unborn {
        vec![checklist.optional[0].file_id]
    } else {
        Vec::new()
    };
    let prepared = trusted
        .prepare(checklist.id, selected, CancellationToken::new())
        .await
        .prepared
        .expect("proof prepared");
    (
        repo,
        read_dir,
        mutation_dir,
        trusted,
        prepared,
        read_log,
        mutation_argv,
        base,
    )
}
