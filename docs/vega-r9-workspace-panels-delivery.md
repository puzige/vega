# R9 workspace panels — implementation delivery

Conversation and composer now coexist with a real Review/preview workspace. The same route-owned views move between the right pane and a bottom dock spanning conversation plus workspace; the task sidebar stays outside the dock. Scope follows `vega-r9-workspace-panels.md` and preserves R8 controllers, drafts, permissions and safe artifact routing. No Antigravity features, new dependencies or schema changes were introduced.

## Freeze

- verified_at_utc: 2026-09-06T00:06:45Z
- verified_at_local: 2026-09-06T08:06:45+08:00
- branch: `feat/r9-workspace-panels`
- source_commit: `436a8eb` (this report is a subsequent documentation-only commit)
- implementation_tree: `24b4f4d0d0b57141850708f4988325f9f6231d8a`
- implementation_diff_sha256: `d14548a24f253f92cd7420e1ffabf7986682cd254fffc4852c009ae974e944d1`
- task_contract: `docs/vega-r9-workspace-panels.md`
- environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0; Git 2.55.0
- compiled artifact: `target/debug/vega`
- artifact_sha256: `e877bb224a26f03572526f31943e1147928d092896daa3eb61ed4049373c580b`

## Changed files

- `crates/vega/src/window/workspace.rs`: root-owned tab/dock state, overflow/add menu, keyboard activation, focus restoration, drag splitters, maximize/restore, narrow visibility, real preview virtualization and production-root regression.
- `crates/vega/src/window/{render,mod,diff,artifact}.rs`: remove Diff takeover; keep conversation entity mounted; connect production Review and bounded preview requests; invalidate previews with their authority; reopen retained Review intent after Settings without unhiding it.
- `crates/vega/src/main.rs`: scoped menu Escape binding and persisted appearance loading.
- `crates/vega_ui/src/conversation_stream/{core,render}.rs`: host-width projection compacts mode/permission/thinking controls without resetting input, keyboard actions or selected values.
- `crates/vega_ui/src/diff_view/{mod,render,state}.rs`: compact accessible controls, filename ellipsis, truthful Chinese empty state and reuse of existing retry behavior.
- `crates/vega_ui/src/artifact_card.rs`: expose the existing bounded preview request as the workspace's production action.
- `crates/vega_ui/src/icons.rs`: original line icons, tooltips, accessibility labels and Enter/Space activation.
- `crates/vega_ui/src/settings/render_impl.rs`, `crates/vega_theme/src/lib.rs`: functioning light/dark/system appearance and existing sidebar preference controls; current appearance stays reflected after the theme shortcut; native system changes follow the selected system preference.

## Button and behavior inventory

| Control / behavior | Delivery |
|---|---|
| Existing conversation Review action | IMPLEMENTED: opens real controller beside conversation |
| Workspace + / tab overflow chevron | IMPLEMENTED: real Review and available artifact preview entries; scrollable menu |
| Add-menu rows | IMPLEMENTED: mouse, Tab/Up/Down navigation, Enter/Space; local Escape and outside-click dismissal |
| Tabs | IMPLEMENTED: selected/hover states, ellipsis, horizontal overflow, keyboard selection |
| Tab close | IMPLEMENTED: closes tab; Review controller closes; adjacent selection or composer receives focus |
| Pane hide / show | IMPLEMENTED: retains tabs, controller and drafts; explicit show control restores pane |
| Move to bottom / right | IMPLEMENTED: same selected tab/view moves; bottom excludes sidebar |
| Maximize / restore | IMPLEMENTED: fills content host, retaining cached conversation and composer state |
| Splitters | IMPLEMENTED: 1px visual separator within 5px drag target; usable size clamps |
| Narrow layout | IMPLEMENTED: panes auto-collapse below usable width/height without deleting state; compact composer controls retain current values in tooltips |
| Review refresh / unified-split / previous-next hunk | IMPLEMENTED: existing production actions; typed failures remain visible |
| Artifact Preview | IMPLEMENTED: existing bounded, provenance-checked channel; virtual rows in dock; external handoff actions remain available |
| Settings appearance / sidebar | IMPLEMENTED: existing config infrastructure; no new unsupported switches |
| Task switch | IMPLEMENTED: old route tabs cleared and controllers fenced |
| Settings round trip | IMPLEMENTED: Review intent reopens via controller; invalidated preview tabs close and are available again when current artifact capture rebuilds |
| Interactive terminal | REMAINING CAPABILITY: no PTY backend exists; no fake terminal or command-output pane is mislabeled |
| Workspace session restoration after application restart | LIMIT: dock sizes, tabs and placement are window-local; appearance/sidebar preferences persist |

