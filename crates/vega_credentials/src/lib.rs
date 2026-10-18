mod proxy;
pub use proxy::ProxyLease;

use reqwest::Url;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("credential binding is invalid")]
    InvalidBinding,
    #[error("credential authorization is unavailable or expired")]
    Unavailable,
    #[error("credential destination is not authorized")]
    DestinationDenied,
    #[error("credential proxy protocol is unsupported")]
    Protocol,
    #[error("credential proxy is unavailable")]
    ProxyUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialBinding {
    pub key_ref: String,
    pub env: String,
    pub origin: String,
    pub header: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default = "root_path")]
    pub path: String,
}

fn root_path() -> String {
    "/".into()
}

impl CredentialBinding {
    pub fn validate(&self) -> Result<(), CredentialError> {
        let url = Url::parse(&self.origin).map_err(|_| CredentialError::InvalidBinding)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || self.key_ref.is_empty()
            || self.env.is_empty()
            || !self
                .env
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            || self.env.as_bytes()[0].is_ascii_digit()
            || matches!(
                self.env.as_str(),
                "PATH"
                    | "HOME"
                    | "SHELL"
                    | "ENV"
                    | "BASH_ENV"
                    | "ZDOTDIR"
                    | "LD_PRELOAD"
                    | "DYLD_INSERT_LIBRARIES"
            )
            || self.env.contains("PROXY")
            || self.env.contains("CERT")
            || self.env.starts_with("DYLD_")
            || !matches!(
                self.header.to_ascii_lowercase().as_str(),
                "authorization" | "x-api-key" | "api-key" | "private-token"
            )
            || !self.prefix.bytes().all(|byte| (32..127).contains(&byte))
            || !self.path.starts_with('/')
            || self.path.contains(['%', '\\', '?', '#'])
            || self.path.split('/').any(|part| part == "." || part == "..")
        {
            return Err(CredentialError::InvalidBinding);
        }
        Ok(())
    }

    fn permits(&self, url: &Url) -> bool {
        let Ok(origin) = Url::parse(&self.origin) else {
            return false;
        };
        url.scheme() == "https"
            && url.host_str() == origin.host_str()
            && url.port_or_known_default() == origin.port_or_known_default()
            && url.username().is_empty()
            && url.password().is_none()
            && (self.path == "/"
                || url.path() == self.path
                || url
                    .path()
                    .strip_prefix(self.path.trim_end_matches('/'))
                    .is_some_and(|rest| rest.starts_with('/')))
    }
}

pub type SecretReader = Arc<dyn Fn(&str) -> Result<String, CredentialError> + Send + Sync>;

struct Record {
    binding: CredentialBinding,
    alias: String,
    secret: String,
}

pub struct CredentialBroker {
    records: Vec<Record>,
    reader: SecretReader,
    revoked: AtomicBool,
    expires: Instant,
    lease_token: String,
}

fn registered_aliases() -> &'static Mutex<BTreeSet<String>> {
    static ALIASES: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    ALIASES.get_or_init(|| Mutex::new(BTreeSet::new()))
}

pub fn is_registered_alias(value: &str) -> bool {
    registered_aliases()
        .lock()
        .is_ok_and(|aliases| aliases.contains(value))
}

impl Drop for CredentialBroker {
    fn drop(&mut self) {
        if let Ok(mut aliases) = registered_aliases().lock() {
            for record in &self.records {
                aliases.remove(&record.alias);
            }
        }
    }
}

pub fn random_token() -> Result<String, CredentialError> {
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| CredentialError::Unavailable)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

