use std::fmt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::{BashError, BashErrorCode, BashLaunchPolicy, PreparedBash, Tools};
use vega_credentials::CredentialBinding;

use serde::{Deserialize, Serialize};

use super::BashInputError;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ScopeInput")]
pub struct BashScopeRequest {
    network: bool,
    write_roots: Vec<String>,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeInput {
    #[serde(default)]
    network: bool,
    #[serde(default)]
    write_roots: Vec<String>,
    reason: String,
}

impl TryFrom<ScopeInput> for BashScopeRequest {
    type Error = BashInputError;

    fn try_from(input: ScopeInput) -> Result<Self, Self::Error> {
        if (!input.network && input.write_roots.is_empty())
            || input.reason.trim().is_empty()
            || input.reason.len() > 512
            || input.reason.chars().any(char::is_control)
            || input.write_roots.iter().any(|root| {
                !Path::new(root).is_absolute()
                    || root.contains(['"', '\\'])
                    || root.chars().any(char::is_control)
                    || root.split('/').any(|part| matches!(part, "." | ".."))
            })
        {
            return Err(BashInputError::InvalidScope);
        }
        Ok(Self {
            network: input.network,
            write_roots: input.write_roots,
            reason: input.reason,
        })
    }
}

impl BashScopeRequest {
    pub fn network(&self) -> bool {
        self.network
    }

    pub fn write_roots(&self) -> &[String] {
        &self.write_roots
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct BashScopeDirectory {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl BashScopeDirectory {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn identity(&self) -> (u64, u64) {
        (self.device, self.inode)
    }

    pub fn validate(&self) -> Result<(), BashError> {
        let fail = || BashError::new(BashErrorCode::ScopeMismatch);
        let path = self.path.canonicalize().map_err(|_| fail())?;
        let metadata = std::fs::symlink_metadata(&self.path).map_err(|_| fail())?;
        if path != self.path
            || !metadata.is_dir()
            || (metadata.dev(), metadata.ino()) != (self.device, self.inode)
        {
            return Err(fail());
        }
        Ok(())
    }
}

pub struct PreparedBashScope {
    instance_id: u64,
    project_root: PathBuf,
    command: String,
    timeout_ms: u64,
    request: BashScopeRequest,
    policy: BashLaunchPolicy,
    pub(crate) directories: Vec<BashScopeDirectory>,
    pub(crate) bindings: Vec<CredentialBinding>,
}

impl PreparedBashScope {
    pub fn directories(&self) -> &[BashScopeDirectory] {
        &self.directories
    }

    pub fn bindings(&self) -> &[CredentialBinding] {
        &self.bindings
    }

    pub(crate) fn network(&self) -> bool {
        self.request.network
    }

    pub fn validate(
        &self,
        prepared: &PreparedBash,
        policy: BashLaunchPolicy,
    ) -> Result<(), BashError> {
        if self.instance_id != prepared.tool_instance_id()
            || self.project_root != prepared.project_root()
            || self.command != prepared.command()
            || self.timeout_ms != prepared.timeout_ms()
            || self.policy != policy
            || prepared.requested_scope() != Some(&self.request)
        {
            return Err(BashError::new(BashErrorCode::ScopeMismatch));
        }
        for directory in &self.directories {
            directory.validate()?;
        }
        Ok(())
    }
}

impl Tools {
    pub fn prepare_bash_scope(
        &self,
        prepared: &PreparedBash,
        policy: BashLaunchPolicy,
        request: BashScopeRequest,
        bindings: Vec<CredentialBinding>,
    ) -> Result<PreparedBashScope, BashError> {
        let fail = || BashError::new(BashErrorCode::ScopeMismatch);
        if policy == BashLaunchPolicy::FullAccess
            || (!request.write_roots.is_empty() && policy != BashLaunchPolicy::WorkspaceWrite)
            || prepared.tool_instance_id() != self.instance_id
            || prepared.project_root() != self.root
        {
            return Err(fail());
        }
        let git_entry = self.root.join(".git");
        let git_dir = crate::fence::discover_git_dir(&self.root)
            .map_err(|_| fail())?
            .unwrap_or_else(|| git_entry.clone());
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.canonicalize().map_err(|_| fail()))
            .transpose()?;
        if !request.write_roots.is_empty() && home.is_none() {
            return Err(fail());
        }
        let mut directories = Vec::new();
        for root in &request.write_roots {
            let path = Path::new(root).canonicalize().map_err(|_| fail())?;
            let text = path.to_str().ok_or_else(fail)?;
            let overlap = |other: &Path| path.starts_with(other) || other.starts_with(&path);
            if path.parent().is_none()
                || text.contains(['"', '\\'])
                || text.chars().any(char::is_control)
                || home.as_ref().is_some_and(|home| home.starts_with(&path))
                || overlap(&git_entry)
                || overlap(&git_dir)
                || path.components().any(|part| part.as_os_str() == ".git")
                || self
                    .credential_policy
                    .as_ref()
                    .is_some_and(|policy| overlap(&policy.protected_root))
            {
                return Err(fail());
            }
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| fail())?;
            if !metadata.is_dir() {
                return Err(fail());
            }
            if directories
                .iter()
                .any(|root: &BashScopeDirectory| root.path == path)
            {
                return Err(fail());
            }
            directories.push(BashScopeDirectory {
                path,
                device: metadata.dev(),
                inode: metadata.ino(),
            });
        }
        directories.sort_by(|left, right| left.path.cmp(&right.path));
        let scope = PreparedBashScope {
            instance_id: self.instance_id,
            project_root: self.root.clone(),
            command: prepared.command().into(),
            timeout_ms: prepared.timeout_ms(),
            request,
            policy,
            directories,
            bindings,
        };
        scope.validate(prepared, policy)?;
        Ok(scope)
    }
}

impl fmt::Debug for BashScopeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BashScopeRequest")
            .field("network", &self.network)
            .field("write_roots", &self.write_roots.len())
            .finish_non_exhaustive()
    }
}
