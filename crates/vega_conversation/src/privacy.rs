use crate::types::ConversationError;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use vega_runtime::{CredentialConfidence, CredentialExceptionFilter};
use vega_store::Store;

const KEY_REF: &str = "vega-privacy-fingerprint-v1";
const RULE_VERSION: &str = "credential-rules-v1";
static ROOTS: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();
static AUTHORITY: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
type ExceptionRevisions = HashMap<(PathBuf, String), u64>;
static EXCEPTION_REVISIONS: OnceLock<Mutex<ExceptionRevisions>> = OnceLock::new();
static KEY_LOCK: Mutex<()> = Mutex::new(());
type LiveCredentials = HashMap<PathBuf, Option<(u64, Vec<String>)>>;
static LIVE: OnceLock<Mutex<LiveCredentials>> = OnceLock::new();

fn unavailable() -> ConversationError {
    ConversationError::Store("凭据检查不可用，请检查凭据设置后重试".into())
}

pub fn register_database(database: PathBuf, root: PathBuf) {
    if let Ok(mut live) = LIVE.get_or_init(Default::default).lock() {
        live.entry(root.clone()).or_insert(None);
    }
    if let Ok(mut roots) = ROOTS.get_or_init(Default::default).lock() {
        roots.insert(database.clone(), root.clone());
    }
    if let Ok(store) = Store::open_read_only(&database) {
        let _ = known(&store, Some(&root));
    }
}

pub fn registered_root(store: &Store) -> Option<PathBuf> {
    ROOTS
        .get_or_init(Default::default)
        .lock()
        .ok()?
        .get(store.database_path()?)
        .cloned()
}

fn known(store: &Store, root: Option<&std::path::Path>) -> Result<Vec<String>, ConversationError> {
    let Some(root) = root else {
        return Ok(Vec::new());
    };
    let before = vega_store::keystore::revision(root);
    let result = crate::mcp_settings::all_owner_credentials(root, store)
        .map_err(|_| unavailable())
        .and_then(|values| {
            if before != vega_store::keystore::revision(root) {
                return Err(unavailable());
            }
            Ok(values)
        });
    if let Ok(mut live) = LIVE.get_or_init(Default::default).lock() {
        live.insert(
            root.to_path_buf(),
            result.as_ref().ok().cloned().map(|values| (before, values)),
        );
    }
    result
}

pub fn safe_live_text(text: &str) -> String {
    match LIVE.get_or_init(Default::default).lock() {
        Ok(roots) => project_live(text, &roots),
        Err(_) => "[凭据检查不可用]".into(),
    }
}

fn project_live(text: &str, roots: &LiveCredentials) -> String {
    if !roots.iter().all(|(root, value)| {
        value
            .as_ref()
            .is_some_and(|(revision, _)| *revision == vega_store::keystore::revision(root))
    }) {
        return "[凭据检查不可用]".into();
    }
    let values: Vec<String> = roots
        .values()
        .filter_map(Option::as_ref)
        .flat_map(|(_, values)| values)
        .cloned()
        .collect();
    vega_runtime::redact_sensitive_credential_text(text, &values)
}

fn fingerprint(key: &[u8], rule: &str, content: &str) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let mut context = ring::hmac::Context::with_key(&key);
    for value in [RULE_VERSION, rule, content] {
        context.update(&(value.len() as u64).to_le_bytes());
        context.update(value.as_bytes());
    }
    context
        .sign()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn key(root: &std::path::Path) -> Result<String, ConversationError> {
    let _lock = KEY_LOCK.lock().map_err(|_| unavailable())?;
    match vega_store::keystore::get_key(root, KEY_REF) {
        Ok(key) => Ok(key),
        Err(vega_store::keystore::Error::Missing) => {
            let key = vega_credentials::random_token().map_err(|_| unavailable())?;
            vega_store::keystore::set_key(root, KEY_REF, &key).map_err(|_| unavailable())?;
            Ok(key)
        }
        Err(_) => Err(unavailable()),
    }
}