impl CredentialBroker {
    pub fn new(
        bindings: &[CredentialBinding],
        reader: SecretReader,
    ) -> Result<Arc<Self>, CredentialError> {
        let mut records: Vec<Record> = Vec::new();
        for binding in bindings {
            binding.validate()?;
            if records
                .iter()
                .any(|record| record.binding.env == binding.env)
            {
                return Err(CredentialError::InvalidBinding);
            }
            let secret = reader(&binding.key_ref)?;
            if secret.is_empty() || secret.contains(['\r', '\n']) {
                return Err(CredentialError::Unavailable);
            }
            let alias = format!("vega_alias_{}", random_token()?);
            if alias == secret {
                return Err(CredentialError::Unavailable);
            }
            records.push(Record {
                binding: binding.clone(),
                alias,
                secret,
            });
        }
        if let Ok(mut aliases) = registered_aliases().lock() {
            for record in &records {
                aliases.insert(record.alias.clone());
            }
        } else {
            return Err(CredentialError::Unavailable);
        }
        Ok(Arc::new(Self {
            records,
            reader,
            revoked: AtomicBool::new(false),
            expires: Instant::now() + Duration::from_secs(3600),
            lease_token: random_token()?,
        }))
    }

    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    pub fn environment(&self) -> Result<BTreeMap<String, String>, CredentialError> {
        self.check_active()?;
        Ok(self
            .records
            .iter()
            .map(|record| (record.binding.env.clone(), record.alias.clone()))
            .collect())
    }

    fn check_active(&self) -> Result<(), CredentialError> {
        if self.revoked.load(Ordering::Acquire) || Instant::now() >= self.expires {
            return Err(CredentialError::Unavailable);
        }
        Ok(())
    }

    fn inject(
        &self,
        url: &Url,
        headers: &mut reqwest::header::HeaderMap,
    ) -> Result<(), CredentialError> {
        self.check_active()?;
        for record in &self.records {
            let expected = format!("{}{}", record.binding.prefix, record.alias);
            let Some(value) = headers.get(&record.binding.header) else {
                continue;
            };
            if value.as_bytes() != expected.as_bytes() {
                continue;
            }
            if !record.binding.permits(url) {
                return Err(CredentialError::DestinationDenied);
            }
            let current = (self.reader)(&record.binding.key_ref)?;
            if current != record.secret {
                return Err(CredentialError::Unavailable);
            }
            let value = reqwest::header::HeaderValue::from_str(&format!(
                "{}{}",
                record.binding.prefix, current
            ))
            .map_err(|_| CredentialError::Unavailable)?;
            let name = reqwest::header::HeaderName::from_bytes(record.binding.header.as_bytes())
                .map_err(|_| CredentialError::InvalidBinding)?;
            headers.insert(name, value);
        }
        if headers.values().any(|value| {
            value
                .as_bytes()
                .windows(b"vega_alias_".len())
                .any(|window| window == b"vega_alias_")
        }) {
            return Err(CredentialError::DestinationDenied);
        }
        Ok(())
    }

    fn permits_host(&self, url: &Url) -> bool {
        self.records.iter().any(|record| {
            Url::parse(&record.binding.origin).is_ok_and(|origin| {
                url.host_str() == origin.host_str()
                    && url.port_or_known_default() == origin.port_or_known_default()
            })
        })
    }

    pub fn redact(&self, bytes: &[u8]) -> Vec<u8> {
        let mut records = self.records.iter().collect::<Vec<_>>();
        records.sort_by_key(|record| std::cmp::Reverse(record.secret.len()));
        let mut output = bytes.to_vec();
        for record in records {
            let mut forms = vec![record.secret.clone()];
            if let Ok(encoded) = serde_json::to_string(&record.secret) {
                forms.push(encoded[1..encoded.len() - 1].to_string());
            }
            forms.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
            forms.dedup();
            for secret in forms {
                output = replace_bytes(&output, secret.as_bytes(), record.alias.as_bytes());
            }
            output =
                replace_percent_encoded(&output, record.secret.as_bytes(), record.alias.as_bytes());
        }
        output
    }
}

fn replace_bytes(input: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return input.to_vec();
    }
    let mut output = Vec::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        if input[cursor..].starts_with(needle) {
            output.extend_from_slice(replacement);
            cursor += needle.len();
        } else {
            output.push(input[cursor]);
            cursor += 1;
        }
    }
    output
}

