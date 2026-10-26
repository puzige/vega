use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use base64::Engine;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{UpdateResult, failure, network};

pub(super) const MANIFEST_NAME: &str = "Vega-update.json";
pub(super) const SIGNATURE_NAME: &str = "Vega-update.json.sig";
pub(super) const MANIFEST_LIMIT: u64 = 16 * 1024;
pub(super) const SIGNATURE_LIMIT: u64 = 256;
const DOMAIN: &[u8] = b"Vega update manifest v1\n";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SignedManifest {
    schema: u32,
    pub version: String,
    bundle_id: String,
    target: String,
    archive: String,
    pub size: u64,
    pub sha256: String,
}

pub(super) fn verify(
    bytes: &[u8],
    signature: &[u8],
    expected_version: &str,
    current_version: &str,
) -> UpdateResult<SignedManifest> {
    if bytes.is_empty()
        || bytes.len() as u64 > MANIFEST_LIMIT
        || signature.len() as u64 > SIGNATURE_LIMIT
    {
        return Err(failure(vega_i18n::t("app.update.signature_limit")));
    }
    verify_signature(bytes, signature, DOMAIN)?;
    let manifest: SignedManifest = serde_json::from_slice(bytes)
        .map_err(|_| failure(vega_i18n::t("app.update.manifest_format")))?;
    if manifest.schema != 1
        || manifest.bundle_id != "ai.vega"
        || manifest.target != "aarch64-apple-darwin"
        || manifest.archive != network::ASSET
        || manifest.version != expected_version
        || manifest.size == 0
        || manifest.size > network::DOWNLOAD_LIMIT
        || manifest.sha256.len() != 64
        || !manifest
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || network::Version::parse(&manifest.version)? <= network::Version::parse(current_version)?
    {
        return Err(failure(vega_i18n::t("app.update.manifest_identity")));
    }
    Ok(manifest)
}

pub(super) fn verify_signature(bytes: &[u8], signature: &[u8], domain: &[u8]) -> UpdateResult<()> {
    let encoded = std::str::from_utf8(signature)
        .map_err(|_| failure(vega_i18n::t("app.update.signature_format")))?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| failure(vega_i18n::t("app.update.signature_format")))?;
    if signature.len() != 64 {
        return Err(failure(vega_i18n::t("app.update.signature_length")));
    }
    let hex = include_str!("../../../../assets/update-public-key.hex").trim();
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(failure(vega_i18n::t("app.update.public_key")));
    }
    let mut key = [0_u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| failure(vega_i18n::t("app.update.public_key")))?;
    }
    let mut message = Vec::with_capacity(domain.len() + bytes.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(bytes);
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&message, &signature)
        .map_err(|_| failure(vega_i18n::t("app.update.signature_rejected")))?;
    Ok(())
}

pub(super) fn load(
    staging: &Path,
    expected_version: &str,
    current_version: &str,
) -> UpdateResult<SignedManifest> {
    let manifest = bounded_file(&staging.join(MANIFEST_NAME), MANIFEST_LIMIT)?;
    let signature = bounded_file(&staging.join(SIGNATURE_NAME), SIGNATURE_LIMIT)?;
    verify(&manifest, &signature, expected_version, current_version)
}

pub(super) fn bounded_file(path: &Path, limit: u64) -> UpdateResult<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(failure(vega_i18n::t("app.update.file_invalid")));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(metadata.len() as usize)
        .map_err(|_| failure(vega_i18n::t("app.update.memory")))?;
    file.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit || bytes.len() as u64 != metadata.len() {
        return Err(failure(vega_i18n::t("app.update.file_changed")));
    }
    Ok(bytes)
}

pub(super) fn archive_bytes(staging: &Path, manifest: &SignedManifest) -> UpdateResult<Vec<u8>> {
    let bytes = bounded_file(&staging.join(network::ASSET), manifest.size)?;
    if bytes.len() as u64 != manifest.size
        || format!("{:x}", Sha256::digest(&bytes)) != manifest.sha256
    {
        return Err(failure(vega_i18n::t("app.update.archive_signature")));
    }
    Ok(bytes)
}
