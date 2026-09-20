//! Private, bounded Skills run state for a trusted Store to persist.
//!
//! The Store must keep `RunBinding` and the whole-snapshot digest separately
//! from the snapshot bytes. The digest is an application persistence integrity
//! check, not protection against a process able to rewrite trusted metadata.

use super::*;
use crate::skills::source::{MAX_REFERENCE_BYTES, valid_reference_path};

const SNAPSHOT_VERSION: u32 = 1;
const MAX_CATALOG_ENTRIES: usize = 512;
const MAX_CATALOG_EXCLUSIONS: usize = 512;
pub(super) const MAX_FROZEN_REFERENCES: usize = 128;

/// Maximum persisted JSON bytes for one frozen Skills run.
pub const MAX_RUN_SNAPSHOT_BYTES: usize = 1024 * 1024;

/// Trusted Store-owned identity and revocation epoch for one run. Persist this
/// outside the opaque snapshot bytes, and regenerate it only for a new run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBinding {
    run_id: String,
    thread_id: String,
    consent_generation: u64,
    revocation_generation: u64,
    catalog_sha256: String,
}

impl RunBinding {
    pub fn for_catalog(
        run_id: &str,
        thread_id: &str,
        consent_generation: u64,
        revocation_generation: u64,
        catalog: &SkillCatalog,
    ) -> Result<Self, SkillError> {
        if !valid_opaque_id(run_id) || !valid_opaque_id(thread_id) {
            return Err(SkillError::InvalidFormat);
        }
        validate_catalog(catalog)?;
        Self::from_trusted_parts(
            run_id,
            thread_id,
            consent_generation,
            revocation_generation,
            &catalog_digest(catalog)?,
        )
    }

    /// Rebuild only from separately stored trusted run metadata. This does
    /// not deserialize a binding from the lower-trust snapshot payload; the
    /// caller must also compare current Store authority generations before
    /// restoring a run.
    pub fn from_trusted_parts(
        run_id: &str,
        thread_id: &str,
        consent_generation: u64,
        revocation_generation: u64,
        catalog_sha256: &str,
    ) -> Result<Self, SkillError> {
        let binding = Self {
            run_id: run_id.into(),
            thread_id: thread_id.into(),
            consent_generation,
            revocation_generation,
            catalog_sha256: catalog_sha256.into(),
        };
        if !binding.valid() {
            return Err(SkillError::InvalidFormat);
        }
        Ok(binding)
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn consent_generation(&self) -> u64 {
        self.consent_generation
    }

    pub fn revocation_generation(&self) -> u64 {
        self.revocation_generation
    }

    pub fn catalog_sha256(&self) -> &str {
        &self.catalog_sha256
    }

    fn valid(&self) -> bool {
        valid_opaque_id(&self.run_id)
            && valid_opaque_id(&self.thread_id)
            && valid_hash(&self.catalog_sha256)
    }
}

/// Opaque export. Store `bytes` privately and retain `sha256` in trusted run
/// metadata independently of those bytes; both are required for restore.
#[derive(Clone, PartialEq, Eq)]
pub struct SkillRunSnapshot {
    bytes: Vec<u8>,
    sha256: String,
}

impl std::fmt::Debug for SkillRunSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SkillRunSnapshot")
            .field("private_bytes", &self.bytes.len())
            .finish()
    }
}

impl SkillRunSnapshot {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotPayload {
    version: u32,
    binding: RunBinding,
    catalog: SkillCatalog,
    direct_user: bool,
    active: Vec<ActiveSkill>,
    references: Vec<FrozenReference>,
    reference_bytes: usize,
    cancelled: bool,
}

impl SkillRun {
    /// A new run's binding is created by its trusted controller, never from
    /// Skill text or model output. `new` remains an in-memory-only constructor.
    pub fn new_bound(
        catalog: SkillCatalog,
        direct_user: bool,
        binding: RunBinding,
    ) -> Result<Self, SkillError> {
        if !binding.valid() || catalog_digest(&catalog)? != binding.catalog_sha256 {
            return Err(SkillError::Stale);
        }
        let mut run = Self::new(catalog, direct_user);
        run.binding = Some(binding);
        Ok(run)
    }

    /// Export exact frozen bodies, first-read references and activation order.
    /// This performs no filesystem access and never silently refreshes a file.
    pub fn export_snapshot(&self) -> Result<SkillRunSnapshot, SkillError> {
        let binding = self.binding.as_ref().ok_or(SkillError::InvalidFormat)?;
        let payload = SnapshotPayload {
            version: SNAPSHOT_VERSION,
            binding: binding.clone(),
            catalog: self.catalog.clone(),
            direct_user: self.direct_user,
            active: self.active.clone(),
            references: self.references.values().cloned().collect(),
            reference_bytes: self.reference_bytes,
            cancelled: self.cancelled,
        };
        payload.validate(binding)?;
        let bytes = serde_json::to_vec(&payload).map_err(|_| SkillError::InvalidFormat)?;
        if bytes.len() > MAX_RUN_SNAPSHOT_BYTES {
            return Err(SkillError::TooLarge);
        }
        Ok(SkillRunSnapshot {
            sha256: digest(&bytes),
            bytes,
        })
    }

