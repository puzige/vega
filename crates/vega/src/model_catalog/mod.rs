mod cache;
mod parse;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use vega_conversation::types::*;

pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(super) const SCHEMA: u32 = 1;
pub(super) const TTL_MS: u64 = 24 * 60 * 60 * 1000;
type CatalogResult<T> = Result<T, String>;
static FILES: Mutex<()> = Mutex::new(());

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

pub(crate) fn load(
    root: &Path,
    request: &ModelCatalogRequested,
    now: u64,
) -> ModelCatalogProjection {
    let _guard = FILES.lock().unwrap_or_else(|error| error.into_inner());
    let mut result = match cache::read(root, now) {
        Ok(value) => value,
        Err(error) => ModelCatalogProjection {
            error: Some(error),
            stale: true,
            ..Default::default()
        },
    };
    match cache::mapping(root, &request.owner, &request.action, &result) {
        Ok(mapped) => result.mapped = mapped,
        Err(error) => {
            result.mapped = cache::mapping(root, &request.owner, &CatalogAction::Load, &result)
                .ok()
                .flatten();
            result.error = Some(error);
        }
    }
    result
}

pub(crate) fn refresh(root: &Path, owner: &CatalogIdentity, now: u64) -> ModelCatalogProjection {
    refresh_with(root, owner, now, || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| vega_i18n::t("app.catalog.network"))?;
        runtime.block_on(fetch())
    })
}

pub(crate) fn refresh_with(
    root: &Path,
    owner: &CatalogIdentity,
    now: u64,
    fetch: impl FnOnce() -> CatalogResult<Vec<u8>>,
) -> ModelCatalogProjection {
    let _guard = FILES.lock().unwrap_or_else(|error| error.into_inner());
    let previous = cache::read(root, now);
    let fetched = fetch().and_then(|bytes| {
        parse::parse(&bytes)?;
        cache::write(root, &bytes, now)?;
        cache::read(root, now)
    });
    let mut result = match fetched {
        Ok(value) => value,
        Err(error) => {
            let mut old = previous.unwrap_or_default();
            old.error = Some(error);
            old.stale = true;
            old
        }
    };
    match cache::mapping(root, owner, &CatalogAction::Load, &result) {
        Ok(mapped) => result.mapped = mapped,
        Err(error) => result.error = Some(error),
    }
    result
}

async fn fetch() -> CatalogResult<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent("Vega-Model-Catalog/1")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| vega_i18n::t("app.catalog.network"))?;
    let mut response = client
        .get("https://models.dev/api.json")
        .send()
        .await
        .map_err(|_| vega_i18n::t("app.catalog.fetch_failed"))?
        .error_for_status()
        .map_err(|_| vega_i18n::t("app.catalog.service_unavailable"))?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(vega_i18n::t("app.catalog.response_limit").into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| vega_i18n::t("app.catalog.response_read"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
            return Err(vega_i18n::t("app.catalog.response_limit").into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
