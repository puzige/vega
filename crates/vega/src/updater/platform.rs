use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Cursor, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::network::Version;
use super::{UpdateResult, failure};

const EXPANDED_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Bundle {
    pub path: PathBuf,
    pub version: String,
    pub team: Option<String>,
    pub installation_error: Option<String>,
}

pub(super) fn validate_target_path(target: &Path) -> UpdateResult<()> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let allowed_user = home.as_ref().is_some_and(|home| {
        target == home.join("Applications/Vega.app")
            || target == home.join("Documents/Vega/Vega.app")
    });
    if target != Path::new("/Applications/Vega.app") && !allowed_user {
        return Err(failure(vega_i18n::t("app.update.install_location")));
    }
    let mut checked = PathBuf::new();
    for component in target.components() {
        checked.push(component.as_os_str());
        if std::fs::symlink_metadata(&checked)?
            .file_type()
            .is_symlink()
        {
            return Err(failure(vega_i18n::t("app.update.symlink_path")));
        }
    }
    if target.canonicalize()? != target {
        return Err(failure(vega_i18n::t("app.update.canonical_path")));
    }
    let parent = target
        .parent()
        .ok_or_else(|| failure(vega_i18n::t("app.update.install_directory")))?;
    let metadata = std::fs::metadata(parent)?;
    // SAFETY: geteuid reads the current effective user identity without pointer arguments.
    let owner = unsafe { libc::geteuid() };
    let trusted = if parent == Path::new("/Applications") {
        metadata.uid() == 0
            && metadata.mode() & 0o002 == 0
            && (metadata.mode() & 0o020 == 0 || metadata.gid() == 80)
    } else {
        metadata.uid() == owner && metadata.mode() & 0o022 == 0
    };
    if !metadata.is_dir() || !trusted {
        return Err(failure(vega_i18n::t("app.update.permissions")));
    }
    Ok(())
}

pub(super) fn discover() -> UpdateResult<Bundle> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err(failure(vega_i18n::t("app.update.platform")));
    }
    let executable = std::env::current_exe()?;
    if executable.canonicalize()? != executable {
        return Err(failure(vega_i18n::t("app.update.executable_link")));
    }
    let path = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| failure(vega_i18n::t("app.update.development_unsupported")))?
        .to_path_buf();
    if path.extension().is_none_or(|extension| extension != "app")
        || path.join("Contents/MacOS/vega") != executable
        || plist(&path, "CFBundleIdentifier")? != "ai.vega"
    {
        return Err(failure(vega_i18n::t("app.update.development_unsupported")));
    }
    let version = plist(&path, "CFBundleShortVersionString")?;
    Version::parse(&version)?;
    let (team, installation_error) =
        match validate_target_path(&path).and_then(|_| verified_team(&path, &version)) {
            Ok(team) => (team, None),
            Err(error) => (None, Some(error.to_string())),
        };
    Ok(Bundle {
        path,
        version,
        team,
        installation_error,
    })
}

pub(super) fn command(command: &mut Command) -> UpdateResult<String> {
    let output = tempfile::tempfile()?;
    let error = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(error.try_clone()?)
        .spawn()?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(90) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failure(vega_i18n::t("app.update.verification_timeout")));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    use std::io::{Seek, SeekFrom};
    let mut result = String::new();
    for mut file in [output, error] {
        if file.metadata()?.len() > 64 * 1024 {
            return Err(failure(vega_i18n::t("app.update.verification_output")));
        }
        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut result)?;
    }
    if !status.success() {
        return Err(failure(vega_i18n::t("app.update.identity_failed")));
    }
    Ok(result)
}

fn plist(bundle: &Path, key: &str) -> UpdateResult<String> {
    let value = command(
        Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(bundle.join("Contents/Info.plist")),
    )?;
    Ok(value.trim().to_string())
}

pub(super) fn verify_integrity(bundle: &Path, version: &str) -> UpdateResult<()> {
    Version::parse(version)?;
    let architectures = command(
        Command::new("/usr/bin/lipo")
            .arg("-archs")
            .arg(bundle.join("Contents/MacOS/vega")),
    )?;
    if architectures.trim() != "arm64" {
        return Err(failure(vega_i18n::t("app.update.architecture")));
    }
    if plist(bundle, "CFBundleIdentifier")? != "ai.vega"
        || plist(bundle, "CFBundleShortVersionString")? != version
    {
        return Err(failure(vega_i18n::t("app.update.identity_version")));
    }
    command(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(bundle),
    )?;
    Ok(())
}

pub(super) fn verified_team(bundle: &Path, version: &str) -> UpdateResult<Option<String>> {
    verify_integrity(bundle, version)?;
    let signature = command(
        Command::new("/usr/bin/codesign")
            .args(["-d", "--verbose=4"])
            .arg(bundle),
    )?;
    if signature.lines().any(|line| line == "Signature=adhoc") {
        return Ok(None);
    }
    verify_identity(bundle, version, None).map(Some)
}

