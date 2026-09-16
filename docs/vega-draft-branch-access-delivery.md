# Draft branch access correction delivery

## Freeze

- verified_at_utc: 2026-09-16T08:24:19Z
- verified_at_local: 2026-09-16T16:24:19+0800
- branch: `feat/branch-popup-upward`
- git_head: `026322c`
- task_contract: `docs/vega-draft-branch-access.md`
- tracked_diff_sha256: `7372cd74a325b022d44751f9197f52778d05d7b3b6d9e477af76d3f21aeb1d21`
- rustc: `1.98.0 (88d9e12ae 2026-08-18)`
- cargo: `1.98.0 (797e8a9bc 2026-08-05)`
- initial focused baseline log: `/private/tmp/vega-draft-branch-baseline-r69-a7.log` (`fc2e08d89582fd98fc675460cc3ea36018f97c5d31aa187b7b93b178e7cdaeeb`)

## Results

| requirement | evidence class | exact command | result | bounded footer/hash |
|---|---|---|---|---|
| Project-bound draft lists and switches through the production route | E2E-REAL | `scripts/cargo-lock.sh test -p vega r69_a7_project_draft_lists_and_switches_without_materializing -- --nocapture` | PASS: 1 passed, 0 failed; real Git list and switch changed only the owned temp repository; zero rows before/after; composer text preserved | exit 0 |
| R69 draft lifecycle and standalone/artifact fences | E2E-REAL | `scripts/cargo-lock.sh test -p vega r69 -- --nocapture` | PASS: 17 passed, 0 failed | exit 0 |
| Branch controller authority and stale/busy/dirty safety | E2E-REAL | `scripts/cargo-lock.sh test -p vega branch_controller -- --nocapture` | PASS: 6 passed, 0 failed | exit 0 |
| Branch selector real projection | E2E-REAL | `scripts/cargo-lock.sh test -p vega branch_selector -- --nocapture` | PASS: 1 passed, 0 failed | exit 0 |
| Upward popup geometry and dismissal | E2E-REAL | `scripts/cargo-lock.sh test -p vega_ui branch_popup_upward -- --nocapture` | PASS: 4 passed, 0 failed | exit 0 |
| Popup outside/inside/trigger dismissal and search-row paint | E2E-REAL | `scripts/cargo-lock.sh test -p vega_ui r68_popup_dismiss -- --nocapture` | PASS: 10 passed, 0 failed | exit 0 |
| Deferred popup geometry | E2E-REAL | `scripts/cargo-lock.sh test -p vega_ui r64_popup_deferred -- --nocapture` | PASS: 4 passed, 0 failed | exit 0 |
| Utility bar geometry and chip ladder | E2E-REAL | `scripts/cargo-lock.sh test -p vega_ui utility_bar -- --nocapture` | PASS: 7 passed, 0 failed | exit 0 |
| Formatting | UNIT/PROPERTY | `cargo fmt --all -- --check` | PASS | exit 0 |
| Workspace lint | UNIT/PROPERTY | `scripts/cargo-lock.sh clippy --all-targets -- -D warnings` | PASS | exit 0 |
| Workspace tests and doctests | E2E-REAL | `scripts/cargo-lock.sh test --workspace` | PASS: all workspace tests/doctests passed; only the repository's load-sensitive tests remained ignored | exit 0 |

## Changes

- Project-bound lazy drafts now initialize the existing branch controller; standalone drafts remain rejected and artifact access remains absent.
- The R69 A7 production test clicks the real composer trigger and branch row, verifies real list/switch behavior, and checks zero durable rows plus draft-text preservation.
- Composer branch chrome is transparent and borderless at rest, uses `rounded_full` and balanced `px_2`, and applies neutral hover only; non-composer chrome is unchanged.
- Branch popup keeps its elevated surface, radius, shadow, upward anchor, deferred paint, search/error/dismissal behavior, and disabled creation action, while dropping its explicit border.
- R69 R6/A7 was corrected narrowly to reflect the project-root versus persisted-thread distinction.

## Residuals

- PASS (parent native light-mode check): candidate and installed app both list the actual selected project's two branches with the current branch checked. Popup opens upward with its explicit border removed; chip has rounded hover chrome. No user branch was switched. Dark-mode native appearance remains NOT RUN.
- NOT RUN: branch switching against any user repository; production test uses only an owned temporary repository.
- No package, install, merge, push, or user-project branch mutation was performed.

## Parent packaging and installation

After executor verification, parent ran `scripts/cargo-lock.sh xtask package` successfully and updated the installed app with a recoverable backup of the previous bundle. Signature verification passed. Candidate and installed executable SHA-256 both equal `7b4e1ba061f21ee92b8ab3d4a9b8cdb24cd3bd995728b90af5fa654ccc00f074`. Production source corresponds to implementation commit `3a27578`; application data was not removed. No merge or push was performed.
