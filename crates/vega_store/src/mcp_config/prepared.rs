use super::*;
use crate::write_coordination::{WriteCategory, WriteTransaction};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Source {
    accepted: Option<Vec<u8>>,
    generation: i64,
    pending_base: Option<Vec<u8>>,
    pending: Option<String>,
    rows: Vec<McpServerRow>,
    identities: Vec<(String, String, i64, bool)>,
    schema: i64,
}

impl Source {
    fn read(conn: &Connection) -> Result<Self, McpStoreError> {
        let (accepted, generation, pending_base, pending) = conn.query_row(
            "SELECT accepted_digest, generation, pending_base_digest, pending_declarations FROM mcp_config_state WHERE singleton = 1",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let mut statement = conn.prepare(
            "SELECT id, declaration, projected_revision, tombstoned FROM mcp_config_identities ORDER BY id",
        )?;
        let identities = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            accepted,
            generation,
            pending_base,
            pending,
            rows: mcp_servers::list(conn)?,
            identities,
            schema: conn.pragma_query_value(None, "schema_version", |row| row.get(0))?,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct FileImport {
    file_import: Vec<Declaration>,
    source: Source,
}

struct Projection {
    row: McpServerRow,
    declaration: String,
    write: bool,
    new_identity: bool,
    cleanup: Vec<String>,
}

pub struct PreparedReconciliation {
    database: crate::DatabaseIdentity,
    source: Source,
    base: Vec<u8>,
    target: Vec<Declaration>,
    encoded: String,
    projections: Vec<Projection>,
    removed: Vec<(McpServerRow, String, Vec<String>)>,
    unchanged: bool,
}

pub struct ReconciliationIntent {
    database: crate::DatabaseIdentity,
    source: Source,
    target: Vec<Declaration>,
    changed: Vec<String>,
    projections: Vec<Projection>,
    removed: Vec<(McpServerRow, String, Vec<String>)>,
}

pub struct PreparedConfirmation {
    intent: ReconciliationIntent,
    digest: Vec<u8>,
}

pub fn prepare_reconciliation(
    store: &crate::Store,
    bytes: Option<&[u8]>,
    validate: impl Fn(&McpServerRow) -> Result<(), McpStoreError>,
) -> Result<PreparedReconciliation, McpStoreError> {
    let declarations = parse(bytes)?;
    let snapshot = store.conn().unchecked_transaction()?;
    let source = Source::read(&snapshot)?;
    let file_import = source
        .pending
        .as_ref()
        .and_then(|pending| serde_json::from_str::<FileImport>(pending).ok());
    if let Some(import) = &file_import {
        let mut original = source.clone();
        original.pending = None;
        original.pending_base = None;
        original.generation = original
            .generation
            .checked_sub(1)
            .ok_or(McpStoreError::Invalid)?;
        if original != import.source {
            return Err(McpStoreError::Conflict);
        }
    }
    let metadata_pending = source.pending.is_some() && file_import.is_none();
    let target = if let Some(pending) = &source.pending {
        let target: Vec<Declaration> = match &file_import {
            Some(import) => import.file_import.clone(),
            None => serde_json::from_str(pending).map_err(|_| McpStoreError::Invalid)?,
        };
        let target = canonical(target)?;
        if declarations != target
            && source.pending_base.as_deref() != Some(digest(bytes).as_slice())
        {
            return Err(McpStoreError::Conflict);
        }
        target
    } else {
        declarations
    };
    let mut projections = Vec::new();
    let mut unchanged = source.pending.is_none()
        && source.accepted.as_deref() == Some(digest(bytes).as_slice())
        && source.rows.len() == target.len();
    for declaration in &target {
        let old = source.rows.iter().find(|row| row.id == declaration.id);
        let historical = source
            .identities
            .iter()
            .find(|entry| entry.0 == declaration.id);
        if historical.is_some_and(|entry| entry.3)
            || old.is_some_and(|row| row.deleting)
            || (historical.is_some() && old.is_none())
        {
            return Err(McpStoreError::Conflict);
        }
        let mut row = declaration.row()?;
        if let Some(old) = old {
            row.config_revision = old.config_revision;
            row.created_at = old.created_at;
        }
        validate(&row)?;
        let encoded = json(declaration)?;
        let same = historical.is_some_and(|entry| {
            entry.1 == encoded
                && old.is_some_and(|row| u64::try_from(entry.2).ok() == Some(row.config_revision))
        });
        if let Some(old) = old {
            let mut projected = Declaration::from_row(old)?;
            projected.enabled = declaration.enabled;
            unchanged &= !old.deleting
                && (!old.enabled || declaration.enabled)
                && projected == *declaration
                && same;
        } else {
            unchanged = false;
        }
        if same {
            continue;
        }
        let mut write = true;
        let mut cleanup = Vec::new();
        if let Some(old) = old {
            if metadata_pending {
                if Declaration::from_row(old)? != *declaration {
                    return Err(McpStoreError::Conflict);
                }
                write = false;
            } else {
                mcp_servers::validate_projection_refs(&row, Some(old))?;
                row.config_revision = row
                    .config_revision
                    .checked_add(1)
                    .ok_or(McpStoreError::Invalid)?;
                cleanup = mcp_servers::projection_cleanup_refs(old, &row)?;
            }
        } else {
            mcp_servers::validate_projection_refs(&row, None)?;
        }
        projections.push(Projection {
            row,
            declaration: encoded,
            write,
            new_identity: old.is_none(),
            cleanup,
        });
    }
    let mut removed = Vec::new();
    for row in &source.rows {
        if target.iter().any(|entry| entry.id == row.id) {
            continue;
        }
        let cleanup = if row.deleting {
            Vec::new()
        } else {
            mcp_servers::projection_cleanup_refs(
                row,
                &McpServerRow {
                    remote_credential_ref: None,
                    local_env_refs_json: None,
                    ..row.clone()
                },
            )?
        };
        removed.push((row.clone(), json(&Declaration::from_row(row)?)?, cleanup));
    }
    snapshot.commit()?;
    Ok(PreparedReconciliation {
        database: store.database_identity(),
        source: source.clone(),
        base: digest(bytes),
        encoded: if metadata_pending {
            json(&target)?
        } else {
            match file_import {
                Some(import) => json(&import)?,
                None => json(&FileImport {
                    file_import: target.clone(),
                    source,
                })?,
            }
        },
        target,
        projections,
        removed,
        unchanged,
    })
}

impl PreparedReconciliation {
    pub fn unchanged(&self) -> bool {
        self.unchanged
    }

    pub fn begin(self, store: &crate::Store) -> Result<ReconciliationIntent, McpStoreError> {
        if self.database != store.database_identity() {
            return Err(McpStoreError::Conflict);
        }
        let tx = WriteTransaction::new(store.conn(), WriteCategory::McpServers)?;
        if Source::read(&tx)? != self.source {
            return Err(McpStoreError::Conflict);
        }
        let changed = self
            .projections
            .iter()
            .filter(|projection| projection.write)
            .map(|projection| projection.row.id.clone())
            .chain(
                self.removed
                    .iter()
                    .filter(|(row, _, _)| !row.deleting)
                    .map(|(row, _, _)| row.id.clone()),
            )
            .collect();
        if self.source.pending.is_none() {
            tx.execute(
                "UPDATE mcp_config_state SET generation = generation + 1, pending_base_digest = ?1, pending_declarations = ?2 WHERE singleton = 1",
                params![self.base, self.encoded],
            )?;
        }
        let source = Source::read(&tx)?;
        tx.commit()?;
        Ok(ReconciliationIntent {
            database: self.database,
            source,
            target: self.target,
            changed,
            projections: self.projections,
            removed: self.removed,
        })
    }
}

fn remember_encoded(
    conn: &Connection,
    id: &str,
    declaration: &str,
    revision: u64,
    dead: bool,
) -> Result<(), McpStoreError> {
    conn.execute(
        "INSERT INTO mcp_config_identities (id, declaration, projected_revision, tombstoned) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO UPDATE SET declaration = excluded.declaration, projected_revision = excluded.projected_revision, tombstoned = excluded.tombstoned",
        params![id, declaration, i64::try_from(revision).map_err(|_| McpStoreError::Invalid)?, dead],
    )?;
    Ok(())
}

impl ReconciliationIntent {
    pub fn changed(&self) -> &[String] {
        &self.changed
    }

    pub fn save(
        self,
        store: &crate::Store,
        path: &Path,
        edit: crate::yaml_config::Edit,
    ) -> Result<PreparedConfirmation, McpStoreError> {
        let snapshot = store.conn().unchecked_transaction()?;
        if self.database != store.database_identity() || Source::read(&snapshot)? != self.source {
            return Err(McpStoreError::Conflict);
        }
        snapshot.commit()?;
        let mut bytes = edit.bytes();
        if parse(bytes.as_deref())? != self.target {
            if self.source.pending_base.as_deref() != Some(digest(bytes.as_deref()).as_slice()) {
                return Err(McpStoreError::Conflict);
            }
            bytes = Some(
                edit.save_fields(&serde_json::json!({"mcp_servers": self.target}))
                    .map_err(io_error)?,
            );
        }
        if parse(bytes.as_deref())? != self.target
            || crate::yaml_config::read(path).map_err(io_error)? != bytes
        {
            return Err(McpStoreError::Conflict);
        }
        Ok(PreparedConfirmation {
            intent: self,
            digest: digest(bytes.as_deref()),
        })
    }
}

impl PreparedConfirmation {
    pub fn confirm(self, store: &crate::Store) -> Result<(), McpStoreError> {
        if self.intent.database != store.database_identity() {
            return Err(McpStoreError::Conflict);
        }
        let tx = WriteTransaction::new(store.conn(), WriteCategory::McpServers)?;
        if Source::read(&tx)? != self.intent.source {
            return Err(McpStoreError::Conflict);
        }
        for projection in self.intent.projections {
            if projection.new_identity {
                let audited: bool = tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM tool_calls WHERE tool LIKE 'mcp_%' AND json_extract(CASE WHEN json_valid(input_json) THEN input_json ELSE '{}' END, '$.server_id') = ?1)",
                    [&projection.row.id], |row| row.get(0),
                )?;
                if audited {
                    return Err(McpStoreError::Conflict);
                }
            }
            for reference in projection.cleanup {
                mcp_servers::enqueue_cleanup(&tx, &projection.row.id, &reference)?;
            }
            if projection.write {
                mcp_servers::write_projection(&tx, &projection.row)?;
            }
            remember_encoded(
                &tx,
                &projection.row.id,
                &projection.declaration,
                projection.row.config_revision,
                false,
            )?;
        }
        for (row, declaration, cleanup) in self.intent.removed {
            remember_encoded(&tx, &row.id, &declaration, row.config_revision, true)?;
            for reference in cleanup {
                mcp_servers::enqueue_cleanup(&tx, &row.id, &reference)?;
            }
            if !row.deleting {
                tx.execute("UPDATE mcp_servers SET enabled = 0, deleting = 1, config_revision = config_revision + 1 WHERE id = ?1", [&row.id])?;
            }
        }
        tx.execute(
            "UPDATE mcp_config_state SET accepted_digest = ?1, generation = generation + 1, pending_base_digest = NULL, pending_declarations = NULL WHERE singleton = 1",
            [self.digest],
        )?;
        tx.commit()?;
        Ok(())
    }
}
