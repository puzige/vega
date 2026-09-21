# Issue #70 — delivery and acceptance record

`PASS` requires fresh evidence; green unit tests do not substitute for native
pixels, and screenshots do not substitute for behavior assertions. Evidence
log names below are relative to the persistent Issue #70 evidence directory.

## Test-first matrix

| ID | Requirement / risk | Precondition | Action | Expected observable result | Level | Evidence | Status |
|---|---|---|---|---|---|---|---|
| T70-1 | One completed command is too visually heavy | One successful bash projection with duration and bounded output | Mount the production timeline | One quiet single-line row shows the command/duration with a neutral Terminal icon; no success Check, permanent card surface or missing chevron | mounted GPUI + painted quads | `green-issue70-category-icons.log`, T70-1 quad, selector and category-visual assertions | PASS |
| T70-2 | Adjacent calls need one resting summary | Assistant segment followed by bash/read/grep calls with no visible entry between | Apply proposals/results | One collapsed mixed-category group occupies one list item and one visible row in exact call order; its neutral leading icon follows the first child category | production stream test | `green-issue70-category-icons.log`, T70-2 exact copy/order/entity/category assertions | PASS |
| T70-3 | A real boundary must not be crossed | Tool calls separated by nonempty assistant text; include permission insertion/removal around an adjacent call | Apply the event sequence | Text splits groups; the transient permission card does not permanently split otherwise adjacent calls | production stream test | `green-issue70.log`, two T70-3 boundary/merge tests | PASS |
| T70-4 | Progressive disclosure must be scoped | Collapsed three-call group; final command has output | Activate group, then activate final child, then close it | Group reveals three compact children; only selected detail opens; full command/output and truthful status are visible; heights return on close | mounted GPUI interaction | `green-issue70.log`, T70-4 mounted clicks/heights/status tokens | PASS |
| T70-5 | Lifecycle updates cannot move or falsely complete the group | Pending/approved/running/success and failed calls in one group | Apply each durable event | Same call entities/order remain; aggregate/child wording, exit code and detail footer reflect failure while leading icons remain category-shaped and neutral | production stream + card unit | `green-issue70-category-icons.log`, T70-5 identity/copy/category/footer-token assertions | PASS |
| T70-6 | Reopen must match live layout | Typed history containing text → three tools → text, plus a separate one-tool segment | Hydrate, expand, switch/reopen | Same boundaries/order/summaries as live; initially collapsed again; no execution/replay | hydration/mounted GPUI | `green-issue70.log`, T70-6 live/hydrated parity and reopen assertions | PASS |
| T70-7 | Redaction and fail-closed behavior must survive redesign | Read-only raw args, strict write/edit results, invalid and corrupt projections | Render collapsed and expanded states | No raw read args, body, fingerprint, call ID, absolute data root or checkpoint ref; invalid/corrupt state remains visible | unit + mounted GPUI | `green-issue70.log`, T70-7 safe/secret allow-deny assertions | PASS |
| T70-8 | Narrow/light/dark native appearance | Packaged candidate with an existing real audited timeline | Open the real timeline; inspect collapsed mixed group and expanded shell detail in Light and Dark | Hierarchy matches spec; text/status contrast remains readable | installed native UI | `tool-activity-light-expanded.png`, `tool-activity-dark-collapsed.png`, `tool-activity-dark-shell-detail.png`, `manifest.md` | PASS (owner accepted current width) |
| T70-9 | Related behavior and architecture do not regress | Final candidate | Run focused and workspace gates | Tool/permission/timeline/hydration suites pass; no UI SQLite, hard-coded color, dependency or migration change | repository gates | final isolated focused/Clippy/workspace/package logs below | PASS |

The category-only leading-icon follow-up used a fresh focused matrix. Its logs
live in the persistent `issue-70-remove-success-check-2026-09-21` evidence
directory; the original delivery logs and screenshots remain unchanged.

