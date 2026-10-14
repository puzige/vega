use std::io::Write;
use std::path::Path;
use std::time::Duration;

use reqwest::{Client, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{UpdateResult, failure};

pub(super) const ASSET: &str = "Vega-macos-arm64.zip";
const METADATA_LIMIT: u64 = 1024 * 1024;
pub(super) const DOWNLOAD_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Version(pub [u64; 3]);

impl Version {
    pub(super) fn parse(value: &str) -> UpdateResult<Self> {
        let parts: Vec<_> = value.split('.').collect();
        if parts.len() != 3 {
            return Err(failure("版本格式不是稳定版本"));
        }
        let mut numbers = [0; 3];
        for (index, part) in parts.into_iter().enumerate() {
            if part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return Err(failure("版本格式不是稳定版本"));
            }
            numbers[index] = part.parse().map_err(|_| failure("版本数字超出范围"))?;
        }
        Ok(Self(numbers))
    }
}

#[derive(Deserialize)]
pub(super) struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    #[serde(default)]
    pub body: Option<String>,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
pub(super) struct Asset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
}

pub(super) fn client() -> UpdateResult<Client> {
    Client::builder()
        .user_agent("Vega-Updater")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !allowed_redirect(attempt.url()) {
                attempt.error("更新资产重定向地址不受信任")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| failure("无法初始化更新网络连接"))
}

fn allowed_redirect(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "api.github.com"
                    | "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}

pub(super) async fn latest(client: &Client) -> UpdateResult<Release> {
    let bytes = bounded_bytes(
        client,
        "https://api.github.com/repos/puzige/vega/releases/latest",
        METADATA_LIMIT,
    )
    .await?;
    let release: Release = serde_json::from_slice(&bytes).map_err(|_| failure("发布元数据无效"))?;
    if release.draft || release.prerelease {
        return Err(failure("发布不是稳定版本"));
    }
    Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )?;
    Ok(release)
}

pub(super) fn asset<'a>(release: &'a Release, name: &str, limit: u64) -> UpdateResult<&'a Asset> {
    let mut matches = release.assets.iter().filter(|asset| asset.name == name);
    let asset = matches.next().ok_or_else(|| failure("发布缺少更新资产"))?;
    if matches.next().is_some() || asset.size == 0 || asset.size > limit {
        return Err(failure("更新资产大小或名称无效"));
    }
    let url = Url::parse(&asset.browser_download_url).map_err(|_| failure("更新资产地址无效"))?;
    let expected = format!(
        "/puzige/vega/releases/download/{}/{}",
        release.tag_name, name
    );
    if !allowed_redirect(&url)
        || url.host_str() != Some("github.com")
        || url.path() != expected
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(failure("更新资产不是该仓库的正式发布"));
    }
    Ok(asset)
}

async fn response(client: &Client, url: &str, limit: u64) -> UpdateResult<reqwest::Response> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| failure("更新请求失败，请检查网络后重试"))?;
    if !response.status().is_success() {
        return Err(failure("更新服务暂不可用或已限流，请稍后重试"));
    }
    if response.content_length().is_some_and(|size| size > limit) {
        return Err(failure("更新响应超出大小限制"));
    }
    Ok(response)
}