pub fn refresh_authority(
    database: &std::path::Path,
    root: &std::path::Path,
) -> Result<(), ConversationError> {
    let result = (|| {
        let store = Store::open(database).map_err(|_| unavailable())?;
        let key = key(root)?;
        let mut owner = known(&store, Some(root))?;
        owner.sort();
        let owner = serde_json::to_string(&owner).map_err(|_| unavailable())?;
        let epoch = fingerprint(key.as_bytes(), "owner-epoch", &owner);
        vega_store::credential_privacy::refresh_epoch(store.conn(), &epoch)
            .map_err(|_| unavailable())?;
        Ok(epoch)
    })();
    let mut epochs = AUTHORITY
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| unavailable())?;
    match result {
        Ok(epoch) => {
            epochs.insert(root.to_path_buf(), epoch);
            Ok(())
        }
        Err(error) => {
            epochs.remove(root);
            if let Ok(mut live) = LIVE.get_or_init(Default::default).lock() {
                live.insert(root.to_path_buf(), None);
            }
            Err(error)
        }
    }
}

fn exception_revision(database: &std::path::Path, thread: &str) -> Option<u64> {
    EXCEPTION_REVISIONS
        .get_or_init(Default::default)
        .lock()
        .ok()
        .map(|revisions| {
            revisions
                .get(&(database.to_path_buf(), thread.to_owned()))
                .copied()
                .unwrap_or(0)
        })
}

fn invalidate_exceptions(
    database: &std::path::Path,
    thread: &str,
) -> Result<(), ConversationError> {
    let mut revisions = EXCEPTION_REVISIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| unavailable())?;
    let revision = revisions
        .entry((database.to_path_buf(), thread.to_owned()))
        .or_default();
    *revision = revision.checked_add(1).ok_or_else(unavailable)?;
    Ok(())
}

pub fn exception_filter(
    database: PathBuf,
    thread: String,
    root: PathBuf,
) -> Result<CredentialExceptionFilter, ConversationError> {
    refresh_authority(&database, &root)?;
    let key = key(&root)?;
    let store = Store::open(&database).map_err(|_| unavailable())?;
    let epoch = AUTHORITY
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| unavailable())?
        .get(&root)
        .cloned()
        .ok_or_else(unavailable)?;
    let revision = exception_revision(&database, &thread).ok_or_else(unavailable)?;
    let allowed: HashSet<_> = vega_store::credential_privacy::exceptions(store.conn(), &thread)
        .map_err(|_| unavailable())?
        .into_iter()
        .collect();
    Ok(Arc::new(move |text, finding| {
        if exception_revision(&database, &thread) != Some(revision) {
            return false;
        }
        if !AUTHORITY
            .get_or_init(Default::default)
            .lock()
            .ok()
            .is_some_and(|epochs| epochs.get(&root) == Some(&epoch))
        {
            return false;
        }
        if finding.confidence == CredentialConfidence::KnownExact {
            return false;
        }
        let Some(content) = text.get(finding.start..finding.end) else {
            return false;
        };
        let digest = fingerprint(key.as_bytes(), finding.rule_id, content);
        allowed.contains(&(finding.rule_id.to_owned(), digest))
    }))
}

pub struct DisplayProjection {
    known: Vec<String>,
    filter: Option<CredentialExceptionFilter>,
}

