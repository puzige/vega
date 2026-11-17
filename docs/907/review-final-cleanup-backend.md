# #907 final cleanup delta — independent V/M review

## Verdict

**No actionable findings in the reviewed backend/vendor and Cargo cleanup delta.** Static comparison supports removal of only the inventoried #907 development tests, observation APIs, timing diagnostics and exclusive manifest/dependency wiring. Production capability additions, original upstream unit tests and upstream dev dependencies are preserved. This is an independent non-author cleanup review, not a full migration re-review or a runtime/merge approval.

## Exact versions and scope

- Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`.
- Development baseline: `ef3dddd6791e3607e40b1b1bd8adb60b48979752`.
- Local evidence tag `evidence/907-nsh-development-ef3dddd6791e` resolves to that exact baseline.
- Cleanup commit: `792d69b58ea9ae543426e04ab20571d572cd4c5b`.
- Stopped review HEAD: `c5cd4ffeceda9665dace2602f1d2429fbf9849b1`.
- Cleanup-to-HEAD comparison adds only `docs/915/draft-shell-recovery.md` and `docs/915/validation.md`; no production change is introduced by that merge.
- Reviewed baseline-to-HEAD paths: `vendor/nsh-platform-gpui/**`, Cargo manifests and root `Cargo.lock`. Application/UI source cleanup belongs to the other reviewer. Protected dependency trees were compared only to establish that this delta does not modify them.
- Rules read: root `AGENTS.md` (including independent-review scope/termination), `docs/vega-exec-guide.md`, `docs/vega-rust-test-delivery-lifecycle.md`, and cleanup inventory §§7–8. No nested `AGENTS.md` was found under `vendor/` or `docs/`.

Tracked scoped worktree bytes matched the stopped HEAD, including all retained platform files, root manifest/lock and both changed application manifests. No untracked platform file was present. Other lanes' documentation edits and pre-existing Python bytecode directories were observed and left untouched.

## V — vendor preservation and removal

### Deleted additions and mixed-file hunks

Exactly the ten non-upstream files enumerated in inventory §8 are deleted, with no platform files added: three `src/app/*integration_support.rs` files, `src/input/scroll_host_tests.rs`, four reading host-test files, and the selectable-view/text-input host-test files. Their original module/export reachability is through the removed `vega-host-regressions` development feature, including the two support submodules reached through `integration_support.rs`.

The complete mixed-file delta was inspected:

- `lib.rs`: removes only development support module wiring, `host_regressions` reexports and integration observation reexports. Production editing/reading modules, scroll bridge/route/remainder, `ScrollOutcome`, `ScrollScope` and native component exports remain.
- `input.rs`, `selectable_view.rs`, `text_input.rs`, `reading.rs`: removes only development host-test module wiring. In particular, native text/IME/paste/history logic is not edited by this cleanup.
- `rt.rs`: removes batch observation hooks and timing spans/module. Queue batching, execution of every job, flush ordering, `surface::flush_all` and upstream `init_test_app` remain.
- `surface.rs`: removes the host-event observation buffer/initialization/clone, observer counters and timing spans. Real event delivery/accounting, repaint coalescing, drawing invalidation state and drain/apply behavior remain.
- `reading/geometry.rs`: removes two timing spans and the feature-only `matches_snapshot` helper, retaining capture/currentness checks, geometry and anchor behavior.
- `reading/opacity.rs`: removes its timing span and feature-only retained-publication observer. The actual negated `is_current(&surface, &publication.fence.snapshot)` predicate is retained, along with fence ownership, publication identity and drawn-content identity rejection. Its compact block is semantically unchanged.
- `scroll.rs`: removes only two development geometry scroll step/frame helpers, not production wheel/frame/anchor handling.
- `text.rs`: removes only the development opacity observation structure/helper, not the rendering implementation or opacity application.

The only added vendor diff line is the retained opacity currentness predicate after removing its diagnostic span. There is no added unsafe code, weakened production assertion, replacement no-op API or new diagnostic feature. A scoped identifier scan found none of the retired feature/module/observer/timing references remaining.

### Six production capability additions

All six inventory-listed capability files remain:

| Capability file | Cleanup comparison |
|---|---|
| `src/editing.rs` | Byte-identical to development baseline |
| `src/events/scroll_bridge.rs` | Byte-identical |
| `src/reading.rs` | Only host-test module declarations removed |
| `src/reading/geometry.rs` | Only timing spans and test-only accessor removed |
| `src/reading/opacity.rs` | Only timing span and test-only accessor removed; real currentness predicate preserved |
| `src/scroll_scope.rs` | Byte-identical |

Production `app.rs`, `events.rs`, `native.rs`, `render.rs` and `system.rs` are also byte-identical. Together with the reviewed mixed hunks, this preserves the baseline's geometry/currentness, IME/native text, actual scroll-consumption/remainder and render-presence paths without reassessing the entire migration.

### Original upstream content and pin

Read the existing cached `nsh-platform-gpui-0.4.0.crate` without extraction or modification. Its SHA-256 is `1e3828245c26e734567b3e6058398c0e4d5809063636460f990a0eaf8a0f74c3`; all 45 original file hashes match `docs/907/nsh-platform-original-files.json`.

The complete original trailing unit-test suffixes of `input.rs`, `native.rs`, `render.rs`, `scroll.rs`, `surface.rs`, `system.rs`, `text.rs` and `text_input.rs` match the cached upstream bytes at both baseline and reviewed HEAD. Current normalized `Cargo.toml`, `Cargo.toml.orig`, `.cargo_vcs_info.json`, `src/tests.rs`, `examples/embed.rs`, original SVG fixtures and `src/dev.rs` likewise match their upstream originals. The normalized manifest returns exactly to the upstream manifest by removing only `vega-host-regressions = []`; upstream features and target dev dependencies are not stripped.

The upstream VCS pin remains `e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`, package version remains `0.4.0`. `vendor/nsh-core`, `vendor/block` and `vendor/mdstream` have no baseline-to-HEAD delta. Thus the core production selection patch and its existing upstream tests are not affected by this cleanup.

## M — manifests and lockfile

Parsed manifest comparisons establish exactly these removals:

- `crates/vega/Cargo.toml`: workspace-integration feature and its one explicit development test target.
- `crates/vega_ui/Cargo.toml`: reading-integration feature, all nine listed development test targets and the two exclusive dev-dependency entries. Normal dependencies are unchanged.
- Platform manifest: only `vega-host-regressions`.

No other Cargo manifest changes exist in this delta. Root `Cargo.toml` is byte-identical, preserving normal pins, vendor exclusions and NSH path patches; no test-discovery suppression setting is introduced.

Root lockfile parsing and dependency-identity resolution found:

- **966 → 952 packages**: exactly 14 removed registry identities, zero added identities.
- Retained package metadata, including versions, sources and checksums: unchanged.
- Resolved edges from retained packages: **12 removed, zero added**. All dependency labels resolve uniquely in each version; shortened labels are not upgrades/new edges.
- Removed edges are GPUI Fast/Pre test-support dependencies (`futures-concurrency`, `postage`, `proptest`, `web-time`), platform test-support activation of WGPU, and WGPU's optional image support. Cached pinned GPUI manifests confirm these feature relationships. All 14 removed identities lie in the retired-edge dependency closure.

Removed registry identities: `atomic 0.5.3`, `convert_case 0.11.0`, `crossbeam-queue 0.3.13`, `fixedbitset 0.5.7`, `futures-concurrency 7.7.1`, `pollster 0.2.5`, `postage 0.5.0`, `proptest 1.11.0`, `proptest-macro 0.5.0`, `quick-error 1.2.3`, `rand_xorshift 0.4.0`, `rusty-fork 0.3.1`, `unarray 0.1.4`, `wait-timeout 0.2.1`.

## Actual checks and limitations

Performed read-only Git identity/tree/diff checks, Python standard-library TOML parsing and resolved lock-edge comparison, SHA-256/tar-member checks against the cached original platform archive, retained-file/unit-test-suffix byte comparisons and scoped retired-identifier searches. `git diff --check ef3dddd6791e c5cd4ffe -- vendor/nsh-platform-gpui Cargo.toml Cargo.lock '**/Cargo.toml'` completed successfully.

Per this review's explicit authorization, **no Cargo/rustc, formatting, Clippy, build, test/discovery command, network operation or Git mutation was run**. Parent owns final formatting/Clippy/discovery and latest-HEAD required CI. Static graph comparison is not a new Cargo feature-resolution run or a compile/runtime pass. No test discovery/execution totals are claimed; excluded upstream dependency tests are intentionally preserved, not residual #907 development tests.

Local tag resolution verifies baseline identity, not remote archive publication/recovery or historical development test execution. Those records, final CI and native/user acceptance remain separate parent responsibilities. This report does not review UI source cleanup or reopen full-migration/native-acceptance work.

Only this new report was written by this reviewer. No implementation changes requested; writes stop after report readback.
