# Issue 63 backend delivery

## Freeze

- verified_at_utc: 2026-09-18T16:49:16Z (focused tests; clippy immediately afterward)
- verified_at_local: 2026-09-19T00:49:16+0800
- branch: feat/issue-63-image-attachments
- base: the existing local master; final integrated commit is assigned by main acceptance, not this executor.
- task_contract: `vega-issue-63-image-attachments.md`, R1–R7 and frozen backend/UI seam.
- os_arch: Darwin arm64; rustc 1.98.0; cargo 1.98.0; git 2.55.0.
- backend tracked_diff_sha256: `15b86b33c0b65bb74d54100b013f8f1d744d7dd2a98fd0bd605ceada4d30daa0`.
- New-file hashes (tracked diff excludes these until integration): images.rs `b03aece788df904e4f019991472ce51735547670d7bbc041dbe05df98af31cef`; attachments.rs `ce006d88d896418b915e45e08bc94cc9385dea79d011db571cd25306beae7c28`; backend images tests `5d0abcfc6862e459e7f2f1181c4b722ed60362e58dc50196c2305f6b3c08f06b`; image_attachments.rs `a653e2df90880feac528ca423c241286e6f2e18c6b6c9593af6c91da1dc44440`; migration 0007 `bc4f1046e4f8fbbff552c2c69137fb5c6b428c7020c54f6374f114879768bb2b`.

## Changes

- Runtime: validated immutable `ImageAttachment` with redacted Debug, actual PNG/JPEG/WebP decode and animation rejection, per-image/turn/request limits. ChatMessage preserves explicit images across rounds; OpenAI-compatible wire uses MIME-correct data URL content parts only for user images. Text-only stays a string. Both direct provider and agent runtime entry validate roles/budgets before dispatch.
- Conversation: worker-only no-follow/nonblocking bounded file importer, shared type re-export and image-aware task entry. Existing APIs delegate with empty images. Atomic user-turn attachment persistence precedes acknowledgment; replay and UI hydration reconstruct the same validated bytes. Existing non-user/assistant prompt filtering remains unchanged.
- Store: additive 0007 ordered attachment table with bounded blobs and cascading message ownership. Page reads batch exact owner IDs inside the existing snapshot; SQLite SUM(length) rejects >32 MiB before retrieving blobs. No N+1 image queries.
- Manifests: only architect-approved locked image/base64 dependencies. No UI or app ownership changes by this executor.
- Schema-version expectations move 6→7 with explicit architect approval for migration 0007; all previous upgrade tests retained, plus a 6→7 test. Existing exhaustive HistoryEntry matches add UserImages without weakening old assertions.

## Results

Raw final focused logs: `/private/tmp/vega-issue63-backend.n280sS/` (local-only).

| Requirement | Class | Exact command | Result |
| --- | --- | --- | --- |
| Backend compilation | build check | `scripts/cargo-lock.sh check -p vega_conversation` | PASS, 14.85 s |
| Production controller, byte-exact recorded HTTP, read-tool round, reopen/next turn, same-thread ownership | E2E-REAL with local owned HTTP provider boundary | `scripts/cargo-lock.sh test -p vega_runtime -p vega_conversation -p vega_store issue63 -- --test-threads=1` | PASS: conversation 5, runtime 4, store 1; one native-fixture generator ignored. Compile 13.93 s; test bodies 0.22 / 1.19 / 0.02 s |
| Transaction rollback, invalid batch, upstream vision rejection keeps durable turn | FAULT-INJECTION | Same command | PASS, production paths; not a live vision service claim |
| PNG/JPEG/WebP, real valid APNG rejection, corrupt/header/pixel/count/byte limits, redaction, direct provider ownership, symlink/FIFO import, aggregate history cap, cascade and additive migration | UNIT / production service | Same command | PASS |
| Backend package formatting | format | `scripts/cargo-lock.sh --wait fmt --package vega_runtime --package vega_conversation --package vega_store` | PASS |
| Strict backend lint | lint | `scripts/cargo-lock.sh clippy -p vega_runtime -p vega_conversation -p vega_store --all-targets -- -D warnings` | PASS, 8.36 s |
| Harmless geometric native fixture | test asset generation only | `scripts/cargo-lock.sh test -p vega_runtime issue63_generate_native_fixture -- --ignored --nocapture` | PASS, 1; retained owned temporary PNG supplied privately to main |

