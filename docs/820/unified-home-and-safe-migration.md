# #820 · Unified Vega home and YAML configuration

## 1. Current contract and supersession

Contract v2 replaces v1/v1.1 after an explicit user decision in this conversation:

> 宽松一点不要求强一致性，清空都行，目前用户只有我。我们重启前手动迁移一下就行，不要在代码里面。

Accordingly **no historical data migration is implemented in product code**. Do not scan/import legacy roots, copy a legacy DB or credentials, maintain a migration journal, implement source isolation or rollback UI, or block implementation on excluding old binaries. The user will handle any desired transfer before restarting. This is not permission for the agent to delete, move or overwrite real user data. Old roots remain untouched by the new product. Missing data in the new root represents a new installation, not a completed migration.

The remaining #820 scope still applies: unified root/VEGA_HOME, config.yaml as declaration authority including model preferences and MCP, separate protected credentials, SQLite in data/vega.db, consistent path consumers, unchanged permission and MCP lifecycle boundaries. No unrelated UI or protocols.

Reviewed initial baseline: `632a5a2e554cecadb06d2a9bfa53e5fc3d6730a2`. Before implementation fetch/rebase incorporated `c6568bb821b24c7512f73dc3b5ea3b2fb3f16f0b` (#821/#823 UI changes, no storage delta). Current saved partial head before this contract revision: `7608a6c1a13b2e37bb02f0851a03e7202b51a695`. Historical v1 spec remains at `93ae6c763b7176ff953526a1fc8c0038c7e8b67c`; its migration acceptance is superseded, not a shipping requirement.

## 2. Root and files

```text
${VEGA_HOME or native user-home/.vega}/
  config.yaml
  credentials/credentials.toml
  data/vega.db
  data/scratch/<owner>/
  skills/
  cache/
  logs/
  run/
  backups/
```

`AppPaths` resolves an explicit nonempty absolute VEGA_HOME first; empty/relative/traversal-containing override is an error, never fallback. Without override resolve the native home (Windows USERPROFILE/native home, not solely HOME). OS paths support Unicode/spaces and do not require UTF-8. Pure resolution creates nothing. Reject unsafe existing links/object types and inaccessible roots before startup initialization. Config and credentials contain private material/references; private writes use owner-only files/directories and retain no-follow protections.

Old XDG and macOS bundle-id directories are not automatically discovered or written. Do not copy unknown existing ~/.vega contents over new files. Existing valid new-format root is used as-is; invalid existing configuration or incompatible DB fails visibly, not default-overwritten. Missing home and invalid explicit roots must reach startup errors, including updater consumers; do not quietly start a second root.

Historical images are SQLite BLOBs, not an invented attachment directory. Scratch remains persistent data under database parent; external project paths remain external. Skills sources under config root follow the root. Owned updater settings and model catalog move by resolver only; no old-file import is implied.

## 3. YAML service

One versioned UTF-8 mapping: `schema_version: 1`, existing `providers`, `defaults`, `ui`, `agent`, `credential_bindings`, plus `reasoning`, `model_capabilities`, and `mcp_servers`. Preserve existing provider/ref vocabulary. Existing reasoning and model capability typed structures become sections in config.yaml; they are user data, not disposable caches. All UI/settings/service writers use the same document service; no parallel preference TOML writer remains.

Reuse `serde-saphyr =0.0.29` deserialize for typed validation; use approved `yaml-edit =0.3.6` without default features for lossless edits. Bound input to 4 MiB, depth to 64 and total nodes to 100,000. Reject duplicate keys, multiple documents, unsupported version, aliases/anchors/merge keys/custom tags. No interpolation, includes, command execution or network during parsing. Unknown fields in supported schema retain their text but confer no behavior. Validate the whole document before applying capability changes. Parsing errors never expose raw credential-bearing content in logs/errors and never trigger default overwrites.

UI patch saves retain comments/unknown fields/untouched entries, including within mappings and identifiable service/provider lists. No-op preserves bytes. File edits compare captured content with latest bytes under a stable cross-process lock, create a unique owner-only temporary file, sync, replace atomically and sync parent. A stable lock inode is not the replaceable YAML inode. Detect concurrent/external edits and return conflicts, not blind overwrites. Non-cooperating editors are not covered by advisory exclusion; no absolute CAS claim.

Copied MCP credential-binding templates use a YAML sequence; a local server with no environment slots produces `credential_bindings: []`, never null. Both empty and populated templates must load through the real config reader after any required origin is filled. This clarifies the existing valid-YAML contract following PR #824 independent delta review; it does not change activation or credential authority.

## 4. MCP authority and lifecycle

Each YAML declaration contains explicit immutable `id`, display name, transport-specific settings, credential references and enabled intent. Display name/order is not identity. DB keeps historical identity/revisions/tombstones, OAuth lifecycle, pending cleanup, live activation grants and audit. YAML is the editable declaration authority, not a second independent SQLite configuration source. Never write token values or connection errors/lifecycle details into YAML.

At startup and before service/registry admission, reconcile validated YAML to DB projection. Imported history is a manual user operation, not automatic migration: if manually copied DB/config disagree, YAML declaration governs current intent while historical IDs/tombstones remain readable; do not regenerate IDs for existing declarations. Reject duplicate/reassigned/deleted IDs. A file-supplied revision/grant does not authorize calls. Preserve #802 local activation/Confirm/FullAccess and test-connection semantics.

Use an additive generation/pending-publication journal for service edits. Persist protected candidate intent before YAML publication; reconcile the durable YAML generation transactionally into DB before activation/admission; recover interrupted publication before any MCP execution or cleanup. Conflicting third-party bytes block instead of choosing a side. No requirement for a cross-file atomic transaction, but no silent loss of the last accepted YAML or unaudited activation is allowed. Semantically unchanged comments/order do not revoke services. Semantic changes use existing revision/revocation and cleanup rules. Removing a declaration tombstones identity, never reassigns historical audit to another service.

All create/replace/enable/delete, local credential rotation and OAuth install/disconnect paths must publish their declaration through this service. Runtime registry must not run a stale DB projection while YAML is invalid/unreconciled. Reconciliation itself launches no program or network request. Credentials remain separate; missing refs are explicit, no synthetic success.

## 5. Permission boundaries

Defaults in YAML are not authority provenance. Existing host-validated FullAccess grant/approval records, authority epochs and permission-reset journals remain DB authority. Manual file edits cannot forge grants, restore revoked services, approve frozen tasks or replay history. Preserve existing permission reset recovery and pending credential cleanup. New directory layout does not authorize external calls or historical replay.

## 6. Implementation map

1. Update this spec before revised code; record explicit acceptance change in #820.
2. `vega_store::paths`: AppPaths/native home/VEGA_HOME with typed failure; production wrappers and all consumers switch together. `startup.rs`, main/updater, window/session, settings/sidebar, runtime/conversation skills and threads use unified paths.
3. New shared YAML document service; config.rs, reasoning.rs and model_capabilities.rs delegate all current reads/writes to sections with preserved snapshot/conflict behavior. No suffix-only TOML parser.
4. MCP store + numbered migration, conversation mcp_settings, runtime registry/admission: YAML projection and durable publication/recovery retaining identity/security. No historical-root migration module.
5. Update technical spec §6, exec-guide references, docs/73 MCP and #802 affected contracts, user configuration/manual offline transfer instructions. UI changes only for actionable existing failure states, with EN/ZH and accessibility if new UI is necessary.
6. Synthetic development behavioral validation in Actions, independent non-writer implementation review, final required CI; no partial path-only release. User real-data manual transfer/test is separate acceptance.

## 7. Acceptance

- Fresh install creates only unified root; custom absolute root isolates; invalid override, missing home, unsafe/inaccessible existing target fail without fallback or overwriting. Spaces/Unicode/native-home behavior covered where available.
- No legacy discovery/import/copy/delete/rewriting occurs. Agent never touches actual user runtime data. Documentation tells the user that histories/credentials/config are not automatically migrated.
- Config is real YAML, human-editable; app/provider/model/MCP settings survive UI saves and restart. No-op/comment/unknown field preservation, strict key/version/type/budget failures, and detected concurrent saves covered.
- YAML is MCP authority. Identity/revision/history, enabled intent, pending cleanup/OAuth/permission state and existing activation boundaries stay correct. YAML/DB interruption recovers or blocks; file edit cannot grant authority; invalid YAML cannot admit calls.
- Provider/Bearer/OAuth secrets remain protected in credentials, references unchanged, no token values in YAML/DB/diagnostics. Existing secure store unsupported platforms remain explicit, not advertised as compatible.
- History in a manually placed compatible DB stays readable without replay; images remain BLOBs, scratch/workspace relationships and project paths are not arbitrarily rewritten. App does not perform a migration or promise transfer completeness.
- New version writes no old-root configuration/DB/cache; no cloud-drive concurrency or cross-version synchronization claims.

Local checks are static/format/diff only. No Vega execution/build/check/test locally. Necessary development Rust behavioral tests run only in existing Actions, original source/results retained before removal; final head contains no Rust tests/exclusive support per current policy. Preserve existing Python checks. Record `no Rust tests` honestly, not as behavioral success. Independent review and final-head required CI precede merge; real integration/user manual transfer is user-tested. No CI or implementation review has run yet for the partial foundation.

## 8. Approved dependency

User explicitly approved `yaml-edit = "=0.3.6"`, `default-features=false`, Apache-2.0, MSRV 1.70. Tagged release commit `780a686e5b882941b710c0ef19fd8d061d03bcc8`; registry checksum `5df55a38673279c691015377f55503186026bc41d448892b01886cb7475cd121`. Normal closure approved: rowan 0.16.1, countme 3.0.1, hashbrown 0.14.5 (defaults off, inline-more), rustc-hash 1.1.0, text-size 1.1.1; all MIT/Apache-2.0. rowan 0.16.2 is yanked. Do not enable optional regex/binary/editor instrumentation or upstream dev dependencies. Verify actual lock resolution/feature union. Existing serde-saphyr deserialize is reused; no include/properties features. With automatic migration removed, no SQLite backup feature is needed.

## 9. Manual transfer documentation

Before restarting, the user may independently stop the old app/helpers and make a full offline backup of desired old config, credentials, DB/WAL and owned persistent files. The agent does not run such operations without explicit paths and authorization. Old TOML is not a YAML file with a renamed extension; desired values must be converted to the documented YAML schema. SQLite must be transferred as a consistent offline set, not by copying a live main file. User may instead start fresh. Warn that old app versions still use old paths, and copied absolute scratch/skill bindings may require manual attention. Do not claim automatic migration or validation happened.

## 10. Change record

- v1/v1.1: strong historical migration contract, source-exclusion assessment and pure inert AppPaths foundation.
- v2: explicit user acceptance change removes all product migration/source-exclusion/strong consistency/automatic rollback work. Keep unified layout/YAML/MCP/security implementation; user manually handles desired historical data before restart. No agent authorization to clear real data.
