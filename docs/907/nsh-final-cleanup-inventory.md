# #907 final cleanup inventory — planning only

## 1. Status, authority and version

**No deletion or implementation is authorized by this document.** Apply the inventory only after a recoverable development-source archive has been published and read back, development validation is recorded, and implementation review (including the two P2 deltas) is complete. A later explicit cleanup task must own the writes. This inspection is not independent implementation review.

Read: root `AGENTS.md`, `docs/vega-exec-guide.md` (including §7.2), `docs/vega-issue-workflow.md`, and `docs/vega-rust-test-delivery-lifecycle.md`. There is no nested `AGENTS.md` in this worktree. The user's stopped-source, document-only authorization overrides ordinary fetch/update and execution defaults: **no fetch, Git mutations, Cargo, network, build, application or test execution in this task**. Independent read-only inspections were batched; no delegated implementation review was performed.

Observed HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`. Working-tree content, including untracked implementation files, is authoritative; HEAD is not a snapshot of this dirty implementation. Initial status: 120 modified, 1 deleted, 141 untracked paths. The existing deleted `conversation_stream/selection.rs` is not a proposed cleanup deletion. Mainline integration references in the review documents are historical evidence, not a newly fetched or ancestry-verified baseline.

Initial source inventory: **2,153 existing regular files**, SHA-256 **`696c4c6fcdd87bedc916b9bbdbbaaa6ee724ec8f6fe8c13004916e2316319eb3`**. Convention: take the sorted unique union of `git ls-files -co --exclude-standard -z`; exclude only this new document; omit nonexistent paths; hash each existing regular file with SHA-256; hash UTF-8 `json.dumps(entries)` with Python defaults, where each entry is `(repository-relative POSIX path, file SHA-256)`. Ignored `target/` logs/manifests are not covered. Recompute and compare before any later cleanup; line numbers below are navigation anchors in these bytes, not instructions to delete everything after an anchor.

## 2. Preconditions and protected boundary

Before deletion, the parent must publish a durable, accessible archive/patch of **all current development source**, including untracked files, custom runners, both P2 fixes, manifests and lockfile. Record full source commit SHA(s), dirty-content identity/patch, per-file hashes, actual toolchain and commands/results, raw discover/execute counts, and run/job URLs where applicable. Read back and verify recovery after branch deletion/squash; a local SHA, `/tmp` file, ignored `target/` manifest, or expiring artifact alone is insufficient. The upstream crate archives verified in §8 are provenance evidence, **not** that development archive. Do not copy executable Rust tests into the final master tree as an archive.

Preserve without production changes:

- Fixed NSH `0.4.0`, Kit `0.7.3`, Fast/platform `0.1.9`; root workspace exclusions and both NSH `[patch.crates-io]` path overrides. The vendor patches deliver real production capabilities, not merely tests.
- Native editing/paste/IME/history/selection/acknowledgment, focus/mount/owner/epoch/serial/carrier checks, privacy/budgets and authoritative mutation behavior. Specifically retain `NativeEditorPolicy::paste_text`, paste-context capture/revalidation and normalized authoritative insertion from the single-line P2 fix; LF becomes space in SingleLine while CR is preserved, Composer behavior is unchanged.
- Settings `SettingsOpen` receiver fences, presentation `revoke` methods and close-observer revocation in `settings/state.rs`; retain accepted-save behavior, section/identity/observed-value/permission-IO guards and real business dispatch. Only their development observation is removable.
- Reading snapshots, selection endpoint setters/getters, geometry/opacity leases, source-caret restoration, scroll consumption/remainder/gesture ownership, real layout/viewport/clip/focus/accessibility hooks and security assertions. An API used by a test is not thereby test-exclusive.
- All existing Python tests and their fixtures, Python harnesses, production diagnostics/errors, translations, resources, migrations, packaging/release/security checks, `.config/nextest.toml`, and `.github/workflows/`.
- `conversation_stream/bench.rs`, production benchmark registration and benchmark fixtures in `render_rows.rs`. In `scripts/i18n_allowlist.json`, preserve the **10 fixture-category entries / 10 occurrences** for `conversation_stream/render_rows.rs`; “fixture” does not mean development-test-exclusive.
- Untouched upstream vendored tests/fixtures/examples/dev dependencies and `Cargo.toml.orig`; §8 distinguishes them from #907-added support. Do not recursively strip `#[cfg(test)]` or all dev dependencies from `vendor/`.

Other application crates (`vega_runtime`, `vega_conversation`, `vega_store`, `vega_tools`, etc.) have production #907/mainline compatibility changes but no Rust test-gate/attribute candidates found by this inventory. They are **not cleanup write domains**. No test cleanup in `xtask` was found. Preserve `tempfile` workspace dependency: it is a normal `xtask` dependency, not a removable workspace dev dependency.

## 3. One-writer-per-domain plan and ordering

