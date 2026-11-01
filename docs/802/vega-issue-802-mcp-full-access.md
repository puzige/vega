# #802 — MCP Full access policy

Issue: https://github.com/puzige/vega/issues/802

## Scope and decision

This explicitly supersedes #73's per-call approval requirement for Execute + Full access only. It does not retroactively classify the old policy as a defect.

| Mode | MCP policy |
| --- | --- |
| Ask / Plan | Not advertised; forged calls rejected |
| Execute + ReadOnly | No calls |
| Execute + Confirm / Auto | Separate approval for every call |
| Execute + Full access | Enabled, discovered, unrevoked tools execute without a per-call prompt under valid task authority |

A Full access authorization is one call's `Once / FullAccess` audit, with no remembered rule, note or danger override. It is not a user click and cannot be granted by model arguments, tool descriptions, annotations or output. Server activation, credentials and OAuth consent remain separate. Existing task authority, epoch/reset fences, frozen server/config/tool identity, enable/delete revocation, limits, cancellation, redaction and subprocess protection remain mandatory. There is no Always policy or automatic replay.

#820 changes paths and declaration storage, not this authorization policy: YAML settings are not approval provenance, and invalid or unreconciled MCP declarations cannot bypass revocation/admission fences. See [#820](../820/unified-home-and-safe-migration.md).

## Minimal implementation

- `tools_exec::authorize_call`: keep run-mode/cancel/revocation rejections first; return a one-call FullAccess policy audit only for trusted runtime FullAccess configuration. Confirm/Auto keep the existing hook and timeout.
- `events::validate_runtime_approval_event`: permit User or FullAccess only with Once, valid MCP identity, no remember rule/danger; FullAccess has no note. Before persisting a new FullAccess approval, verify the recorded run fence is valid Execute/FullAccess for this thread/database and current epoch, with reset not pending.
- `pipeline`: accept both audit sources on success/failure/cancellation and recovery without loosening decision, identity, output or error checks. Keep legacy User records readable. No migration is required: existing approval codec already has FullAccess.
- Use mode-neutral provider provenance and concise accurate settings text; remove the conflicting approval-hook comment rather than adding code comments.
- Amend directly conflicting #73 requirements and the independent manual fixture README; historical results remain historical.

## Acceptance and validation

All cases below require both stdio and Streamable HTTP where applicable. In-process regression evidence is not external integration evidence.

| ID | Required behavior |
| --- | --- |
| P01–P02 | Full access receipt call has no prompt, one dispatch, Once/FullAccess audit; two calls have separate IDs/audits and no rule |
| P03 | Confirm dispatches zero while waiting; approval executes once; next call prompts again |
| P04 | Confirm deny/timeout/stop dispatch zero, no retry |
| P05 | Auto still prompts; Ask/Plan/ReadOnly cannot dispatch even forged calls |
| P06 | Next task Confirm prompts; invalid authority or epoch/reset blocks |
| B01–B02 | Disabled/test-only services unavailable; connection/auth/credential failures remain failures; no credential borrowing |
| B03 | Disable/delete/config revision revokes frozen tools before dispatch; new capability cannot enter old snapshot |
| B04 | Stop/disable cancels running wait; uncertain side effects remain unknown and are never replayed |
| B05 | Malformed/oversized input and unknown/stale alias rejected |
| R01 | Receipt/tool error/malformed/oversized/unsupported results retain their actual classification and audit without Full access prompts |
| R02 | Approval/running persistence failure dispatches zero |
| R03 | FullAccess and old User records persist/read/recover without replay or claiming a user click |
| R04 | Mismatched source/decision/identity, Always and remember rule rejected; arguments cannot grant FullAccess |

Development regressions should exercise production authorization, projection and audit validators with isolated external dispatch; Actions runs them. Archive the exact source SHA/results before removing Rust tests and exclusive wiring for final master delivery. Final head requires independent non-writer review and complete required CI with `no Rust tests` (0 discovered/0 executed). Local work is static-only: no Vega execution, builds or tests. Real model/server/UI cases remain user manual validation after merge; keep the issue open/In review until explicit acceptance.

## Non-goals

Auto redesign, permission-card ID presentation, built-in Bash/file policies, transport/OAuth features, sandbox changes, credential policy changes and unrelated historical-document cleanup.