    /// Restore only under the Store's separately retained binding and whole-
    /// snapshot digest. The controller must first compare live revocation and
    /// consent generations with `expected_binding`; this module has no Store.
    pub fn restore_snapshot(
        bytes: &[u8],
        expected_binding: &RunBinding,
        expected_sha256: &str,
    ) -> Result<Self, SkillError> {
        if bytes.len() > MAX_RUN_SNAPSHOT_BYTES {
            return Err(SkillError::TooLarge);
        }
        if !expected_binding.valid() || !valid_hash(expected_sha256) {
            return Err(SkillError::InvalidFormat);
        }
        if digest(bytes) != expected_sha256 {
            return Err(SkillError::Stale);
        }
        let payload: SnapshotPayload =
            serde_json::from_slice(bytes).map_err(|_| SkillError::InvalidFormat)?;
        payload.validate(expected_binding)?;
        let mut references = BTreeMap::new();
        for reference in payload.references {
            let key = (reference.name.clone(), reference.path.clone());
            if references.insert(key, reference).is_some() {
                return Err(SkillError::InvalidFormat);
            }
        }
        Ok(Self {
            catalog: payload.catalog,
            binding: Some(payload.binding),
            direct_user: payload.direct_user,
            active: payload.active,
            references,
            reference_bytes: payload.reference_bytes,
            cancelled: payload.cancelled,
        })
    }
}

impl SnapshotPayload {
    fn validate(&self, expected: &RunBinding) -> Result<(), SkillError> {
        if self.version != SNAPSHOT_VERSION {
            return Err(SkillError::InvalidFormat);
        }
        if &self.binding != expected || catalog_digest(&self.catalog)? != expected.catalog_sha256 {
            return Err(SkillError::Stale);
        }
        if !self.direct_user && (!self.active.is_empty() || !self.references.is_empty()) {
            return Err(SkillError::InvalidFormat);
        }
        if self.active.len() > MAX_RUN_ACTIVATIONS || self.references.len() > MAX_FROZEN_REFERENCES
        {
            return Err(SkillError::AggregateLimit);
        }
        let mut active_names = BTreeSet::new();
        for active in &self.active {
            if !active_names.insert(active.candidate.name.as_str())
                || active.body.len() > MAX_SKILL_BYTES
                || !valid_hash(&active.body_sha256)
                || digest(active.body.as_bytes()) != active.body_sha256
                || !self
                    .catalog
                    .entries
                    .iter()
                    .any(|entry| entry.candidate == active.candidate && entry.label == active.label)
            {
                return Err(SkillError::InvalidFormat);
            }
        }
        let mut reference_bytes = 0usize;
        let mut reference_keys = BTreeSet::new();
        for reference in &self.references {
            if !active_names.contains(reference.name.as_str())
                || !valid_reference_path(&reference.path)
                || reference.text.len() > MAX_REFERENCE_BYTES
                || reference.text.contains('\0')
                || !reference.lower_trust
                || !valid_hash(&reference.content_sha256)
                || digest(reference.text.as_bytes()) != reference.content_sha256
                || !reference_keys.insert((&reference.name, &reference.path))
            {
                return Err(SkillError::InvalidFormat);
            }
            reference_bytes = reference_bytes
                .checked_add(reference.text.len())
                .ok_or(SkillError::AggregateLimit)?;
        }
        if reference_bytes > MAX_RUN_REFERENCE_BYTES || reference_bytes != self.reference_bytes {
            return Err(SkillError::AggregateLimit);
        }
        Ok(())
    }
}

fn validate_catalog(catalog: &SkillCatalog) -> Result<(), SkillError> {
    if catalog.entries.len() > MAX_CATALOG_ENTRIES
        || catalog.exclusions.len() > MAX_CATALOG_EXCLUSIONS
        || catalog.model_catalog.len() > MAX_CATALOG_BYTES
        || (!catalog.automatic_enabled && !catalog.model_winners.is_empty())
    {
        return Err(SkillError::TooLarge);
    }
    let mut identities = BTreeSet::new();
    for entry in &catalog.entries {
        let candidate = &entry.candidate;
        if !valid_name(&candidate.name)
            || !valid_name(&entry.label)
            || !valid_description(&candidate.description)
            || !candidate.source.snapshot_shape_valid()
            || !valid_hash(&candidate.sha256)
            || candidate.size_bytes == 0
            || candidate.size_bytes > MAX_SKILL_BYTES
            || !identities.insert((candidate.source.identity(), candidate.name.as_str()))
        {
            return Err(SkillError::InvalidFormat);
        }
    }
    for (name, index) in &catalog.model_winners {
        if catalog
            .entries
            .get(*index)
            .map(|entry| entry.candidate.name.as_str())
            != Some(name.as_str())
        {
            return Err(SkillError::InvalidFormat);
        }
    }
    if render_model_catalog(&catalog.entries, &catalog.model_winners)? != catalog.model_catalog {
        return Err(SkillError::InvalidFormat);
    }
    Ok(())
}

fn catalog_digest(catalog: &SkillCatalog) -> Result<String, SkillError> {
    validate_catalog(catalog)?;
    let bytes = serde_json::to_vec(catalog).map_err(|_| SkillError::InvalidFormat)?;
    if bytes.len() > MAX_RUN_SNAPSHOT_BYTES {
        return Err(SkillError::TooLarge);
    }
    Ok(digest(&bytes))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}
