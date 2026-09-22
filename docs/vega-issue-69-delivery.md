# Issue #69 implementation evidence

Implementation ready for main-agent review; native candidate acceptance, cloud CI,
integration and card closure remain pending. Contract:
[Environment toggle selection](vega-issue-69-environment-highlight.md).

## Freeze

- verified_at_utc: 2026-09-22T09:53:22Z; local: 2026-09-22 17:53:22 +08:00
- branch: `feat/issue-69-environment-highlight`; base: fresh origin/master supplied by main agent
- source diff SHA-256 (three Rust files, before packaging):
  `e823f5d0cce06c8f9d044d8080f080d34095744408ea8a526f4ec0aa339bd7ca`
- Darwin arm64; rustc 1.98.0; cargo 1.98.0; git 2.55.0
- Persistent local evidence manifest identifier: `issue-69-2026-09-22`.
  Raw output remains outside the worktree; filenames/hashes below identify it.

## Root cause and implementation

`shell_icon_button` used the selected `bg_active` fill for a focused unselected
button. Clicking the header toggle closes Environment but retains focus, so
selection and focus could show the same fill. Closing with the panel X does not
leave focus on the header toggle.

Remove the unselected focus fill and use a keyboard-only 2px `accent` border on
enabled shell controls (design guidelines §8). Selection and keyboard focus can
now coexist and remain visually distinct. Hover, handlers and fixed hitboxes
are unchanged. Environment selection also includes the same project/right-dock/
fullscreen guards as the actual rail/overlay mount, avoiding a selected disabled
slot when another surface replaces Environment.

Changed Rust files: `crates/vega_ui/src/icons.rs`,
`crates/vega/src/window/render.rs`, `crates/vega/src/window/workspace.rs`.
No dependency, persistence, user-config or public API changes. Spec deviations: none.

## Results

| Requirement / class | Exact command | Result / bounded footer | Log |
|---|---|---|---|
| Initial regression compile | `cargo test -p vega issue69_environment_selection_is_not_focus -- --nocapture` | Exit 101: helper used incorrect ScaledPixels conversion and missing theme import; corrected test-only API usage | `red-production-test.log` |
| Pre-fix painted regression / production UI integration | same command | Exit 101: 0 passed, 1 failed; `dark closed rail must not paint selection` | `red-production-test-2.log` |
| Pre-fix regression with RGBA comparison tolerance / production UI integration | same command | Exit 101: 0 passed, 1 failed; same selected-fill assertion | `red-production-test-3.log` |
| A1–A6 / production UI integration | same command | Exit 0: 1 passed, 0 failed, 0 ignored; 0.22s test execution | `green-production-test.log` |
| A7, existing R45 contracts / production UI integration | `cargo test -p vega r45_ -- --nocapture` | Exit 0: 8 passed, 0 failed, 0 ignored; 0.57s test execution | `r45-regressions.log` |
| Formatting | `cargo fmt --all -- --check` | Exit 0, empty output | `fmt.log` |
| Native candidate build | `cargo xtask package` | RUNNING at handoff; main agent owns completion | `package.log` |

The regression mounts the real VegaWindow, creates an owned temporary Git repo
and store, clicks real selectors, checks rail/overlay mounting, retained focus,
and the rendered scene's `painted_quads()` within the shell control bounds.
It covers pointer close/reopen, X close, narrow toggle, keyboard Enter and focus
border, light/dark theme colors, disabled Environment and three-slot geometry.
The reliable pre-fix painted failure occurred after switching to dark and
refreshing the rendered window; the first light-theme assertion alone did not
catch the original code. Native light-theme reproduction is independently
main-owned. Neither test-platform quads nor native screenshots alone prove both
focus ownership and final installed pixels.

SHA-256:

| Log | Hash |
|---|---|
| `red-production-test.log` | `50c6e817e3a9472a7b3d23089804c1989ea7ce04eae6adcf9aad5b22dd0968db` |
| `red-production-test-2.log` | `c87a85badf7638fd1873b1223b5e6538b2ae8a9506365ce5d4c018361397d3cd` |
| `red-production-test-3.log` | `3736dd375a1f38c306e5204c14227b165d38dda1d3f6bec2f79b34bdbfe8ede6` |
| `green-production-test.log` | `ff0d8aa84b5ad355245eb949d7afef4b0bf4902c0a3dcd81999d8ddd2841be63` |
| `r45-regressions.log` | `48b52203c8b883a2937e1f9e2a39692e51c465ff224f2e442c18edeff1920cc0` |
| `fmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

## Residuals

- NOT RUN by implementation agent: native candidate screenshots/installation,
  cloud required checks, PR, merge, task cleanup, Issue closure/Project Done.
- Existing dependency warning: `block v0.1.6` future incompatibility; no new warning.
- Shared component intentionally changes keyboard focus presentation of all
  three shell slots; R45 action/focus/terminal regressions pass. Final native
  appearance remains main-owned before acceptance.
- Persistence/service/provider failure paths: N/A; no changes in these areas.
