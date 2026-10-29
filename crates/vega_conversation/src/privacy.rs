use crate::types::ConversationError;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use vega_runtime::{CredentialConfidence, CredentialExceptionFilter};
use vega_store::Store;

#[cfg(test)]
mod tests;

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
    ConversationError::Store(vega_i18n::t("domain.privacy.unavailable").into())
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
        Err(_) => vega_i18n::t("domain.privacy.check_unavailable").into(),
    }
}

fn project_live(text: &str, roots: &LiveCredentials) -> String {
    if !roots.iter().all(|(root, value)| {
        value
            .as_ref()
            .is_some_and(|(revision, _)| *revision == vega_store::keystore::revision(root))
    }) {
        return vega_i18n::t("domain.privacy.check_unavailable").into();
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

pub(crate) fn invalidate_authority(root: &std::path::Path) {
    if let Ok(mut epochs) = AUTHORITY.get_or_init(Default::default).lock() {
        epochs.remove(root);
    }
    if let Ok(mut live) = LIVE.get_or_init(Default::default).lock() {
        live.insert(root.to_path_buf(), None);
    }
}

pub fn refresh_authority(
    database: &std::path::Path,
    root: &std::path::Path,
) -> Result<(), ConversationError> {
    refresh_authority_values(database, root).map(|_| ())
}

pub(crate) fn refresh_authority_values(
    database: &std::path::Path,
    root: &std::path::Path,
) -> Result<Vec<String>, ConversationError> {
    let result = (|| {
        let store = Store::open(database).map_err(|_| unavailable())?;
        let key = key(root)?;
        let mut owner = known(&store, Some(root))?;
        owner.sort();
        let encoded = serde_json::to_string(&owner).map_err(|_| unavailable())?;
        let epoch = fingerprint(key.as_bytes(), "owner-epoch", &encoded);
        vega_store::credential_privacy::refresh_epoch(store.conn(), &epoch)
            .map_err(|_| unavailable())?;
        Ok((epoch, owner))
    })();
    let mut epochs = AUTHORITY
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| unavailable())?;
    match result {
        Ok((epoch, owner)) => {
            epochs.insert(root.to_path_buf(), epoch);
            Ok(owner)
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

#[derive(Clone, Debug)]
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
    source_label_key: &'static str,
    preview_parts: Option<(String, String)>,
}

impl PartialEq for PrivacyFinding {
    fn eq(&self, other: &Self) -> bool {
        self.message_id == other.message_id
            && self.source_id == other.source_id
            && self.field == other.field
            && self.rule == other.rule
            && self.start == other.start
            && self.end == other.end
            && self.exact == other.exact
            && self.cleanable == other.cleanable
            && self.excepted == other.excepted
            && self.fingerprint == other.fingerprint
            && self.source_label_key == other.source_label_key
            && self.preview_parts == other.preview_parts
    }
}

impl Eq for PrivacyFinding {}

impl PrivacyFinding {
    pub fn source_label(&self) -> &'static str {
        vega_i18n::t(self.source_label_key)
    }

    pub fn preview_text(&self) -> String {
        match &self.preview_parts {
            Some((before, after)) => vega_i18n::t_args(
                "domain.privacy.redacted_preview",
                &[("before", before), ("after", after)],
            ),
            None => vega_i18n::t("domain.privacy.redacted").into(),
        }
    }
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
            let source_label_key =
                vega_store::credential_privacy::source_label_key(store.conn(), &field, &id)
                    .map_err(|_| unavailable())?;
            let (safe, offsets) = projection.offsets(&text, &[finding.start, finding.end]);
            let preview_parts = offsets
                .first()
                .zip(offsets.get(1))
                .and_then(|(start, end)| Some((safe.get(..*start)?, safe.get(*end..)?)))
                .map(|(before, after)| {
                    let before: String = before.chars().rev().take(64).collect();
                    let before: String = before.chars().rev().collect();
                    let after: String = after.chars().take(96).collect();
                    (before, after)
                });
            let mut projected = PrivacyFinding {
                source_label: vega_i18n::t(source_label_key).into(),
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
                preview: String::new(),
                source_label_key,
                preview_parts,
            };
            projected.preview = projected.preview_text();
            findings.push(projected);
            if findings.len() >= 500 {
                return Err(ConversationError::Store(
                    vega_i18n::t("domain.privacy.too_many").into(),
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
        return Err(ConversationError::Store(
            vega_i18n::t("domain.privacy.changed").into(),
        ));
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
    .map_err(|_| ConversationError::Store(vega_i18n::t("domain.privacy.exception_failed").into()))
}

pub fn permanently_clean(
    store: &Store,
    thread: &str,
    snapshot: &PrivacyReview,
) -> Result<(), ConversationError> {
    permanently_clean_with_replay(store, thread, snapshot, None)
}

pub fn permanently_clean_with_replay(
    store: &Store,
    thread: &str,
    snapshot: &PrivacyReview,
    replay: Option<&crate::agent::ReplaySessionRegistry>,
) -> Result<(), ConversationError> {
    let _coordination = crate::permission_contracts::coordinate().map_err(|_| unavailable())?;
    let admission = vega_store::thread_admission::try_pause_database(store.database_identity())
        .map_err(|_| unavailable())?
        .ok_or(ConversationError::ContextCleanupPending)?;
    crate::ordinary_submissions::require_database_settled(store.database_identity(), &admission)
        .map_err(|_| ConversationError::ContextCleanupPending)?;
    let precompute = crate::types::ContextPrecomputeRegistry::global()
        .pause_database(store.database_identity())
        .map_err(|error| ConversationError::Runtime(Arc::new(error)))?;
    if !precompute
        .drain()
        .is_finished()
        .map_err(|error| ConversationError::Runtime(Arc::new(error)))?
    {
        return Err(ConversationError::ContextCleanupPending);
    }
    let current = review(store, thread)?;
    if !current.findings.iter().any(|finding| finding.cleanable) {
        return Err(ConversationError::Store(
            vega_i18n::t("domain.privacy.nothing").into(),
        ));
    }
    if current != *snapshot {
        return Err(ConversationError::Store(
            vega_i18n::t("domain.privacy.findings_changed").into(),
        ));
    }
    let projection = DisplayProjection::load(store, thread)?;
    if let Some(replay) = replay {
        replay
            .invalidate_all_acknowledged()
            .map_err(|_| unavailable())?;
    }
    vega_store::credential_privacy::clean(
        store.conn(),
        thread,
        &snapshot.source_fingerprint,
        |text, offsets| projection.offsets(text, offsets),
    )
    .map_err(|_| ConversationError::Store(vega_i18n::t("domain.privacy.cleanup_failed").into()))
}