| Later writer | Exclusive ownership | Interdependencies / completion packet |
|---|---|---|
| A — application | `crates/vega/src/` entries in §4 | Remove custom workspace harness and separator observations together. Do not delete its imported production `controller.rs` or `separator.rs`. Report registration removal to M. |
| B — conversation | `crates/vega_ui/src/conversation_stream/` entries in §§4–6 | Own `conversation_stream/mod.rs`, copy-receipt special cases, body tracing and all corresponding reexports. Coordinate consumers of link-preview/scroll fixture helpers with S; do not co-edit S files. |
| C — settings/sidebar | `crates/vega_ui/src/settings/` and `sidebar/` entries | Own both P2 settings fixtures, inline development helpers and sidebar rename fixture. Preserve close/paste production behavior. Report settings/sidebar runner exports retired to M. |
| E — editing | `crates/vega_ui/src/text_input/` entries | Remove adapter/recovery tests and native integration diagnostics; retain P2 normalization and transaction fences. Coordinate rename fixture consumer with C. |
| S — shared UI | Remaining UI source entries: icons, disclosure, nsh facade, links, palette, tools, leaf controls; **also `vega_ui/src/lib.rs`** | Own shared reexports and the feature-only half of `nsh/scroll.rs`; retain production `consume_list`. B/C/E submit export-removal requests instead of editing `lib.rs`. |
| V — vendor | Only §8 development additions/gated regions under `vendor/nsh-platform-gpui/` | Remove test API producers/diagnostics after consumer inventory is agreed. No broad vendor test stripping; no `nsh-core` cleanup change identified. V removes its vendor feature; report that edge retirement to M for dependency integration. |
| M — integration | Root `Cargo.toml`/`Cargo.lock`, `crates/vega/Cargo.toml`, `crates/vega_ui/Cargo.toml`, **all `crates/vega_ui/tests/`**, `scripts/i18n_allowlist.json`, final provenance/evidence updates | Single owner for shared manifests/lockfile and lint allowances. Root manifest is expected unchanged. Do not let source writers independently regenerate lockfiles or edit shared runner/manifest files. |

All are **future ownership assignments**, not parallel implementation started here. Establish exclusive files before writes. First archive/validate/review the test-bearing source. Then remove each source domain's tests/support coherently and their custom target roots/registrations; remove shared feature/dev-dependency edges once consumers/producers are accounted for. Intermediate integration may not compile; do not use it as acceptance evidence. M performs a single integrated dependency resolution, final static inventory and current-head validation. Reuse completed implementation review; later independent review checks the cleanup delta and affected invariants, not an unrelated full implementation audit. Unchanged bytes need no re-review.

## 4. Exclusive test source files to remove later

All paths here are repository-relative unless a prefix is stated. Remove the named **whole file**, plus its exact parent module declaration (`#[cfg(test)]`, optional `#[path]`, and `mod`), not the production parent. Forty-nine files match `tests.rs` / `*_tests.rs`; some are feature-only mounted suites rather than libtest suites. Additional exclusive support files are listed in §§5–8. Counts below are static source inventory, not Cargo discovery/execution results.

### A — application (prefix `crates/vega/src/`)

| Whole file | Parent registration |
|---|---|
| `startup_controller/tests.rs` | `startup_controller.rs:22–24` |
| `window/conversation_loading/tests.rs` | `window/conversation_loading.rs:8–9` |
| `window/nsh_workspace/tests.rs` | `window/nsh_workspace/mod.rs:1179–1180` |
| `window/nsh_workspace/detail_tests.rs` | `window/subagent_panel.rs:1150–1152` |
| `window/shell_chrome/tests.rs` | `window/shell_chrome/mod.rs:701–702` |

Also remove whole `window/nsh_workspace/{mounted_main.rs,mounted_entry.rs,mounted_regressions.rs,controller_regressions.rs}`. `mounted_entry.rs` imports production `controller.rs` and `separator.rs` by path; **retain both**. Remove only all ten `#[cfg(all(test, feature = "nsh-workspace-integration"))]` items/blocks in `separator.rs` at lines **5,63,68,72,81,91,100,119,169,185**: observer import/module, state/lifecycle callbacks and instrumentation. Preserve real masks, backgrounds, drag-region behavior and surface lifecycle. Its Cargo target is in §7.

### B — conversation (prefix `crates/vega_ui/src/conversation_stream/`)

Whole test files (parent is normally the same path minus `/tests`, or the named `#[path]` module):

```text
attachments/presentation/pixels/tests.rs
attachments/presentation/removal/tests.rs
composer_actions/native/pixels/tests.rs
composer_actions/native/shell/tests.rs
composer_actions/native/tests.rs
composer_selectors/tests.rs
computer_use/presentation/tests.rs
conversation_status/tests.rs
foreground_chrome/context_usage/tests.rs
newrunactivity/tests.rs
newshimmer/tests.rs
newthinking/tests.rs
nsh_parent/tests.rs
ordinary_submissions/presentation/tests.rs
pending_messages/native/tests.rs
resume_tail/tests.rs
run_notice/presentation/tests.rs
selection_menu/body_integration_tests.rs
selection_menu/body_opacity_tests.rs
selection_menu/body_tests.rs
selection_menu/source_anchor_tests.rs
selection_menu/tests.rs
shell_recovery/tests.rs
stream_fade/tests.rs
timestamps/presentation/tests.rs
user_disclosure/native/tests.rs
user_disclosure/tests.rs
utility_bar/host/tests.rs
```

Registration exceptions/anchors: `newrunactivity/mod.rs:16`; `newthinking/presentation.rs:9`; `selection_menu/body.rs:6,10`; `selection_menu/body_opacity.rs:312`; `selection_menu/source_anchor.rs:9`; `shell_recovery/nsh_view.rs:8`. Most other registrations are the two-line `#[cfg(test)] mod tests;` in their matching production parent. `composer_actions/native/pixels.rs:381` is **not EOF**: retain the subsequent `reveal_destination` and other production code.

### C/E/S — remaining UI (prefix `crates/vega_ui/src/`)

```text
artifact_card/tests.rs
automation_editor/nsh_editor/tests.rs
command_palette/presentation/tests.rs
commit_panel/tests.rs
cua_onboarding/card/nsh_view/tests.rs
link_preview_tests.rs
navigation/nsh_controls/tests.rs
scroll_edge_fade/tests.rs
settings/model_configuration/nsh_navigation/tests.rs
settings/provider_management/development_tests.rs
sidebar/nsh_leaf/tests.rs
sidebar/threads_block/rename_tests.rs
text_input/editing_core_tests.rs
tool_activity_group/nsh_header/tests.rs
tool_activity_group/nsh_scroll/tests.rs
tool_card/nsh_tool/layout/tests.rs
```