async fn bounded_bytes(client: &Client, url: &str, limit: u64) -> UpdateResult<Vec<u8>> {
    let mut response = response(client, url, limit).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("更新响应读取失败"))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(failure("更新响应超出大小限制"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) async fn download(
    client: &Client,
    release: &Release,
    staging: &Path,
    version: &str,
    bundle: &super::platform::Bundle,
    allow_delta: bool,
    mut progress: impl FnMut(u64, u64),
) -> UpdateResult<bool> {
    use super::signature;
    let archive = asset(release, ASSET, DOWNLOAD_LIMIT)?;
    let manifest_asset = asset(release, signature::MANIFEST_NAME, signature::MANIFEST_LIMIT)
        .map_err(|_| failure("此发布缺少独立更新签名，请从官方发布页手动安装"))?;
    let signature_asset = asset(
        release,
        signature::SIGNATURE_NAME,
        signature::SIGNATURE_LIMIT,
    )
    .map_err(|_| failure("此发布缺少独立更新签名，请从官方发布页手动安装"))?;
    let manifest_bytes = bounded_bytes(
        client,
        &manifest_asset.browser_download_url,
        signature::MANIFEST_LIMIT,
    )
    .await?;
    let signature_bytes = bounded_bytes(
        client,
        &signature_asset.browser_download_url,
        signature::SIGNATURE_LIMIT,
    )
    .await?;
    if manifest_bytes.len() as u64 != manifest_asset.size
        || signature_bytes.len() as u64 != signature_asset.size
    {
        return Err(failure("更新签名下载不完整"));
    }
    let manifest = signature::verify(&manifest_bytes, &signature_bytes, version, &bundle.version)?;
    if archive.size != manifest.size {
        return Err(failure("发布资产大小与签名不匹配"));
    }
    for name in [signature::MANIFEST_NAME, signature::SIGNATURE_NAME] {
        let path = staging.join(name);
        if path.try_exists()? {
            std::fs::remove_file(path)?;
        }
    }
    super::platform::write_new(&staging.join(signature::MANIFEST_NAME), &manifest_bytes)?;
    super::platform::write_new(&staging.join(signature::SIGNATURE_NAME), &signature_bytes)?;
    let progress = std::cell::RefCell::new(&mut progress);
    choose_download(
        allow_delta,
        async || {
            try_delta(
                client,
                release,
                staging,
                &manifest,
                bundle,
                &mut |received, total| {
                    (progress.borrow_mut())(received, total);
                },
            )
            .await
        },
        async || {
            let path = staging.join(ASSET);
            if path.try_exists()? {
                std::fs::remove_file(path)?;
            }
            download_asset(
                client,
                archive,
                staging,
                manifest.size,
                &manifest.sha256,
                |received, total| {
                    (progress.borrow_mut())(received, total);
                },
            )
            .await?;
            let candidate_root = super::platform::private_tempdir_in(staging, "verified-full-")?;
            let candidate = super::platform::extract(
                signature::archive_bytes(staging, &manifest)?,
                candidate_root.path(),
            )?;
            super::platform::verify_candidate(&candidate, version, bundle.team.as_deref())
        },
    )
    .await
}

async fn choose_download(
    allow_delta: bool,
    mut incremental: impl AsyncFnMut() -> UpdateResult<()>,
    mut full: impl AsyncFnMut() -> UpdateResult<()>,
) -> UpdateResult<bool> {
    if allow_delta {
        match incremental().await {
            Ok(()) => return Ok(true),
            Err(error) => {
                tracing::info!(%error, "incremental update unavailable; downloading full archive")
            }
        }
    }
    full().await?;
    Ok(false)
}

async fn try_delta(
    client: &Client,
    release: &Release,
    staging: &Path,
    full: &super::signature::SignedManifest,
    bundle: &super::platform::Bundle,
    progress: &mut impl FnMut(u64, u64),
) -> UpdateResult<()> {
    use super::{delta, platform, signature};
    let metadata = asset(release, delta::MANIFEST_NAME, delta::MANIFEST_LIMIT)?;
    let signed = asset(release, delta::SIGNATURE_NAME, signature::SIGNATURE_LIMIT)?;
    let bytes = bounded_bytes(
        client,
        &metadata.browser_download_url,
        delta::MANIFEST_LIMIT,
    )
    .await?;
    let signature = bounded_bytes(
        client,
        &signed.browser_download_url,
        signature::SIGNATURE_LIMIT,
    )
    .await?;
    if bytes.len() as u64 != metadata.size || signature.len() as u64 != signed.size {
        return Err(failure("增量签名下载不完整"));
    }
    let manifest = delta::verify(&bytes, &signature, full)?;
    let patch = manifest.patch(&bundle.version)?;
    delta::validate_reuse(&manifest, patch, &bundle.path)?;
    let archive = asset(release, &patch.archive, DOWNLOAD_LIMIT)?;
    if archive.size != patch.size {
        return Err(failure("增量资产大小与签名不匹配"));
    }
    let attempt = platform::private_tempdir_in(staging, "delta-attempt-")?;
    platform::write_new(&attempt.path().join(delta::MANIFEST_NAME), &bytes)?;
    platform::write_new(&attempt.path().join(delta::SIGNATURE_NAME), &signature)?;
    download_asset(
        client,
        archive,
        attempt.path(),
        patch.size,
        &patch.sha256,
        progress,
    )
    .await?;
    let candidate = delta::reconstruct(
        attempt.path(),
        &bundle.path,
        attempt.path(),
        full,
        &bundle.version,
    )?;
    platform::verify_candidate(&candidate, &full.version, bundle.team.as_deref())?;
    for name in [delta::MANIFEST_NAME, delta::SIGNATURE_NAME, &patch.archive] {
        std::fs::rename(attempt.path().join(name), staging.join(name))?;
    }
    Ok(())
}

async fn download_asset(
    client: &Client,
    archive: &Asset,
    staging: &Path,
    size: u64,
    sha256: &str,
    mut progress: impl FnMut(u64, u64),
) -> UpdateResult<()> {
    progress(0, size);
    let mut response = response(client, &archive.browser_download_url, size).await?;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging.join(&archive.name))?;
    let mut hash = Sha256::new();
    let mut received = 0_u64;
    let mut last_progress = std::time::Instant::now();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("更新下载中断"))?
    {
        received += chunk.len() as u64;
        if received > size {
            return Err(failure("更新下载超出大小限制"));
        }
        file.write_all(&chunk)?;
        hash.update(&chunk);
        if last_progress.elapsed() >= Duration::from_millis(250) {
            progress(received, size);
            last_progress = std::time::Instant::now();
        }
    }
    file.sync_all()?;
    if received != size || format!("{:x}", hash.finalize()) != sha256 {
        return Err(failure("更新下载不完整或签名摘要不匹配"));
    }
    progress(received, size);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn download_selection_falls_back_once_and_preserves_full_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        for (allow, delta_ok, full_ok, expected_delta, expected_full) in [
            (true, true, true, 1, 0),
            (true, false, true, 1, 1),
            (true, false, false, 1, 1),
            (false, false, true, 0, 1),
        ] {
            let delta_calls = Cell::new(0);
            let full_calls = Cell::new(0);
            let result = runtime.block_on(choose_download(
                allow,
                async || {
                    delta_calls.set(delta_calls.get() + 1);
                    if delta_ok {
                        Ok(())
                    } else {
                        Err(failure("delta rejected"))
                    }
                },
                async || {
                    full_calls.set(full_calls.get() + 1);
                    if full_ok {
                        Ok(())
                    } else {
                        Err(failure("full rejected"))
                    }
                },
            ));
            assert_eq!(delta_calls.get(), expected_delta);
            assert_eq!(full_calls.get(), expected_full);
            if !full_ok {
                assert_eq!(result.unwrap_err().to_string(), "full rejected");
            } else {
                assert_eq!(result.unwrap(), allow && delta_ok);
            }
        }
    }

    #[test]
    fn delta_asset_requires_exact_release_repository_and_bounded_size() {
        let mut release = Release { tag_name: "v1.0.2".into(), draft: false, prerelease: false, body: None,
            assets: vec![Asset { name: "Vega-delta-1.0.0-to-1.0.2.zip".into(), size: 10,
                browser_download_url: "https://github.com/puzige/vega/releases/download/v1.0.2/Vega-delta-1.0.0-to-1.0.2.zip".into() }] };
        let name = release.assets[0].name.clone();
        assert!(asset(&release, &name, DOWNLOAD_LIMIT).is_ok());
        release.assets[0].browser_download_url = release.assets[0]
            .browser_download_url
            .replace("/puzige/", "/other/");
        assert!(asset(&release, &name, DOWNLOAD_LIMIT).is_err());
    }
}
