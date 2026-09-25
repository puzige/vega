# A7-02 · Explicit Pi Agent credential import

Status: HISTORICAL CONTRACT · superseded for current product behavior by [Issue #81](vega-issue-81-remove-pi-agent.md).

This document records the original A7-02 contract. The Pi Agent import action
and source-reading flow have been removed. Use Issue #81 for current acceptance;
the historical implementation and native acceptance evidence remain unchanged.

## Why

The user explicitly authorized Vega to obtain its credential from the local
Pi Agent instead of asking them to copy a key or edit a file. Current Vega has
one disabled `cpa` provider with no stored key. Pi Agent's current `cpa`
entry has the same base URL and `glm-5.3-flash` model, but a live Pi request
returned an upstream insufficient-balance error. Importing the credential
must therefore be represented as credential setup, **not** proof of a usable
model or a passing live E2E.

## Contract

1. In Settings → Providers, a selected provider has an explicit, keyboard-
   accessible “从 Pi Agent 导入凭据” action. Merely opening Settings, selecting
   a provider, or starting Vega never reads Pi files. The operation runs on a
   background worker and has visible in-progress, success, and failure state.
2. The only production source is the current user's
   `$HOME/.pi/agent/models.json`. Resolve this path inside the service on
   explicit invocation; reject an absent, symlink, non-regular, overly large,
   or group/world-readable source. Tests may inject a temporary source path.
   Do not modify Pi's configuration or login state.
3. Parse only the selected provider entry. Require a non-empty `apiKey`,
   `api == "openai-completions"`, an exact base-URL match, and at least one
   model ID shared with the selected Vega provider. A stale selected-provider
   snapshot must conflict rather than silently import into a changed target.
   Do not import other Pi providers or silently create Vega providers.
4. Write the secret only through Vega's existing owner-only keystore and
   enable the selected provider only after the credential has been stored.
   Use the existing config edit lock and rollback the key if the config save
   fails. A failure may not claim success or enable a provider without a
   stored credential. Existing form edits and provider network operations
   must remain cancellable/consistent.
5. The credential is never returned to the UI, a command DTO, a log, a test
   failure, a document, or the clipboard. The key input stays blank. UI
   feedback may name the provider and source, never the key; source parsing
   and IO errors are sanitized. No synchronous source or keystore IO occurs
   in a render/event handler.
6. A successful import means only that local setup succeeded. A separate
   explicit model test and actual first-message E2E determine network and
   quota readiness. If either fails due upstream credit, report that as an
   external blocker, not a Vega pass or an invalid credential.

## Boundaries

- No automatic Pi sync, fallback provider, credential discovery across
  arbitrary files, or use of Pi's `auth.json`.
- No changes to A7-01 draft identity, submit preflight, branch/project menus,
  or composer geometry. No raw key in fixtures; use an unmistakably fake key.
- No non-test `unwrap()`/`expect()`. Preserve existing configuration and
  keystore safety rules; do not weaken permissions or symlink checks.
- Implementation subagent must not install, push, merge, or inspect/use the
  actual local Pi secret. Native acceptance is owned by the primary agent.

## Acceptance

- Production service tests cover success (credential present, enabled,
  unrelated providers unchanged), stale snapshot, missing key, wrong API,
  wrong URL, no matching model, missing/oversized/symlink/insecure source,
  and save failure rollback. Verify redacted errors and unchanged state on
  every rejected case.
- Mounted Settings tests cover explicit-action reachability, idle no-read,
  in-progress disabled state, success status and enabled marker, failure
  status, and no key rendered or placed in the form.
- Deliberately bypass one source validation and one non-leak rule to show a
  new test fails, then restore. Run focused tests, full workspace tests,
  format check, and strict workspace clippy via `scripts/cargo-lock.sh`.
- Primary packages one integrated candidate and verifies the native UI.
  The current Pi `cpa` and `opencode-go` credentials both return upstream
  insufficient-balance responses; a real model reply is blocked until the
  external account balance changes or another working credential is provided.
