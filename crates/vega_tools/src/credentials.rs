use crate::{BashError, BashErrorCode, ToolError, Tools};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use vega_credentials::{CredentialBinding, CredentialBroker, ProxyLease, SecretReader};

pub struct ToolCredentialPolicy {
    pub(crate) protected_root: PathBuf,
    bindings: Vec<CredentialBinding>,
    reader: SecretReader,
}

impl ToolCredentialPolicy {
    pub(crate) fn has_bindings(&self) -> bool {
        !self.bindings.is_empty()
    }
    pub fn new(
        root: PathBuf,
        bindings: Vec<CredentialBinding>,
        reader: SecretReader,
    ) -> Result<Self, ToolError> {
        let protected_root = root.canonicalize()?;
        for binding in &bindings {
            binding.validate().map_err(|_| {
                ToolError::InvalidInput("Credential binding configuration is invalid".into())
            })?;
        }
        Ok(Self {
            protected_root,
            bindings,
            reader,
        })
    }

    pub(crate) async fn start(
        &self,
        ca_path: &Path,
        cancel: CancellationToken,
    ) -> Result<Option<(ProxyLease, BTreeMap<String, String>)>, BashError> {
        if self.bindings.is_empty() {
            return Ok(None);
        }
        let fail = || BashError::new(BashErrorCode::SandboxUnavailable);
        let broker =
            CredentialBroker::new(&self.bindings, self.reader.clone()).map_err(|_| fail())?;
        let lease = ProxyLease::start(broker, cancel)
            .await
            .map_err(|_| fail())?;
        std::fs::write(ca_path, &lease.ca_pem).map_err(|_| fail())?;
        let environment = lease
            .environment(ca_path.to_str().ok_or_else(fail)?)
            .map_err(|_| fail())?;
        Ok(Some((lease, environment)))
    }
}

impl Tools {
    pub fn with_credential_policy(mut self, policy: ToolCredentialPolicy) -> Self {
        self.credential_policy = Some(Arc::new(policy));
        self
    }

    pub(crate) fn check_credential_path(&self, path: &Path) -> Result<(), ToolError> {
        if let Some(policy) = &self.credential_policy {
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if resolved.starts_with(&policy.protected_root)
                || policy.protected_root.starts_with(&resolved)
            {
                return Err(ToolError::InvalidInput(
                    "Vega credential configuration is not accessible to agent tools".into(),
                ));
            }
            use std::os::unix::fs::MetadataExt;
            if resolved
                .metadata()
                .is_ok_and(|metadata| metadata.is_file() && metadata.nlink() > 1)
            {
                return Err(ToolError::InvalidInput(
                    "Hard-linked files cannot be accessed while credential protection is active"
                        .into(),
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn controlled_environment(temp: &Path) -> BTreeMap<String, String> {
    let mut values = BTreeMap::from([
        (
            "PATH".into(),
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".into(),
        ),
        ("HOME".into(), temp.to_string_lossy().into_owned()),
        ("ZDOTDIR".into(), temp.to_string_lossy().into_owned()),
        ("LANG".into(), "en_US.UTF-8".into()),
    ]);
    for name in ["TMPDIR", "TMP", "TEMP", "TEMPDIR"] {
        values.insert(name.into(), temp.to_string_lossy().into_owned());
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_is_an_allowlist_without_inherited_secrets_or_startup_injection() {
        let values = controlled_environment(Path::new("/private/tmp/synthetic"));
        for name in [
            "AWS_SECRET_ACCESS_KEY",
            "SSLKEYLOGFILE",
            "NODE_OPTIONS",
            "PYTHONPATH",
            "BASH_ENV",
            "DYLD_INSERT_LIBRARIES",
        ] {
            assert!(!values.contains_key(name));
        }
        assert_eq!(values["HOME"], "/private/tmp/synthetic");
    }
    #[test]
    fn file_tools_deny_config_root_and_symlink_aliases() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        std::fs::create_dir(&config).unwrap();
        let file = config.join("config.toml");
        std::fs::write(&file, "synthetic").unwrap();
        std::os::unix::fs::symlink(&file, root.path().join("alias")).unwrap();
        let tools = Tools::new(root.path()).unwrap().with_credential_policy(
            ToolCredentialPolicy::new(config, Vec::new(), Arc::new(|_| unreachable!())).unwrap(),
        );
        assert!(tools.read("alias", None, None).is_err());
        assert!(tools.read("config/config.toml", None, None).is_err());
        assert!(
            !tools
                .grep("synthetic", None)
                .unwrap()
                .text
                .contains("synthetic")
        );
    }
}
