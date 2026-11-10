use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    store: Store,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("vega-redaction-token-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        store.conn().execute_batch(
            "INSERT INTO threads(id,model,created_at,updated_at) VALUES('source','model',1,1);",
        )?;
        let key = key(&root)?;
        let epoch = fingerprint(key.as_bytes(), "owner-epoch", "[]");
        vega_store::credential_privacy::refresh_epoch(store.conn(), &epoch)?;
        AUTHORITY
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "authority lock")?
            .insert(root.clone(), epoch);
        Ok(Self { store, root })
    }

    fn authority(&self) -> Result<PreparedRedactionAuthority, CredentialReadError> {
        PreparedRedactionAuthority::capture(
            &self.store,
            "source",
            &self.root,
            vega_store::keystore::revision(&self.root),
            &[],
            CredentialStorageVersion::capture(&self.root)?,
            true,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(mut authority) = AUTHORITY.get_or_init(Default::default).lock() {
            authority.remove(&self.root);
        }
        if let Ok(mut revisions) = EXCEPTION_REVISIONS.get_or_init(Default::default).lock() {
            revisions.retain(|(database, _), _| database != &self.root.join("fixture.db"));
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn redaction_commit_token_validates_after_policy_is_dropped_on_another_connection() -> TestResult {
    let fixture = Fixture::new()?;
    let authority = fixture.authority()?;
    let token = authority.commit_token(&fixture.store)?;
    drop(authority);
    let external = Store::open_existing(fixture.root.join("fixture.db"))?;
    std::thread::spawn(move || token.validate(&external))
        .join()
        .map_err(|_| "token validation panicked")??;
    Ok(())
}

#[test]
fn redaction_commit_token_rejects_credential_and_exception_generations() -> TestResult {
    let fixture = Fixture::new()?;
    let token = fixture.authority()?.commit_token(&fixture.store)?;
    vega_store::keystore::set_key(&fixture.root, "development-only", "synthetic-value")?;
    assert!(token.validate(&fixture.store).is_err());
    let token = fixture.authority()?.commit_token(&fixture.store)?;
    invalidate_exceptions(
        fixture.store.database_path().ok_or("database path")?,
        "source",
    )?;
    assert!(token.validate(&fixture.store).is_err());
    Ok(())
}

#[test]
fn redaction_commit_token_rejects_database_schema_epoch_and_exception_changes() -> TestResult {
    for mutation in [
        "CREATE TABLE changed_schema(id INTEGER)",
        "UPDATE credential_privacy_epoch SET digest='changed' WHERE singleton=1",
        "INSERT INTO credential_privacy_exceptions(thread_id,rule_id,fingerprint) VALUES('source','rule','digest')",
    ] {
        let fixture = Fixture::new()?;
        let token = fixture.authority()?.commit_token(&fixture.store)?;
        fixture.store.conn().execute_batch(mutation)?;
        assert!(token.validate(&fixture.store).is_err(), "{mutation}");
    }
    let first = Fixture::new()?;
    let second = Fixture::new()?;
    let token = first.authority()?.commit_token(&first.store)?;
    assert!(token.validate(&second.store).is_err());
    Ok(())
}

#[test]
fn redaction_commit_token_rejects_storage_replacement_without_generation_change() -> TestResult {
    let fixture = Fixture::new()?;
    let token = fixture.authority()?.commit_token(&fixture.store)?;
    let revision = vega_store::keystore::revision(&fixture.root);
    let path = fixture.root.join("credentials/credentials.toml");
    std::fs::rename(&path, fixture.root.join("credentials/original.toml"))?;
    std::fs::write(&path, "synthetic replacement")?;
    assert_eq!(vega_store::keystore::revision(&fixture.root), revision);
    assert!(token.validate(&fixture.store).is_err());
    Ok(())
}
