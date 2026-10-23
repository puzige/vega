# Issue #344 — development Rust test cleanup evidence

## Freeze and authorization

- Static verification recorded 2026-10-23, starting at `2026-10-23T03:36:15Z` (`2026-10-23T11:36:15+0800`).
- Starting and unchanged Git HEAD: `3c8f21b67a7a6d95dc1ab69ae51e44005c88e9a1`.
- Current master comparison: `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`; the parent reported fetching/updating before this stage. No fetch or Git mutation was performed by the cleanup writer.
- Contract: remove the development Rust tests and their exclusive support, with no production behavior changes, under AGENTS, execution guide §7.2, the Rust test delivery lifecycle, and design guidelines.
- Assigned payload: `Cargo.lock`, `crates/vega_ui/Cargo.toml`, and conversation-stream `message_anchor_tests.rs`, `message_anchors.rs`, `mod.rs`. This new evidence file is the only documentation written by this stage.
- The parent's preexisting `docs/344/vega-issue-344-delivery.md` modification is excluded from the patch identity below and was not edited. Its observed stage-start and verification SHA-256 was `2950b78881b381f49db31e49cd82b2b7d7d48aa304ad03c2f37e396e42c243c4`.

## Prior development evidence retained, not re-run

The parent supplied these completed results; this stage did not query Actions or treat them as final-cleanup validation:

