# #820 · Unified Vega home, YAML configuration and safe historical migration

## 1. Status and authority

- User authorization: take over #820, review the proposed approach, land this spec before code, implement only the card scope, prioritize historical data safety.
- Reviewed source baseline: `632a5a2e554cecadb06d2a9bfa53e5fc3d6730a2`; origin fetched and local master updated before creating `feat/820-unified-home`.
- Contract version: 1. Implementation has not started. This document is not a claim that migration, locking, dependency approval or validation has succeeded.
- Issue: <https://github.com/puzige/vega/issues/820>. The issue supplies scope and progress; this document supplies the implementation contract.
- Replaces the old no-migration / XDG configuration / macOS bundle-id data layout decision in [technical spec §6](../vega-tech-spec-p1.md#6-配置与密钥a1-10a11-05) for the completed #820 implementation only. Until the complete cutover is verified, current releases retain their existing behavior.
- Required implementation prerequisites: approval of the lossless YAML editor in §11; demonstrated offline source exclusion and supported SQLite snapshot protocol in §5. No dependency approval is implied by accepting the card.

## 2. Review conclusions and source facts

| Existing source | Verified behavior | Consequence |
| --- | --- | --- |
| `vega_store/src/paths.rs` | XDG config; macOS bundle-id data; HOME-only resolution | Introduce one resolver; retain old resolvers solely for discovery. Do not change defaults before migration is connected. |
| `vega_conversation/src/startup.rs::prepare_default_workspace/prepare_workspace` | Creates directories, opens/migrates DB, recovers permission reset, creates config, recovers automation | Migration gate must precede all of these effects and any equivalent entry point. |
| `vega_store/src/lib.rs::Store::open/open_existing` | Writable connections can change pragmas; default open creates missing DB | Neither is the source inspection API. A source snapshot must not run source schema upgrades or silently create a missing source. |
| `vega_store/src/config.rs::ConfigEdit` | TOML whole-document save, process-local edit mutex | A suffix change is insufficient: require lossless editing, cross-process coordination, CAS and durable replacement. |
| workspace `Cargo.toml`, runtime manifest | `serde-saphyr =0.0.29`, deserialize feature already used | Reuse semantic deserialization; it does not establish lossless editing. Disable include/properties/interpolation extensions. |
| `vega_store/src/image_attachments.rs` | Encoded historical images live in SQLite BLOB rows | Preserve complete rows and bytes through DB backup; do not invent a filesystem attachment migration. |
| `vega_conversation/src/threads.rs::task_project_path` | App-owned standalone scratch lives under DB parent/scratch/owner | Scratch is persistent task content; copy it and relocate only typed owned references. |
| `vega_store/src/model_capabilities.rs` | Model records include user `allowed_efforts` and `image_input` overrides | Merge into YAML user preferences, never discard as derived cache. |
| `vega_store/src/reasoning.rs` | User reasoning profiles and file snapshots | Preserve values and source absence; reject invalid profiles rather than defaulting over them. |
| `vega_store/src/mcp_servers.rs`, conversation `mcp_settings.rs` | SQLite has immutable IDs, revisions, deleting state, OAuth transitions, cleanup queue | YAML authority requires a recoverable projection, not export-and-forget or regenerated IDs. |
| `vega_store/src/keystore.rs` | Separate owner-only store, reference validation, Unix no-follow descriptor IO; unsupported non-Unix boundary | Keep credential format and exact references; never flatten secrets into YAML or weaken file IO. |
| `vega_store/src/delegated_ownership.rs` | Existing scoped flock is not an installation-wide legacy writer fence | A new root lock alone cannot exclude already released apps. Offline exclusion must be proven separately. |

No real user DB, configuration or secrets were read for these conclusions. Fixtures must be synthetic.

## 3. Scope and root contract

```text
${VEGA_HOME or user-home/.vega}/
  config.yaml
  credentials/credentials.toml
  data/vega.db
  data/scratch/<owner>/
  skills/
  logs/
  cache/
  run/
  backups/
```

- `VEGA_HOME` is an explicit absolute OS path, not a shell expression. Empty, relative, inaccessible or unsafe explicit values are errors, never fallback. Support spaces/Unicode; do not require UTF-8 paths merely to resolve a home.
- Without an override, resolve the native user directory on the target platform. Windows must not depend exclusively on HOME. Unsupported secure storage remains an explicit error, not a platform compatibility claim.
- Resolve into an immutable `AppPaths` value. Resolution itself creates nothing. All production consumers receive this value or a path derived from it; no independent environment reads per subsystem.
- Explicit VEGA_HOME isolates an installation. It does not silently import another root. Existing content is validated before initialization; migration requires explicit source selection.
- Existing unknown `~/.vega` content, multiple candidates, or target/source overlap block automatic migration. Never choose by mtime, silently merge, recursively scan arbitrary directories, or overwrite unrelated files.
- The root contains secrets even when config contains only references. New sensitive directories are owner-only and files owner-read/write; existing unsafe access, links/reparse points, unexpected object types and path escapes fail closed. Validate parent chains and use descriptor-based/no-follow operations where supported, including staging and backup IO.
- `run/` contains local leases and temporary state; never import old locks/PIDs/sockets. It is not portable state. Cache may be rebuilt only after durable and user-owned content has been classified.
- Do not implement concurrent cloud-drive SQLite synchronization, unrelated MCP protocols/headers, server numbering UI or unrelated refactoring.

## 4. Configuration authority and projection

### 4.1 YAML document

A versioned single mapping contains `schema_version: 1`, existing providers/defaults/ui/agent/credential_bindings, reasoning profiles, model capability user overrides and `mcp_servers`. Preserve existing names and semantics rather than silently renaming provider IDs or credential refs.

Each MCP item includes immutable `id`, display name, transport-specific settings, credential references and enabled intent. Encode args/env slots structurally rather than JSON strings. Display name, sequence position and readable keys are not identity. IDs must be unique; renaming never changes ownership. Missing IDs for a newly added service receive a new host-generated ID under the config transaction; existing IDs cannot be reassigned to a different historical service.

Runtime authorization is not ordinary YAML configuration. FullAccess grant provenance, approval audit, authority epoch, OAuth lifecycle, cleanup state, deletion tombstones and accepted revision history remain host/DB authority. Preserved legacy grant reference strings do not independently authorize anything. YAML edits cannot mint approvals, resurrect revoked services, steal historical identity or reactivate a suspended/deleting service through a stale file.

### 4.2 Validation and edits

- UTF-8 single document; bound input to 4 MiB, depth to 64 and total nodes to 100,000 before unbounded processing. Reject duplicate mapping keys, non-string configuration keys, custom tags, aliases/anchors, merge keys and unsupported versions. Do not execute commands, substitute environment variables, include other files or perform network access while parsing.
- Validate the entire typed candidate and cross-field constraints before exposing any changed capability. Unknown fields within the supported version are retained without granting behavior; they are not silently deleted by UI saves.
- Keep a lossless syntax document plus typed view. UI edits patch only changed owned fields, preserving comments, unrelated fields and untouched service entries. A no-op must preserve original bytes. Never recover an unreadable/invalid existing document by overwriting it with defaults.
- A stable lock inode is shared by all new-version configuration writers; do not lock the replaceable config inode. Capture the original content digest, re-read and compare under the lock, patch, revalidate, write a unique create-new temporary file, sync, compare again, atomically replace and sync the parent.
- CAS catches observed external changes; cooperative locking cannot guarantee exclusion of an editor that ignores the lock. Do not claim an OS-level atomic compare-and-swap against arbitrary applications. Surface detected conflicts, keep the original/candidate recoverable, and never blindly retry a save over newer bytes.
- External editing is applied through the same whole-candidate validation and projection service before use, including startup and new-run admission. Invalid content blocks new affected operations with a repair/reload action; it is never a source of new permissions.

### 4.3 YAML/SQLite durable coordination

YAML is the only editable declaration; MCP SQLite rows become identity/history and runtime projection, not an independent settings source. Add a numbered additive schema migration for configuration generations and projection journal. Never delete or rewrite prior migration files.

A journal records operation ID, base/candidate digests, base/projected generation and recovery phase, not secrets. Under config and DB coordination: validate candidate; persist prepared intent and protected candidate; publish durable YAML; transactionally reconcile the DB projection and existing revocation rules; mark applied only after both durable representations agree. A crash between YAML and DB enters recovery before any MCP connection, tool admission, automation or credential cleanup. If current bytes match neither known generation, report conflict instead of choosing a side or overwriting user edits.

- Stable IDs/revisions are preserved on initial import, not reset to 1. Accepted semantic changes use existing revision/revocation rules. Revision tokens supplied by a user file are not trusted authority.
- A removed service is tombstoned through the existing lifecycle; historical audit still resolves the original ID. Reordering/comments do not trigger semantic revocation.
- OAuth token installation, disconnect and secret rotation must coordinate intent and projection without writing token/lifecycle details to YAML. Interruption must never delete a live secret or activate an unaudited config.
- Preserve #802 local activation, Confirm/FullAccess, connection-test and frozen-task semantics. A connection test must not toggle enabled. Reconciliation performs no process/network/authentication calls.
- Preserve pending permission reset and cleanup records. Recovery is not permission to replay historical calls or reinterpret completed/cancelled/unknown runs.

## 5. Legacy discovery and consistent source capture

### 5.1 Offline exclusion gate

Before creating target runtime/config/database state, discover only known default and XDG locations and legacy ~/.vega candidates. Acquire a stable migration lock outside any directory to be atomically published. Identify existing new-format complete/pending markers first.

A legacy app does not participate in the new lock. Therefore migration also requires verified offline source ownership: detect active legacy/new installation processes and hold the supported SQLite writer exclusion through capture and publication. A process list or two matching hashes alone is not proof of exclusion. A source DB connection must not upgrade schema, run recovery jobs, checkpoint away evidence or mutate application rows. Validate SQLite WAL locking and backup interaction with the pinned API in CI before selecting the exact protocol; do not assume a read-only `BEGIN EXCLUSIVE` or a second backup connection provides writer exclusion.

All app-owned mutable filesystem content must be covered by the offline protocol, not merely the DB. If the platform cannot establish the required exclusion, return a blocking quit/retry/offline recovery outcome and leave source and target runtime uninitialized. No PID-only stale lock removal, chmod-based exclusion, renaming live source files, killing unrelated processes or new-lock-only safety claims.

This contract does not claim an old binary cannot be launched after migration. Source retention and version/rollback documentation must explain that old versions still target old roots; cross-version concurrent use is unsupported. Any stricter cross-version fence must be demonstrable before being presented as protection.

### 5.2 Inventory

Inventory before copying, including absent/present state, supported schema versions, type, size, content digest, ownership/mode, recognized references and classification:

| Content | Migration action |
| --- | --- |
| config.toml, reasoning-preferences.toml, model-capabilities.toml | Read-only typed import into YAML, semantic equivalence check |
| credentials/ | Safe byte-preserving protected copy; retain exact reference names and format |
| vega.db plus active WAL state | Supported consistent SQLite backup including committed uncheckpointed rows |
| image_attachments BLOBs | Remain in DB; compare row identities and byte digests |
| scratch owned by task/workspace owner | Copy recursively without following links; preserve owner relationships and files |
| delegation/checkpoint/audit/automation/permission state in DB | Preserve every table, identity, terminal state and pending journal |
| skills roots, bindings, owned sources | Preserve owned sources and typed bindings; external sources remain external |
| updater.json, model-catalog and any additional owned entries | Classify by actual writer/reader; migrate durable settings, rebuild only demonstrably derived cache |
| unknown entries | Preserve source; require explicit conflict handling, never silently omit |
| runtime locks/sockets/PIDs | Do not migrate |

This table is a starting inventory, not permission to ignore other source-owned persistent files. Complete the typed-reference/writer inventory before enabling migration. Do not move external projects, ordinary user files, transcript text or arbitrary JSON strings just because they contain an old prefix.

### 5.3 Snapshot and conversion

Use the SQLite online backup API with existing `rusqlite =0.40.2` plus its `backup` feature if validated. Copying only `vega.db` or combining DB/WAL from different moments is forbidden. Snapshot availability does not by itself prove writer exclusion or filesystem consistency. Preserve source application bytes and schema; record any unavoidable SQLite sidecar IO and do not claim byte-identical preservation without verifying it.

Only the staged copy receives additive schema migration and typed owned-path relocation. Reject unsupported future schema, corrupt DB, invalid config/credentials, missing required content, path escapes, source changes, disk full or unsafe permissions. Missing credential references produce explicit needs-attention state; do not replace refs or pretend authentication succeeded. If safe offline credential validation is unavailable, block rather than weakening storage.

## 6. Durable migration state machine

Use a versioned protected journal with operation ID, source identities/digests, destination identity, inventory and phase. Do not persist raw configuration, token values or sensitive command strings in diagnostics/manifests. Content backups can contain secrets and require credential-equivalent protection.

| Durable phase | Allowed work and recovery |
| --- | --- |
| Discovered / AwaitingSelection | Read-only discovery. Cancel creates no empty runtime DB/default config. Conflicting roots require an explicit source choice. |
| Prepared | Source exclusion held; inventory persisted; create-new staging on target filesystem. A foreign/unowned staging directory is a conflict, not something to delete. |
| Captured | Consistent DB and safe files copied and synced. Incomplete capture can be discarded only when proven owned and source is unchanged; otherwise stop for recovery. |
| Converted | YAML import and target-only schema/path changes complete. Conversion is deterministic; recovery never imports IDs a second time. |
| Verified | Config equivalence, integrity, foreign keys, history IDs/counts/digests, BLOBs, owned references, credentials permissions and pending lifecycle states pass. Recheck source exclusion/content identity. |
| Published | Rename owned staging to a previously absent destination, sync parent, publish durable marker. Never replace an existing root or treat mere directory existence as success. |
| Active | Startup uses only the new root after marker and generation validation. Failure remains an explicit recovery error; no fallback to a fresh empty installation. |

A crash between rename and marker is recovered by journal and inventory identity, not by existence heuristics. An inconsistent/unknown journal version or unexpected target bytes blocks. Every transition must tolerate a crash before and after each durable write. Only proven operation-owned temporary content may be cleaned automatically. Retain legacy sources and backups; no automatic source deletion.

Once the new installation has accepted a durable write, simple rollback to legacy is forbidden because it loses new history. Explain the conflict and require an explicit export/restore plan; do not set the old root active just because the new startup failed. Pre-cutover cancellation leaves the legacy installation usable.

## 7. Integration map and ordered implementation

1. Land this spec and dependency decision. Resolve §5.1 source exclusion and pinned backup API using static API inspection plus synthetic cloud validation; do not touch real installations.
2. `vega_store::paths`: add pure `AppPaths`/legacy-discovery APIs and typed errors. Keep production old-layout behavior until the full gate is wired. Add protected filesystem primitives using existing security patterns.
3. `vega_store::config`, reasoning and model_capabilities: unified versioned YAML document, lossless edits, byte snapshots/CAS, cross-process locks and legacy read-only decoders. Update config services rather than creating parallel TOML/YAML writers.
4. `vega_store::mcp_servers`, numbered migrations; conversation `mcp_settings`; runtime registry/tools execution: journaled projection, preserved IDs/revisions, lifecycle and admission checks. Coordinate all UI, OAuth/cleanup and permission-reset writer paths.
5. New migration module; conversation startup/types: preflight, inventory, staging, backup, verification, publication and recovery. Distinguish migration-required, conflict, busy, corrupt and unsupported outcomes; do not collapse them into generic default creation.
6. `vega/src/main.rs`, window/session/settings/sidebar/model_catalog, updater, runtime skills/source and conversation skills/threads: pass AppPaths; remove production hardcoded config.toml and old-root writes. Preserve owned scratch and external paths. Inspect other path users through targeted searches before cutover.
7. Startup UI and EN/ZH strings: source selection/cancel, busy quit-and-retry, invalid config repair, failure retry/recovery and post-write rollback block. Read design guidelines before UI implementation. Show only current actionable state; keyboard-accessible controls, no success claims before actual publication.
8. Update technical spec, exec-guide security/config references, docs/73 MCP contract, #802 affected documentation, configuration and offline backup/restore instructions to match final implemented behavior. Do not describe the target layout as already shipping during partial work.
9. Stop writes at an exact version; non-implementer implementation review; required final-head Actions; only then merge and move to In review for user real migration testing. Issue stays open until user acceptance.

Do not release a path-only/config-only partial implementation as #820 complete. No changes to external-call permission policy, historical tool results or unrelated UI are authorized.

## 8. Acceptance and required regressions

All entries below are pending implementation and evidence. No check has run merely because it appears here.

| Group | Required observable result |
| --- | --- |
| Fresh/override | Fresh install uses only unified root; absolute custom root isolated; empty/relative/inaccessible override, missing home, spaces/Unicode and preexisting unsafe target have deterministic no-fallback results. |
| Legacy sources | Default macOS, valid custom XDG, config-only, DB-only, missing secrets, historical ~/.vega, dual roots and unknown files neither overwrite nor silently initialize an empty DB. |
| WAL/concurrency | Last committed uncheckpointed message and MCP edit survive; known legacy writer and concurrent new migrator block before capture; source held immutable throughout capture/publication. |
| Interruptions | Inject failure before/after every phase/durable write, full disk, denied access, corrupt DB/credentials, unsafe links and unexpected target. Source application content retained, restart idempotent, no partial root activation or repeated import. |
| History | Threads/messages, BLOB bytes, drafts, compacted/delegated checkpoints, workspace relationships, scratch files, audits and terminal/unknown states preserved; no automatic replay. |
| MCP | Local/HTTP settings, enabled intent, ID/revision/ref, deletion/OAuth/cleanup state preserved. Rename/removal leaves historical identity resolvable. Test connection and activation obey #802. Migration issues no process/network calls. |
| Secrets | Provider/Bearer/OAuth refs unchanged; no token values in YAML/DB/log/manifest; backup access no weaker than source; missing refs actionable; stale cleanup never removes live secrets. |
| YAML | UI save/restart consistent; external valid edits applied through admission gate; comments/unknown fields/no-op bytes preserved; duplicate keys, syntax/type/depth/size/version errors and detected races preserve original document. File edits never forge grants. |
| Projection | Crash after YAML publication before DB reconciliation, prepared journal before publication, DB commit before applied marker and third-party conflicting edit recover or block without two independent authorities or unaudited activation. |
| Cutover/rollback | Successful new root receives all subsequent writes, old roots unchanged by new app, repeat startup does not import twice. New-history writes prevent lossy legacy rollback. |
| Platforms | Pure native-home/override behavior and unsupported secure-store errors verified on available platforms. Do not assert Windows secure-storage compatibility or cloud-drive synchronization. |

Use synthetic installations only. Add focused development Rust behavioral tests where needed for the migration/projection failure boundaries, run only in existing Actions, preserve exact test-source SHA/patch and actual results, then remove Rust tests and exclusive support before master per current policy. Do not replace behavioral checks with source-string assertions or count an empty final test suite as migration validation. Final required CI still checks the exact cleaned head; existing Python checks remain unchanged. Native integration, actual historical user data and real external services remain user hand-test items, not agent-certified acceptance.

## 9. Evidence and release gates

Record separately: static inspection, local diff/format checks, development cloud behavioral results, test-source retention/cleanup, independent review, final required CI, merged version and user acceptance. Local execution of Vega/build/check/test is prohibited. CI status follows the five-minute polling rule. No CI or review is claimed in this initial spec.

Migration is release-blocking until offline source exclusion, WAL consistency, crash recovery, secret safety and projection recovery have concrete evidence. If one cannot be established within authorized architecture/permissions, retain old production startup, report the specific blocker and request a decision; do not weaken the assertions or silently narrow delivery.

## 10. Recovery and user documentation contract

Document the old locations, discovery rules, source selection, what is retained, protected backup location, missing-credential repair, how to quit older versions, retry after disk/permission errors, how incomplete migration is recovered and why post-write rollback is blocked. Restore requires an offline full-root generation-consistent copy; do not recommend copying a live SQLite main file. Exclude run state from portable backups. State that old apps may still write old locations and simultaneous cross-version/device use is unsupported.

## 11. Dependency approval request

Proposed new direct dependency: `yaml-edit = "=0.3.6"`, `default-features = false`, only in the store/config layer. Purpose: lossless syntax editing preserving comments and unknown fields. Semantic validation continues through existing `serde-saphyr =0.0.29` deserialize support; no YAML include/properties features and no optional regex/binary editor features.

Verified upstream release: <https://github.com/jelmer/yaml-edit/tree/v0.3.6>, commit `780a686e5b882941b710c0ef19fd8d061d03bcc8`; package Apache-2.0; MSRV 1.70. Registry record checksum: `5df55a38673279c691015377f55503186026bc41d448892b01886cb7475cd121`, not yanked at inspection. crates.io API returned HTTP 403, so release metadata was cross-checked via GitHub-hosted crates.io index and the tagged upstream manifest, not guessed from main.

Expected normal dependency closure with optional/default editor features disabled:

| Package | Proposed lock version | Enabled normal edge |
| --- | --- | --- |
| yaml-edit | 0.3.6 | rowan ^0.16 |
| rowan | 0.16.1 | countme ^3.0.0; hashbrown ^0.14.3, defaults off, inline-more; rustc-hash ^1.0.1; text-size ^1.1.0 |
| countme | 3.0.1 | none with default features; do not enable instrumentation |
| hashbrown | 0.14.5 | none with rowan's feature set |
| rustc-hash | 1.1.0 | none |
| text-size | 1.1.1 | none without optional serde |

rowan/countme/hashbrown/rustc-hash/text-size metadata inspected reports MIT OR Apache-2.0 (equivalent spelling for rustc-hash). text-size 1.1.1 license was confirmed from its published archive Cargo.toml at `https://static.crates.io/crates/text-size/text-size-1.1.1.crate`, read in memory without installation or execution. rowan 0.16.2 is yanked in the inspected registry; ensure the actual lock resolution uses approved non-yanked 0.16.1. hashbrown 0.14.5 and rustc-hash 1.1.0 already exist in the baseline lock, but feature unification and final closure still need verification. No upstream dev-dependencies, benchmarks or tests become Vega dependencies.

Approval is pending. Do not modify Cargo manifests/lock or vendor an alternative before approval. Existing rusqlite backup feature is defined as `backup = []` in the pinned package and adds no new crate. Its inspected `Backup::new` requires distinct source/destination connections; `run_to_completion` retries Busy/Locked without a deadline. Migration must instead use bounded steps with cancellation and an explicit busy outcome. Exact writer-exclusion/backup interaction still requires synthetic CI validation; feature availability is not evidence of a safe source protocol.

## 12. Change record

- v1: transcribed #820 into a repository implementation contract; corrected existing YAML dependency, SQLite BLOB images, user model-override classification and legacy-lock assumptions; added explicit dependency and offline-source exclusion release gates. No product behavior change.