## Results

Raw logs are retained under `/private/tmp` (equivalent to `/tmp` on this host). These are separate evidence boundaries: passing focused tests does not erase the full-suite failure.

| Requirement | Evidence class | Exact command | Result / bounded footer | Raw log |
|---|---|---|---|---|
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS; empty output | `vega-r9-fmt-final.log` |
| Existing production Diff regressions | E2E-REAL | `cargo test -p vega --locked diff -- --test-threads=1` | 5 passed; 0 failed; 2.52s test execution | `vega-r9-diff-regression-initial.log` |
| Production root, real Git, moves/hide/reopen, route and keyboard | E2E-REAL | `cargo test -p vega --locked workspace_root -- --test-threads=1` | 1 passed; 0 failed; 0.44s test execution | `vega-r9-workspace-regression-keyboard.log` |
| Strict final lint | STATIC | `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS; finished dev profile in 1.65s | `vega-r9-clippy-gate.log` |
| Complete workspace | MIXED existing workspace suite | `cargo test --workspace --locked --no-fail-fast` | FAIL: 901 passed / 1 failed / 1 ignored across unit/integration/doc targets | `vega-r9-test-workspace-initial.log` |
| Final workspace build | BUILD | `cargo build --workspace --locked` | PASS; finished dev profile in 2.70s | `vega-r9-build-workspace-final.log` |
| Runtime dependency direction | STATIC | `cargo tree -p vega_runtime --locked` | No GPUI/UI/theme crate in runtime tree | `vega-r9-runtime-tree.log` |
| Source whitespace / dependency / token review | STATIC | `git diff --check`; scoped diff and token inspection | PASS; no dependency, lockfile, migration or runtime/provider edits; new colors/font sizes use tokens | task tool transcript |
| Native visuals | MAIN-OWNED | real macOS app, compact and large windows | Main reported coexistence, dock movement, splitters, hide/close, composer draft, Settings return, themes and menu Escape checks; final package verification remains main-owned | main's external native acceptance record |

The new production-root test uses an owned temporary repository, the actual `VegaWindow` renderer and real Diff controller/Git worker, with an owned in-memory store and injected owned Settings config. It checks unchanged composer entity/draft, real changed-file rows, right-to-bottom and back movement, hidden-view reuse, close versus reopen, Settings controller recovery, task switch invalidation and menu Tab+Enter. The app-global Escape fallback is installed while local menu Escape is exercised. This is not real-provider, IME or native pixel-parity evidence.

### Preserved failures

- The full workspace run has exactly one failed target (`-p vega --bin vega`). Existing test `diff_refresh_intents_keep_content_during_background_and_retry` failed at retry with `generation=Some(1) refreshing=false refresh_error=Some(GitFailed) row_count=1 snapshot_stats=Some(WorkspaceStats { file_count: 1, additions: Known(1), deletions: Known(0) })`. The content was retained. This matches R8's unresolved intermittent area; no assertion, timeout, controller or Git backend was weakened. Main instructed retaining this result without repeatedly rerunning the workspace to chase green.
- Initial `cargo fmt --all` found a mismatched delimiter in the new workspace renderer; exact output remains in the task tool transcript. Corrected before successful compilation.
- First `cargo check -p vega --locked` found ambiguous `ArtifactCard` glob imports; retained in `vega-r9-check-initial.log` and `vega-r9-check-second.log`; fixed with the explicit UI entity import.
- Initial strict lint found one collapsible conditional; `vega-r9-clippy-initial.log` retained; corrected without altering behavior.
- Initial production-root test compilation encountered recursively shadowed GPUI test macros through a wildcard import; `vega-r9-workspace-regression-initial.log` retained. Explicit test imports fixed the macro collision; second and keyboard-enhanced runs passed.
- Native review found a Settings round-trip orphan Review tab, narrow composer wrapping, and clipped filenames. These were corrected; main reported the corrected Settings round trip, single-row controls and visible filename in the subsequent native build.

### Raw-log hashes

| Log | SHA256 |
|---|---|
| `vega-r9-fmt-final.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `vega-r9-workspace-regression-keyboard.log` | `d9df644ced7b2daca4bd99e6a3599ccd9d433701208c048cd402108418e55b0c` |
| `vega-r9-clippy-gate.log` | `e3f9b4a5d7a134bd8c2dcb300e5341f8e73cdac28ae29e84813fc2982b8cf207` |
| `vega-r9-test-workspace-initial.log` | `dc2213a21632ec3946f936fe999ae899454f4f9aa7c40c4b6f244f1c650d092f` |
| `vega-r9-build-workspace-final.log` | `fbc4e4bd52a07ed36861c145b3ee4ccec80c087e4dbbdac00fd46fe5c5602dcb` |
| `vega-r9-runtime-tree.log` | `de380771d971cfd93bd71672cd8a96bf56233b430fa7876f1bed0f737bc4eaa7` |

