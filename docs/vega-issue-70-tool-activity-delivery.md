# Issue #70 — delivery and acceptance record

`PASS` requires fresh evidence; green unit tests do not substitute for native
pixels, and screenshots do not substitute for behavior assertions. Evidence
log names below are relative to the persistent Issue #70 evidence directory.

## Test-first matrix

| ID | Requirement / risk | Precondition | Action | Expected observable result | Level | Evidence | Status |
|---|---|---|---|---|---|---|---|
| T70-1 | One completed command is too visually heavy | One successful bash projection with duration and bounded output | Mount the production timeline | One quiet single-line row shows the command/duration; no permanent card surface; chevron is present | mounted GPUI + painted quads | `green-issue70.log`, T70-1 quad and selector assertions | PASS |
| T70-2 | Adjacent calls need one resting summary | Assistant segment followed by bash/read/grep calls with no visible entry between | Apply proposals/results | One collapsed mixed-category group occupies one list item and one visible row in exact call order | production stream test | `green-issue70.log`, T70-2 exact copy/order/entity assertions | PASS |
| T70-3 | A real boundary must not be crossed | Tool calls separated by nonempty assistant text; include permission insertion/removal around an adjacent call | Apply the event sequence | Text splits groups; the transient permission card does not permanently split otherwise adjacent calls | production stream test | `green-issue70.log`, two T70-3 boundary/merge tests | PASS |
| T70-4 | Progressive disclosure must be scoped | Collapsed three-call group; final command has output | Activate group, then activate final child, then close it | Group reveals three compact children; only selected detail opens; full command/output and truthful status are visible; heights return on close | mounted GPUI interaction | `green-issue70.log`, T70-4 mounted clicks/heights/status tokens | PASS |
| T70-5 | Lifecycle updates cannot move or falsely complete the group | Pending/approved/running/success and failed calls in one group | Apply each durable event | Same call entities/order remain; aggregate and child wording/colors reflect active/failure; exit code visible | production stream + card unit | `green-issue70.log`, T70-5 identity/copy/footer-token assertions | PASS |
| T70-6 | Reopen must match live layout | Typed history containing text → three tools → text, plus a separate one-tool segment | Hydrate, expand, switch/reopen | Same boundaries/order/summaries as live; initially collapsed again; no execution/replay | hydration/mounted GPUI | `green-issue70.log`, T70-6 live/hydrated parity and reopen assertions | PASS |
| T70-7 | Redaction and fail-closed behavior must survive redesign | Read-only raw args, strict write/edit results, invalid and corrupt projections | Render collapsed and expanded states | No raw read args, body, fingerprint, call ID, absolute data root or checkpoint ref; invalid/corrupt state remains visible | unit + mounted GPUI | `green-issue70.log`, T70-7 safe/secret allow-deny assertions | PASS |
| T70-8 | Narrow/light/dark native appearance | Packaged candidate with an existing real audited timeline | Open at ordinary and narrow widths; inspect collapsed mixed group and expanded shell detail in Light and Dark | No clipping/wrap-induced width growth; hierarchy matches spec; text/status contrast remains readable | installed native UI | PNGs + SHA-256 manifest | NOT RUN |
| T70-9 | Related behavior and architecture do not regress | Final candidate | Run focused and workspace gates | Tool/permission/timeline/hydration suites pass; no UI SQLite, hard-coded color, dependency or migration change | repository gates | focused/package/static logs below; workspace gate remains with integration owner | PARTIAL |

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

There is no implementation deviation from the frozen specification. No schema,
dependency, runtime, provider, tool-execution or persistence code changed.

## Residuals

- T70-8 packaged native Light/Dark and narrow-width acceptance remains with the
  integration owner.
- The final full-workspace/package gates, candidate installation and evidence
  manifest remain with the integration owner. The implementation agent ran the
  full `vega_ui` package suite and the focused `vega` artifact integration
  case.