Special registrations: `link_preview.rs:1354–1356`; `scroll_edge_fade/nsh.rs:1–3`; `settings/provider_management/nsh_providers.rs:2345–2347`; `sidebar/threads_block.rs:9–10,12–13` (feature-only suite/reexport); `text_input/mod.rs:133–134`; `disclosure.rs:1–3` imports `tool_activity_group/nsh_scroll/tests.rs`. Preserve the disclosure implementation, which also implements real scroll chrome.

### Inline unit-test modules (prefix `crates/vega_ui/src/`)

Remove the gated module, not its production file. Each listed module runs from the gate to EOF in the inspected bytes:

| Domain | File | Lines |
|---|---|---|
| S | `branch_selector/host.rs` | 979–1101 |
| S | `branch_selector/nsh_view.rs` | 713–780 |
| S | `commit_panel/draft.rs` | 70–119 |
| B | `conversation_stream/copy_receipt.rs` | 175–229 |
| B | `conversation_stream/message_footer.rs` | 912–1120 |
| B | `conversation_stream/thread_permissions/native_leaf.rs` | 817–917 |
| S | `link_preview/native.rs` | 795–846 |
| S | `nsh/keyboard.rs` | 102–164 |
| C | `settings/agents/nsh_agents.rs` | 739–819 |
| C | `settings/mcp/nsh_mcp.rs` | 1359–1492 |
| C | `settings/model_capabilities/nsh_capabilities.rs` | 620–712 |
| C | `settings/model_catalog/nsh_catalog.rs` | 804–919 |
| C | `settings/model_configuration/pricing_bridge.rs` | 74–120 |
| C | `settings/nsh_appearance.rs` | 445–486 |
| C | `settings/nsh_default_model.rs` | 407–470 |
| C | `settings/nsh_input_preferences.rs` | 409–457 |
| C | `settings/nsh_pricing.rs` | 922–1009 |
| C | `settings/nsh_shell.rs` | 584–626 |
| C | `settings/permission_reset.rs` | 491–535 |
| C | `settings/permissions/nsh_permission.rs` | 575–637 |
| C | `settings/provider_management/draft.rs` | 203–254 |
| C | `settings/skills/nsh_skills.rs` | 862–1037 |
| C | `settings/updater/nsh_update.rs` | 602–657 |
| C | `settings/usage/nsh_usage.rs` | 951–1094 |
| E | `text_input/native_adapter.rs` | 506–1794 |
| E | `text_input/nsh.rs` (`recovery_tests`) | 2209–3041 |

### Non-module test-only helpers in mixed files

- B: `conversation_stream/copy_receipt.rs:7,15,23,31,34,41`: remove gated `MessageCopy` import, `CopySource::Message` variant and its match arms/fallback. Remove matching `CopySource::Message` arm in `copy_receipt/native.rs:116`. Keep `CopySource::Code`, code-copy timers/receipts and **all actual message-copy behavior in its production owner**; do not delete `MessageCopy` itself.
- B: `conversation_stream/model.rs:1140`: remove gated `MessageCopy::publish_retained` helper only; keep retained content/selection production methods around it.
- B: `selection_menu/body.rs:136,193`: remove test-only fragment `row` field/initializer together; `:452,460`: remove gated `row_at` and `remap_row_anchor`. Preserve source mapping, source-anchor implementation and production fragment fields.
- B: `ordinary_submissions/presentation/pixels.rs:151,198`: remove test-only `presentation_id` capture and `tests::retain_press(...)` shadow binding; preserve the actual `press` closure and `.on_click` wiring.
- B: `stream_fade.rs:2,245,255,262`: remove test-only `TextRun` import, `FadeFrame::line`, `LineFade` type and impl. Keep actual fade calculation/application and production frame/parts state.
- C: `settings/model_configuration/provider_bridge.rs:52`: remove the entire gated `impl NavigationAuthority` containing development fixture/change methods; retain production authority type/read logic. `settings/provider_management/draft.rs:175,180`: remove gated `development_fixture` and `development_change` helpers, retaining production stamps and revisions.

## 5. Feature-only exclusive modules, exports and observations

Feature-only support is executable development code even without `#[test]`. Remove it after archive; merely deleting features, renaming files, suppressing warnings or leaving unreachable modules is not cleanup.

### Whole exclusive UI support files (prefix `crates/vega_ui/src/`)

```text
command_palette/presentation/mounted_regressions.rs
conversation_stream/message_anchors/mounted_regressions.rs
conversation_stream/newrunactivity/header_integration.rs
conversation_stream/ordinary_submissions/presentation/mounted_regressions.rs
conversation_stream/scroll_composition_integration.rs
conversation_stream/selection_menu/body_scroll_integration.rs
conversation_stream/shell_recovery/command_control_regressions.rs
conversation_stream/shell_recovery/mounted_regressions.rs
conversation_stream/timestamps/presentation/mounted_regressions.rs
disclosure/scroll_group/host_regressions.rs
icons/host_regressions.rs
link_preview/focus_integration.rs
link_preview/integration_support.rs
settings/nsh_appearance_close_support.rs
settings/nsh_close_regressions.rs
settings/nsh_tail/mounted_validation.rs
text_input/nsh/native_integration.rs
tool_card/nsh_tool/mounted_regressions.rs
```

`body_integration_tests.rs` and `rename_tests.rs` were already listed in §4; do not count or assign them twice. The settings-close regression is currently **wired** into the four-case settings runner, even though its author report describes a proposed parent wiring step. Current source supersedes that older report status.