## Residuals

- UNRESOLVED: existing intermittent Diff retry `GitFailed`; full workspace is not all-green and this delivery is not a release approval.
- LIMIT: no PTY terminal, browser, IDE editor or proprietary reference-client feature is fabricated. Existing preview data is the genuine additional dock content.
- LIMIT: tabs and geometry are per-window memory state; task switch deliberately clears old authority, and Settings invalidation closes previews.
- NOT RUN by implementation agent: native CUA, real provider/network/billing, IME acceptance and installed-app replacement. Main owns native packaging and final screenshot verification.
- SKIP: performance explicitly deferred; real Keychain roundtrip remains the existing ignored test, not a pass.
- No push, integration-branch modification, installed-app change or user-config operation was performed by the implementation agent. Scope deviation: none; capability and verification limits are explicit above.

## Real-user preparation follow-up — separate automated freeze

At 2026-09-06T00:38:21Z (08:38:21+08:00), a narrowly authorized follow-up corrects a gap found by main's actual user-path check. Main closed old instances, made one controlled submission, observed zero durable messages and sampled the worker waiting inside macOS Keychain. The original process remains user/main-owned; this correction does not claim successful real-provider or native acceptance.

- Source changes: `crates/vega_ui/src/conversation_stream/render.rs` shows “正在准备请求…” and a conditional system-authorization hint while submission is pending; draft and durable echo semantics are unchanged.
- `crates/vega/src/app_agent.rs` checks cancellation after provider/credential construction returns, before **both** user-message and approved-plan runtime/durable entry points. This does not interrupt synchronous Keychain access or dismiss system authorization. No misleading Cancel control was added.
- `crates/vega/src/tests/agent.rs` contains one bounded deferred-provider-construction regression, using the existing MockProvider boundary and owned repository/store. Cancellation during that delay produces zero provider requests and zero durable messages. The delay is `cfg(test)` only; no production-public test seam or credential access was added.
- Spec supplement was written before code in `vega-r9-workspace-panels.md`. The source commit is the commit containing this follow-up section.
- Follow-up source SHA256: `de80cd63970335cfb7ab93a66f451892aeabb0f091f753b1d4e633687510dd55` (concatenate the three Rust paths listed above, in app-agent/test/UI order, each UTF-8 path + NUL + file bytes).
- Corrected build artifact remains `target/debug/vega`; SHA256 `fbb1226a26c618e5aa09b81dae134db15be378b02f39913b8da4c9b9de9f15b9`. This supersedes the earlier binary only for the follow-up source; main has not yet replaced the original pending native instance.

| Exact command | Evidence / result | Raw log under `/private/tmp` |
|---|---|---|
| `cargo fmt --all -- --check` | PASS, no output | `vega-r9-preparing-fmt.log` |
| `cargo test -p vega --locked tests::agent::deferred_provider_construction_cancel_starts_no_request_or_durable_message -- --exact` | FAULT-INJECTION at MockProvider construction; actual worker/store path; 1 PASS | `vega-r9-preparing-cancel-final.log` |
| Same exact regression with only the two new guards temporarily removed | Negative control FAILED as intended: “no durable event may precede cancellation terminal”; guards restored before final checks | `vega-r9-preparing-cancel-mutation.log` |
| `cargo test -p vega --locked tests::agent::` | 10 PASS, including production keyboard submit/permission continuation and the restored-guard regression | `vega-r9-preparing-agent-tests.log` |
| `cargo test -p vega_ui --locked` | 136 PASS | `vega-r9-preparing-ui-tests.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS; 2.41s | `vega-r9-preparing-clippy.log` |
| `cargo build --workspace --locked` | PASS; 3.20s | `vega-r9-preparing-build.log` |

The first exact-test invocation omitted the full module path and selected **zero** tests; it is not counted as a pass and remains in `vega-r9-preparing-cancel-initial.log`. The corrected exact command and final affected-agent suite each execute the regression. Full workspace was intentionally not rerun; the earlier 901/1/1 result and unresolved Diff retry failure remain unchanged evidence boundaries. Performance remains deferred. Real user completion, OS authorization and native verification of this supplementary status text remain **PENDING**, not PASS.
