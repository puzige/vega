use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mcp_servers::{self, McpEnvSlot, McpServerRow, McpStoreError};
use crate::write_coordination::{WriteCategory, WriteTransaction};

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

fn io_error(error: io::Error) -> McpStoreError {
    match error.kind() {
        io::ErrorKind::WouldBlock => McpStoreError::Conflict,
        io::ErrorKind::InvalidData => McpStoreError::Invalid,
        _ => McpStoreError::Configuration,
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

fn parse(bytes: Option<&[u8]>) -> Result<Vec<Declaration>, McpStoreError> {
    canonical(match bytes {
        Some(bytes) => {
            let _: crate::config::AppConfig =
                crate::yaml_config::decode(bytes).map_err(io_error)?;
            let reasoning: crate::reasoning::ReasoningConfig =
                crate::yaml_config::section(bytes, "reasoning").map_err(io_error)?;
            reasoning.validate().map_err(|_| McpStoreError::Invalid)?;
            let capabilities: crate::model_capabilities::ModelCapabilityConfig =
                crate::yaml_config::section(bytes, "model_capabilities").map_err(io_error)?;
            capabilities
                .validate()
                .map_err(|_| McpStoreError::Invalid)?;
            crate::yaml_config::section(bytes, "mcp_servers").map_err(io_error)?
        }
        None => Vec::new(),
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
        let previous: Option<(String, u64)> = conn.query_row(
            "SELECT declaration, projected_revision FROM mcp_config_identities WHERE id = ?1 AND tombstoned = 0",
            [&row.id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let declaration = match previous {
            Some((declaration, revision)) if revision == row.config_revision => {
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

fn remember(
    conn: &Connection,
    declaration: &Declaration,
    revision: u64,
) -> Result<(), McpStoreError> {
    conn.execute(
        "INSERT INTO mcp_config_identities (id, declaration, projected_revision) VALUES (?1, ?2, ?3) \
         ON CONFLICT(id) DO UPDATE SET declaration = excluded.declaration, projected_revision = excluded.projected_revision",
        params![declaration.id, json(declaration)?, revision],
    )?;
    Ok(())
}

pub fn reconcile(
    conn: &Connection,
    path: &Path,
    validate: impl Fn(&McpServerRow) -> Result<(), McpStoreError>,
) -> Result<Vec<String>, McpStoreError> {
    let edit = crate::yaml_config::begin(path).map_err(io_error)?;
    let tx = WriteTransaction::new(conn, WriteCategory::McpServers)?;
    let mut bytes = edit.bytes();
    let mut declarations = parse(bytes.as_deref())?;
    let pending: Option<(Vec<u8>, String)> = tx.query_row(
        "SELECT pending_base_digest, pending_declarations FROM mcp_config_state WHERE singleton = 1 AND pending_declarations IS NOT NULL",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    if let Some((base, target)) = pending {
        let target: Vec<Declaration> =
            serde_json::from_str(&target).map_err(|_| McpStoreError::Invalid)?;
        if declarations != target {
            if digest(bytes.as_deref()) != base {
                return Err(McpStoreError::Conflict);
            }
            bytes = Some(
                edit.save_fields(&serde_json::json!({"mcp_servers": target}))
                    .map_err(io_error)?,
            );
            declarations = parse(bytes.as_deref())?;
            if declarations != target {
                return Err(McpStoreError::Conflict);
            }
        }
    }
    let pending: bool = tx.query_row(
        "SELECT pending_declarations IS NOT NULL FROM mcp_config_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let current = mcp_servers::list(&tx)?;
    let current_by_id: HashMap<_, _> = current.iter().map(|row| (row.id.as_str(), row)).collect();
    let mut changed = Vec::new();
    for declaration in &declarations {
        let mut row = declaration.row()?;
        let historical: Option<(String, bool, u64)> = tx.query_row(
            "SELECT declaration, tombstoned, projected_revision FROM mcp_config_identities WHERE id = ?1",
            [&declaration.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        if historical.as_ref().is_some_and(|(_, dead, _)| *dead) {
            return Err(McpStoreError::Conflict);
        }
        let old = current_by_id.get(declaration.id.as_str()).copied();
        if old.is_some_and(|row| row.deleting) || (historical.is_some() && old.is_none()) {
            return Err(McpStoreError::Conflict);
        }
        if let Some(old) = old {
            row.config_revision = old.config_revision;
            row.created_at = old.created_at;
        }
        validate(&row)?;
        let unchanged = historical.as_ref().is_some_and(|(previous, _, revision)| {
            json(declaration).is_ok_and(|value| value == *previous)
                && old.is_some_and(|row| row.config_revision == *revision)
        });
        if unchanged {
            continue;
        }
        if let Some(old) = old {
            row.config_revision = old.config_revision;
            row.created_at = old.created_at;
            if pending {
                if Declaration::from_row(old)? != *declaration {
                    return Err(McpStoreError::Conflict);
                }
                remember(&tx, declaration, old.config_revision)?;
                continue;
            }
            mcp_servers::validate_projection_refs(&row, Some(old))?;
            row.config_revision = row
                .config_revision
                .checked_add(1)
                .ok_or(McpStoreError::Invalid)?;
            mcp_servers::queue_projection_cleanup(&tx, old, &row)?;
        } else {
            let audited: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM tool_calls WHERE tool LIKE 'mcp_%' AND json_extract(CASE WHEN json_valid(input_json) THEN input_json ELSE '{}' END, '$.server_id') = ?1)",
                [&row.id], |row| row.get(0),
            )?;
            if audited {
                return Err(McpStoreError::Conflict);
            }
            mcp_servers::validate_projection_refs(&row, None)?;
        }
        mcp_servers::write_projection(&tx, &row)?;
        remember(&tx, declaration, row.config_revision)?;
        changed.push(row.id.clone());
    }
    let declared: HashSet<_> = declarations.iter().map(|entry| entry.id.as_str()).collect();
    for row in current
        .iter()
        .filter(|row| !declared.contains(row.id.as_str()))
    {
        let declaration = Declaration::from_row(row)?;
        remember(&tx, &declaration, row.config_revision)?;
        tx.execute(
            "UPDATE mcp_config_identities SET tombstoned = 1 WHERE id = ?1",
            [&row.id],
        )?;
        if !row.deleting {
            mcp_servers::queue_projection_cleanup(
                &tx,
                row,
                &McpServerRow {
                    remote_credential_ref: None,
                    local_env_refs_json: None,
                    ..row.clone()
                },
            )?;
            tx.execute("UPDATE mcp_servers SET enabled = 0, deleting = 1, config_revision = config_revision + 1 WHERE id = ?1", [&row.id])?;
            changed.push(row.id.clone());
        }
    }
    if crate::yaml_config::read(path).map_err(io_error)? != bytes {
        return Err(McpStoreError::Conflict);
    }
    tx.execute(
        "UPDATE mcp_config_state SET accepted_digest = ?1, generation = generation + 1, pending_base_digest = NULL, pending_declarations = NULL \
         WHERE singleton = 1 AND (accepted_digest IS NULL OR accepted_digest != ?1 OR pending_declarations IS NOT NULL)",
        [digest(bytes.as_deref())],
    )?;
    tx.commit()?;
    Ok(changed)
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

pub fn projection_current(
    conn: &Connection,
    path: &Path,
) -> Result<Vec<Declaration>, McpStoreError> {
    let bytes = crate::yaml_config::read(path).map_err(io_error)?;
    let declarations = parse(bytes.as_deref())?;
    let (accepted, pending): (Option<Vec<u8>>, bool) = conn.query_row(
        "SELECT accepted_digest, pending_declarations IS NOT NULL FROM mcp_config_state WHERE singleton = 1",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if pending || accepted.as_deref() != Some(digest(bytes.as_deref()).as_slice()) {
        return Err(McpStoreError::Conflict);
    }
    Ok(declarations)
}
