# R12 A — task actions implementation contract

## Service and visit semantics

`threads::visit_thread(&Store, &str)` delegates one store transaction that touches the task and project timestamps and clears unread before returning its row. Existing `open_thread` metadata callers keep their behavior. Palette genuine task opening uses visit. No schema or dependency changes. Owned migrated file databases verify restart persistence, unknown task, and transaction rollback on project-touch failure.

## Menu and durable acknowledgements

One captured task identity supports pin, rename, archive/restore, read/unread, registered project Finder/copy path, session ID copy, settings and existing delete confirmation. New persistence/path work runs in a worker; ack merges only the committed fields, checks captured project/target, and never navigates a noncurrent task. Rename failure retains its editor. Menu uses its existing deferred anchored focus scope and bounded scrolling.

## Delivery

Implemented the nine-entry menu, captured task operations, asynchronous rename/read writes and registered path resolution. Existing pin/archive/restore/delete confirmation routes remain reused. No shared type additions, migrations, dependencies, fabricated task/log paths, provider access, or native automation.

Rename success merges title and its timestamp only, retaining later timestamps and updating pinned/recent ordering. Read acknowledgement merges unread only. New writes are serialized per block; project generation invalidates late results; project-copy/Finder acknowledgement additionally validates the captured route and cached target. Genuine sidebar visits and new-task creation use N's draft-capacity preflight before database mutations. N owns history/draft restoration and stale palette eligibility.

The menu retains deferred anchoring and uses viewport-bounded scrolling with arrow-key scroll-follow. A scoped Tab handler advances focus without intercepting the rename editor. Production keyboard dispatch exercises Enter/arrows/unread commit/Escape/Tab to another task. Finder delegates to GPUI's parameterized platform reveal API; that API returns no completion acknowledgement, so no success toast is shown.

## Freeze

- branch: `codex/r12-task-actions`
- service commit: `afa7509`; navigation API dependency: `5b45a71` (local cherry-pick `d246618`).
- verified_at_utc: 2026-09-06T04:20:37.743751+00:00; verified_at_local: 2026-09-06T12:20:37.743751+08:00.
- final source diff SHA256: `a73ee3cac7904399e450fb7061c478aa91f576bb5e16639fe3049a0bd6c45dd1` relative to local dependency head.
- broader affected-suite source diff SHA256: `7687ca8c8f86b73bf0c4d1272f0d1321cd61f3510a0cb8cff06581128f5eea72`; final change thereafter only preserves rename timestamp/order, covered by final task handlers and strict checks.
- Darwin arm64; rustc 1.98.0; cargo 1.98.0; git 2.55.0. Each Cargo command uses `CARGO_TARGET_DIR=target` in this card's worktree.

## Results

