use super::*;
use vega_store::mcp_config::{Declaration, Transport};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
    service: McpServerSettingsService,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("vega-mcp-protocol-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("database.db"))?;
        store.migrate()?;
        let service = McpServerSettingsService::new(root.join("database.db"), root.clone());
        Ok(Self { root, store, service })
    }

    fn config(&self) -> PathBuf { self.root.join("config.yaml") }

    fn declaration(&self) -> Declaration {
        Declaration {
            id: ulid::Ulid::generate().to_string(), display_name: "Protocol test".into(), enabled: true,
            transport: Transport::Local { executable: "/usr/bin/true".into(), args: Vec::new(), working_directory: None, environment: Vec::new() },
        }
    }

    fn write(&self, entries: &[Declaration]) -> Result<(), Box<dyn std::error::Error>> {
        let edit = vega_store::yaml_config::begin(&self.config())?;
        edit.save_fields(&serde_json::json!({"mcp_servers": entries}))?;
        Ok(())
    }

    fn begin(&self) -> Result<vega_store::mcp_config::ReconciliationIntent, Box<dyn std::error::Error>> {
        let bytes = vega_store::yaml_config::read(&self.config())?;
        let prepared = vega_store::mcp_config::prepare_reconciliation(&self.store, bytes.as_deref(), validate_yaml_row)?;
        Ok(crate::database_writer::commit(&self.root.join("database.db"), self.store.database_identity(), writer::BeginMcpReconciliation { registry: self.service.registry.clone(), prepared })??)
    }

    fn save(&self, intent: vega_store::mcp_config::ReconciliationIntent) -> Result<vega_store::mcp_config::PreparedConfirmation, Box<dyn std::error::Error>> {
        Ok(intent.save(&self.store, &self.config(), vega_store::yaml_config::begin(&self.config())?)?)
    }

    fn confirm(&self, prepared: vega_store::mcp_config::PreparedConfirmation) -> Result<(), Box<dyn std::error::Error>> {
        crate::database_writer::commit(&self.root.join("database.db"), self.store.database_identity(), writer::ConfirmMcpReconciliation { registry: self.service.registry.clone(), prepared })??;
        Ok(())
    }

    fn pending(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(self.store.conn().query_row("SELECT pending_declarations IS NOT NULL FROM mcp_config_state", [], |row| row.get(0))?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); }
}

#[test]
fn import_is_disabled_until_file_and_sql_confirmation_and_recovers_lost_intent() -> TestResult {
    let fixture = Fixture::new()?;
    let declaration = fixture.declaration();
    fixture.write(std::slice::from_ref(&declaration))?;
    let intent = fixture.begin()?;
    assert!(fixture.pending()?);
    assert!(mcp_servers::list(fixture.store.conn())?.is_empty());
    assert!(vega_store::mcp_config::projection_current(fixture.store.conn(), &fixture.config()).is_err());
    drop(intent);
    let recovered = fixture.begin()?;
    let confirmation = fixture.save(recovered)?;
    fixture.confirm(confirmation)?;
    assert!(!fixture.pending()?);
    let row = mcp_servers::find(fixture.store.conn(), &declaration.id)?.ok_or("missing projection")?;
    assert!(!row.enabled);
    assert_eq!(vega_store::mcp_config::projection_current(fixture.store.conn(), &fixture.config())?, vec![declaration]);
    assert!(vega_store::mcp_config::admitted(fixture.store.conn(), &fixture.config(), &row.id, row.config_revision).is_err());
    Ok(())
}

#[test]
fn saved_but_unconfirmed_file_recovers_without_resaving_or_changing_identity() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write(&[fixture.declaration()])?;
    drop(fixture.save(fixture.begin()?)?);
    let before = std::fs::read(fixture.config())?;
    fixture.confirm(fixture.save(fixture.begin()?)?)?;
    assert_eq!(std::fs::read(fixture.config())?, before);
    assert!(!fixture.pending()?);
    assert_eq!(mcp_servers::list(fixture.store.conn())?.len(), 1);
    Ok(())
}

#[test]
fn metadata_change_after_prepare_rejects_intent_without_partial_projection() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write(&[fixture.declaration()])?;
    let bytes = vega_store::yaml_config::read(&fixture.config())?;
    let prepared = vega_store::mcp_config::prepare_reconciliation(&fixture.store, bytes.as_deref(), validate_yaml_row)?;
    fixture.store.conn().execute("UPDATE mcp_config_state SET generation=generation+1", [])?;
    let result = crate::database_writer::commit(&fixture.root.join("database.db"), fixture.store.database_identity(), writer::BeginMcpReconciliation { registry: fixture.service.registry.clone(), prepared })?;
    assert!(result.is_err());
    assert!(!fixture.pending()?);
    assert!(mcp_servers::list(fixture.store.conn())?.is_empty());
    Ok(())
}

#[test]
fn changed_file_after_save_cannot_admit_stale_projection_and_reconciles_new_source() -> TestResult {
    let fixture = Fixture::new()?;
    let mut declaration = fixture.declaration();
    fixture.write(std::slice::from_ref(&declaration))?;
    let confirmation = fixture.save(fixture.begin()?)?;
    declaration.display_name = "Externally changed".into();
    fixture.write(std::slice::from_ref(&declaration))?;
    fixture.confirm(confirmation)?;
    assert!(vega_store::mcp_config::projection_current(fixture.store.conn(), &fixture.config()).is_err());
    fixture.service.reconcile_declarations().map_err(|_| "reconcile new source")?;
    assert_eq!(vega_store::mcp_config::projection_current(fixture.store.conn(), &fixture.config())?, vec![declaration]);
    Ok(())
}

#[test]
fn prepared_confirmation_is_bound_to_database_and_sql_source() -> TestResult {
    let fixture = Fixture::new()?;
    let other = Fixture::new()?;
    fixture.write(&[fixture.declaration()])?;
    let confirmation = fixture.save(fixture.begin()?)?;
    let result = crate::database_writer::commit(&other.root.join("database.db"), other.store.database_identity(), writer::ConfirmMcpReconciliation { registry: other.service.registry.clone(), prepared: confirmation })?;
    assert!(result.is_err());
    assert!(mcp_servers::list(other.store.conn())?.is_empty());
    let confirmation = fixture.save(fixture.begin()?)?;
    fixture.store.conn().execute("UPDATE mcp_config_state SET generation=generation+1", [])?;
    assert!(fixture.confirm(confirmation).is_err());
    assert!(fixture.pending()?);
    assert!(mcp_servers::list(fixture.store.conn())?.is_empty());
    Ok(())
}