### Exact mixed-file feature gate anchors

Remove only the complete `#[cfg(feature = "nsh-reading-integration")]` item/statement/field and its exclusive captures/imports. Do not delete neighboring production expressions or loops. Prefix for this table: `crates/vega_ui/src/`.

| Owner | Mixed file | Gate line(s) / purpose |
|---|---|---|
| S | `lib.rs` | 7,16: test entry/reexport facade |
| S | `command_palette.rs`; `command_palette/presentation.rs` | 3; 551,913: entry reexports, observe-only effect, fixture module |
| S | `disclosure/scroll_group.rs`; `icons.rs` | 11; 6: regression modules |
| S | `icons/nsh.rs` | 15,31,92,114,147,168,187,192,204,211: observation storage/setup/layout/paint/callbacks |
| S | `icons/tooltip.rs` | 21,62,216,236,266,287,317,325,337: observation and test-only accessibility assertion helper |
| S | `link_preview.rs` | 17,21,24,37,917,1102,1111: support/focus modules, export, registry request override, measured-focus helper, captured override and loader branch |
| S | `nsh/scroll.rs` | 1,5,10,16,24: test-only prelude, `scoped`, `Scoped` type/impls; remove gated lines 1–2 and 5–67; **retain the ungated `use gpui_kit::*;` at line 3, lines 69–94 `ScrollRemainder`/`consume_list`, and facade module registration** |
| C | `settings/mod.rs`; `settings/nsh_tail.rs` | 203; 12: mounted-validation reexports/module |
| C | `settings/nsh_appearance.rs` | 21,266: `close_support` module and observe call; keep production `revoke`/receiver fence |
| C | `settings/nsh_input_preferences.rs` | 22,272: close regression module/observe call; keep production `revoke`/receiver fence |
| C | `sidebar/mod.rs`; `sidebar/threads_block.rs`; `sidebar/threads_block/rename.rs` | 761; 9,12; 16: mounted rename export/module and `test_state` diagnostic |
| E | `text_input/mod.rs`; `text_input/nsh.rs` | 141; 436,443,471,2204,2206: mounted export, `nsh_test_host`, `nsh_test_state`, `nsh_test_registered`, fixture module/reexport |
| S | `tool_card.rs`; `tool_card/nsh_tool.rs` | 2; 2,815,844: mounted fixture export/module and observe-only effect/layout measurements |

When removing link-preview's injected `external_request` loader branch, retain the normal `service.load(...)`, cancellation/current-key checks, focus painting and real links. When removing feature-only shadowed `root`/`text`/`button` variables in icons or other leaves, preserve the original builder and the unconditional downstream usage. A visible accessibility label or semantic role is not a test selector merely because observers inspect it.

### Conversation feature anchors (owner B; prefix `crates/vega_ui/src/conversation_stream/`)

| Mixed file | Gate line(s) |
|---|---|
| `mod.rs` | 4,6,15,338,347,362,365,367,369,379 |
| `message_anchors.rs` | 4 |
| `message_anchors/pixels.rs` | 8,27,118,166,179,213,234,264,272,284,294,306,328 |
| `newrunactivity/mod.rs`; `run_activity.rs` | 11,13; 8 |
| `ordinary_submissions.rs`; `ordinary_submissions/presentation.rs` | 4; 17,524 |
| `ordinary_submissions/presentation/pixels.rs` | 62,109,162 |
| `selection_menu/body.rs` | 10,13,982,1017,1054,1192,1586,1588,1747,1760,2011,2025,2060,2073,2118,2121,2123,2128,2130,2267,2272,2274,2277,2279 |
| `selection_menu/mounted.rs` | 67 (`ReadingList::with_owners` fixture constructor) |
| `shell_recovery.rs`; `shell_recovery/nsh_view.rs` | 2,5; 3,771,919,964,966,974 |
| `thinking.rs`; `thinking_body.rs` | 211 (`integration_source_copy`); 203 (`[nsh-height]` stderr trace) |
| `timestamps.rs`; `timestamps/presentation.rs` | 5; 8,200,220,260,289 |

`mod.rs` includes the exclusive scroll-body factory/access installer and `ReadingList` reexport, not just test functions. Remove that entire gated factory, not the production list constructor. Keep production header mounts and lifecycle methods in `newrunactivity`/`run_activity`.

## 6. Diagnostic scaffolding that survives a naive gate scan

Owner B must also remove the ungated, exclusively diagnostic `macro_rules! trace_anchor` at `selection_menu/body.rs:1052` and **all `trace_anchor!(...)` invocations in `apply_source_anchor`**, not leave an empty macro after its gated body is removed. Preserve every surrounding rejection/return, lease/currentness check and actual anchor actuator call. Remove gated direct anchor traces at `:982,1017,1192`, all `Stage::begin`/explicit `drop(stage)` pairs, and feature-injected `projected_body`/content shadowing at `:1586,1588`. Keep actual layout request handling, `prepaint`, `apply_source_anchor`, `paint_opacity`, child painting and link painting in the same order. Test-only timers do not own these operations.

Other B observation captures belong with their callbacks: `shell_recovery/nsh_view.rs:964,966` layout captures pair with `:974` recorder; `ordinary_submissions/presentation.rs:524` measurement recording pairs with its fixture; timestamps/message-anchor test layout hooks must not take real production measurements with them. Owner S applies the same rule to icon/tooltip/tool observers. Owner V removes all backend timing spans with their definitions (§8).

M owns `scripts/i18n_allowlist.json`: remove only the **60 fixture entries / 132 counted occurrences** tied to the removed development source, after confirming their literals disappeared from those scoped regions. Exact path groups and current entry counts:

