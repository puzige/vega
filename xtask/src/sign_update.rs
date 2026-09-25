use std::fs::{self, File};
use std::io::Read;

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::Serialize;
use sha2::{Digest, Sha256};

const ARCHIVE: &str = "Vega-macos-arm64.zip";
const LIMIT: u64 = 512 * 1024 * 1024;
const DOMAIN: &[u8] = b"Vega update manifest v1\n";
const PUBLIC_KEY: &str = include_str!("../../assets/update-public-key.hex");

#[derive(Serialize)]
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