| ID | Follow-up requirement | Evidence | Status |
|---|---|---|---|
| F70-1 | Every ToolCard lifecycle state keeps the category icon and neutral `text_secondary`; all nine category mappings are exact | `red-tool-card-leading-visual.log`, `green-tool-card-leading-visual.log`, `green-tool-card.log` | PASS |
| F70-2 | Mounted successful Shell, successful mixed group, failed Shell group and failed child keep category icons and neutral leading color while failure copy, exit code and semantic footer remain truthful | `red-issue70-leading-visual.log`, `green-issue70-category-icons.log` | PASS |
| F70-3 | Strict focused Clippy, format and diff gates remain clean | `clippy-first-failure.log`, `green-clippy.log`, `green-static-gates.log` | PASS |

The Bash live-elapsed follow-up uses a separate fresh matrix. Its logs live in
the persistent `issue-70-command-elapsed-2026-09-21` evidence directory; the
original and category-icon evidence remain unchanged.

| ID | Follow-up requirement | Precondition / action | Expected observable result | Level | Evidence | Status |
|---|---|---|---|---|---|---|
| E70-1 | Running begins at the runtime boundary, never approval | Project an actual `RuntimeEvent::ToolCallRunning`, then apply approval and running to one Bash card | The safe `ToolCallRunning { call_id }` reaches the UI; approval has no elapsed copy; running begins at `0 秒` and immediately repaints the mounted row | conversion unit + mounted production stream | `red-running-projection.log`, `final-rebased-running-projection.log`, `final-rebased-running-persistence.log`, `final-rebased-issue70.log` | PASS |
| E70-2 | Live formatting and cadence are deterministic | Advance the GPUI test executor clock across 0, 1 and 65 seconds | The same Bash entity reads `0 秒`, `1 秒`, then `1 分 5 秒`, updating from the executor clock without real sleep | mounted GPUI controlled-clock test | `red-ui-elapsed.log`, `final-rebased-issue70.log` | PASS |
| E70-3 | Runtime terminal duration stays authoritative and polling stops | Finish the running Bash with exact `duration_ms`, then advance the executor clock again | Live copy is replaced immediately by the precise terminal duration; the elapsed refresh task is absent and later clock advances do not change it | production stream controlled-clock test | `final-rebased-issue70.log` | PASS |
| E70-4 | Aggregate and per-child ownership remain exact | Run two Bash children at different start instants, keep the group collapsed, then expand it | Aggregate summary contains no child or total time; expanded children show independent elapsed values and retain neutral Terminal icons | mounted GPUI group test | `red-ui-elapsed.log`, `final-rebased-issue70.log` | PASS |
| E70-5 | Non-Bash tools never gain time | Send approval/running for read/search/write/edit/MCP/Skill projections | Their truthful running copy has no `毫秒`/`秒`/`分钟`, and no elapsed refresh task starts | production card/stream test | `final-rebased-issue70.log` | PASS |
| E70-6 | Restart does not fabricate a start instant | Hydrate a persisted running Bash without a fresh runtime Running event | The row remains truthful but shows no invented elapsed value and owns no refresh task | hydration test | `final-rebased-issue70.log`, `final-rebased-hydration.log` | PASS |
| E70-7 | Category-only leading visuals do not regress | Exercise running and terminal Bash rows and a Bash group | Terminal category icon and neutral `text_secondary` remain unchanged across lifecycle states | mounted GPUI + unit regression | `final-rebased-issue70.log`, `final-rebased-tool-card.log` | PASS |

## Implementation plan

1. Add a UI-owned tool activity group that keeps ordered `Entity<ToolCard>`
   children and UI-only aggregate expansion. Replace each visible `Tool` entry
   with one natural-height group entry while retaining the call-id → card map.
2. Group adjacent live and hydrated tools without crossing visible timeline
   boundaries. Update observers/invalidation so child lifecycle and expansion
   changes remeasure the owning list item only.