```text
automation_editor/nsh_editor/tests.rs                         3
command_palette/presentation/tests.rs                        2
commit_panel/tests.rs                                        3
conversation_stream/conversation_status/tests.rs             7
conversation_stream/newshimmer/tests.rs                      5
conversation_stream/resume_tail/tests.rs                     1
conversation_stream/selection_menu/body_opacity_tests.rs      3
conversation_stream/selection_menu/body_tests.rs              1
conversation_stream/selection_menu/source_anchor_tests.rs     2
conversation_stream/selection_menu/tests.rs                  1
conversation_stream/stream_fade/tests.rs                     1
conversation_stream/user_disclosure/tests.rs                 1
settings/agents/nsh_agents.rs                                1
text_input/editing_core_tests.rs                             7
text_input/native_adapter.rs                                6
text_input/nsh.rs                                           11
text_input/nsh/native_integration.rs                         5
```

All paths above have prefix `crates/vega_ui/src/`. For mixed files, remove only allowances identifying the now-deleted inline test module; preserve any production literal classification. **Keep** the separate `render_rows.rs` benchmark-fixture entries and all protocol/model-prompt allowances. Do not modify the Python checker, Python tests/fixtures or actual translations to accommodate deletion. P2-authored strings may expose stale/missing counts when later checked; reconcile from actual source, never weaken the checker.

## 7. Cargo targets, features, dev dependencies and lockfile (owner M)

### Exact removable edges

- `crates/vega/Cargo.toml:6–13`: `[features] nsh-workspace-integration = ["gpui_kit/test-support"]` and the **single** `[[test]] name = "nsh_workspace_mounted"`, path `src/window/nsh_workspace/mounted_main.rs`, `harness = false`, required feature `nsh-workspace-integration`. No `[dev-dependencies]` block exists in this manifest. Retain normal app dependencies.
- `crates/vega_ui/Cargo.toml:6–7`: `nsh-reading-integration = ["gpui_kit/test-support", "nsh-platform-gpui/vega-host-regressions"]`.
- UI manifest `[[test]]` registrations: `nsh_reading_geometry` (harness false, **no feature requirement**), and `nsh_reading_integration`, `nsh_scroll_integration`, `nsh_icons_integration`, `nsh_settings_integration`, `nsh_recovery_integration`, `nsh_message_anchors_integration`, `nsh_palette_integration`, `nsh_ordinary_controls_integration` (all harness false, required `nsh-reading-integration`). Remove all nine named blocks.
- UI manifest `[dev-dependencies]:71–73`: remove **only** `gpui_kit = { workspace = true, features = ["test-support"] }` and `nsh-platform-gpui = { workspace = true, features = ["vega-host-regressions"] }`; the now-empty table can go. Retain both dependencies in normal `[dependencies]`.
- Remove all ten exclusive files in `crates/vega_ui/tests/`: `nsh_icons_integration.rs`, `nsh_message_anchors_integration.rs`, `nsh_native_host.rs`, `nsh_ordinary_controls_integration.rs`, `nsh_palette_integration.rs`, `nsh_reading_geometry.rs`, `nsh_reading_integration.rs`, `nsh_recovery_integration.rs`, `nsh_scroll_integration.rs`, `nsh_settings_integration.rs`.

`nsh_native_host.rs` has **no explicit manifest target**: Cargo auto-discovers it and its macro expands calls into vendor `host_regressions`. Deleting only `[[test]]` registrations leaves implicit discovery; deleting only features leaves source residue. Geometry is also feature-ungated. Do not set `autotests = false`, `test = false`, ignore tests, change filters/CI or otherwise hide these files instead of deleting exclusive source.

Current custom case array sizes (source assertions, not observed results): geometry **24**, reading **15**, scroll **16**, icons **3**, settings **4**, recovery **3**, message anchors **6**, palette **3**, ordinary controls **4**, workspace mounted **6**. These harnesses run their own `main`/reporting; zero libtest attributes does not establish zero cases. The host macro supplies additional libtest tests; do not equate the macro's one literal attribute with one discovered test. Archive actual full runner output before cleanup.

### Lockfile discipline

Root `Cargo.toml` normal pins, NSH path patches and vendor exclusions stay. Do not revert `Cargo.lock` to HEAD: this would undo production path-source replacement and mainline dependency content.

Static comparison to HEAD finds 14 new registry identities associated with the development feature graph, plus the two intentional NSH registry→path identity replacements. **Candidates, not an unconditional package deletion list**: `atomic 0.5.3`, `convert_case 0.11.0`, `crossbeam-queue 0.3.13`, `fixedbitset 0.5.7`, `futures-concurrency 7.7.1`, `pollster 0.2.5`, `postage 0.5.0`, `proptest 1.11.0`, `proptest-macro 0.5.0`, `quick-error 1.2.3`, `rand_xorshift 0.4.0`, `rusty-fork 0.3.1`, `unarray 0.1.4`, `wait-timeout 0.2.1`.

Cached pinned Kit/Fast manifests confirm `test-support` forwards into GPUI collections/http-client/proptest and optional concurrency/postage/platform support. This static evidence does not compute Cargo feature unification or prove every candidate will be unreachable. After all source/manifest cleanup, M must resolve using the existing approved pinned toolchain (offline where feasible), remove only truly unreachable test-support packages/edges, and compare retained versions/source/checksums. No dependency upgrades, Kit/Fast changes or broad dev-dependency pruning; `tempfile`, shared `syn`/Tokio/font parsing packages and platform dependencies may remain production-reachable. Record exact resulting lock graph rather than promising a fixed removal count now.

## 8. Vendor boundary (owner V)