- Cache repair `994a5ca12007f4d457d2f2413b3a61dab81e73d9` independently approved with cloud 11/11.
- Merge HEAD `3c8f21b67a7a6d95dc1ab69ae51e44005c88e9a1`: [Actions 39894736114](https://github.com/puzige/vega/actions/runs/39894736114), Clippy job `120222421814`, Nextest job `120222422081`, required job `120223348446`, SUCCESS. Rust: 11 discovered, 11 run, 11 passed, 0 skipped. Python: 5 + 15 + 40. No unresolved tests were reported.
- Recoverable original test source/complete patch: [PR #562 preservation comment](https://github.com/puzige/vega/pull/562#issuecomment-6356532937), original source commit `9bda1ecb54cec85d4e217760408f2aafd03ad3a3`, preserved patch SHA-256 `d69be17930e6dcffb5d362333eb47b7d8afc0ac0315cd73e4cf6fa4069dff1a1`. Remote matching was reported by the parent, not re-fetched here.
- The deleted file was locally verified before deletion against SHA-256 `c84c479c6f8f3b6d00290f7091abebd1537dcf7142cd4ab3f86e6ebdc2651565`. No runnable test source was copied into documentation or another path.

## Removed inventory

| Artifact | Actual removal |
|---|---|
| `message_anchor_tests.rs` | One file, 29,615 bytes, 801 lines, 11 `#[gpui_kit::test]` cases |
| In-file exclusive support | Two structs (`Fixture`, `AnchorHost`), two impl blocks, 14 non-test free helper functions, two methods |
| `mod.rs` | One test module declaration and its `#[cfg(test)]` attribute; two lines |
| `message_anchors.rs` | Three transparent `.map` wrappers (rail, marker, preview), three `TestSupportExt::test_support` calls, six test/not-test attributes; 30 lines |
| UI manifest | One `[dev-dependencies]` section and its `gpui_kit` workspace `test-support` activation; three lines including separator |
| Lockfile | Ten exclusive packages and two test-support edges from retained `gpui-pre`; no package additions/upgrades |

Lockfile packages: **924 → 914**. Removed identities:

- `bit-set 0.8.0`
- `bit-vec 0.8.0`
- `convert_case 0.11.0`
- `proptest 1.11.0`
- `proptest-macro 0.5.0`
- `quick-error 1.2.3`
- `rand_xorshift 0.4.0`
- `rusty-fork 0.3.1`
- `unarray 0.1.4`
- `wait-timeout 0.2.1`

Resolved edges removed from retained packages: `gpui-pre 0.3.4 → backtrace 0.3.76` and `gpui-pre 0.3.4 → proptest 1.11.0`. The `backtrace` package remains for legitimate users. Retained package identities, sources, checksums and non-dependency content are equal; resolving abbreviated dependency labels finds no added edges and no other removed edges from retained packages. Nine lockfile inserted lines merely remove now-unnecessary version disambiguation. The cached dependency manifests confirm the GPUI test-support feature activates proptest and leak detection/backtrace.

`Cargo.lock` and `crates/vega_ui/Cargo.toml` are byte-identical to current master. Root `Cargo.toml` is unchanged and also byte-identical to current master.

## Production projection equality

Source-only Python checks loaded the exact starting HEAD using read-only `git show`:

1. Removed only the three exact `.map(|rail| …)`, `.map(|marker| …)`, `.map(|preview| …)` wrappers from the starting `message_anchors.rs`. Each former non-test arm returned its argument unchanged. The resulting bytes equal the final file, SHA-256 `2c6c369cdd661fc81738588388605c41e655e84634e23da550aef4d8272e079f`.
2. Removed only the exact `#[cfg(test)]` plus `mod message_anchor_tests;` declaration from the starting `mod.rs`. The resulting bytes equal the final file, SHA-256 `5774e655493845fa154d8e81bdccbceeaa52807cf81406199ddf0ffad9e8d278`.
3. The read-only path inventory confirms all other tracked payload code is unchanged. In particular `core.rs`, `content.rs`, `render.rs`, root manifest, Python, fixtures, workflows and other Rust sources were not edited.

These byte comparisons preserve real element IDs, accessibility labels, focus and input handlers, layout/hit regions, the post-list observer, changed-only refresh, true-tail continuity, and the repaired live `ListState` height sampling. Snapshot heights remain change-detection data; the cache importer still reads current list bounds keyed to current identities. No production assertion, guard, comment, `unwrap`/`expect`, dependency or product text was added.

## Local static verification

All commands ran at the task worktree root. Tool metadata: Cargo `1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`, Git `2.55.0`, Python `3.14.7` (standard-library TOML/JSON/hash/text processing only).

| Exact command or static operation | Actual result |
|---|---|
| `cargo fmt --all -- --check` | Exit 0; no formatting writes |
| `git diff --check` | Exit 0 |
| `cargo metadata --no-deps --offline --locked --format-version 1` | Exit 0, independently captured with `subprocess.run`; JSON parsed successfully |
| Metadata inspection | 13 workspace members, 0 explicit test targets, 0 dev dependencies, 0 dependency `test-support` activations |
| `git diff --numstat 3c8f21b67a7a6d95dc1ab69ae51e44005c88e9a1 -- Cargo.lock crates/vega_ui/Cargo.toml crates/vega_ui/src/conversation_stream/message_anchor_tests.rs crates/vega_ui/src/conversation_stream/message_anchors.rs crates/vega_ui/src/conversation_stream/mod.rs` | Exit 0; per-file counts below |
| `git diff --name-only 3c8f21b67a7a6d95dc1ab69ae51e44005c88e9a1` plus Python set comparison | Exit 0; exactly five payload paths besides the parent's delivery edit, before creating this evidence |
| `git show <starting-head-or-master>:<path>` plus source-only Python projection/TOML/identity comparisons | Exit 0; exact production projections, test-file counts/hash, master manifest/lock identity, retained lock content/edge invariants verified |
| `git ls-files -- '*.rs'` plus source-only Python syntax/path/fence scan | Exit 0; 450 remaining tracked Rust files, no test/support syntax or test-named source files; one existing `text` doc block, no executable Rust doctest fences |
| Added-line scan of payload `git diff` | Exit 0; no added comments or `unwrap`/`expect` |
| `shasum -a 256 <final-payload-paths>` and `hashlib.sha256` | Final identities below |

The source scan checked test/cfg-test/cfg-attr-test/ignore attributes, `TestSupportExt`, test-support calls/feature names, the removed module name, test-named paths, and doc fences. A direct `rg` test/support scan returned **1 (no matches)**, not an execution failure. The doctest-fence scan returned the two boundaries of the existing `//! ```text` block in `crates/vega_markdown/src/stream.rs`; its content and language were read and left unchanged. The subsequent Python inventory checked that these are the only doc-code fences.

One preparatory lock transformation exited 1 on a **pre-write guard**: it incorrectly expected four occurrences of the retained `bit-set 0.9.1` label; the actual source has three. Inspection confirmed both lockfile and test file still had their original hashes and no lock/test-source writes had occurred. The count was corrected to the observed three; all package/edge assertions and the final exact-master-byte assertion were retained. The next transformation completed successfully. This was a static cleanup-script observation error, not a product/test failure or relaxed acceptance condition.

No Vega code, compiler, build, app, Clippy, tests/nextest, Python tests, Git mutation, fetch, push or CI operation was executed locally. Metadata is not Rust test discovery, and the static inventory is not type/borrow/runtime validation.

## Exact delta and final payload identities

Against `3c8f21b67a7a6d95dc1ab69ae51e44005c88e9a1`, excluding this evidence and the parent-owned delivery document:

| Path | Added lines | Removed lines | Final SHA-256 |
|---|---:|---:|---|
| `Cargo.lock` | 9 | 109 | `71388bbb23bdeddcf884a02731f1190f22f32c889521ce431f933a834d12ce9d` |
| `crates/vega_ui/Cargo.toml` | 0 | 3 | `45f54ccb5909337fc1e31329bf85aaa254a829225e3e952d44ffa3fc5950787d` |
| `crates/vega_ui/src/conversation_stream/message_anchor_tests.rs` | 0 | 801 | Deleted; prior SHA-256 `c84c479c6f8f3b6d00290f7091abebd1537dcf7142cd4ab3f86e6ebdc2651565` |
| `crates/vega_ui/src/conversation_stream/message_anchors.rs` | 0 | 30 | `2c6c369cdd661fc81738588388605c41e655e84634e23da550aef4d8272e079f` |
| `crates/vega_ui/src/conversation_stream/mod.rs` | 0 | 2 | `5774e655493845fa154d8e81bdccbceeaa52807cf81406199ddf0ffad9e8d278` |

Payload total: **five paths, 9 insertions, 945 deletions**.

Exact payload patch command:

```sh
git diff --binary --full-index 3c8f21b67a7a6d95dc1ab69ae51e44005c88e9a1 -- Cargo.lock crates/vega_ui/Cargo.toml crates/vega_ui/src/conversation_stream/message_anchor_tests.rs crates/vega_ui/src/conversation_stream/message_anchors.rs crates/vega_ui/src/conversation_stream/mod.rs
```

Patch length **39,750 bytes**, SHA-256 **`08297e4fc64c9d164531a06fc423258dcc0c37b65cbd48aaa15504dc76ecc6c9`**. The READY handoff reports this evidence file's final hash and the combined assigned patch identity separately, avoiding a self-referential file hash.

Unchanged protected hashes:

| Path | SHA-256 |
|---|---|
| `Cargo.toml` | `b2745ea61e0b9434b97562a71653c99a88a4f32436be778b911ee4edf914156f` |
| conversation-stream `core.rs` | `c62672089fc7fe42dee9d15ffaca8e8a5034197521fa41e31200273f33aaef61` |
| conversation-stream `content.rs` | `9df2cb56f9d61ed7260aa20076f5c7176260daa2830c8f2f7d84a98b55f0e6db` |
| conversation-stream `render.rs` | `e15997c81edf6ef0cf0455ec72d4038e590566b8d03ec0ea83f7b2cc5302d2a9` |

## Residuals and handoff

- No intended production behavior or acceptance change; no known static scope blocker.
- Independent cleanup review and complete required CI on the final committed cleanup head remain **pending**, owned by the parent. The previous 11-test success does not validate the cleanup head.
- The final cloud Rust result must be recorded as **`no Rust tests` (0 discovered / 0 executed)**, not as passed Rust tests. Local discovery/execution was not run, and final-head cloud counts have not been obtained in this stage.
- Parent retains Git/index/commit/PR/delivery-document ownership. User hand testing remains separate from CI and merge.
- After final read-only verification and READY, the cleanup writer stops writing.