impl DisplayProjection {
    pub fn load(store: &Store, thread: &str) -> Result<Self, ConversationError> {
        let root = registered_root(store);
        let known = known(store, root.as_deref())?;
        let filter = match (store.database_path(), root) {
            (Some(database), Some(root)) => Some(exception_filter(
                database.to_path_buf(),
                thread.into(),
                root,
            )?),
            _ => None,
        };
        Ok(Self { known, filter })
    }
    pub fn text(&self, text: &str) -> String {
        vega_runtime::redact_sensitive_credential_text_with_policy(
            text,
            &self.known,
            self.filter.as_ref(),
        )
    }
    pub fn offsets(&self, text: &str, offsets: &[usize]) -> (String, Vec<usize>) {
        vega_runtime::redact_sensitive_credential_text_with_offsets_and_policy(
            text,
            &self.known,
            offsets,
            self.filter.as_ref(),
        )
    }
    pub fn json(&self, text: &str) -> String {
        fn visit(value: &mut serde_json::Value, projection: &DisplayProjection) {
            match value {
                serde_json::Value::String(text) => *text = projection.text(text),
                serde_json::Value::Array(items) => {
                    for item in items {
                        visit(item, projection);
                    }
                }
                serde_json::Value::Object(fields) => {
                    let original = std::mem::take(fields);
                    for (name, mut value) in original {
                        visit(&mut value, projection);
                        let projected = projection.text(&name);
                        let mut name = projected.clone();
                        let mut suffix = 0usize;
                        while fields.contains_key(&name) {
                            suffix += 1;
                            name = format!("{projected}_{suffix}");
                        }
                        fields.insert(name, value);
                    }
                }
                _ => {}
            }
        }
        let Ok(mut value) = serde_json::from_str(text) else {
            return self.text(text);
        };
        visit(&mut value, self);
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivacyFinding {
    pub source_label: String,
    pub message_id: Option<String>,
    pub source_id: String,
    pub field: String,
    pub rule: String,
    pub start: usize,
    pub end: usize,
    pub exact: bool,
    pub cleanable: bool,
    pub excepted: bool,
    pub fingerprint: String,
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivacyReview {
    pub source_fingerprint: String,
    pub findings: Vec<PrivacyFinding>,
}

pub fn review(store: &Store, thread: &str) -> Result<PrivacyReview, ConversationError> {
    let root = registered_root(store).ok_or_else(unavailable)?;
    let projection = DisplayProjection::load(store, thread)?;
    let key = key(&root)?;
    let source = vega_store::credential_privacy::source_fingerprint(store.conn(), thread)
        .map_err(|_| unavailable())?;
    let exceptions = vega_store::credential_privacy::exceptions(store.conn(), thread)
        .map_err(|_| unavailable())?;
    let mut findings = Vec::new();
    for (id, field, text) in
        vega_store::credential_privacy::fields(store.conn(), thread).map_err(|_| unavailable())?
    {
        for finding in vega_runtime::inspect_credential_text(&text, &projection.known) {
            let Some(content) = text.get(finding.start..finding.end) else {
                continue;
            };
            let digest = fingerprint(key.as_bytes(), finding.rule_id, content);
            let exact = finding.confidence == CredentialConfidence::KnownExact;
            findings.push(PrivacyFinding {
                source_label: vega_store::credential_privacy::source_label(
                    store.conn(),
                    &field,
                    &id,
                )
                .map_err(|_| unavailable())?,
                message_id: vega_store::credential_privacy::source_message_id(
                    store.conn(),
                    &field,
                    &id,
                )
                .map_err(|_| unavailable())?,
                source_id: projection.text(&id),
                field: field.clone(),
                rule: finding.rule_id.into(),
                start: finding.start,
                end: finding.end,
                exact,
                cleanable: finding.confidence != CredentialConfidence::Low
                    && !exceptions
                        .iter()
                        .any(|(rule, value)| !exact && rule == finding.rule_id && value == &digest),
                excepted: !exact
                    && exceptions
                        .iter()
                        .any(|(rule, value)| rule == finding.rule_id && value == &digest),
                fingerprint: digest,
                preview: {
                    let (safe, offsets) = projection.offsets(&text, &[finding.start, finding.end]);
                    let preview = offsets
                        .first()
                        .zip(offsets.get(1))
                        .and_then(|(start, end)| Some((safe.get(..*start)?, safe.get(*end..)?)));
                    match preview {
                        Some((before, after)) => {
                            let before: String = before.chars().rev().take(64).collect();
                            let before: String = before.chars().rev().collect();
                            let after: String = after.chars().take(96).collect();
                            format!("{before}[已遮盖]{after}")
                        }
                        None => "[已遮盖]".into(),
                    }
                },
            });
            if findings.len() >= 500 {
                return Err(ConversationError::Store(
                    "检测结果过多，请先缩小对话范围".into(),
                ));
            }
        }
    }
    Ok(PrivacyReview {
        source_fingerprint: source,
        findings,
    })
}

pub fn set_exception(
    store: &Store,
    thread: &str,
    snapshot: &PrivacyReview,
    index: usize,
    allow: bool,
) -> Result<(), ConversationError> {
    let current = review(store, thread)?;
    if current.source_fingerprint != snapshot.source_fingerprint {
        return Err(ConversationError::Store("对话已变化，请重新检查".into()));
    }
    let finding = current
        .findings
        .get(index)
        .filter(|finding| !finding.exact)
        .ok_or_else(unavailable)?;
    if snapshot.findings.get(index) != Some(finding) {
        return Err(unavailable());
    }
    let database = store.database_path().ok_or_else(unavailable)?;
    invalidate_exceptions(database, thread)?;
    vega_store::credential_privacy::set_exception_checked(
        store.conn(),
        thread,
        &snapshot.source_fingerprint,
        &finding.rule,
        &finding.fingerprint,
        allow,
    )
    .map_err(|_| ConversationError::Store("例外未保存，对话可能已变化，请重新检查".into()))
}

pub fn permanently_clean(
    store: &Store,
    thread: &str,
    snapshot: &PrivacyReview,
) -> Result<(), ConversationError> {
    let current = review(store, thread)?;
    if !current.findings.iter().any(|finding| finding.cleanable) {
        return Err(ConversationError::Store("没有需要永久清理的凭据".into()));
    }
    if current != *snapshot {
        return Err(ConversationError::Store(
            "检测结果已变化，请重新检查后确认".into(),
        ));
    }
    let projection = DisplayProjection::load(store, thread)?;
    vega_store::credential_privacy::clean(
        store.conn(),
        thread,
        &snapshot.source_fingerprint,
        |text, offsets| projection.offsets(text, offsets),
    )
    .map_err(|_| ConversationError::Store("清理未保存，对话可能已变化，请重新检查".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(text: &str) -> (Store, tempfile::TempDir, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let database = data.path().join("privacy.db");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        store.conn().execute_batch("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('p','/synthetic','Synthetic',0,0); INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES('t','p','model',0,0),('other','p','model',0,0);").unwrap();
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('u','t',1,'user',?1,'done',0)", [text]).unwrap();
        register_database(database, config.path().to_path_buf());
        (store, data, config)
    }

    #[test]
    fn history_projection_masks_owner_values_without_mutating_original() {
        let secret = "synthetic-owner-value-DO-NOT-EXPOSE";
        let (store, _data, config) = fixture(secret);
        vega_store::keystore::set_key(config.path(), "test-provider", secret).unwrap();
        let page = crate::history::latest_history_page(&store, "t", 20).unwrap();
        assert!(!format!("{page:?}").contains(secret));
        let original: String = store
            .conn()
            .query_row("SELECT content FROM messages WHERE id='u'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(original, secret);
        let review = review(&store, "t").unwrap();
        assert!(review.findings.iter().any(|finding| finding.exact));
        assert!(!format!("{review:?}").contains(secret));
        assert!(set_exception(&store, "t", &review, 0, true).is_err());
    }

    #[test]
    fn exceptions_are_reversible_and_never_bypass_owner_matches() {
        let token = "ghp_syntheticTestValue0123456789";
        let (store, _data, config) = fixture(token);
        let snapshot = review(&store, "t").unwrap();
        assert!(!snapshot.findings.is_empty());
        set_exception(&store, "t", &snapshot, 0, true).unwrap();
        assert_eq!(
            DisplayProjection::load(&store, "t").unwrap().text(token),
            token
        );
        assert_ne!(
            DisplayProjection::load(&store, "other")
                .unwrap()
                .text(token),
            token
        );
        assert_ne!(
            DisplayProjection::load(&store, "t")
                .unwrap()
                .text(&format!("{token}changed")),
            format!("{token}changed")
        );
        let allowed = review(&store, "t").unwrap();
        let old_filter = exception_filter(
            store.database_path().unwrap().to_path_buf(),
            "t".into(),
            config.path().to_path_buf(),
        )
        .unwrap();
        let detection = vega_runtime::inspect_credential_text(token, &[]).remove(0);
        assert!(old_filter(token, &detection));
        set_exception(&store, "t", &allowed, 0, false).unwrap();
        assert!(!old_filter(token, &detection));
        assert_ne!(
            DisplayProjection::load(&store, "t").unwrap().text(token),
            token
        );
        let snapshot = review(&store, "t").unwrap();
        set_exception(&store, "t", &snapshot, 0, true).unwrap();
        vega_store::keystore::set_key(config.path(), "owner-now", token).unwrap();
        assert_ne!(
            DisplayProjection::load(&store, "t").unwrap().text(token),
            token
        );
    }

    #[test]
    fn cleanup_rejects_stale_preview_and_preserves_other_thread() {
        let secret = "ghp_cleanupSynthetic0123456789";
        let (store, _data, _config) = fixture(secret);
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('other-user','other',1,'user',?1,'done',0)", [secret]).unwrap();
        let stale = review(&store, "t").unwrap();
        store
            .conn()
            .execute(
                "UPDATE messages SET content=?1 WHERE id='u'",
                [format!("updated {secret}")],
            )
            .unwrap();
        assert!(permanently_clean(&store, "t", &stale).is_err());
        let fresh = review(&store, "t").unwrap();
        permanently_clean(&store, "t", &fresh).unwrap();
        let original: String = store
            .conn()
            .query_row("SELECT content FROM messages WHERE id='u'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!original.contains(secret));
        let other: String = store
            .conn()
            .query_row(
                "SELECT content FROM messages WHERE id='other-user'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(other, secret);
    }

    #[test]
    fn live_cache_fails_closed_after_rotation_and_recovers_after_refresh() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let one = "opaqueSyntheticOwnerOne";
        let two = "opaqueSyntheticOwnerTwo";
        vega_store::keystore::set_key(first.path(), "owner", one).unwrap();
        vega_store::keystore::set_key(second.path(), "owner", two).unwrap();
        let mut cache = HashMap::from([
            (
                first.path().to_path_buf(),
                Some((
                    vega_store::keystore::revision(first.path()),
                    vec![one.into()],
                )),
            ),
            (
                second.path().to_path_buf(),
                Some((
                    vega_store::keystore::revision(second.path()),
                    vec![two.into()],
                )),
            ),
        ]);
        let text = format!("{one} {two}");
        assert!(!project_live(&text, &cache).contains(one));
        assert!(!project_live(&text, &cache).contains(two));
        let rotated = "opaqueSyntheticOwnerRotated";
        vega_store::keystore::set_key(first.path(), "owner", rotated).unwrap();
        assert_eq!(project_live(rotated, &cache), "[凭据检查不可用]");
        cache.insert(
            first.path().to_path_buf(),
            Some((
                vega_store::keystore::revision(first.path()),
                vec![rotated.into()],
            )),
        );
        assert_ne!(project_live(rotated, &cache), "[凭据检查不可用]");
        assert!(!project_live(rotated, &cache).contains(rotated));
        cache.insert(first.path().to_path_buf(), None);
        assert_eq!(project_live(rotated, &cache), "[凭据检查不可用]");
    }

    #[test]
    fn failed_authority_key_read_invalidates_live_cache_and_old_filter() {
        let (store, data, config) = fixture("ghp_syntheticAuthority0123456789");
        let snapshot = review(&store, "t").unwrap();
        set_exception(&store, "t", &snapshot, 0, true).unwrap();
        let filter = exception_filter(
            data.path().join("privacy.db"),
            "t".into(),
            config.path().into(),
        )
        .unwrap();
        let finding =
            vega_runtime::inspect_credential_text("ghp_syntheticAuthority0123456789", &[])
                .remove(0);
        assert!(filter("ghp_syntheticAuthority0123456789", &finding));
        std::fs::write(
            config.path().join("credentials/credentials.toml"),
            "invalid = [",
        )
        .unwrap();
        assert!(refresh_authority(&data.path().join("privacy.db"), config.path()).is_err());
        assert!(!filter("ghp_syntheticAuthority0123456789", &finding));
        let cached = LIVE
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .get(config.path())
            .cloned();
        assert_eq!(cached, Some(None));
        LIVE.get_or_init(Default::default)
            .lock()
            .unwrap()
            .remove(config.path());
    }

    #[test]
    fn exception_store_cas_rejects_mutation_after_review_without_writing() {
        let (store, _data, _config) = fixture("ghp_syntheticCas0123456789");
        let snapshot = review(&store, "t").unwrap();
        let finding = &snapshot.findings[0];
        store
            .conn()
            .execute("UPDATE messages SET content='changed' WHERE id='u'", [])
            .unwrap();
        assert!(
            vega_store::credential_privacy::set_exception_checked(
                store.conn(),
                "t",
                &snapshot.source_fingerprint,
                &finding.rule,
                &finding.fingerprint,
                true
            )
            .is_err()
        );
        assert!(
            vega_store::credential_privacy::exceptions(store.conn(), "t")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn review_preview_preserves_local_context_without_adjacent_owner_values() {
        let secret = "synthetic-adjacent-owner-value";
        let text = format!(
            "[已遮盖] {} context-before {secret} context-after",
            "远".repeat(200)
        );
        let (store, _data, config) = fixture(&text);
        vega_store::keystore::set_key(config.path(), "test", secret).unwrap();
        let snapshot = review(&store, "t").unwrap();
        let finding = snapshot
            .findings
            .iter()
            .find(|finding| finding.exact)
            .unwrap();
        assert!(!finding.preview.contains(secret));
        assert!(finding.preview.contains("context-before"));
        assert!(finding.preview.contains("context-after"));
        assert!(finding.preview.chars().count() <= 165);
    }

    #[test]
    fn cleanup_covers_frozen_request_and_plan_note_and_preserves_images() {
        let secret = "synthetic-frozen-owner-value";
        let (store, _data, config) = fixture("ordinary message");
        vega_store::keystore::set_key(config.path(), "test", secret).unwrap();
        store
            .conn()
            .execute(
                "UPDATE messages SET plan_review_note=?1 WHERE id='u'",
                [secret],
            )
            .unwrap();
        let payload = serde_json::json!({"content": secret, "images": [[1, 2, 3]]}).to_string();
        store.conn().execute("INSERT INTO pending_messages(id,thread_id,ordinal,behavior,generation,payload,model_content,created_at) VALUES('q','t',0,'queue','g',?1,?2,0)", [payload.as_str(), secret]).unwrap();
        let snapshot = review(&store, "t").unwrap();
        assert!(snapshot.findings.iter().any(|f| f.field == "pending.model"));
        permanently_clean(&store, "t", &snapshot).unwrap();
        let (payload, model): (String, String) = store
            .conn()
            .query_row(
                "SELECT payload,model_content FROM pending_messages WHERE id='q'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(!payload.contains(secret));
        assert!(!model.contains(secret));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&payload).unwrap()["images"],
            serde_json::json!([[1, 2, 3]])
        );
        let note: String = store
            .conn()
            .query_row(
                "SELECT plan_review_note FROM messages WHERE id='u'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!note.contains(secret));
    }

    #[test]
    fn raw_content_fence_rejects_same_sequence_edit_after_review() {
        let (store, _data, _config) = fixture("ghp_syntheticRawFence0123456789");
        store.conn().execute("INSERT INTO pending_messages(id,thread_id,ordinal,behavior,generation,payload,model_content,state,created_at) VALUES('u','t',0,'queue','g','{}','frozen request','delivered',0)", []).unwrap();
        let snapshot = review(&store, "t").unwrap();
        store
            .conn()
            .execute(
                "UPDATE messages SET content='changed original' WHERE id='u'",
                [],
            )
            .unwrap();
        assert!(
            vega_store::credential_privacy::clean(
                store.conn(),
                "t",
                &snapshot.source_fingerprint,
                |text, _| (text.into(), vec![])
            )
            .is_err()
        );
    }

    #[test]
    fn json_projection_masks_secret_keys_without_losing_colliding_fields() {
        let projection = DisplayProjection {
            known: vec!["first-owner-key".into(), "second-owner-key".into()],
            filter: None,
        };
        let input = r#"{"first-owner-key":"one","second-owner-key":"two"}"#;
        let safe = projection.json(input);
        assert!(!safe.contains("first-owner-key"));
        assert!(!safe.contains("second-owner-key"));
        let value: serde_json::Value = serde_json::from_str(&safe).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
    }

    #[test]
    fn exception_digest_is_keyed_scoped_and_versioned() {
        assert_ne!(
            fingerprint(b"a", "rule", "value"),
            fingerprint(b"b", "rule", "value")
        );
        assert_ne!(
            fingerprint(b"a", "rule", "value"),
            fingerprint(b"a", "other", "value")
        );
        assert_ne!(
            fingerprint(b"a", "rule", "value"),
            fingerprint(b"a", "rule", "changed")
        );
        assert!(!fingerprint(b"a", "rule", "value").contains("value"));
    }
}