| Requirement | Evidence | Exact command | Result | Raw log / SHA256 |
|---|---|---|---|---|
| Atomic visit / rollback | E2E-REAL owned file DB | `cargo test -p vega_conversation genuine_visit` | 2/0/0 | `/private/tmp/vega-r12-a-visit.log` / `5910d1efbf526556ecabf203ae78c812f51842393ea113e7cd874d218991f5ea` |
| Palette compatibility | E2E-REAL owned filesystem/DB | `cargo test -p vega_conversation palette::tests` | PASS | `/private/tmp/vega-r12-a-palette.log` / `d0cd50361e029165b2dcf3b88fe688078e870f96d1610c3c9f4b9781f757aa1c` |
| Affected packages | E2E-REAL + existing unit/fault tests | `cargo test -p vega_ui -p vega_conversation -p vega_store` | 563/0/0 | `/private/tmp/vega-r12-a-affected-first.log` / `ede19757e0c94cce8e9f4b3e4119132a65c79a8f9d8ead826b0e8a62665da64d` |
| Final handler / keyboard | E2E-REAL owned file DB, real GPUI dispatch | `cargo test -p vega_ui task_action_tests` | 4/0/0; 0.04 s test runtime | `/private/tmp/vega-r12-a-actions-final.log` / `d9405dace5bec4efd26e5800add31cf70c980230bde18b625f1869cc624f14f6` |
| Strict affected lint | BUILD | `cargo clippy -p vega_ui -p vega_conversation -p vega_store --all-targets -- -D warnings` | PASS; 3.79 s | `/private/tmp/vega-r12-a-clippy-final.log` / `fe54818409c447fd0f9ded5cc68c04c77630082d9ffb5af22b4d313ec04c042c` |
| Affected all-target build | BUILD | `cargo build -p vega_ui -p vega_conversation -p vega_store --all-targets` | PASS; 5.57 s | `/private/tmp/vega-r12-a-build-final.log` / `dfa26ea6cb036dcdd1f761c8b1e67892312f5e23faafd4a97b0cf837439367c9` |
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS | `/private/tmp/vega-r12-a-fmt-final.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

## First failures and residuals

- First keyboard test began with no focused platform element, so `Tab Enter` had no dispatch target and the menu-open assertion failed (3 pass / 1 fail). Adding scoped Tab handling alone retained the failure because initial focus was still absent. The fixture now establishes the first platform tab stop before real key dispatch; all assertions were retained, and actual Tab-to-next-task traversal added. Both original failures are retained below; neither is relabeled PASS.
- `/private/tmp/vega-r12-a-actions-keyboard-first.log` SHA256 `6f9e503a9a80701938ec7d19a7104ee7b4bee231f98bcd16522deae80a1e06ef`.
- `/private/tmp/vega-r12-a-actions-keyboard-fix.log` SHA256 `d3045d49594153ba5a10c0afefd1ce31b636901f95eb0141e81ebd15ec4b8736`.
- LIMIT: native single-instance acceptance, draft restoration/capacity integration and full workspace gate belong to the root integrator. This executor did not run native apps or providers. Existing tests may use MockProvider; no real network/model/key evidence is claimed.
- LIMIT: prior R11 full-workspace 925/3/0 Git timing failures remain tracked; this bounded affected-suite result does not resolve that historical gate. No retries, assertions or Git runner changes were used to clear it.
- Existing dependency future-incompatibility notice for `block v0.1.6` remains; affected strict lint/build passed.
- Spec deviation: none.

## Integration correction: task mutation fence

N's pending navigation and palette results must not install a task snapshot captured before A changes that task. A brackets synchronous pin/status and actual confirmed deletion with the shared navigation mutation begin/finish API, and brackets asynchronous rename/unread from dispatch through foreground completion. Completion releases the fence even if the originating block was dropped or switched projects. Merely showing a delete dialog, reloading metadata, and read-only project actions do not represent mutations. N owns the global epoch/pending state and destination-acceptance guards.

Navigation accepted-visit persistence shares the same pending fence: A rejects overlapping mutations and genuine sidebar opens/new task creation with a retry message. Busy confirmed deletion retains its dialog.

- Correction verified UTC: 2026-09-06T04:29:27.359374+00:00; API dependency `0128519` (local `f170867`).
- Correction source diff SHA256: `08a85d09766b32727db46789f65f150ac8562cc028fa94077de9c1400840589b`.
- `cargo test -p vega_ui task_action_tests`: 5 passed / 0 failed / 0 ignored; 0.04 s; `/private/tmp/vega-r12-a-epoch-tests-first.log` SHA256 `04ecee6533fd10b20bde26d6325afde478e6ac9f6b90a15dd348fdb4eb28d5c5`.
- `cargo clippy -p vega_ui --all-targets -- -D warnings`: PASS; 4.03 s; `/private/tmp/vega-r12-a-epoch-clippy.log` SHA256 `efe4cc591c95c514354af5efb4cbf54b64404224ab47e69c6b30b5c433cb0dfd`.
- `cargo build -p vega_ui --all-targets`: PASS; 4.40 s; `/private/tmp/vega-r12-a-epoch-build.log` SHA256 `77b2194b670a5ccef7c37fbcada8e8fcb08b13abdce4719fd5ad64c1efdca9c6`.
- `cargo fmt --all -- --check`: PASS; `/private/tmp/vega-r12-a-epoch-fmt.log` SHA256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
- Owned production handler evidence covers N-pending refusal, actual archive invalidating a captured resolution epoch, and durable asynchronous rename releasing the pending fence after its originating UI entity is dropped. Root/N owns the full pending-navigation/confirmed-delete scenario. No broad suites rerun for this correction.

## Native menu width polish

Root native acceptance at 960 × 600 confirmed all nine entries and preserved drafts, but the popup unnecessarily occupied the whole 330 px sidebar. Use explicit `Layout::TASK_MENU_WIDTH = 240.0` for this menu. Retain its viewport height bound, deferred anchoring and keyboard scroll behavior.

- Verified UTC 2026-09-06T04:34:22.144902+00:00: `cargo fmt --all -- --check` and `CARGO_TARGET_DIR=target cargo check -p vega_ui` PASS (check 1.73 s). Native combined rerun belongs to root.
- `/private/tmp/vega-r12-a-menu-width-fmt.log` SHA256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
- `/private/tmp/vega-r12-a-menu-width-check.log` SHA256 `864c3f04194c4fa6db848b23d3114ce9f048a28eced76a3f5cb3c29aa6594d36`.

## Native pointer isolation correction

Root reproduced clicks on the deferred menu reaching an underlying task row, changing route and cancelling captured project-copy completion. The popup must occlude underlying hitboxes and consume mouse down/up without moving focus out of its trigger scope. Validate on a mounted 960 × 600 GPUI root, with the Copy project path item geometrically over a different task row; assert actual clipboard content, unchanged opened task, and retained draft.

- Verified UTC 2026-09-06T04:50:00.440609+00:00. Mounted production ThreadsBlock plus TextInput root and owned migrated database, using actual pointer down/up dispatch and geometrically asserted overlap; no injected action result. Original test failed because underlying row replaced OpenedThread. Identical regression passes after hitbox/event isolation, verifying clipboard path, route identity and draft text. Full six task-action tests retain keyboard coverage.
- `cargo test -p vega_ui deferred_project_copy_pointer`: FAIL 0/1: underlying task opened (preserved before fix); `/private/tmp/vega-r12-a-pointer-first.log` SHA256 `7b975b493833f96f3a56ccbca14459663682c84de7388f4bb2d1dbb3af79b273`.
- `cargo test -p vega_ui task_action_tests`: PASS 6/0/0; 0.08 s; `/private/tmp/vega-r12-a-pointer-fixed.log` SHA256 `04b7434819c4c4ed4cd2f15ce20a34d9e811ec5db14844a25160701399954b29`.
- `cargo clippy -p vega_ui --all-targets -- -D warnings`: PASS; 1.76 s; `/private/tmp/vega-r12-a-pointer-clippy.log` SHA256 `e59f4c370bcc48bf67a50e30fc1ca51e8818544c4d3e5380c5b5c919f68abb25`.
- `cargo fmt --all -- --check`: PASS; `/private/tmp/vega-r12-a-pointer-fmt.log` SHA256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
- Full VegaWindow/native combined rerun remains root-owned.
