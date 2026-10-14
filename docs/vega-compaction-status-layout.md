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

## User correction: one unified failure presentation (2026-10-14)

User hand-test found restored failed runs showing a generic run failure card plus context-budget error row. The previous live-only duplicate suppression is insufficient and its choice of retaining a separate context error is superseded.

Context failures now belong in the shared run error presentation, with their specific actionable reason replacing generic provider/credit suggestions. Suppress the redundant compaction failure row only when represented in that owned error card. Keep active/success/cancelled process states compact. Standalone manual compaction failures without a run must still have one visible actionable error. Preserve independent errors from other runs. Restored history must recover typed failure/ownership using durable evidence, not guess by latest visual adjacency; add a minimal additive durable anchor only if current evidence cannot reliably supply ownership. Never misattribute an unowned historical context status. Preserve all raw audit and status semantics.

Regression coverage required: live context failure, same failure after reopen, two adjacent failed runs, independent provider failure, manual/unowned compaction failure, running/success/cancelled states. Assert one error surface with specific cause and no misleading generic copy for a known context failure. No local build/test; cloud CI only.