Both packages are **excluded dependencies**, not workspace members. Workspace nextest does not enumerate their upstream unit/integration tests. Distinguish “no delivered workspace Rust development tests” from “no `#[test]` text anywhere in a vendored dependency.” If a later authority requires literal repository-wide upstream stripping, that is a separate scope decision; do not silently widen #907 cleanup.

Verified cached `.crate` archives, without extracting or modifying files:

| Package | Archive SHA-256 | Original manifest verification |
|---|---|---|
| `nsh-core 0.4.0` | `171f17b402bedb0cead975d3abcefbf0e7963583fbdbf1dbcb03ef73ba11191d` | All **180** archived files equal `nsh-core-original-files.json` hashes |
| `nsh-platform-gpui 0.4.0` | `1e3828245c26e734567b3e6058398c0e4d5809063636460f990a0eaf8a0f74c3` | All **45** archived files equal `nsh-platform-original-files.json` hashes |

Original `Cargo.toml.orig` files are unchanged. Platform normalized manifest's **only** archive delta is `vega-host-regressions = []` at line 29; remove that feature, not upstream `dev`, `shot`, `runtime-shaders`, JS/webview/video features or upstream target dev dependencies (`gpui` test-support and `nsh-bundler`). Core normalized manifest/dev dependencies/`attachment_compat` registration are original; **no core manifest deletion**.

### #907-added exclusive files — delete whole file after archive

Prefix `vendor/nsh-platform-gpui/`:

```text
src/app/integration_support.rs
src/app/scroll_integration_support.rs
src/app/text_integration_support.rs
src/input/scroll_host_tests.rs
src/reading/anchor_host_tests.rs
src/reading/geometry_host_tests.rs
src/reading/host_tests.rs
src/reading/opacity_host_tests.rs
src/selectable_view/host_tests.rs
src/text_input/host_tests.rs
```

### Mixed production files — remove only development regions

All gates here are `#[cfg(feature = "vega-host-regressions")]`; preserve all other code unless proven exclusive and explicitly listed. Prefix `vendor/nsh-platform-gpui/src/`:

| File | Gate anchors / exact development role |
|---|---|
| `lib.rs` | 45: integration-support module; 75: complete public `host_regressions` reexport module; 158: integration observation type reexports |
| `input.rs` | 88: `scroll_host_tests` module |
| `reading.rs` | 20,27,29,32: opacity/geometry/anchor/general host-test modules |
| `selectable_view.rs`; `text_input.rs` | 41; 3140: added host-test modules |
| `rt.rs` | 48,51,57,61: batch/Jobs timing start/end and explicit drop; 129: flush timing span; 134–264: entire exclusive `diagnostics` module |
| `surface.rs` | 136,259,444: host-event buffer/initialization/event cloning; 544,562: repaint/drawing counters; 591,650: drain/apply timing spans |
| `scroll.rs` | 162,167: geometry scroll test step/frame helpers |
| `reading/geometry.rs` | 257,283: capture/current timing spans; 357: test-only `matches_snapshot` |
| `reading/opacity.rs` | 189: currentness timing span; 215: `retained_publications` accessor |
| `text.rs` | 2261,2268: `OpacityObservation` and `observe_reading_opacity` paint-observation helper |

In `reading/opacity.rs`, remove the timing statement but retain the real currentness predicate/block. In `rt.rs`, retain queue batching/job execution/flush and upstream `#[cfg(test)] init_test_app`. In `surface.rs`, retain event delivery/repaint coalescing/drawing state/drain/apply behavior. Remove observer state and producer callbacks as a unit with the integration support files; no production “no-op replacement” or diagnostic-only feature remains.

### Explicitly retain production capability additions and upstream tests

Whole added capability files to **keep**: `src/editing.rs`, `src/events/scroll_bridge.rs`, `src/reading.rs`, `src/reading/geometry.rs`, `src/reading/opacity.rs`, `src/scroll_scope.rs`. The mixed files above remain with only their development regions removed. Keep patched production portions of `app.rs`, `events.rs`, `input.rs`, `native.rs`, `render.rs`, `rt.rs`, `scroll.rs`, `selectable_view.rs`, `surface.rs`, `system.rs`, `text.rs`, `text_input.rs` and exports in `lib.rs`.

Core's only changed original is `vendor/nsh-core/src/selection.rs`: added `endpoints` and validated `set_endpoints` are production capability patches. Its original `#[cfg(test)]` module at current line 745 is unchanged and remains. For every changed platform original with a trailing unit-test module (`input.rs`, `native.rs`, `render.rs`, `scroll.rs`, `surface.rs`, `system.rs`, `text.rs`, `text_input.rs`), the complete original unit-test-module suffix is still present byte-for-byte. Do not delete those upstream modules. Preserve upstream `src/tests.rs`, `src/svg/testdata`, core `tests/attachment_compat.rs`, platform `examples/embed.rs`, existing `shot` tests and upstream test helpers. `vendor/block` and `vendor/mdstream` need no #907 cleanup.

## 9. Verification plan for the later cleanup (not executed here)

