# Issue #70 — delivery and acceptance record

This record is created before implementation. `PASS` requires fresh evidence;
green unit tests do not substitute for native pixels, and screenshots do not
substitute for behavior assertions.

## Test-first matrix

| ID | Requirement / risk | Precondition | Action | Expected observable result | Level | Evidence | Status |
|---|---|---|---|---|---|---|---|
| T70-1 | One completed command is too visually heavy | One successful bash projection with duration and bounded output | Mount the production timeline | One quiet single-line row shows the command/duration; no permanent card surface; chevron is present | mounted GPUI + painted quads | test log / quad assertions | NOT RUN |
| T70-2 | Adjacent calls need one resting summary | Assistant segment followed by bash/read/grep calls with no visible entry between | Apply proposals/results | One collapsed mixed-category group occupies one list item and one visible row in exact call order | production stream test | entry/row/debug selectors | NOT RUN |
| T70-3 | A real boundary must not be crossed | Tool calls separated by nonempty assistant text; include permission insertion/removal around an adjacent call | Apply the event sequence | Text splits groups; the transient permission card does not permanently split otherwise adjacent calls | production stream test | ordered mounted entries | NOT RUN |
| T70-4 | Progressive disclosure must be scoped | Collapsed three-call group; final command has output | Activate group, then activate final child, then close it | Group reveals three compact children; only selected detail opens; full command/output and truthful status are visible; heights return on close | mounted GPUI interaction | input assertions + row counts | NOT RUN |
| T70-5 | Lifecycle updates cannot move or falsely complete the group | Pending/approved/running/success and failed calls in one group | Apply each durable event | Same call entities/order remain; aggregate and child wording/colors reflect active/failure; exit code visible | production stream + card unit | text/status/identity assertions | NOT RUN |
| T70-6 | Reopen must match live layout | Typed history containing text → three tools → text, plus a separate one-tool segment | Hydrate, expand, switch/reopen | Same boundaries/order/summaries as live; initially collapsed again; no execution/replay | hydration/mounted GPUI | hydration regression | NOT RUN |
| T70-7 | Redaction and fail-closed behavior must survive redesign | Read-only raw args, strict write/edit results, invalid and corrupt projections | Render collapsed and expanded states | No raw read args, body, fingerprint, call ID, absolute data root or checkpoint ref; invalid/corrupt state remains visible | unit + mounted GPUI | leak assertions | NOT RUN |
| T70-8 | Narrow/light/dark native appearance | Packaged candidate with an existing real audited timeline | Open at ordinary and narrow widths; inspect collapsed mixed group and expanded shell detail in Light and Dark | No clipping/wrap-induced width growth; hierarchy matches spec; text/status contrast remains readable | installed native UI | PNGs + SHA-256 manifest | NOT RUN |
| T70-9 | Related behavior and architecture do not regress | Final candidate | Run focused and workspace gates | Tool/permission/timeline/hydration suites pass; no UI SQLite, hard-coded color, dependency or migration change | repository gates | raw logs | NOT RUN |

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

Pending implementation and review.

## Residuals

Pending implementation and review.

