mod prepared;

pub use prepared::{
    PreparedConfirmation, PreparedReconciliation, ReconciliationIntent, prepare_reconciliation,
};

use std::collections::HashSet;
use std::io;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mcp_servers::{self, McpEnvSlot, McpServerRow, McpStoreError};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declaration {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub enabled: bool,
    pub transport: Transport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Transport {
    Local {
        executable: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        working_directory: Option<String>,
        #[serde(default)]
        environment: Vec<McpEnvSlot>,
    },
    Remote {
        endpoint: String,
        #[serde(default)]
        allow_loopback_http: bool,
        authorization: Authorization,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Authorization {
    None,
    Bearer {
        #[serde(default)]
        credential_ref: Option<String>,
    },
    OAuth {
        #[serde(default)]
        credential_ref: Option<String>,
        #[serde(default)]
        issuer: Option<String>,
        #[serde(default)]
        client_id: Option<String>,
    },
}

fn json<T: Serialize>(value: &T) -> Result<String, McpStoreError> {
    serde_json::to_string(value).map_err(|_| McpStoreError::Invalid)
}

pub fn io_error(error: io::Error) -> McpStoreError {
    if let Some(busy) = crate::yaml_config::lock_busy(&error) {
        return McpStoreError::ConfigurationBusy(busy);
    }
    match crate::yaml_config::failure(&error) {
        Some(failure) => McpStoreError::ConfigurationFailure(failure),
        None => McpStoreError::ConfigurationIo(error.kind()),
    }
}

fn digest(bytes: Option<&[u8]>) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update([u8::from(bytes.is_some())]);
    hash.update(bytes.unwrap_or_default());
    hash.finalize().to_vec()
}

impl Declaration {
    pub fn from_row(row: &McpServerRow) -> Result<Self, McpStoreError> {
        let mut transport = match row.transport.as_str() {
            "local" => Transport::Local {
                executable: row.local_executable.clone().ok_or(McpStoreError::Invalid)?,
                args: serde_json::from_str(
                    row.local_args_json
                        .as_deref()
                        .ok_or(McpStoreError::Invalid)?,
                )
                .map_err(|_| McpStoreError::Invalid)?,
                working_directory: row.local_working_directory.clone(),
                environment: serde_json::from_str(
                    row.local_env_refs_json
                        .as_deref()
                        .ok_or(McpStoreError::Invalid)?,
                )
                .map_err(|_| McpStoreError::Invalid)?,
            },
            "remote" => Transport::Remote {
                endpoint: row.remote_endpoint.clone().ok_or(McpStoreError::Invalid)?,
                allow_loopback_http: row.remote_allow_loopback_http,
                authorization: match row.remote_auth_mode.as_deref() {
                    Some("none") => Authorization::None,
                    Some("bearer") => Authorization::Bearer {
                        credential_ref: row.remote_credential_ref.clone(),
                    },
                    Some("oauth") => Authorization::OAuth {
                        credential_ref: row.remote_credential_ref.clone(),
                        issuer: row.remote_oauth_issuer.clone(),
                        client_id: row.remote_oauth_client_id.clone(),
                    },
                    _ => return Err(McpStoreError::Invalid),
                },
            },
            _ => return Err(McpStoreError::Invalid),
        };
        if let Transport::Local { environment, .. } = &mut transport {
            environment.sort_by(|left, right| left.variable.cmp(&right.variable));
        }
        Ok(Self {
            id: row.id.clone(),
            display_name: row.display_name.clone(),
            enabled: row.enabled,
            transport,
        })
    }

    pub fn row(&self) -> Result<McpServerRow, McpStoreError> {
        let mut row = McpServerRow {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            transport: String::new(),
            local_executable: None,
            local_args_json: None,
            local_working_directory: None,
            local_env_refs_json: None,
            remote_endpoint: None,
            remote_allow_loopback_http: false,
            remote_auth_mode: None,
            remote_credential_ref: None,
            remote_oauth_issuer: None,
            remote_oauth_client_id: None,
            enabled: false,
            deleting: false,
            config_revision: 1,
            last_error_code: None,
            created_at: 0,
            updated_at: 0,
        };
        match &self.transport {
            Transport::Local {
                executable,
                args,
                working_directory,
                environment,
            } => {
                row.transport = "local".into();
                row.local_executable = Some(executable.clone());
                row.local_args_json = Some(json(args)?);
                row.local_working_directory = working_directory.clone();
                row.local_env_refs_json = Some(json(environment)?);
            }
            Transport::Remote {
                endpoint,
                allow_loopback_http,
                authorization,
            } => {
                row.transport = "remote".into();
                row.remote_endpoint = Some(endpoint.clone());
                row.remote_allow_loopback_http = *allow_loopback_http;
                match authorization {
                    Authorization::None => row.remote_auth_mode = Some("none".into()),
                    Authorization::Bearer { credential_ref } => {
                        row.remote_auth_mode = Some("bearer".into());
                        row.remote_credential_ref = credential_ref.clone();
                    }
                    Authorization::OAuth {
                        credential_ref,
                        issuer,
                        client_id,
                    } => {
                        row.remote_auth_mode = Some("oauth".into());
                        row.remote_credential_ref = credential_ref.clone();
                        row.remote_oauth_issuer = issuer.clone();
                        row.remote_oauth_client_id = client_id.clone();
                    }
                }
            }
        }
        Ok(row)
    }
}

fn canonical(mut declarations: Vec<Declaration>) -> Result<Vec<Declaration>, McpStoreError> {
    let mut ids = HashSet::new();
    for declaration in &mut declarations {
        if declaration
            .id
            .parse::<ulid::Ulid>()
            .ok()
            .is_none_or(|id| id.to_string() != declaration.id)
            || !ids.insert(declaration.id.clone())
        {
            return Err(McpStoreError::Invalid);
        }
        if let Transport::Local { environment, .. } = &mut declaration.transport {
            environment.sort_by(|left, right| left.variable.cmp(&right.variable));
        }
    }
    if declarations.iter().filter(|entry| entry.enabled).count() > 8 {
        return Err(McpStoreError::Capacity);
    }
    declarations.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(declarations)
}

fn parse_error(kind: io::ErrorKind) -> McpStoreError {
    McpStoreError::ConfigurationFailure(crate::yaml_config::ConfigurationFailure {
        stage: crate::yaml_config::ConfigurationFailureStage::Parse,
        kind,
    })
}

fn parse(bytes: Option<&[u8]>) -> Result<Vec<Declaration>, McpStoreError> {
    let declarations = match bytes {
        Some(bytes) => {
            let _: crate::config::AppConfig =
                crate::yaml_config::decode(bytes).map_err(|error| parse_error(error.kind()))?;
            let reasoning: crate::reasoning::ReasoningConfig =
                crate::yaml_config::section(bytes, "reasoning")
                    .map_err(|error| parse_error(error.kind()))?;
            reasoning
                .validate()
                .map_err(|_| parse_error(io::ErrorKind::InvalidData))?;
            let capabilities: crate::model_capabilities::ModelCapabilityConfig =
                crate::yaml_config::section(bytes, "model_capabilities")
                    .map_err(|error| parse_error(error.kind()))?;
            capabilities
                .validate()
                .map_err(|_| parse_error(io::ErrorKind::InvalidData))?;
            crate::yaml_config::section(bytes, "mcp_servers")
                .map_err(|error| parse_error(error.kind()))?
        }
        None => Vec::new(),
    };
    canonical(declarations).map_err(|error| match error {
        McpStoreError::Invalid | McpStoreError::Capacity => parse_error(io::ErrorKind::InvalidData),
        error => error,
    })
}

pub(crate) fn stage(conn: &Connection) -> Result<(), McpStoreError> {
    let (accepted, pending): (Option<Vec<u8>>, Option<String>) = conn.query_row(
        "SELECT accepted_digest, pending_declarations FROM mcp_config_state WHERE singleton = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let Some(accepted) = accepted else {
        return Err(McpStoreError::Conflict);
    };
    if pending.is_some() {
        return Err(McpStoreError::Conflict);
    }
    let mut declarations = Vec::new();
    for row in mcp_servers::list(conn)? {
        if row.deleting {
            continue;
        }
        let previous: Option<(String, i64)> = conn.query_row(
            "SELECT declaration, projected_revision FROM mcp_config_identities WHERE id = ?1 AND tombstoned = 0",
            [&row.id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let declaration = match previous {
            Some((declaration, revision))
                if u64::try_from(revision).ok() == Some(row.config_revision) =>
            {
                serde_json::from_str(&declaration).map_err(|_| McpStoreError::Invalid)?
            }
            _ => Declaration::from_row(&row)?,
        };
        declarations.push(declaration);
    }
    conn.execute(
        "UPDATE mcp_config_state SET pending_base_digest = ?1, pending_declarations = ?2 WHERE singleton = 1",
        params![accepted, json(&canonical(declarations)?)?],
    )?;
    Ok(())
}

pub fn admitted(
    conn: &Connection,
    path: &Path,
    id: &str,
    revision: u64,
) -> Result<(), McpStoreError> {
    let declarations = projection_current(conn, path)?;
    let row = mcp_servers::find(conn, id)?.ok_or(McpStoreError::NotFound)?;
    if !row.enabled
        || row.deleting
        || row.config_revision != revision
        || !declarations
            .iter()
            .any(|entry| entry.id == id && entry.enabled)
    {
        return Err(McpStoreError::Conflict);
    }
    Ok(())
}

pub fn verify_current_projection(conn: &Connection, path: &Path) -> Result<(), McpStoreError> {
    let bytes = crate::yaml_config::read(path).map_err(io_error)?;
    if matches_accepted_projection(conn, bytes.as_deref())? {
        return Ok(());
    }
    parse(bytes.as_deref())?;
    Err(McpStoreError::Conflict)
}

fn matches_accepted_projection(
    conn: &Connection,
    bytes: Option<&[u8]>,
) -> Result<bool, McpStoreError> {
    let (accepted, pending): (Option<Vec<u8>>, bool) = conn.query_row(
        "SELECT accepted_digest, pending_declarations IS NOT NULL FROM mcp_config_state WHERE singleton = 1",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(!pending && accepted.as_deref() == Some(digest(bytes).as_slice()))
}

pub fn projection_current(
    conn: &Connection,
    path: &Path,
) -> Result<Vec<Declaration>, McpStoreError> {
    let bytes = crate::yaml_config::read(path).map_err(io_error)?;
    let declarations = parse(bytes.as_deref())?;
    if !matches_accepted_projection(conn, bytes.as_deref())? {
        return Err(McpStoreError::Conflict);
    }
    Ok(declarations)
}
