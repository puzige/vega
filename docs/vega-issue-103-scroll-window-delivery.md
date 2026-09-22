# Issue #103 implementation delivery

## Freeze

- Contract: [bounded disclosures](vega-issue-103-scroll-window.md), frozen 2026-09-22.
- Branch: `feat/issue-103-scroll-window`; baseline: `d2422179`.
- Verified date: 2026-09-22 UTC / Asia-Shanghai; Darwin arm64.
- Rust: `1.98.0 (88d9e12ae 2026-08-18)`; Cargo: `1.98.0 (797e8a9bc 2026-08-05)`.
- Source patch SHA-256 (`git diff --binary -- crates/vega_ui crates/vega_theme`): `066cbb264c018f443ee9a77b94866663174a8cec18c68f08ee70e6fbdd2aecee`.
- Persistent local evidence set: `issue-103/implementation-manifest.json`.
  The local manifest records absolute raw-log paths; public documentation uses
  evidence-set-relative names only.

## Implementation

Tool detail and thinking bodies now retain a per-entity `ScrollHandle`, use a
240px maximum, and keep their disclosure headers outside the viewport. Groups
use their own 320px viewport. Tool rows cannot flex-shrink to fit the viewport;
short content keeps its natural height. Tool/group scroll element IDs contain
the owning entity identity, while thinking uses its normal entity scope.

The pinned GPUI version applies its built-in scroll delta before custom bubble
listeners but does not consume movement. A small shared listener consumes only
actual inner movement. At either boundary and for short content, the existing
parent propagation continues. GPUI clamps retained offsets during layout when
content shrinks. No persistence, runtime, safe-projection, dependency or content
limit changes were made. Spec deviations: **none**.

## Results

These are production GPUI regressions through `ConversationStream`, real event
application, disclosure clicks, wheel dispatch and observable layout/scroll
state. They are not native screenshot acceptance or real provider evidence.

| Matrix | Evidence | Result |
|---|---|---|
| A1 | Tool detail ≤240px, non-shrinking content extent, footer reachability, wheel isolation, both boundary directions and outside-body scrolling | GPUI PASS |
| A2 | Thinking ≤240px; scroll then append; stable reading offset and header | GPUI PASS |
| A3 | Group ≤320px excluding header; child detail ≤240px; independent group/child offsets; final child reachable | GPUI PASS |
| A4 | Short/empty shell output with failure exit status; natural height and parent propagation; existing safe/truncation tests | GPUI PASS |
| A5 | Tool, thinking and group collapse/reopen; unaffected sibling offset; reasoning shrink clamp; real Running→Finished transition retains offset | GPUI PASS |
| A6 | Existing hydration and chronology regressions | GPUI regression PASS; native restart PENDING |
| A7 | Light/Dark, required window sizes and breakpoint widths | Native PENDING |

### Commands and raw logs

All names below are relative to the persistent `issue-103` evidence set.
Raw logs retain the first failures as well as successful runs. Successful Cargo
footers have zero failures/ignored tests. Formatting returned exit 0 with an
empty output file. Cloud workspace CI remains the merge gate.

