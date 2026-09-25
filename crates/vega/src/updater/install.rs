use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::platform::{self, Bundle};
use super::{UpdateResult, failure, signature};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    target: PathBuf,
    old_version: String,
    new_version: String,
    old_hash: String,
    new_hash: String,
    device: u64,
    inode: u64,
    parent_pid: u32,
}

pub(super) fn prepare(bundle: &Bundle, staging: &Path, version: &str) -> UpdateResult<()> {
    platform::validate_target_path(&bundle.path)?;
    let team = platform::verified_team(&bundle.path, &bundle.version)?;
    if team != bundle.team {
        return Err(failure("当前应用签名身份已变化，请重新启动后检查更新"));
    }
    let authenticated = signature::load(staging, version, &bundle.version)?;
    signature::archive_bytes(staging, &authenticated)?;
    let metadata = std::fs::symlink_metadata(&bundle.path)?;
    let manifest = Manifest {
        target: bundle.path.clone(),
        old_version: bundle.version.clone(),
        new_version: version.into(),
        old_hash: platform::hash(&bundle.path.join("Contents/MacOS/vega"))?,
        new_hash: String::new(),
        device: metadata.dev(),
        inode: metadata.ino(),
        parent_pid: std::process::id(),
    };
    for name in ["helper-ready", "startup-ok"] {
        let path = staging.join(name);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    let path = staging.join("install.json");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    platform::write_new(
        &path,
        &serde_json::to_vec(&manifest).map_err(|_| failure("无法准备安装记录"))?,
    )?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--vega-update-helper")
        .arg(staging)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    while !staging.join("helper-ready").exists() {
        if child.try_wait()?.is_some() || started.elapsed() > Duration::from_secs(240) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failure("更新安装程序未就绪，当前应用保持运行"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn load(staging: &Path) -> UpdateResult<Manifest> {
    platform::private_dir(staging)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(staging.join("install.json"))?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > 16 * 1024 {
        return Err(failure("安装记录无效"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|_| failure("安装记录无效"))
}

fn validate_layout(staging: &Path, manifest: &Manifest) -> UpdateResult<()> {
    platform::validate_target_path(&manifest.target)?;
    if staging.canonicalize()? != staging
        || staging.parent() != manifest.target.parent()
        || !staging
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".vega-update-"))
        || manifest
            .target
            .file_name()
            .is_none_or(|name| name != "Vega.app")
        || !manifest.target.is_absolute()
        || manifest.old_version == manifest.new_version
    {
        return Err(failure("安装路径无效"));
    }
    let old = super::network::Version::parse(&manifest.old_version)?;
    let new = super::network::Version::parse(&manifest.new_version)?;
    if new <= old {
        return Err(failure("安装版本必须较新"));
    }
    Ok(())
}

fn validate_old(manifest: &Manifest) -> UpdateResult<Option<String>> {
    let metadata = std::fs::symlink_metadata(&manifest.target)?;
    if !metadata.is_dir()
        || metadata.dev() != manifest.device
        || metadata.ino() != manifest.inode
        || manifest.target.canonicalize()? != manifest.target
        || platform::hash(&manifest.target.join("Contents/MacOS/vega"))? != manifest.old_hash
    {
        return Err(failure("原应用已变化，取消更新"));
    }
    platform::verified_team(&manifest.target, &manifest.old_version)
}

fn validate_parent(manifest: &Manifest) -> UpdateResult<()> {
    // SAFETY: getppid reads the parent process identifier and takes no pointers.
    let parent = unsafe { libc::getppid() };
    if manifest.parent_pid <= 1
        || manifest.parent_pid > i32::MAX as u32
        || parent != manifest.parent_pid as i32
    {
        return Err(failure("安装请求并非来自原应用进程"));
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut buffer = [0_u8; 4096];
        // SAFETY: proc_pidpath receives a valid writable buffer and its exact byte capacity.
        let length =
            unsafe { libc::proc_pidpath(parent, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if length <= 0 {
            return Err(failure("无法确认安装请求的来源"));
        }
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| failure("父进程路径无效"))?;
        let executable = Path::new(std::ffi::OsStr::from_bytes(&buffer[..end])).canonicalize()?;
        if executable != manifest.target.join("Contents/MacOS/vega") {
            return Err(failure("安装请求并非来自原应用"));
        }
    }
    #[cfg(not(target_os = "macos"))]
    return Err(failure("安装程序仅支持 macOS"));
    #[cfg(target_os = "macos")]
    Ok(())
}

fn helper(staging: &Path) -> UpdateResult<()> {
    let mut manifest = load(staging)?;
    validate_parent(&manifest)?;
    validate_layout(staging, &manifest)?;
    if std::env::current_exe()?.canonicalize()? != manifest.target.join("Contents/MacOS/vega") {
        return Err(failure("安装程序不属于原应用"));
    }
    let team = validate_old(&manifest)?;
    let authenticated = signature::load(staging, &manifest.new_version, &manifest.old_version)?;
    signature::archive_bytes(staging, &authenticated)?;
    let previous = staging.join("previous.app");
    if previous.exists() || staging.join("startup-ok").exists() {
        return Err(failure("安装记录已被使用"));
    }
    platform::write_new(&staging.join("helper-ready"), b"ready")?;
    let start = Instant::now();
    loop {
        // SAFETY: kill with signal zero only queries the validated numeric process identifier.
        let status = unsafe { libc::kill(manifest.parent_pid as libc::pid_t, 0) };
        if status != 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                break;
            }
            return Err(failure("无法确认旧进程已退出"));
        }
        if start.elapsed() > Duration::from_secs(90) {
            return Err(failure("应用仍未退出，取消安装"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let result = replace_and_launch(staging, &mut manifest, team.as_deref());
    if result.is_err() {
        let _ = platform::write_new(&staging.join("install-failed.txt"), b"Update did not complete. Keep this directory for recovery. If previous.app exists, quit Vega before manually restoring it. User data is unchanged.");
        if validate_old(&manifest).is_ok() {
            let _ = launch(&manifest.target, None, true);
        }
    }
    result
}

fn replace_and_launch(
    staging: &Path,
    manifest: &mut Manifest,
    team: Option<&str>,
) -> UpdateResult<()> {
    if validate_old(manifest)?.as_deref() != team {
        return Err(failure("原应用签名身份已变化"));
    }
    let authenticated = signature::load(staging, &manifest.new_version, &manifest.old_version)?;
    let archive = signature::archive_bytes(staging, &authenticated)?;
    let candidate_root = platform::private_tempdir_in(staging, "authenticated-")?;
    platform::private_dir(candidate_root.path())?;
    let candidate = platform::extract(archive, candidate_root.path())?;
    platform::verify_candidate(&candidate, &authenticated.version, team)?;
    manifest.new_hash = platform::hash(&candidate.join("Contents/MacOS/vega"))?;
    let bytes = serde_json::to_vec(manifest).map_err(|_| failure("无法保存安装身份"))?;
    let mut install_record = tempfile::NamedTempFile::new_in(staging)?;
    use std::io::Write;
    install_record.write_all(&bytes)?;
    install_record.as_file().sync_all()?;
    install_record
        .persist(staging.join("install.json"))
        .map_err(|_| failure("无法保存安装身份"))?;
    let previous = staging.join("previous.app");
    validate_old(manifest)?;
    std::fs::rename(&manifest.target, &previous)?;
    if std::fs::rename(&candidate, &manifest.target).is_err() {
        std::fs::rename(&previous, &manifest.target)?;
        return Err(failure("替换失败，已恢复原应用"));
    }
    let mut child = match launch(&manifest.target, Some(staging), false) {
        Ok(child) => child,
        Err(error) => {
            rollback(staging, manifest, team)?;
            return Err(error);
        }
    };
    let start = Instant::now();
    loop {
        if staging.join("startup-ok").is_file() {
            if platform::hash(&manifest.target.join("Contents/MacOS/vega"))? == manifest.new_hash {
                if std::fs::remove_dir_all(&previous).is_err() {
                    let _ = platform::write_new(&staging.join("cleanup-required.txt"), b"Update succeeded and the new application confirmed startup. The previous application could not be removed with current user permissions. Keep or manually remove this backup when convenient; no rollback is required.");
                    return Ok(());
                }
                if std::fs::remove_dir_all(staging).is_err() {
                    let _ = platform::write_new(&staging.join("cleanup-required.txt"), b"Update succeeded. Temporary update files could not be completely removed with current user permissions. They may be manually cleaned up; no rollback is required.");
                }
            }
            return Ok(());
        }
        if child.try_wait()?.is_some() {
            rollback(staging, manifest, team)?;
            return Err(failure("新版本未正常启动，已恢复原应用"));
        }
        if start.elapsed() > Duration::from_secs(90) {
            platform::write_new(&staging.join("recovery-required.txt"), b"Startup acknowledgment timed out. The new application is still running. Quit it before manually restoring previous.app. User data is unchanged.")?;
            return Err(failure("启动确认超时；保留运行应用和原版备份，需手动恢复"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn launch(
    target: &Path,
    staging: Option<&Path>,
    failed: bool,
) -> UpdateResult<std::process::Child> {
    let mut command = Command::new(target.join("Contents/MacOS/vega"));
    if let Some(staging) = staging {
        command.arg("--vega-update-started").arg(staging);
    }
    if failed {
        command.arg("--vega-update-failed");
    }
    Ok(command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
}

fn rollback(staging: &Path, manifest: &Manifest, team: Option<&str>) -> UpdateResult<()> {
    let failed = staging.join("failed.app");
    let previous = staging.join("previous.app");
    platform::verify_candidate(&previous, &manifest.old_version, team)?;
    if platform::hash(&previous.join("Contents/MacOS/vega"))? != manifest.old_hash {
        return Err(failure("备份应用身份已变化，保留恢复记录"));
    }
    std::fs::rename(&manifest.target, &failed)?;
    if let Err(error) = std::fs::rename(&previous, &manifest.target) {
        let _ = std::fs::rename(&failed, &manifest.target);
        return Err(Box::new(error));
    }
    Ok(())
}

pub(crate) fn run_helper_if_requested() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != "--vega-update-helper") {
        return false;
    }
    if args.len() == 3 {
        let staging = Path::new(&args[2]);
        if let Err(error) = helper(staging) {
            tracing::error!(%error, "update helper failed");
        }
    }
    true
}

pub(crate) fn acknowledge_startup() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 || args[1] != "--vega-update-started" {
        return;
    }
    let staging = PathBuf::from(&args[2]);
    let _ = std::thread::Builder::new()
        .name("vega-update-startup".into())
        .spawn(move || {
            let result = (|| -> UpdateResult<()> {
                let manifest = load(&staging)?;
                validate_layout(&staging, &manifest)?;
                let executable = std::env::current_exe()?.canonicalize()?;
                if executable != manifest.target.join("Contents/MacOS/vega")
                    || platform::hash(&executable)? != manifest.new_hash
                    || !File::open(staging.join("helper-ready"))?
                        .metadata()?
                        .is_file()
                {
                    return Err(failure("启动确认身份不匹配"));
                }
                platform::write_new(&staging.join("startup-ok"), b"ready")
            })();
            if let Err(error) = result {
                tracing::error!(%error, "update startup acknowledgment failed");
            }
        });
}