Bounded raw footer:

```text
conversation: test result: ok. 5 passed; 0 failed; 0 ignored
runtime:      test result: ok. 4 passed; 0 failed; 1 ignored
store:        test result: ok. 1 passed; 0 failed; 0 ignored
clippy:       Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.36s
```

- focused-tests.log SHA-256: `89d431d76af8e8cd9ba1d3d46fcae1c974a7908ed1f2e05f49e640bbbeda86d0`
- clippy.log SHA-256: `e1dd844bed62e4fb5ab1eafce979dc9b49f1c7bb5e31cdadfce03c075813e559`

## First failure retained

First `test -p vega_conversation issue63 -- --test-threads=1` did not compile:
`E0004`, `HistoryEntry::UserImages` not covered in existing pagination_hydration_e2e.rs matches (then lines 182 and 416). Tool transcript retains original output. Added explicit sequence extraction and an unreachable-image assertion for its text-only fixture. Next focused run passed two initial tests; final expanded suite above passed ten. No runtime assertion failures occurred in backend focused runs; no passing assertion was relaxed to hide a failure.

### Full-workspace follow-up correction

Main's first full-workspace run failed `tests::r52_load_sensitive_ignores_are_frozen`
because the temporary native-fixture generator introduced a new ignored test
(`images.rs`, then line 281). That gate reported 149 passed / 1 failed; original
log remains `/private/tmp/vega-issue63-acceptance.0KSLa5/workspace-tests.log`.
The frozen ignore guard was not changed. The temporary generator was removed
from final source after its owned PNG had already been produced and retained
for native acceptance. Its earlier command/result above is historical asset
generation evidence, not a permanent test or a reproducible command in final
source. Final Issue 63 backend test inventory is **10 active tests, 0 new ignored
tests**; the historical raw footer correctly retains its then-present ignored
generator. Main re-runs the guard and final gates on the corrected tree.

Only `images.rs` changed for this correction; formatting via
`scripts/cargo-lock.sh fmt --package vega_runtime` passed. Corrected new-file
SHA-256: `e0b2bd61372106e0954ab9594ea246b8627910a673ef147dbdad1311b44953fe`.
The original images.rs hash in the first freeze section identifies the earlier
focused run, not this correction.

## Residuals

### Exact-schema inventory follow-up

Main's next full-workspace run failed
`agent::tests::stream_persistence::persists_messages_tool_lifecycle_and_zero_cost_usage`:
the exact table inventory still expected the ten pre-0007 tables. App tests
were 150 passed; conversation was 318 passed / 1 failed / 3 ignored. Original
failure remains `/private/tmp/vega-issue63-acceptance.0KSLa5/workspace-tests-rerun.log`.
Architect explicitly authorized only the intended 0007 inventory/version changes.

The scan found matching stale assertions in store lib/permissions and conversation
todo/S7 integration tests. These exact inventories now include `image_attachments`,
table counts are exactly 11 and schema version exactly 7. Equality guards and
all non-schema behavior assertions remain intact; historical upgrade fixtures
remain at their original starting versions.

Focused correction verification:

- `scripts/cargo-lock.sh test -p vega_conversation persists_messages_tool_lifecycle_and_zero_cost_usage -- --test-threads=1`: 1 passed, 0 failed; body 0.03 s (tool transcript).
- `scripts/cargo-lock.sh --wait test -p vega_store --lib -- --test-threads=1`: 99 passed, 0 failed, 0 ignored; body 0.93 s.
- `scripts/cargo-lock.sh test -p vega_conversation --test todo_e2e --test s7_acceptance_e2e -- --test-threads=1`: 2 passed, 0 failed; bodies 0.02 s each.
- Scoped package formatting passed. Raw new logs are `schema-store-tests.log`
  and `schema-integration-tests.log` in the same backend evidence directory.

Final full-workspace validation remains owned by main; these focused successes
do not replace it.

- LIMIT: 64 MiB is the hard decoded-output cap, not total process/codec working memory. Image::Limits max_alloc is also set to 64 MiB defensively but is not a strict JPEG/WebP internal-memory quota. Architect clarified R3 accordingly before delivery.
- NOT RUN by this executor: whole-workspace gates, native installed-app UI and real configured vision provider. Main owns these; local recorded HTTP does not prove a live model supports vision.
- No commits, pushes, installation, credentials edits or user-file deletion performed.
