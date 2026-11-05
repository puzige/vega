use super::*;
use std::os::unix::fs::DirBuilderExt;

struct Fixture {
    root: PathBuf,
    store: Store,
    owner: Vec<String>,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-privacy-plan-{}", ulid::Ulid::generate()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let store = Store::open(root.join("state.db")).unwrap();
        store.migrate().unwrap();
        store.conn().execute("INSERT INTO threads(id,title,model,created_at,updated_at) VALUES('thread','synthetic','synthetic',0,0)", []).unwrap();
        let secret = key(&root).unwrap();
        let owner = vec!["synthetic owner value".to_owned()];
        let epoch = fingerprint(
            secret.as_bytes(),
            "owner-epoch",
            &serde_json::to_string(&owner).unwrap(),
        );
        vega_store::credential_privacy::refresh_epoch(store.conn(), &epoch).unwrap();
        AUTHORITY
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(root.clone(), epoch);
        Self { root, store, owner }
    }

    fn capture(&self, owner: &[String]) -> Result<PreparedRedactionAuthority, CredentialReadError> {
        PreparedRedactionAuthority::capture(
            &self.store,
            "thread",
            &self.root,
            vega_store::keystore::revision(&self.root),
            owner,
            CredentialStorageVersion::capture(&self.root).unwrap(),
            true,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn immutable_exception_policy_is_rejected_when_revocation_commits_after_revision_increment() {
    let fixture = Fixture::new();
    let text = "synthetic finding";
    let digest = fingerprint(
        key(&fixture.root).unwrap().as_bytes(),
        "synthetic-rule",
        text,
    );
    vega_store::credential_privacy::allow(
        fixture.store.conn(),
        "thread",
        "synthetic-rule",
        &digest,
    )
    .unwrap();
    invalidate_exceptions(fixture.store.database_path().unwrap(), "thread").unwrap();
    let authority = fixture.capture(&fixture.owner).unwrap();
    let finding = vega_runtime::CredentialFinding {
        rule_id: "synthetic-rule",
        start: 0,
        end: text.len(),
        confidence: CredentialConfidence::High,
    };
    assert!((authority.policy())(text, &finding));
    vega_store::credential_privacy::revoke(
        fixture.store.conn(),
        "thread",
        "synthetic-rule",
        &digest,
    )
    .unwrap();
    let tx = fixture.store.immediate_transaction().unwrap();
    assert!(authority.validate(&fixture.store).is_err());
    tx.rollback().unwrap();
    let current = fixture.capture(&fixture.owner).unwrap();
    assert!(!(current.policy())(text, &finding));
}

#[test]
fn owner_values_must_match_the_actual_privacy_epoch() {
    let fixture = Fixture::new();
    assert!(fixture.capture(&fixture.owner).is_ok());
    assert!(fixture.capture(&["different owner value".into()]).is_err());
}

#[test]
fn external_credential_file_replacement_invalidates_prepared_authority_without_revision_change() {
    let fixture = Fixture::new();
    let authority = fixture.capture(&fixture.owner).unwrap();
    let revision = vega_store::keystore::revision(&fixture.root);
    let path = fixture.root.join("credentials/credentials.toml");
    let bytes = std::fs::read(&path).unwrap();
    let replacement = fixture.root.join("credentials/replacement.toml");
    std::fs::write(&replacement, bytes).unwrap();
    std::fs::rename(replacement, path).unwrap();
    assert_eq!(vega_store::keystore::revision(&fixture.root), revision);
    assert!(authority.validate(&fixture.store).is_err());
}
