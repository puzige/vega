# Utility chip states delivery

Implementation branch: `feat/branch-popup-upward` (base `2163f63`). The
utility-bar folder and branch triggers now share the requested chip contract:

- 28px actual chip height, 8px horizontal padding, and a 14px effective full
  pill radius; the icon and 13px label remain unchanged.
- 8px gap between the two chip bounds. The utility bar remains 37px high with
  its 14.5px leading inset.
- Rest is transparent. Pointer hover and an open popup use one shared theme
  token (`bg_utility_chip_overlay`); pointer exit clears a closed chip and an
  open chip keeps its fill.
- The branch virtualized list is padded by 4px and each row fills that content
  width (`312px` in the 320px fixture). The selected marker remains trailing,
  10px from the row's right edge.
- The project popup keeps its pre-change 350px geometry and capture/outside
  dismissal. Its wrapper retains the old 32px anchor box while centering the
  new 28px chip, so R64's frozen y-bound remains unchanged.
- The branch selector's `!chip_chrome` path keeps the legacy 32px trigger.
  Existing menu row selected colors and popup max/height/row/font geometry are
  untouched.

## Freeze

- verified_at_utc: `2026-09-16T15:53:20Z`
- verified_at_local: `2026-09-16T23:53:20+0800` (Asia/Shanghai)
- branch: `feat/branch-popup-upward`
- git_head_before_delivery_commit: `2163f637f44a7914a16f6f53dfdcba511e7a1e32`
- git_tree_before_delivery_commit: `f66ce71f24bd445928a116ce8aad2cd003dffa32`
- implementation_diff_sha256 (production/test files, excluding this report): `9025139798b3e320803656f00665641b9830d2bf7b2724dbd96362e18cf8cf29`
- task_contract: `docs/vega-utility-chip-states.md` R1–R5; the spec and design-guidelines files were main-owned and not edited here
- os_arch: `Darwin 24.6.0 arm64`
- rustc: `1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`
- cargo: `1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`
- git: `2.55.0`

## Color math

The light token is `rgba(#DBDBDB, 0.60)` over utility surface `#FAF9F9`:

`0.60 * (219,219,219) + 0.40 * (250,249,249) = (231.4,231.0,231.0)`,
which rounds to `#E7E7E7`.

The dark token is `rgba(#FFFFFF, 0.10)` over `#191919`:

`0.10 * (255,255,255) + 0.90 * (25,25,25) = (48,48,48)`, or `#303030`.

Theme unit tests assert the literal RGB/alpha values and both composite samples.
The painted-quad tests assert the actual shared token in both light and dark
frames, exact chip bounds, and four 14px corners; rest assertions reject any
solid quad at the chip's own bounds, not only the dedicated token.

## Tests and gates

All Cargo commands below were run through the repository lock wrapper:

```text
./scripts/cargo-lock.sh test -p vega_theme r1_composer_utility_chip -- --nocapture
  1 passed, 24 filtered out
./scripts/cargo-lock.sh test -p vega_theme r2_composer_utility_chip -- --nocapture
  2 passed, 23 filtered out
./scripts/cargo-lock.sh test -p vega_ui r19_legacy_branch_trigger_keeps_32px_when_chip_chrome_is_disabled -- --nocapture
  1 passed, 326 filtered out
./scripts/cargo-lock.sh test -p vega_ui --lib
  326 passed, 0 failed, 0 ignored (the final R19 test was added immediately
  afterward and passed as the targeted command above)
./scripts/cargo-lock.sh fmt --all -- --check
  passed
./scripts/cargo-lock.sh test --workspace
  passed; no failures
./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings
  passed; no warnings
```

The parent executor is retaining the current-tree workspace rerun at
`/private/tmp/vega-utility-chips-workspace-review.log`. Earlier targeted,
mutation, and formatting output was observed in the executor terminal but was
not separately redirected to a file; the named commands and their outcomes
above are the durable record.

