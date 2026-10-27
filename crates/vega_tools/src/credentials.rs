use crate::{BashError, BashErrorCode, ToolError, Tools};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vega_credentials::{CredentialBinding, SecretReader};

pub struct ToolCredentialPolicy {
    pub(crate) protected_root: PathBuf,
    pub(crate) protected_identity: (u64, u64),
    pub(crate) bindings: Vec<CredentialBinding>,
    pub(crate) reader: SecretReader,
    bindings_reader:
        Option<Arc<dyn Fn() -> Result<Vec<CredentialBinding>, BashError> + Send + Sync>>,
}

impl ToolCredentialPolicy {
    pub fn new(
        root: PathBuf,
        bindings: Vec<CredentialBinding>,
        reader: SecretReader,
    ) -> Result<Self, ToolError> {
        let protected_root = root.canonicalize()?;
        use std::os::unix::fs::MetadataExt;
        let metadata = protected_root.metadata()?;
        let protected_identity = (metadata.dev(), metadata.ino());
        for binding in &bindings {
            binding.validate().map_err(|_| {
                ToolError::InvalidInput("Credential binding configuration is invalid".into())
            })?;
        }
        Ok(Self {
            protected_root,
            protected_identity,
            bindings,
            reader,
            bindings_reader: None,
        })
    }

    pub fn with_bindings_reader(
        mut self,
        reader: Arc<dyn Fn() -> Result<Vec<CredentialBinding>, BashError> + Send + Sync>,
    ) -> Self {
        self.bindings_reader = Some(reader);
        self
    }

    fn binding_snapshot(&self) -> Result<Vec<CredentialBinding>, BashError> {
        let current = self
            .bindings_reader
            .as_ref()
            .ok_or_else(|| BashError::new(BashErrorCode::ScopeMismatch))?()?;
        if current != self.bindings {
            return Err(BashError::new(BashErrorCode::ScopeMismatch));
        }
        Ok(current)
    }

    pub(crate) fn validate_bindings(&self) -> Result<(), BashError> {
        let fail = || BashError::new(BashErrorCode::SandboxUnavailable);
        let mut aliases = BTreeSet::new();
        for binding in &self.bindings {
            binding.validate().map_err(|_| fail())?;
            if !aliases.insert(&binding.env) {
                return Err(fail());
            }
            let value = (self.reader)(&binding.key_ref).map_err(|_| fail())?;
            if value.is_empty() || value.contains(['\r', '\n']) {
                return Err(fail());
            }
        }
        Ok(())
    }
}

impl Tools {
    pub fn with_credential_policy(mut self, policy: ToolCredentialPolicy) -> Self {
        self.credential_policy = Some(Arc::new(policy));
        self
    }

    pub fn bash_binding_snapshot(&self) -> Result<Vec<CredentialBinding>, BashError> {
        self.credential_policy
            .as_ref()
            .ok_or_else(|| BashError::new(BashErrorCode::ScopeMismatch))?
            .binding_snapshot()
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