1. **Before deletion:** recover/read back the published development archive, identify source hashes including both fixes, reconcile actual new regression results with original logs. The settings review includes a static P2 closure appendix. The historical native-security report still records the paste P2; the concurrently published `review-single-line-paste-final-delta.md` separately records its static resolution for four exact files. Bind both current review conclusions to the archive; do not treat the unchanged historical report as a reopened finding. Inspection lanes here do not satisfy the non-author implementation review requirement. Do not erase an outstanding build/test failure with cleanup.
2. **Static final-tree inventory:** enumerate all workspace Rust sources, module `#[path]` edges, implicit `tests/` targets, macros emitting tests, unit/GPUI/Tokio/property/bench attributes, `cfg(test)`/`cfg_attr`, test-only modules/helpers/fixtures, development feature strings, observers/diagnostics, source literal allowances, and executable doc fences. Expect no workspace Rust tests/exclusive support, not zero upstream vendor markers. Check no stale `trace_anchor!`, `Stage`, integration `Request` overrides or removed feature references. Existing `vega_markdown/src/stream.rs` has a `text` doc fence (not executable); no other workspace executable Rust doctest fence was found by this scan. Recheck final source, rather than merely disabling doctests.
3. **Preservation comparison:** compare production projection/diff of mixed files, all Python tests/fixtures and benchmark/packaging/workflow/config paths to the archived pre-cleanup source. Verify security guards/production assertions, native APIs, settings-close revocation and paste policy remain. Vendor provenance must classify ten deleted additions, development-only mixed-file hunks and six retained capability additions; unchanged originals still match archive. Recompute current vendor manifest/evidence in a later authorized documentation step, rather than mutating old historical identities or pretending the pre-cleanup manifest still matches.
4. **Cargo graph/compile checks under later authorization:** record actual Cargo/rustc/fork versions, current source SHA/content hash and elapsed/result for `cargo metadata --no-deps --offline --locked --format-version 1`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and necessary production compile checks. Review metadata targets to ensure no residual explicit or autodiscovered integration tests. Library/binary test-capable metadata targets can exist even with zero test functions; that alone is not failure or proof of discovery. Inspect feature graph and lock identities before/after; don't remove upstream dev features merely to force global text-zero.
5. **Actual discovery/execution:** use the existing CI-equivalent commands `cargo nextest list --workspace --message-format json` and `cargo nextest run --workspace --no-tests=pass`; retain JSON `test-count` and real run output. Require **0 discovered and 0 executed**, no ignored/skipped hidden tests or removed filters. `harness = false` targets need source/metadata auditing too; nextest JSON alone is not evidence their custom case arrays disappeared. Confirm no runnable workspace doctests separately (source audit and, when authorized, workspace doc-test check); nextest does not cover doctests. Report `no Rust tests`, not “Rust tests passed.” Do not execute excluded vendor test suites as part of this zero-workspace gate.
6. **Required final-head CI:** retain unchanged `.github/workflows/pr-check.yml`: fmt, all-target Clippy, Python release/preparation/unittest suites, nextest discovery/run and aggregate **`check (fmt, clippy, test)`** must succeed for the exact final head. The existing workflow reports an empty suite but also allows nonempty successful suites: the parent must independently assert JSON count zero. Check status at the required two-minute interval (or an actual completion notification), no watch/short polling. Archive developer functional results separately from empty-suite gate; earlier green or this static inventory cannot substitute for final head.
7. **Independent cleanup review and native acceptance:** review only cleanup diff/affected preservation invariants by a non-writer; retain already completed implementation review for unchanged regions. Merge only after required gates. User hand testing remains independent; stay In review/open until explicit user acceptance, never equate cleanup/CI with native acceptance or issue completion.

## 10. Native acceptance gaps and unresolved uncertainties

- This task ran no Cargo/rustc, builds, Clippy, formatter, tests, application, OS input/clipboard/accessibility operations or network/CI queries. No discovery/execution/pass totals are claimed. Static literal attributes (**31 app / 380 UI**) are not runtime counts and include macro declarations; they must not be used as archive execution evidence.
- Both author fix documents say their new regressions were unrun in those lanes. Current settings runner contains the fourth case. The settings report's later static independent appendix closes its P2. The historical native-security report still records the paste finding, but the newly published `review-single-line-paste-final-delta.md` records no actionable findings and resolution for current four-file identity `125bd6a51d143d483c69501ec70c317bf551114199b0c58b921d7fcf5144cced`. This is the independent reviewer's conclusion, not review performed by this inventory. The paste baseline-recovery document alone is evidence recovery, not approval. Parent must bind these scoped review identities and actual central logs to the eventual archive. The concurrent native-acceptance inventory cites parent execution receipts, but those receipts were not independently read or rerun here, so this inventory makes no new runtime-pass claim.
- Settings retained-owner fixture uses real queues and a real retained SettingsView, but not full application loading-route/sidebar persistence. Accepted-save-close coverage does not deterministically suspend a worker at the close boundary; no new deterministic permission-IO-pending coverage was established. These are recorded limits, not permission to broaden implementation or weaken assertions.
- Native macOS/Windows/Linux pointer/focus/keyboard/IME candidate, scrolling/gesture, layout/pixels and actual OS accessibility/assistive-technology acceptance remain separate. GPUI TestAppContext and public backend hook tests are not actual OS AX, clipboard acquisition or cross-platform acceptance. Screenshots can establish geometry/colors but not focus-chain correctness; synthetic input is not an accepted replacement for user focus testing under root guidance.
- Real clipboard text/image/opaque metadata and SingleLine LF/CRLF/lone-CR behavior, native Return persistence after rename paste, composition/undo/redo and stale-session rejection need user/native acceptance appropriate to their product boundary. No OS HTML acquisition capability is authorized or implied.
- Published durable development-archive URL/full recovery proof and final-head CI are **not obtained** in this offline planning task. Local original `.crate` integrity is established, not development archive publication. Ignored `target/` review manifests/logs are outside the content hash and cannot alone satisfy archival.
- The exact final lock package removals cannot be certified without permitted post-cleanup Cargo feature resolution. Listed lock identities are candidates only. No broad vendor test/dev-dependency stripping is proposed; if literal all-vendor zero-tests is desired, it requires an explicit scope decision.
- Later source changes invalidate line anchors/content identity and require inventory delta reconciliation. The source-stop premise is user supplied; this task checks bytes rather than inspecting or stopping other processes. Future writer activation, publishing, cleanup, CI and final provenance edits need separate authorization.

