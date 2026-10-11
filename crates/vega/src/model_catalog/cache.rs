use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

#[derive(Serialize, Deserialize)]
struct Cache {
    schema: u32,
    fetched_at_ms: u64,
    digest: String,
    payload: String,
}
#[derive(Serialize, Deserialize)]
struct Mapping {
    owner: CatalogIdentity,
    target: CatalogIdentity,
}

fn read_bytes(path: &Path, limit: usize) -> CatalogResult<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|_| "模型目录缓存不可用")?;
    if !file.metadata().map_err(|_| "模型目录缓存不可用")?.is_file() {
        return Err("模型目录缓存不可用".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "模型目录缓存不可用")?;
    if bytes.len() > limit {
        return Err("模型目录缓存超过大小限制".into());
    }
    Ok(bytes)
}
fn atomic(path: &Path, bytes: &[u8]) -> CatalogResult<()> {
    let root = path.parent().ok_or("模型目录路径无效")?;
    std::fs::create_dir_all(root).map_err(|_| "模型目录缓存无法创建")?;
    let mut file = tempfile::NamedTempFile::new_in(root).map_err(|_| "模型目录缓存无法写入")?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| "模型目录缓存无法写入")?;
    file.persist(path).map_err(|_| "模型目录缓存无法保存")?;
    Ok(())
}
pub(super) fn read(root: &Path, now: u64) -> CatalogResult<ModelCatalogProjection> {
    let cache: Cache = serde_json::from_slice(&read_bytes(
        &root.join("catalog.json"),
        MAX_BYTES * 2 + 4096,
    )?)
    .map_err(|_| "模型目录缓存格式无效")?;
    if cache.schema != SCHEMA
        || cache.fetched_at_ms > now
        || format!("{:x}", Sha256::digest(cache.payload.as_bytes())) != cache.digest
    {
        return Err("模型目录缓存校验失败".into());
    }
    Ok(ModelCatalogProjection {
        providers: Arc::new(parse::parse(cache.payload.as_bytes())?),
        mapped: None,
        fetched_at_ms: Some(cache.fetched_at_ms),
        digest: Some(cache.digest),
        schema_version: SCHEMA,
        stale: now.saturating_sub(cache.fetched_at_ms) >= TTL_MS,
        refreshing: false,
        error: None,
    })
}
pub(super) fn write(root: &Path, payload: &[u8], now: u64) -> CatalogResult<()> {
    let cache = Cache {
        schema: SCHEMA,
        fetched_at_ms: now,
        digest: format!("{:x}", Sha256::digest(payload)),
        payload: String::from_utf8(payload.to_vec()).map_err(|_| "模型目录编码无效")?,
    };
    atomic(
        &root.join("catalog.json"),
        &serde_json::to_vec(&cache).map_err(|_| "模型目录缓存格式无效")?,
    )
}
pub(super) fn mapping(
    root: &Path,
    owner: &CatalogIdentity,
    action: &CatalogAction,
    catalog: &ModelCatalogProjection,
) -> CatalogResult<Option<CatalogIdentity>> {
    let path = root.join("mappings.json");
    let mut entries: Vec<Mapping> = if path.exists() {
        serde_json::from_slice(&read_bytes(&path, 1024 * 1024)?)
            .map_err(|_| "模型目录映射格式无效")?
    } else {
        Vec::new()
    };
    if let CatalogAction::Map(target) = action {
        if !catalog
            .providers
            .iter()
            .any(|p| p.id == target.provider && p.models.iter().any(|m| m.id == target.model))
        {
            return Err("请选择目录中存在的提供方与完整模型 ID".into());
        }
        entries.retain(|entry| entry.owner != *owner);
        entries.push(Mapping {
            owner: owner.clone(),
            target: target.clone(),
        });
    } else if *action == CatalogAction::Unmap {
        entries.retain(|entry| entry.owner != *owner);
    }
    if matches!(action, CatalogAction::Map(_) | CatalogAction::Unmap) {
        let bytes = serde_json::to_vec(&entries).map_err(|_| "模型目录映射格式无效")?;
        if bytes.len() > 1024 * 1024 {
            return Err("模型目录映射超过大小限制".into());
        }
        atomic(&path, &bytes)?;
    }
    Ok(entries
        .into_iter()
        .find(|entry| entry.owner == *owner)
        .map(|entry| entry.target))
}