The first workspace attempt failed before the final rerun at
`conversation_stream::tests::r64_popup_deferred::r64_project_menu_bounds_match_the_baseline`:
the 28px chip moved the bottom-relative project popup from `y=711.5` to
`y=713.5`. The compatibility anchor-box fix above restored the frozen y-bound.
The rerun covered the existing load-sensitive ignored tests without enabling
them: 1212 passed, 9 ignored, 0 failed, plus 5 doctests passed. Its relevant
per-binary counts were: `vega` 130 passed;
`vega_conversation` 304 passed/3 ignored; integration binaries 45 passed/2
ignored; `vega_markdown` 32; `vega_runtime` 96/4 ignored; `vega_store` 95;
`vega_theme` 25; `vega_token` 25; `vega_tools` 98; `vega_ui` 326; and
`xtask` 36. All doctests passed.

The final R19 test-only addition was made after that workspace/clippy run and
passed independently (`1 passed, 326 filtered out`); no production logic
changed after the gate. The production UI tests added for this task include two combined painted-state
tests (light/dark), four independent project/branch painted-state tests
(light/dark), one strict chip geometry test, one R19 legacy test, and one real
uniform-list row geometry test. The independent tests ensure a failure in one
chip path cannot mask the other path.

## Mutation evidence

Each mutation was applied alone with `apply_patch`, run through the exact
named test below, and restored before the next mutation. Every listed negative
run exited 101 with `0 passed; 1 failed` and the named assertion shown.

| Mutation | Independent checks and observed failure |
| --- | --- |
| M1: unconditional rest overlay | `r2_light_project_chip_paint_states_are_independently_verified` → `project-only rest state must be transparent`; `r2_light_branch_chip_paint_states_are_independently_verified` → `branch-only rest state must be transparent`. |
| M2: remove hover overlay | Same project test → `project-only hover state must paint the overlay`; same branch test → `branch-only hover state must paint the overlay`. |
| M3: remove open overlay | Same project test → `project-only open-away state must keep the overlay`; same branch test → `branch-only open-away state must keep the overlay`. The first project-only attempt was inconclusive because the synthetic click left the pointer away; the test was strengthened to move onto the chip before opening, then the rerun failed at the named open assertion. |
| M4-height | `r49_utility_bar_keeps_the_frozen_inset_and_chip_ladder` → `folder chip height: expected literal 28px, got 27px`. A first permissive ±1px assertion let this 1px mutant pass; the assertion was tightened to ±0.1px and the rerun failed as required. |
| M4-padding (project) | Same R49 geometry test → `folder chip horizontal padding: expected literal 8px, got 7px`. |
| M4-padding (branch) | Same R49 geometry test → `branch chip horizontal padding: expected literal 8px, got 7px`. |
| M4-gap | Same R49 geometry test → `chip gap: expected literal 8px, got 7px`. |
| M4-radius (project) | `r2_light_project_chip_paint_states_are_independently_verified` → `project-only hover state must paint the overlay` because exact painted corner radius was no longer 14px. |
| M4-radius (branch) | `r2_light_branch_chip_paint_states_are_independently_verified` → `branch-only hover state must paint the overlay` because exact painted corner radius was no longer 14px. |
| M4-branch width | `r4_branch_uniform_rows_fill_the_popup_content_and_keep_the_marker_trailing` → `branch row left edge gutter: expected 4±1px, got 0px`. |

Before the list constraint was fixed, the real R4 test also caught the initial
implementation (`w_full()` on the item with the old margin) at the same 0px
left gutter. The final `uniform_list` `px_1()` plus row `w_full().mx_0()` is the
constraint that makes the mounted row, rather than only a token, pass.

## Risks and residuals

- `rounded_full()` is intentionally used for both production chips; the
  painted scene verifies its effective radius is 14px at 28px height. The
  public layout radius token and literal theme test document the same contract.
- The project anchor wrapper is a compatibility box only; the interactive
  chip's debug bounds and hit target remain 28px. R64 and R68 production tests
  pass after this adjustment.
- No global menu hover or selected-row colors were changed. The branch row
  width change is scoped to its virtualized list; the shared `menu_list` module
  was not modified.
