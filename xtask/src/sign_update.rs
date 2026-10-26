use std::fs::{self, File};
use std::io::Read;

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ARCHIVE: &str = "Vega-macos-arm64.zip";
const LIMIT: u64 = 512 * 1024 * 1024;
const DOMAIN: &[u8] = b"Vega update manifest v1\n";
const PUBLIC_KEY: &str = include_str!("../../assets/update-public-key.hex");

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest<'a> {
    schema: u8,
    version: &'a str,
    bundle_id: &'a str,
    target: &'a str,
    archive: &'a str,
    size: u64,
    sha256: String,
}

pub fn run(args: &[String]) -> Result<()> {
    let [flag, version] = args else {
        bail!("usage: cargo xtask sign-update --version <MAJOR.MINOR.PATCH>");
    };
    if flag != "--version" {
        bail!("sign-update requires --version");
    }
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
                || part.parse::<u64>().is_err()
        })
    {
        bail!("update version must contain three canonical unsigned 64-bit components");
    }
    let key = signing_key()?;
    let dist = crate::workspace_root()?.join("dist");
    let path = dist.join(ARCHIVE);
    let metadata = fs::symlink_metadata(&path).context("update archive is missing")?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > LIMIT {
        bail!("update archive size or type is invalid");
    }
    let mut archive = File::open(&path)?.take(LIMIT + 1);
    let mut size = 0_u64;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = archive.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        hash.update(&buffer[..count]);
    }
    if size != metadata.len() || size > LIMIT {
        bail!("update archive changed during signing");
    }
    let manifest = serde_json::to_vec(&Manifest {
        schema: 1,
        version,
        bundle_id: "ai.vega",
        target: "aarch64-apple-darwin",
        archive: ARCHIVE,
        size,
        sha256: format!("{:x}", hash.finalize()),
    })?;
    let mut message = DOMAIN.to_vec();
    message.extend_from_slice(&manifest);
    let signature = key.sign(&message);
    fs::write(dist.join("Vega-update.json"), manifest)?;
    fs::write(
        dist.join("Vega-update.json.sig"),
        format!("{}\n", STANDARD.encode(signature.as_ref())),
    )?;
    println!("signed update manifest for {version}");
    Ok(())
}

fn signing_key() -> Result<Ed25519KeyPair> {
    let encoded = std::env::var("VEGA_UPDATE_PRIVATE_KEY")
        .map_err(|_| anyhow::anyhow!("update signing key is missing"))?;
    if encoded.len() > 4096 {
        bail!("update signing key is invalid");
    }
    let der = STANDARD
        .decode(encoded.trim())
        .map_err(|_| anyhow::anyhow!("update signing key encoding is invalid"))?;
    let key = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&der)
        .map_err(|_| anyhow::anyhow!("update signing key is invalid"))?;
    let pin = PUBLIC_KEY.trim();
    if pin.len() != 64 || !pin.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("pinned update public key is invalid");
    }
    let expected: Vec<u8> = (0..64)
        .step_by(2)
        .map(|index| u8::from_str_radix(&pin[index..index + 2], 16))
        .collect::<std::result::Result<_, _>>()
        .context("pinned update public key is invalid")?;
    if key.public_key().as_ref() != expected {
        bail!("update signing key does not match repository public key");
    }
    Ok(key)
}

pub fn sign_delta(args: &[String]) -> Result<()> {
    if !args.is_empty() {
        bail!("usage: cargo xtask sign-delta");
    }
    let dist = crate::workspace_root()?.join("dist");
    let manifest = read_bounded(&dist.join("Vega-delta.json"), 1024 * 1024)?;
    if manifest.len() > 1024 * 1024 {
        bail!("delta manifest exceeds size limit");
    }
    let mut message = b"Vega file delta manifest v1\n".to_vec();
    message.extend_from_slice(&manifest);
    let signature = signing_key()?.sign(&message);
    fs::write(
        dist.join("Vega-delta.json.sig"),
        format!("{}\n", STANDARD.encode(signature)),
    )?;
    Ok(())
}

pub fn verify_update(args: &[String]) -> Result<()> {
    let [directory, version] = args else {
        bail!("usage: cargo xtask verify-update <directory> <version>");
    };
    let directory = std::path::Path::new(directory);
    let raw = read_bounded(&directory.join("Vega-update.json"), 64 * 1024)?;
    let signature_bytes = read_bounded(&directory.join("Vega-update.json.sig"), 256)?;
    let signature = std::str::from_utf8(&signature_bytes)?;
    if raw.len() > 64 * 1024 || signature.len() > 256 {
        bail!("update metadata exceeds size limits");
    }
    let pin = PUBLIC_KEY.trim();
    let public: Vec<u8> = (0..pin.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&pin[index..index + 2], 16))
        .collect::<std::result::Result<_, _>>()?;
    verify_signature(DOMAIN, &public, &raw, signature)?;
    let manifest: Manifest<'_> = serde_json::from_slice(&raw)?;
    if manifest.schema != 1
        || manifest.version != version
        || manifest.bundle_id != "ai.vega"
        || manifest.target != "aarch64-apple-darwin"
        || manifest.archive != ARCHIVE
        || manifest.size == 0
        || manifest.size > LIMIT
    {
        bail!("update manifest identity mismatch");
    }
    let path = directory.join(ARCHIVE);
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.len() != manifest.size {
        bail!("update archive size mismatch");
    }
    let mut reader = File::open(path)?.take(LIMIT + 1);
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if format!("{:x}", hash.finalize()) != manifest.sha256 {
        bail!("update archive hash mismatch");
    }
    Ok(())
}

fn read_bounded(path: &std::path::Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("update metadata exceeds size limits");
    }
    Ok(bytes)
}

fn verify_signature(domain: &[u8], public: &[u8], raw: &[u8], signature: &str) -> Result<()> {
    let mut message = domain.to_vec();
    message.extend_from_slice(raw);
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public)
        .verify(&message, &STANDARD.decode(signature.trim())?)
        .map_err(|_| anyhow::anyhow!("update signature mismatch"))
}
