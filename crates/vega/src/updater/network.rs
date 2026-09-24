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
    path: &Path,
    mut progress: impl FnMut(u64, u64),
) -> UpdateResult<()> {
    let archive = asset(release, ASSET, DOWNLOAD_LIMIT)?;
    let checksum = asset(release, &format!("{ASSET}.sha256"), 4096)?;
    let checksum_bytes = bounded_bytes(client, &checksum.browser_download_url, 4096).await?;
    if checksum_bytes.len() as u64 != checksum.size {
        return Err(failure("摘要下载不完整"));
    }
    let checksum_text =
        std::str::from_utf8(&checksum_bytes).map_err(|_| failure("摘要格式无效"))?;
    let fields: Vec<_> = checksum_text.split_whitespace().collect();
    if fields.len() != 2
        || fields[0].len() != 64
        || !fields[0].bytes().all(|b| b.is_ascii_hexdigit())
        || fields[1].trim_start_matches('*') != ASSET
    {
        return Err(failure("摘要格式无效"));
    }
    let mut response = response(client, &archive.browser_download_url, archive.size).await?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut hash = Sha256::new();
    let mut received = 0_u64;
    let mut last_progress = std::time::Instant::now();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("更新下载中断"))?
    {
        received += chunk.len() as u64;
        if received > archive.size {
            return Err(failure("更新下载超出大小限制"));
        }
        file.write_all(&chunk)?;
        hash.update(&chunk);
        if last_progress.elapsed() >= Duration::from_millis(250) {
            progress(received, archive.size);
            last_progress = std::time::Instant::now();
        }
    }
    file.sync_all()?;
    if received != archive.size
        || format!("{:x}", hash.finalize()) != fields[0].to_ascii_lowercase()
    {
        return Err(failure("更新下载不完整或摘要不匹配"));
    }
    progress(received, archive.size);
    Ok(())
}
