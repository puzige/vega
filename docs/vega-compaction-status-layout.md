# Context compaction presentation

2026-10-14 user-requested polish; issue #365. Authority: docs/vega-design-guidelines.md and AGENTS.md UI/UX rules. Supersedes conflicting #117 restored-prefix and compaction-specific spacing presentation only.

## Behavior and layout

- Treat compaction as a compact process status within the existing conversation axis. Reuse process typography (13px token), icon slot (16px), process row minimum height (28px) and existing shared spacing tokens. Avoid additive top/bottom padding and global message-gap stacking that detach it from its owning activity. Wrapped error text must grow naturally; align icon to first line.
- Remove visible "已恢复 ·" prefix: persistence provenance is not the current operation status. Retain restoration metadata internally.
- Keep a single clear active compaction label and real run duration semantics. Avoid introducing a second timer, separator or enclosing card for compaction. Preserve unrelated run header contracts.
- Success should be low emphasis and compact, with no extra explanatory paragraph. Genuine failure/cancellation remains truthful; failures must identify the problem and appropriate next step using concise task language. Do not rewrite model/user content.
- When compaction and run error refer to the same failure in the same owning run, show one actionable failure presentation. Do not hide unrelated tool/provider failures or permission requests; use typed failure and ownership, never global text matching or cross-run suppression.
- Restored states must use consistent spacing and truthful status. No layout shifts on state changes beyond required wrapped text. Respect narrow widths, dark/light semantic colors and keyboard/accessibility of any existing actions.
- No compaction algorithm, persistence or permission changes. Do not add decorative assets or new always-visible explanatory UI.

## Acceptance and plan

Implementation subagent traces materialization + virtual list gap + row renderer, updates shared tokens only if required, and adds focused production UI regressions before final integration. Cover active-only run, success, failure with duplicate corresponding runtime error, unrelated runtime error, restored failure/success, cancelled and multiline/narrow layout. Assert actual render/projection behavior rather than only helper strings. Preserve chronology, geometry/virtual measurement, stable item identity and scroll behavior.

Local: cargo fmt --all -- --check and git diff --check only; no builds, tests, clippy or cargo check. Cloud PR check runs all compilation/tests. Native visual acceptance remains user hand-test after merge. No claim of pixel validation from static checks.

Validation pending.