fn replace_percent_encoded(input: &[u8], secret: &[u8], replacement: &[u8]) -> Vec<u8> {
    fn hex(byte: u8) -> Option<u8> {
        (byte as char).to_digit(16).map(|value| value as u8)
    }
    let mut output = Vec::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let mut end = cursor;
        let matched = secret.iter().all(|expected| {
            if input.get(end) == Some(&b'%')
                && let (Some(high), Some(low)) = (
                    input.get(end + 1).and_then(|byte| hex(*byte)),
                    input.get(end + 2).and_then(|byte| hex(*byte)),
                )
                && high * 16 + low == *expected
            {
                end += 3;
                true
            } else if input.get(end) == Some(expected) {
                end += 1;
                true
            } else {
                false
            }
        });
        if matched && end > cursor {
            output.extend_from_slice(replacement);
            cursor = end;
        } else {
            output.push(input[cursor]);
            cursor += 1;
        }
    }
    output
}

pub struct ProcessPolicy {
    pub root: std::path::PathBuf,
    pub bindings: Vec<CredentialBinding>,
    pub reader: SecretReader,
}

pub struct ProtectedProcess {
    lease: Option<ProxyLease>,
    directory: std::path::PathBuf,
    environment: BTreeMap<String, String>,
    protected_root: std::path::PathBuf,
}

impl ProtectedProcess {
    pub async fn start(policy: &ProcessPolicy) -> Result<Self, CredentialError> {
        use std::os::unix::fs::DirBuilderExt;
        let directory = std::env::temp_dir().join(format!("vega-credential-{}", random_token()?));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|_| CredentialError::ProxyUnavailable)?;
        let directory = directory
            .canonicalize()
            .map_err(|_| CredentialError::ProxyUnavailable)?;
        let result = async {
            let lease = if policy.bindings.is_empty() {
                None
            } else {
                Some(
                    ProxyLease::start(
                        CredentialBroker::new(&policy.bindings, policy.reader.clone())?,
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await?,
                )
            };
            let mut environment = BTreeMap::new();
            if let Some(lease) = &lease {
                let ca_path = directory.join("ca.pem");
                std::fs::write(&ca_path, &lease.ca_pem)
                    .map_err(|_| CredentialError::ProxyUnavailable)?;
                environment = lease
                    .environment(ca_path.to_str().ok_or(CredentialError::ProxyUnavailable)?)?;
            }
            environment.insert(
                "PATH".into(),
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".into(),
            );
            environment.insert("HOME".into(), directory.to_string_lossy().into_owned());
            environment.insert("TMPDIR".into(), directory.to_string_lossy().into_owned());
            environment.insert("LANG".into(), "en_US.UTF-8".into());
            Ok(Self {
                lease,
                directory: directory.clone(),
                environment,
                protected_root: policy
                    .root
                    .canonicalize()
                    .map_err(|_| CredentialError::Unavailable)?,
            })
        }
        .await;
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&directory);
        }
        result
    }

    pub fn directory(&self) -> &std::path::Path {
        &self.directory
    }

    pub fn command(
        &self,
        executable: &std::path::Path,
    ) -> Result<std::process::Command, CredentialError> {
        let root = self
            .protected_root
            .to_str()
            .ok_or(CredentialError::InvalidBinding)?;
        if root.contains(['"', '\\', '\n', '\r']) {
            return Err(CredentialError::InvalidBinding);
        }
        let mut profile = format!(
            "(version 1)(allow default)(deny file-read* file-write* (subpath \"{root}\"))(deny process-info*)(deny mach-lookup (global-name \"com.apple.taskgated\"))"
        );
        if let Some(lease) = &self.lease {
            profile.push_str(&format!(
                "(deny network*)(allow network-outbound (remote tcp \"127.0.0.1:{}\"))",
                lease.port
            ));
        }
        let mut command = std::process::Command::new("/usr/bin/sandbox-exec");
        command
            .arg("-p")
            .arg(profile)
            .arg("--")
            .arg(executable)
            .env_clear()
            .envs(&self.environment);
        Ok(command)
    }
}

impl Drop for ProtectedProcess {
    fn drop(&mut self) {
        if let Some(lease) = &self.lease {
            lease.cancel();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