| Log | Exact command | Bounded output / SHA-256 |
|---|---|---|
| `01-before.log` | `cargo test -p vega_ui issue103_ -- --nocapture` | test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 427 filtered out; finished in 0.03s; error: test failed, to rerun pass `-p vega_ui --lib`<br>`f53e90e08e1ad60f96fd8bda24a97dbdaa0118c4f57d15bce574f7568ec8a1ad` |
| `02-group-before.log` | `cargo test -p vega_ui issue103_tool_group -- --nocapture` | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 429 filtered out; finished in 0.03s; error: test failed, to rerun pass `-p vega_ui --lib`<br>`c5de209480ec592bcb0d6fd1f03c6e7a3d78cdcfacbca7bcf42bde827d8749ad` |
| `03-after-geometry.log` | `cargo test -p vega_ui issue103_ -- --nocapture` | test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 427 filtered out; finished in 0.02s<br>`2b388faea02648c9c9012e76bfa69f43f98ca8c732141c89eaa627b3b4594efe` |
| `04-scroll.log` | `cargo test -p vega_ui issue103_ -- --nocapture` | error: could not compile `vega_ui` (lib test) due to 4 previous errors<br>`4e346fe5a62ff96e5f0e272c69daa495d83d86d7d10caded10cf06c9a7ff787f` |
| `05-scroll.log` | `cargo test -p vega_ui issue103_ -- --nocapture` | test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 427 filtered out; finished in 0.07s; error: test failed, to rerun pass `-p vega_ui --lib`<br>`58506ce4e6b1cc3c6e7255eaf865f11b86d3bf9edd8c7cb7802073eb91ed344a` |
| `06-contained-scroll.log` | `cargo test -p vega_ui issue103_ -- --nocapture` | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 427 filtered out; finished in 0.16s<br>`754e866e5013b477e0b56ddd42a6ce0082f5162213bc04057f461254dccb1c6f` |
| `07-boundary-scroll.log` | `cargo test -p vega_ui issue103_ -- --nocapture` | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 427 filtered out; finished in 0.10s; error: test failed, to rerun pass `-p vega_ui --lib`<br>`baaca078df0503ad2819793d660734d8868529961d95a86b08a4cea8e3ae9db5` |
| `08-boundary-scroll.log` | `cargo test -p vega_ui issue103_ -- --nocapture` | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 427 filtered out; finished in 0.10s<br>`cbf3a67e0676c46359f5eace0060304411edae6ba0a48e287d70ed3b51a4d5af` |
| `09-tool-regressions.log` | `cargo test -p vega_ui tool -- --nocapture` | test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 389 filtered out; finished in 0.12s<br>`8b6f5ca057186f31429c64a18b80e187618401a32a82b587cc216021f6edeb09` |
| `10-thinking-regressions.log` | `cargo test -p vega_ui thinking -- --nocapture` | test result: ok. 70 passed; 0 failed; 0 ignored; 0 measured; 362 filtered out; finished in 0.05s<br>`b1d95f839cadecb7e45936198fc87679d455ead96bcb7237615c4c161750907a` |
| `11-hydration-regressions.log` | `cargo test -p vega_ui hydration -- --nocapture` | test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 424 filtered out; finished in 0.02s<br>`de26a7191986745cf5b264d313ddbc0fb34fbb3c1879ece5c6466f68b2c241a3` |
| `12-theme.log` | `cargo test -p vega_theme issue103 -- --nocapture` | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 26 filtered out; finished in 0.00s<br>`dbef96cf42ef7f0d3267eaa41068f33b76077b032478f03b3bc5ed3ceaac4c6c` |
| `13-fmt.log` | `cargo fmt --all -- --check` | exit 0; no output<br>`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `14-status-update.log` | `cargo test -p vega_ui issue103_tool_status -- --nocapture` | 1 passed; exit 0<br>`7a18c5d2f1ea8da319ad2280c2cf6cd2c22aaf832ab92c5abbf9fb9ffcec66da` |
| `15-final-issue103.log` | `cargo test -p vega_ui issue103_ -- --nocapture` | 6 passed; exit 0<br>`7e03f4447d1eba980044bc2ed1188bbb4d76c35d3f89aa77944721a9a6e6e9fb` |
| `16-final-fmt.log` | `cargo fmt --all -- --check` | exit 0; no output<br>`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `clippy.log` | `cargo clippy -p vega_ui -p vega_theme --all-targets -- -D warnings` | exit 0; 2m 22s; confirmed by main agent<br>`833ab04b003f98f83dad43afd11f33599ab9360a93add768901845fc02419483` |
| `package-final.log` | `cargo xtask package` | exit 0; confirmed by main agent<br>`71cd9b5c922bb39fd1cebbabe08d4f593d7e2456af642ac08decc0b327afb482` |

### Initial failures and corrections

- `01-before.log`: original production layout was 1701px for thinking and
  2325px for tool detail; both 240px assertions failed before the fix.
- `02-group-before.log`: 30 adjacent calls occupied 744px, exceeding the
  344px limit including the 24px header, before the fix.
- `04-scroll.log`: the new test initially compared GPUI `ListOffset` directly,
  which has no `PartialEq`; corrected the observation to its index/offset tuple.
- `05-scroll.log`: plain GPUI overflow moved inner and outer viewports together.
  This real failure led to the movement-consumption listener, not relaxed assertions.
- `07-boundary-scroll.log`: the short-error test initially supplied the wrong
  audited shell result shape (`Failed` with exit metadata), which correctly
  rendered corrupt. Corrected the fixture to completed execution (`Success`)
  with exit code 1, matching production shell semantics and error presentation.

## Residuals

- Native acceptance remains **NOT RUN**: Issue #117 currently owns installation/native acceptance of the same app; exclusive access is unavailable. This blocks A1–A3
  wheel/trackpad interaction, A6 restart/hydrated geometry and A7 theme/window
  matrix until main-agent exclusive access to the actual app is available.
- Main-agent package evidence: `package-final.log`; binary SHA-256
  `03b18e51b712e86f0e6d97b1d4e9ff12b3b9d90a09bd7b910b74ae5ca85e7e68`.
  Main reported `codesign --verify --deep --strict dist/Vega.app` exit 0.
  Packaging/signing does not establish native interaction acceptance.
- The movement listener relies on the pinned GPUI built-in/custom listener
  ordering. Dispatched wheel regressions cover inner movement, exact boundaries,
  short content and nested groups; rerun them when upgrading GPUI.
- Existing dependency notice: `block v0.1.6` has future-incompatibility warnings;
  no dependency was changed for this task.
- Scoped local clippy passed. Workspace cloud checks and native acceptance remain owned by the
  main agent. No merge, installation or external issue-state change was made by
  the implementation agent. Rollback is a revert; stored conversation data is unchanged.