3. Refactor `ToolCard` into a compact activity row plus optional detail rows.
   Derive localized safe summaries and aggregate categories from existing typed
   projections. Reuse Vega icons/theme/layout/typography tokens.
4. Add red tests for the matrix before production behavior, then focused tests,
   strict Clippy/format, the locked workspace suite and package verification.
5. Install the exact packaged candidate, capture persistent Light/Dark native
   evidence outside the disposable worktree, inspect it, hash it and record the
   final tree/build identity here.

## Scope and ownership

- Expected code: `crates/vega_ui/src/tool_card.rs`, a small UI-owned grouping
  module if needed, and `conversation_stream` model/content/render/tests.
- Expected docs: this spec/delivery record plus the component-level UI spec.
- No database, migration, provider, runtime, tool-execution or dependency
  change.

## Results

Implementation:

- `ToolCard` now renders a compact, one-line activity row and constructs safe
  detail only while expanded. Shell details retain the complete command,
  bounded output and semantic terminal footer; empty output adds no blank row.
- The 2026-09-21 category-icon follow-up makes every activity leading icon and
  its color category-owned: lifecycle state no longer substitutes Check or
  Warning and no longer applies success/danger to the leading glyph. The
  status summary, exit code and terminal footer remain unchanged and truthful.
- A UI-owned `ToolActivityGroup` retains the exact ordered card entities,
  defaults collapsed, derives truthful aggregate copy/state and scopes group
  and child disclosure independently.
- Live events and each typed history page group only adjacent tool entries.
  Assistant/artifact/plan/summary/Skill provenance boundaries remain visible;
  permission removal restores otherwise-adjacent grouping and preserves the
  disclosure state of either side.
- Tool-card and group observers remeasure their owning natural-height list
  item. The call-id map, typed strict projections, hydration reset and
  permission identity remain unchanged.
- Artifact placement required a compatibility adaptation in
  `conversation_stream/composer.rs`: exact-tool adjacency now checks whether a
  single activity or group contains the target card. This preserves the
  existing exact-tool contract for a non-first child and does not change
  artifact behavior or data.
- The live-elapsed follow-up projects the content-free runtime Running boundary
  into the in-memory conversation event stream after durable state is visible.
  A concrete Bash card records `BackgroundExecutor::now()`, owns its refresh
  task and computes whole seconds from that clock. Running immediately notifies
  the mounted row; terminal results cancel the task and restore the exact
  persisted `duration_ms`. Aggregate summaries and non-Bash cards own no time.

Test-first evidence:

- `red-test-behavior.log`: the first production regression run failed 0/2 as
  intended. The old UI used two rows for one completed shell call and five
  visible entries where the compact design requires three.
- `issue70-first-expanded-suite-failure.log`: the first expanded matrix run
  passed 7/8. T70-7 used a noncanonical invalid-result body; the strict
  projector correctly failed it closed as corrupt. The fixture was changed to
  the deterministic invalid projection while a separate forged case retains
  the raw-secret leak check.
- `permissions-first-compat-failure.log`: the first compatibility run passed
  11/13; two assertions still expected the retired card copy. They now assert
  the compact safe copy.
- `clippy-first-failure.log`: strict Clippy found 13 redundant closures in test
  readers. They were replaced with direct function references before the final
  clean run.
- Follow-up `red-tool-card-leading-visual.log` and
  `red-issue70-leading-visual.log`: both new category-visual regressions failed
  on the previous success Check exactly as intended. Follow-up
  `clippy-first-failure.log` then recorded one test-only `matches!` lint before
  the final strict Clippy run passed.
- Live-elapsed `red-running-projection.log`: the new exact conversion test
  failed 0/1 because `RuntimeEvent::ToolCallRunning` was still discarded at the
  conversation boundary.
- Live-elapsed `red-ui-elapsed.log`: the first production UI run passed 1/3 and
  failed 2/3 because Bash rows had no `0 秒` value and expanded Bash children
  had no independent elapsed values. The already-passing non-Bash/hydration
  case proved the negative behavior before production code changed.