## 11. Content identities and actual inspection result

SHA-256 of important inspected inputs (no self-referential document hash; final handoff supplies this document's hash):

| Path | SHA-256 |
|---|---|
| `Cargo.toml` | `1d516f3491e8a782b1b6ef3b7bdc34a92147dba4e9e4dab011c5da521844f2d9` |
| `Cargo.lock` | `948d0839ab76c6256e0af190d84998d3f3505d3d9f24e27f1be0423955529eae` |
| `crates/vega/Cargo.toml` | `cec7e1b1d9688ea3a85ae526e5bd84021882bd89271113449224830aa982ad77` |
| `crates/vega_ui/Cargo.toml` | `58857f2f9198dde03ff6b0e896a07836859f764ee6df842bcbccb0a5389122e6` |
| `vendor/nsh-core/Cargo.toml` | `cf4757fe74ba4fe4b2bfb0733a7495f7fd8f04ee185cdb71efb9b6d5f5cd9313` |
| `vendor/nsh-core/Cargo.toml.orig` | `44babeb3a88337d9afd8f4c1a593d491822dd72a0f4f18575fdfd31c71234e73` |
| `vendor/nsh-platform-gpui/Cargo.toml` | `6519a8053c0731a80fa40d5d21e880b85808db41dc72860abfd20c239ad0db05` |
| `vendor/nsh-platform-gpui/Cargo.toml.orig` | `cdc7473424475eaf01d800bc552c10a85965dd505864542e2c9c55c8c1e733bc` |
| `docs/907/nsh-core-original-files.json` | `6e0856cdbc345291557ff5dfe3d3e0d14e78cb29a742b791f067d93e78ca34fd` |
| `docs/907/nsh-platform-original-files.json` | `612172f1c99ce52e34ad9fce8ef545a2c1eda190d0544e2b9210b66d6e325037` |
| `docs/907/nsh-backend-current-manifest.json` | `73318b7f0de0621ea9537f6704fdbec7a89fc75940c5073ed1ed3b9681cc64fc` |
| `docs/907/nsh-review-single-line-paste-fix.md` | `99db079d6180d38a0c411466b5e35e922410c37c920de99a83886197254bfd5a` |
| `docs/907/nsh-review-settings-close-fix.md` | `84adff5768b7c05abd26339a37156b57f7d43d9749c4c69efa12d2081ec8070e` |
| `docs/907/review-native-security.md` | `cfd52a85052b1e1663b0cc707ef49e02bee8adfc80b7eecbfaff722b0bcc06a3` |
| `docs/907/review-app-settings-workspace.md` | `fc1af91c7c991f3804200dcf3228df26b5d93f679daabf287ff3d1c34cbe0218` |
| `docs/907/review-conversation-production.md` | `f7df5bee29688a3cc8cae076280c0c3185eebd91f8814471b3a338266146cbf0` |
| `scripts/i18n_allowlist.json` | `dd2c6d07e28e25377bc6f5196edb516396c6843b2ae12259eb5d363672914c34` |
| `.github/workflows/pr-check.yml` | `536ee05031209834437b7c03be7e076392ec9ab7f81b77b25f2ed21ef977150a` |

Current vendor full-tree hashes, using sorted `(package-relative path, SHA-256)` entries and Python-default `json.dumps`, are core **`dd6cc201bddb9508db77d0470c1b1f12dda9e32fa8e6f1de452feb8fd804afa1`** and platform **`d2654a80a9cc336403342eeba8098ef79c7b48e1c42769bdfe6bdb2f320104ee`**. Vendor diff against verified original manifests: core **1 changed / 0 added / 0 missing**; platform **14 changed / 16 added / 0 missing**, comprising **10 development-only additions + 6 production capability additions**. These are provenance/static classification results, not behavior/security validation.

Actual operations: read-only Git HEAD/status/diff/show/ls-files; focused source/TOML/JSON/CI inspection; Python static scans and in-memory archive comparisons; SHA-256 calculations; creation/read-back/static validation of this document only. One probe used nonexistent `crates/vega_ui/src/nsh/mod.rs`; its `rg` exited 2, then the actual facade `crates/vega_ui/src/nsh.rs` was located/read. No source changed to address that observation error. No forbidden operation was attempted.

The first final snapshot equality assertion failed because two concurrent documents appeared: `docs/907/nsh-native-acceptance-blockers.md` (SHA-256 `4b193dee3dd1fa9e5647f9ce827548f98de03adba15e21e80877cbeb9e3aec65`) and `docs/907/nsh-paste-baseline-recovery.md` (SHA-256 `a4180eafaf6fe520f382f270ef98c2da4c0a1c79cb2d868c43515c0cb31ad82e`). Excluding this inventory and those two new documents reproduces **exactly the original 2,153-file digest** above: original membership and bytes are unchanged, not merely selected input hashes. With both concurrent documents included but this inventory excluded, the 2,155-file digest is `c1c0443141f27dca58c8cca8f9c161060c1ab1fddcefaf31002643414802e2e1`. Their author conclusions are separate evidence, not this lane's review or runtime results. Neither document was written or edited by this task. A second final snapshot assertion subsequently found a third concurrent addition, `docs/907/review-single-line-paste-final-delta.md`; its separately authored static paste-review conclusion is incorporated above, not rerun here. Final verification excludes this inventory and all three documented concurrent additions when comparing the original 2,153-file digest. Final read-back verified all 18 input hash-table rows, enumerated exclusive source paths, Markdown fences and whitespace. These are document/static integrity checks, not Cargo or behavioral tests.

**STOP — planning inventory only. No test/support removal, source/manifest/lockfile modification, archive publication, runtime validation, CI query, merge or native acceptance is claimed.**