pub(super) fn verify_candidate(
    bundle: &Path,
    version: &str,
    expected_team: Option<&str>,
) -> UpdateResult<()> {
    if let Some(team) = expected_team {
        verify_identity(bundle, version, Some(team))?;
    } else {
        verify_integrity(bundle, version)?;
    }
    Ok(())
}

fn verify_identity(
    bundle: &Path,
    version: &str,
    expected_team: Option<&str>,
) -> UpdateResult<String> {
    verify_integrity(bundle, version)?;
    let signature = command(
        Command::new("/usr/bin/codesign")
            .args(["-d", "--verbose=4"])
            .arg(bundle),
    )?;
    let team = signature
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| {
            team.len() == 10
                && team
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        })
        .ok_or_else(|| failure(vega_i18n::t("app.update.developer_id")))?;
    if !signature
        .lines()
        .any(|line| line.starts_with("Authority=Developer ID Application:"))
        || expected_team.is_some_and(|expected| expected != team)
    {
        return Err(failure(vega_i18n::t("app.update.team_mismatch")));
    }
    let requirement = format!(
        "anchor apple generic and identifier \"ai.vega\" and certificate leaf[subject.OU] = \"{team}\""
    );
    command(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict", "-R", &requirement])
            .arg(bundle),
    )?;
    let assessment = command(
        Command::new("/usr/sbin/spctl")
            .args(["--assess", "--type", "execute", "--verbose=2"])
            .arg(bundle),
    )?;
    if !assessment
        .lines()
        .any(|line| line.trim() == "source=Notarized Developer ID")
    {
        return Err(failure(vega_i18n::t("app.update.notarization")));
    }
    Ok(team.to_string())
}

pub(super) fn extract(archive: Vec<u8>, staging: &Path) -> UpdateResult<PathBuf> {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive))
        .map_err(|_| failure(vega_i18n::t("app.update.archive_invalid")))?;
    if archive.len() > 20_000 {
        return Err(failure(vega_i18n::t("app.update.archive_entries")));
    }
    let mut names = HashSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| failure(vega_i18n::t("app.update.archive_read")))?;
        let name = entry.name().to_string();
        let path = entry
            .enclosed_name()
            .ok_or_else(|| failure(vega_i18n::t("app.update.archive_escape")))?;
        if name.split('/').any(|part| matches!(part, "." | ".."))
            || name.contains('\\')
            || name.contains('\0')
            || Path::new(&name)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || !names.insert(path.to_string_lossy().to_ascii_lowercase())
        {
            return Err(failure(vega_i18n::t("app.update.archive_paths")));
        }
        let mode = entry.unix_mode().unwrap_or(0);
        let kind = mode & 0o170000;
        if !matches!(kind, 0 | 0o040000 | 0o100000)
            || (kind == 0o040000 && !entry.is_dir())
            || (kind == 0o100000 && entry.is_dir())
        {
            return Err(failure(vega_i18n::t("app.update.archive_special")));
        }
        total = total
            .checked_add(entry.size())
            .filter(|total| *total <= EXPANDED_LIMIT)
            .ok_or_else(|| failure(vega_i18n::t("app.update.archive_expanded")))?;
        if !path.starts_with("Vega.app") {
            if path == Path::new("INSTALL.txt") && entry.size() <= 64 * 1024 && !entry.is_dir() {
                continue;
            }
            return Err(failure(vega_i18n::t("app.update.archive_unauthorized")));
        }
        let output = staging.join(&path);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&output)?;
        let expected = entry.size();
        let written = std::io::copy(&mut entry.by_ref().take(expected + 1), &mut file)?;
        if written != expected {
            return Err(failure(vega_i18n::t("app.update.archive_length")));
        }
        file.set_permissions(std::fs::Permissions::from_mode(if mode & 0o111 != 0 {
            0o755
        } else {
            0o644
        }))?;
        file.sync_all()?;
    }
    let bundle = staging.join("Vega.app");
    if !bundle.join("Contents/MacOS/vega").is_file() {
        return Err(failure(vega_i18n::t("app.update.archive_app_missing")));
    }
    Ok(bundle)
}

pub(super) fn hash(path: &Path) -> UpdateResult<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > EXPANDED_LIMIT {
        return Err(failure(vega_i18n::t("app.update.app_file")));
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(super) fn private_dir(path: &Path) -> UpdateResult<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    // SAFETY: geteuid reads the effective user identity and has no pointer arguments.
    let owner = unsafe { libc::geteuid() };
    if !metadata.is_dir() || metadata.uid() != owner || metadata.mode() & 0o077 != 0 {
        return Err(failure(vega_i18n::t("app.update.staging_permissions")));
    }
    Ok(())
}

pub(super) fn private_tempdir_in(parent: &Path, prefix: &str) -> UpdateResult<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix(prefix)
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)
        .map_err(Into::into)
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> UpdateResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