Fresh green gates:

| Command | Exit | Result | Evidence |
|---|---:|---|---|
| `scripts/cargo-lock.sh test -p vega_ui issue70_ -- --nocapture` | 0 | 8 passed | `green-issue70.log` |
| `scripts/cargo-lock.sh test -p vega_ui tool_card` | 0 | 18 passed | `green-tool-card.log` |
| `scripts/cargo-lock.sh test -p vega_ui timeline` | 0 | 6 passed | `green-timeline.log` |
| `scripts/cargo-lock.sh test -p vega_ui hydration` | 0 | 8 passed | `green-hydration.log` |
| `scripts/cargo-lock.sh test -p vega_ui permissions_cards` | 0 | 13 passed | `green-permissions.log` |
| `scripts/cargo-lock.sh test -p vega artifact_controller_preview_open_latest_stale_and_max_fences -- --nocapture` | 0 | 1 passed | `green-artifact-adjacency.log` |
| `scripts/cargo-lock.sh test -p vega_ui` | 0 | 404 passed; doc tests 0 | `green-vega-ui-full.log` |
| `scripts/cargo-lock.sh clippy -p vega_ui --all-targets -- -D warnings` | 0 | clean | `green-clippy-vega-ui.log` |
| `cargo fmt --all -- --check` | 0 | clean | `green-static-gates.log` |
| `git diff --check` | 0 | clean | `green-static-gates.log` |

Bash live-elapsed follow-up, rerun after rebasing onto exact
`origin/master` `32de6a3` (direct Cargo with isolated
`CARGO_TARGET_DIR=/Users/puzige/Workspace/vega-targets/issue70-command-elapsed`
because another worktree owned the repository-wide cargo lock):

