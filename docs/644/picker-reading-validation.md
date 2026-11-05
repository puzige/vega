# Skill invocation picker and reading activity validation

## Scope and acceptance

This follow-up implements the user's two UI corrections: a compact, input-width Skill candidate list with separate icon/name/description/human source, and actual Skill reads inside the existing collapsible activity groups. The persistent Using/Stop/Disable bar and its focus targets are removed. Exact editor bindings, invocation protocol, runtime permissions and frozen snapshots are unchanged. Revoked/unverified history remains visible. No new trigger, recording action or reply overflow menu is added.

## Development evidence

- Base: `a818e320aaa6343e6e9b6f5391007c817dbd9dc6`.
- Green development head: `341414c7936539db90df49cefca749a5fb310f1c`.
- [Actions 41964582445](https://github.com/puzige/vega/actions/runs/41964582445): **5 discovered / 5 executed / 5 passed / 0 skipped** Rust regressions. All-target Clippy, fmt, required aggregate check and 60 Python checks (5 + 15 + 40) passed.
- Recoverable remote tag: `evidence/644-picker-reading-development-341414c7`, read back at the exact development head.
- [Original temporary source](https://github.com/puzige/vega/blob/341414c7936539db90df49cefca749a5fb310f1c/crates/vega_ui/src/tool_card/reading_development_tests.rs): 123 lines, SHA-256 `06e5d510e24235a6530bec78daa08a51f788da5030a01305a450d944fb33bb63`.

The regressions cover compact/group lifecycle headlines, description override resistance, read icons, resource/reuse details without a description, terminal failure/rejection/cancellation, activation receipt presentation without permission identity, and corrupt-result precedence. They do not execute the native picker, live-event grouping or history hydration paths.

A nonimplementer independently reviewed the exact development delta and affected picker/binding, live/history grouping, duplicate suppression, lifecycle/detail and removed-focus contracts. No blocking findings were reported. This static review is not native visual or behavioral acceptance.

## Final cleanup and limits

Cleanup deletes only the 123-line temporary test file and its three-line module separator/declaration, and adds this evidence record. No production behavior, dependencies, workflows or Python tests change in cleanup. Exact cleanup review and a fresh complete required CI on the final head remain mandatory before merge. Final `no Rust tests` (0 discovered / 0 executed) is delivery cleanliness, not behavioral regression success.

Local fmt and whitespace checks passed; no local Vega execution, compilation, build, packaging or tests were performed. Native narrow/wide layout, light/dark appearance, keyboard, IME and accessibility acceptance remain for user testing. No user database/configuration, Skill source or installed app was changed. #644 stays open; merge, publication, installation and user acceptance are separate facts.
