# Issue #87 · S14 on-demand asset metadata

Status: **FROZEN contract; metadata implementation and scoped in-process acceptance PASS** (2026-10-06). Native/system acceptance remains NOT RUN.
Parent: [#87](https://github.com/puzige/vega/issues/87), OPEN / In progress / PARTIAL.
Authority: [Skills S2/S3/S4](vega-issue-74-skills.md), [S14/S15 matrix](vega-issue-74-skills-delivery.md), [execution guide](vega-exec-guide.md), and the current #140/#149 in-process test boundary.

## Existing requirement and verified gap

The frozen Skills contract already requires binary assets to be listed by validated relative path, type and size, with `unknown` permitted as the type. It prohibits text injection, automatic uploads, and asset-derived file/MCP/Bash authority. Existing tools remain separately selected and authorized.

The current production path implements only text references:

- `skills/source.rs`: `SkillSource::read_reference` accepts only `references/...`; `assets/...` fails before the descriptor/body reader. Discovery reads only immediate `SKILL.md` candidates.
- `skills/catalog.rs`: `SkillRun` freezes reference text/hash on first read and contains no asset metadata collection.
- `skills/catalog/snapshot.rs`: private snapshot version 1 carries active bodies and references; it rejects unknown fields and validates a separately retained run binding and whole-snapshot digest.
- `agent/loop_.rs`: existing `read_skill_resource` calls `read_reference`, estimates the complete labelled result, and emits a durable snapshot after successful reads.
- `agent/mcp_registry.rs`: the existing resource schema is `{name, path}` and its description advertises references only.
- `vega_conversation/src/agent/pipeline.rs` and `types/tool_calls.rs`: success decoding accepts only the reference prefix/five-field reference JSON. A runtime-only asset result would fail persistence or project as `Corrupt`.
- `vega_ui/src/tool_card.rs`: content-free `SkillCardOutcome::ResourceRead` renders reference size/hash; it has no metadata outcome.

The old `issue74_s14_binary_asset_path_is_rejected_without_text_injection` test proves the binary marker stays out of model messages/images, but expects `unsafe_path`. That expectation must change only for a valid metadata request; the original no-body/no-upload assertions remain mandatory. Existing S14 evidence does not establish asset metadata or existing-tool asset use.

## Frozen narrow contract

### Request and result

1. Reuse reserved `read_skill_resource({"name":"reviewer","path":"assets/pixel.png"})`. Tool name, `{name,path}` shape, additional-property rejection, proposal path-length/hash projection, and read-only approval remain unchanged. No new provider tool, public filesystem handle, dependency, credential flow or migration.
2. Only an exact activated Skill in this direct-user run can answer. Invalid names/paths, unactivated Skills and cancelled runs fail before asset access. A source approval continues to mean instruction availability; metadata is not approval of asset bytes.
3. Accept exactly a normal relative path below `assets/`, with at least one filename component and at most the existing 1024 UTF-8 bytes. Reject absolute, parent, dot, empty, NUL, prefix-confusion and directory-only paths. Reuse existing regular-file, root identity, descriptor-relative no-follow, direct/nested symlink and hardlink fences. No glob, directory listing, neighbouring traversal or recursive asset discovery.
4. A success has the exact prefix `[Lower-trust Skill asset metadata]\n` followed by this **five-field** bounded JSON, for example:

   ```json
   {"name":"reviewer","path":"assets/pixel.png","type":"unknown","size_bytes":42,"lower_trust":true}
   ```

   `type` is always `unknown` in this slice. Do not sniff magic bytes, infer a trusted MIME type from the extension, claim a content digest, or include body/base64/image/file-handle fields. `size_bytes` is the validated regular-file length as `u64`, including zero. The relative path is lower-trust data. Absolute source paths, device/inode values and private source labels never enter this result.
5. Failures keep the existing `{name,status}` stable bounded receipt and `SkillError` vocabulary (`unsafe_path`, `not_regular`, `hardlink`, `root_changed`, `stale`, `not_activated`, `aggregate_limit`, `over_budget`, `cancelled`, `io`). No raw path, filesystem error or asset bytes in errors/audits.
6. Text `references/...` retains its exact current labelled JSON, content hash, UTF-8/32 KiB check, frozen bytes and permission behavior. `SkillSource::read_reference` remains reference-only. Script bodies remain unavailable through this resource route.

### Minimal API and durable/UI representation

- Introduce only a **crate-private** `SkillRun::read_resource` operation for the existing runtime tool loop. It selects reference versus asset handling, returns the complete labelled output, and gives that exact output to the existing next-wire budget callback. The current public `SkillRun::read_reference` API remains unchanged.
- Asset descriptor inspection and the frozen metadata struct stay private to `vega_runtime::skills`; no re-exported public asset loader or file capability is added.
- Add an additive shared projection variant in `vega_conversation::types`: `SkillCardOutcome::AssetMetadata { size_bytes: u64 }`. Its meaning is metadata only, with type unknown. It contains no path or content hash/body. This is required to distinguish an asset observation from a text read in durable history/UI.
- Strictly decode successful metadata in the persistence pipeline and tool-card projection. Require the exact prefix, five keys, correct name, valid `assets/` path, matching proposal path byte length/SHA-256, `type == "unknown"`, unsigned size, `lower_trust == true`, and the bounded output. Reject duplicate keys, extra keys, malformed/oversized values, altered path/name, forged type and body-bearing results. Decode raw JSON through a strict typed serde struct so duplicate keys cannot be folded away in a `Value`. Share the new private decoder so pipeline and history do not diverge.
- Render using the existing Skill tool card/theme/layout: `已查看 Skill {name} 资源元数据 · {size_bytes} bytes · 类型未知 · 未读取内容`. No asset-open/upload/run action or new approval controls. Existing audit, raw-result privacy and reused-result projection remain intact. Native glyph/layout acceptance is separate from GPUI/unit evidence.
- Update only the existing resource tool description to advertise bounded reference text or validated asset metadata, expressly without body/upload/permission authority. The next-wire estimator uses the actual changed description and exact result wrapper.

### No body reads and filesystem race boundary

First metadata access binds to the activated candidate's frozen source identity and approved Skill hash. It rechecks the current configured root/canonical target/device/inode and every descriptor-relative path component. The activated `SKILL.md` body remains frozen exactly as it is for existing references; no new Skill body is adopted during a run.

The metadata-only helper reuses `ensure_current` and `open_chain` flags (`O_NOFOLLOW`, `O_NONBLOCK`, directory checks). It inspects the final descriptor's metadata, requires a regular single-link file, reopens the same descriptor chain, compares directory identities and the file identity/length/link count/timestamps, then rechecks the root. A race yields typed failure. Opening a read-only descriptor is permitted for this existing fence; **no asset `Read`, seek, content hash, mmap, image decode or upload occurs**. Source review must verify no asset path reaches `read_relative`/`read_limited`.

Evidence uses an owned-file descriptor identity counter around the existing private `read_limited` path under `cfg(test)`, scoped to the test thread/fixture. Discovery/activation can read `SKILL.md`; the asset inode must have zero body-reader calls. This observer does not choose outcomes, bypass checks, expose a production API or record bytes. Pair it with the metadata helper's call-chain audit and provider/snapshot marker assertions: the counter measures this known body-read path, not all OS syscalls. A private test-only transition hook may mutate the owned asset/root between first stat and recheck to prove the real race rejection; no fabricated successful authority/result.

Discovery/catalog/activation/export/recovery never inspect an unrequested asset. Large/sparse binary files may return their length without a body-size ceiling because their contents are never loaded; the existing text 32 KiB ceiling is not a binary metadata size cap. Final descriptors are dropped before the result is returned. The separately authorized generic file tool must perform its own live checks later.

### Frozen metadata, Stop and revocation

First successful metadata is cached by `(activated name, exact validated relative path)`. The JSON observation is frozen for that run. A repeated request returns the identical result even after the asset grows, disappears, is replaced by a symlink, or the source root changes; it does not refresh a live file or convey continued access. This mirrors reference snapshot semantics and prevents silent substitution. A new run's first request performs all live checks again.

Cancellation is checked before returning a cached result. Existing conversation Store consent/revocation generation probes and runtime cancellation remain the dispatch authority; a revoked/changed generation interrupts before further provider/tool progression, with the existing content-free revoked activation audits. Recovery stays read-only and never resumes a run or automatically resends a provider/tool call.

Over-budget/over-count/over-byte/failed inspections do not insert a metadata record, consume a frozen-record slot or write a success snapshot. A bound run also checks the prospective full snapshot against its existing 1 MiB ceiling before accepting a new metadata record; `too_large` preserves the prior cache/snapshot. Repeated accepted cached requests do not charge retained metadata storage twice, but each actual returned result still enters the next wire estimate exactly once.

An unbound in-memory `SkillRun::new` applies the same path, count, metadata JSON and wire-budget checks, but cannot export any durable snapshot (`invalid_format`, as before). Full snapshot preflight applies to a real `new_bound` run; no invented binding is supplied to make a unit run persistable. Tests must distinguish these two cases.

### Approved private bounds

Preserve all existing reference/activation/catalog/Store limits. Add private asset limits without changing text capacity:

| Bound | Approved value | Validation |
|---|---:|---|
| Distinct frozen metadata records per run | 128 across all activated Skills | 128 succeeds; 129th new record is `aggregate_limit`; cached record remains usable |
| Retained metadata JSON bytes per run | 128 KiB | Sum the exact compact five-field JSON UTF-8 bytes; enforce before insertion and after restore; no asset body bytes included |
| One complete metadata result | 8 KiB | Strict runtime/pipeline/history bound, including prefix; not a new asset-body cap |
| Path length | Existing 1024 bytes | Original spelling is retained and fenced; no normalized alias |
| Whole private snapshot | Existing 1 MiB | Preserve export/restore and Store checks; asset metadata does not increase it |

The result bound is conservative and computed: JSON fixed syntax/type/boolean bytes excluding size digits = 71; validated name <=64 ASCII bytes; a 1024-byte path may escape to <=6144 JSON bytes; `u64` has <=20 decimal digits. Therefore JSON <=`71 + 64 + 6144 + 20 = 6299` bytes, prefix 35 bytes, complete result <=6334 bytes, below 8192. Filesystem component limits can reduce this maximum but are not relied upon. Existing UTF-8 text limits and 256 KiB escaped-reference decoder bound remain untouched.

### Frozen private snapshot version and compatibility

- A run with **no frozen asset metadata** continues to export the exact existing strict version 1 field shape, field order and byte representation. Activation-only and reference-only runs are unchanged. A run with at least one frozen asset observation exports **private version 2** with an added asset metadata array. No new Store table/column or migration; existing opaque snapshot bytes, separate binding and whole-snapshot digest carry it.
- Restore genuine version 1 using its existing strict field shape and an empty asset collection; preserve all catalog/body/reference/hash/epoch/size checks. Version 1 must not accept an `assets` key, even empty, as an implicit schema upgrade. Version 2 requires its asset collection and rejects unknown fields/future versions.
- Validate every asset's active-Skill name, normal `assets/` path, exact five-field shape without duplicate JSON keys, unknown type, unsigned size, lower-trust flag, unique `(name,path)`, count and serialized-byte sum. Strict typed raw JSON decoding rejects repeated `name`/`type`/`size_bytes` fields even when their values match. Metadata integrity comes from the **separately retained whole-snapshot digest**; it is not an asset content hash or proof that current disk bytes are unchanged.
- Restore reads no source/asset filesystem. Cached metadata after restore remains the frozen observation. The conversation recovery/provenance API is unchanged; the new result projects through `AssetMetadata` without exposing path/body.
- A genuine restored v1 run keeps exporting v1 until a first accepted asset metadata request makes it v2. A structurally valid v2 with an explicit empty asset array is accepted, then canonical export uses v1; missing/unknown fields remain errors. History recovery itself writes nothing. Older binaries reject version 2 rather than ignoring it. A code rollback after v2 persistence must retain the compatibility decoder to preserve v2 provenance; a binary downgrade may show unverified/Corrupt new metadata rows but must never read files or replay work to repair them. This slice does not promise old-version full rendering.

## Test-first acceptance matrix

All rows initially **NOT RUN**. Automatic tests use owned `TempDir` files, production fences/loop/Store and in-process `MockProvider`/existing bounded execution seams. No real shell/Git/MCP child, network/model, user database/config/credentials or workspace-wide local test run.

| ID | Requirement/risk | Operation and expected observable result | Layer/evidence |
|---|---|---|---|
| A01 | Existing business gap | Existing tool asks for one valid binary asset after `load_skill`; preserve an initial failure showing `unsafe_path`/missing metadata, then require exact five-field metadata success | Production runtime loop + mock request capture |
| A02 | Metadata only/optional inputs | Request one of two assets; type unknown and exact size including empty/sparse >32 KiB file; body markers, other asset names, images/base64/file capability absent; source reader counter zero for both asset inodes | Source/SkillRun + provider capture; call-chain audit |
| A03 | Discovery isolation | Put binary/script/reference decoys below owned roots; discovery/catalog/activation expose only the approved Skill metadata/body; zero asset body-reader calls, no asset enumeration/inspection | Source discovery + observer |
| A04 | Relative path grammar | Table: absolute, parent, dot, empty component, NUL, wrong prefix, directory-only, 1025 bytes, maximum valid spelling; typed bounded failures/no asset body read, requested valid nested path succeeds | Source + SkillRun; representative loop path |
| A05 | File/link/root fences | Regular file success; directory/FIFO, direct/nested inside/outside symlink and hardlink rejected; imported-root retarget rejected; source/project/global/import binding tested without ambient scan | Real owned-file fence tests |
| A06 | Metadata race | Owned file length/identity/link state or parent/root replaced between metadata check/reopen; production checks yield stale/unsafe/root-changed failure, no insertion/snapshot/body read | Private fault transition + production descriptor checks |
| A07 | Activated/frozen/Stop | Unactivated name fails; accepted repeat after resize/delete/symlink/root change returns exact old result; new run sees current checks; cancelled run fails even for cached item | SkillRun + repeat provider rounds |
| A08 | Bounded count/bytes | Exactly128 distinct metadata records, 129th rejection; metadata JSON sum just-at/over128 KiB; cached retry does not double count; prospective full snapshot >1 MiB fails without inserting the new record; existing text count/byte capacity remains available | SkillRun bounds + restored payload limits |
| A09 | Wire budget/failure atomicity | Exact full labelled metadata result and changed schema counted once in actual next request; over-budget first read leaves no frozen item; cached over-budget attempt preserves old item | Runtime loop/mock preflight capture + SkillRun |
| A10 | Snapshot compatibility/integrity | Activation-only/reference-only export retains exact old v1 bytes; genuine v1 restores after source deletion and remains v1 until first metadata, then v2; valid v2 empty assets restores and canonicalizes to v1; v2 round-trip restores exact metadata after external mutation/deletion; bad digest/binding/version, v1 assets key, missing v2 assets, malformed/extra-field records, inactive name, escaped path, duplicate/count/byte overflow reject | Private snapshot tests; no live read/replay |
| A11 | Production durable result | Store persists success and frozen metadata; proposal stores path length/hash, terminal audit content-free; close/drop actual Store and reopen, validated activation/history asset card survives with unchanged original user/assistant rows; no Provider/executor replay | Conversation production service + owned Store + MockProvider |
| A12 | Strict result decoder | Reject body/text/hash/base64/extra keys, false lower-trust, forged type/name/path/hash/length, signed/floating/oversized size and >8 KiB result; accept exact result; old reference valid/corrupt cases retained | Pipeline + shared type projection |
| A13 | Stop/revocation authority | Stop and Store revocation before asset dispatch cause interrupted/no metadata success/no later provider call; existing consent/permission rule rows and dispatch counters unchanged; audit remains content-free | Runtime/conversation in-process barrier tests |
| A14 | Product statement/UI truth | Asset projection renders exact metadata-only summary and size, has no raw relative/absolute path/body/hash/upload control; existing reference summary unchanged; GPUI uses normal card tokens/light/dark/narrow fixture | Projection/unit + mounted GPUI where existing harness supports it |
| A15 | Regression boundary | Existing references freeze/hash/read bounds and path/link/root tests; snapshot v1 catalog/body/binding tamper tests; Skill approval/operational tool permission semantics retained | Task-specific Nextest selection |
| A16 | Native/system acceptance | After this exact implementation is integrated and installed, verify real rendered metadata-only card and harmless separately approved existing-tool behavior if that tool supports the selected asset | NOT RUN; system use/asset tool authorization remains separate |

The old S14 valid-asset rejection expectation is superseded by this metadata contract once approved; its body/upload/path-privacy assertions are retained. No deletion, ignore, weaker permission assertion or automatic rerun-to-green. Record the first business failure and each diagnosed correction before the final result.

## Implementation order and ownership

1. Main agent approved the metadata result, shared projection, private bounds and first-observation freezing, with the v1 compatibility correction above. Write failing regression tests before changing production code.
2. Add the failing production-loop metadata expectation and owned source/freeze/decoder/snapshot cases. Use `issue87_s14_` names so all newly added task cases have one native Nextest selector.
3. Implement private source metadata inspection; SkillRun/cache/bounds; v2 export/v1-v2 strict restore; existing tool-loop result/budget/description; strict conversation pipeline/projection; truthful UI summary. No shared S16/S21/S22 tree or common delivery-matrix edits.
4. Run only task filters, then related reference/snapshot safety tests. Planned task commands: `cargo nextest run -p vega_runtime -E 'test(/issue87_s14_/)'`, `cargo nextest run -p vega_conversation -E 'test(/issue87_s14_/)'`, and `cargo nextest run -p vega_ui -E 'test(/issue87_s14_/)'`. Exact final related filter and run counts must come from the actual final test inventory, not estimated values. Default owned worktree target, retries0; complete workspace gate stays in cloud CI.
5. Fetch/rebase current `origin/master` before final handoff; resolve only owned task files and rerun affected filters after baseline drift. Return reviewed diff, exact commands/exit codes/raw logs/run IDs, first-failure and final hashes/source freeze, <=3 commits and residuals to main. No push/PR/install/model call by this subagent.

## Scope and acceptance limits

This delivers the already-frozen **on-demand metadata observation** part of S14. Metadata does not select a native multimodal attachment or authorize later file/script/MCP execution. Existing-tool asset transport/selection, real model behavior, OS integration and native installed-build UI remain PARTIAL/NOT RUN until separately observed. Parent #87 remains OPEN; a metadata-only green test result cannot make full S14 or #87 PASS/Done.

## Change record

- 2026-10-06: source audit and minimum metadata contract/test plan; main-agent implementation approval with the compatibility correction that no-asset runs continue to export exact v1 bytes. Added explicit bound/unbound behavior and v2-empty decoding/canonicalization rules. No broader asset transport/tool authority approved.

## Scoped implementation evidence

Tested implementation commit: `5d298986bd430b9857d12e0e532e5f553a640cae`; tree `aaa2c09b2ae69638b3c110c554b6999312d7d264`; rebased on `a53662e46f85cfd6155dc1f556b4fb4e578613e7`, preserving merged S16 durable registration and text/Plan provenance. Source/tree and all owned source hashes match before and after final execution. This evidence section is a subsequent documentation-only change.

| Stage | Nextest run ID | Result / exit |
|---|---|---|
| Initial business red | `6e260f96-233f-4e2c-a108-9c9f26a25bbe` | 1 failed / 252 skipped; exit100; valid asset returned Failed when Success was required |
| Compile/harness correction | No run ID | exit101; test-only inaccessible sibling digest helper; corrected to existing Sha256 calculation |
| Owned fixture correction | `0affd4ee-827e-44cb-8a0d-a105d2ad0130` | 17 passed / 2 setup failures / 1352 skipped; exit100; macOS long-path creation used full paths; corrected owned fixture creation to descriptor-relative mkdirat/openat |
| First task green | `cb43632b-e96d-4b9c-9603-4a614e5cf59d` | 19 passed / 1352 skipped; exit0 |
| Final runtime task | `49b13788-d670-46f7-b566-2b0916036a1b` | 13 passed / 252 skipped; exit0 |
| Final conversation task | `f6bc0db3-362c-4837-8a68-dd6e0f65126d` | 4 passed / 553 skipped; exit0 |
| Final UI task | `a1ef0cb0-1a27-4b97-9477-a4c1004d2276` | 2 passed / 550 skipped; exit0 |
| Final runtime reference/snapshot/permission selection | `06743c73-c06a-45f2-b6b7-6ea2499d53e7` | 37 passed / 228 skipped; exit0 |
| Final conversation/UI safety selection, including merged S16 durable cases | `f5e07630-bdf0-45bd-8560-f11a97be08c4` | 11 passed / 1098 skipped; exit0 |

All runs use the worktree default target and default Nextest profile with retries0. The three final task commands are the package-specific `issue87_s14_` filters listed above. Exact safety selectors, original log filenames/run IDs/SHA-256 values, before/after source hashes and corrections are preserved privately in `final-evidence.json` (SHA-256 `1647314b41b8c9c5dbc3d1d360af694ac41449dd288b3b31555d9631fdcef9ea`) and `pre-rebase-evidence.json` (SHA-256 `d2574e6e5fd82ae8ce600d746fe1021ec5bd8031836a807e49907edc04ec30bf`); raw logs are not committed. No workspace-wide local gate, provider request outside MockProvider, push, PR, install or native acceptance occurred in this subtask.

Initial-red evidence limitation: the raw log and the main agent's contemporaneous test/spec-only status were retained, but no full contemporaneous first-red source manifest was collected. Preserved initial test code and the original production baseline supply an explicitly **post-run reconstruction**, not an original-time source freeze. Final executions have complete matching before/after source/tree/log evidence.

The source call-chain review confirms assets do not reach `read_relative`/`read_limited`; the test observer establishes zero calls for the two owned asset inodes through that existing reader only. It does not count all OS reads. GPUI tests assert mounted bounds across light/dark and 960/1200/1229/1230/1403 widths; native glyph/rendered screenshot acceptance is still A16 NOT RUN. This is metadata-only S14 progress; full S14 and parent #87 remain PARTIAL / OPEN.