| Command | Exit | Result | Evidence |
|---|---:|---|---|
| `cargo test -p vega_ui issue70_ -- --nocapture` | 0 | 11 passed | `final-rebased-issue70.log` |
| `cargo test -p vega_ui tool_card -- --nocapture` | 0 | 19 passed | `final-rebased-tool-card.log` |
| `cargo test -p vega_ui timeline -- --nocapture` | 0 | 6 passed | `final-rebased-timeline.log` |
| `cargo test -p vega_ui hydration -- --nocapture` | 0 | 8 passed | `final-rebased-hydration.log` |
| `cargo test -p vega_conversation issue70_runtime_tool_running_reaches_the_safe_ui_event_boundary -- --nocapture` | 0 | 1 passed | `final-rebased-running-projection.log` |
| `cargo test -p vega_conversation agent::tests::stream_persistence::persists_messages_tool_lifecycle_and_zero_cost_usage -- --exact --nocapture` | 0 | 1 passed | `final-rebased-running-persistence.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | clean | `final-rebased-clippy-workspace.log` |
| `cargo fmt --all -- --check` | 0 | clean | `final-rebased-static-gates.log` |
| `git diff --check` | 0 | clean | `final-rebased-static-gates.log` |

Category-only leading-icon follow-up:

| Command | Exit | Result | Evidence |
|---|---:|---|---|
| `scripts/cargo-lock.sh test -p vega_ui tool_activity_leading_visual_is_category_owned_and_neutral_for_every_state -- --nocapture` | 0 | 1 passed | `green-tool-card-leading-visual.log` |
| `scripts/cargo-lock.sh test -p vega_ui issue70_ -- --nocapture` | 0 | 8 passed | `green-issue70-category-icons.log` |
| `scripts/cargo-lock.sh test -p vega_ui tool_card -- --nocapture` | 0 | 19 passed | `green-tool-card.log` |
| `scripts/cargo-lock.sh clippy -p vega_ui --all-targets -- -D warnings` | 0 | clean | `green-clippy.log` |
| `cargo fmt --all -- --check` | 0 | clean | `green-static-gates.log` |
| `git diff --check` | 0 | clean | `green-static-gates.log` |

Final integration gates were run again after rebasing onto `origin/master`, with
an independent target directory so another worktree could not supply stale test
artifacts:

| Command | Exit | Result | Evidence |
|---|---:|---|---|
| `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings` | 0 | clean | `final-clippy-workspace.log` |
| `scripts/cargo-lock.sh test -p vega_ui issue70_ -- --nocapture` | 0 | 8 passed; 402 filtered | `final-focused-issue70.log` |
| `RUST_TEST_THREADS=1 scripts/cargo-lock.sh test --workspace` | 0 | all workspace and doc tests passed; `vega_ui` 410 passed | `final-test-workspace-serial.log` |
| `scripts/cargo-lock.sh xtask package --version 0.1.1` | 0 | signed `.app` and arm64 zip created | `final-package-0.1.1.log` |

The default parallel workspace run first exposed two existing process-control
timing flakes. Exact serialized reruns passed in
`final-exact-mutation-cancel.log` and `final-exact-lease-cleanup.log`; the final
serialized workspace run then passed in full. An earlier shared-target run that
reported only 401 `vega_ui` tests was rejected as contaminated evidence before
the independent target directory was used.

The installed `/Applications/Vega.app` binary exactly matches the packaged
candidate (SHA-256
`07c538fc962213cfbecdb54b1f39e3c647d8ae344f73dcd102e8ef391c8222a7`),
passes strict deep signature verification, and preserves version `0.1.1`. The
arm64 zip SHA-256 is
`c73af87f9342378793a2bda158e7576d291cc6db2b414b496eab9a61b28743c9`.
Persistent native evidence records the pre-candidate baseline, Light expanded
group, Dark collapsed group, and Dark shell detail with exact hashes in
`manifest.md`. The owner accepted the current ordinary-width rendering on
2026-09-21 and asked to merge it as-is, so no additional narrow-window capture
was required for this delivery.

There is no code deviation from the frozen specification. The follow-up adds
only the safe in-memory conversation Running projection and UI clock state; no
schema, dependency, `vega_runtime`, provider, tool-execution or persistence
behavior changed.

## Residuals

None for Issue #70. The owner explicitly accepted the current native rendering
without a separate narrow-window screenshot.

## PR #106 integration (2026-09-21)

The command elapsed follow-up is reviewed against the current multi-conversation master. Each ToolCard owns its clock/task; duplicate Running events retain the original start, terminal/corrupt transitions stop refreshing, and hydrated history never invents a start time. Running projection uses the exact call ID inside the owning ConversationStream.

The integration gate follows Issue #107: `python3 scripts/verify.py` selects changed packages and transitive workspace consumers, with the existing tool activity, hydration, stream persistence and multi-conversation regressions included. Final commands/counts and frozen content identity are retained under evidence label `remaining-merges-2026-09-21` and written to PR #106. Prior package/install/native observations above remain historical; this integration request does not reinstall the app or claim a new real-provider UI session.

### Integration regression synchronization amendment

The first scoped integration run failed R69 A3: the durable thread existed,
but its title was still empty. The fixture stops at draft materialization;
the accepted user message and fallback title are committed by the following
agent transaction. Materialization alone is therefore not a completion signal
for this assertion. These production stages are unchanged by PR #106.

Before changing the regression, freeze this correction: A3 waits, with the
existing bounded test pump, for its own durable user message, independently
of the expected title. It then asserts exactly one user message with the
submitted content and retains every existing thread/title/identity assertion.
Do not alter the shared submit helper, production code, timeouts, or retries.

| Case | Operation | Expected result | Evidence |
|---|---|---|---|
| R69 A3 | Submit through the mounted production composer; observe the durable user transaction | Exactly one submitted user message; same draft ID; fallback title and all existing metadata assertions hold | Focused regression plus scoped integration gate |

Preserve the first failed gate under `issue106-first-failure`. Run the focused
regression after correction, then the unified scoped gate against the final
source tree. This fixes the completion condition rather than accepting a
retry of the unchanged failing test.
