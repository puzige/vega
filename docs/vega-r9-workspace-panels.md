# R9 — Workspace panels and native UI polish

Authorized 2026-09-06: user asks to implement the reviewed layout and closely reproduce its interaction experience. ZCode remains the primary reference. User-supplied Codex screenshots establish three columns plus an optional bottom pane; Antigravity features are explicitly excluded.

## Implementation contract

Preserve R8 controllers, request routing, permissions, drafts, streaming, themes and keyboard contracts. Replace full-conversation Diff takeover with a right workspace pane while keeping the live conversation and composer visible. Reuse production Diff, artifact/file preview and commit controllers; never weaken provenance or stale-route checks. Workspace tabs support selection, close, reopen via a real add menu, overflow, panel collapse/expand and maximization. Present only implemented capabilities. Distinguish closing a tab from hiding a pane. Restore focus appropriately; local Escape must win over global fallback. Switching task must not display old task data.

Provide an optional bottom dock spanning the conversation and right workspace, excluding the task sidebar. Support moving supported workspace content between right and bottom, preserving draft and tab state. Horizontal and vertical splitters clamp to usable minimum sizes; narrow windows collapse panes without losing user state. Existing previews can supply real bottom content. Inspect terminal availability: a command-output viewer must not be labeled an interactive terminal. A genuine PTY terminal, if absent, is a separately reported capability gap and must not be faked. Do not introduce an empty fake terminal solely to match a screenshot.

Polish spacing, neutral surfaces, separators, original line icons, compact toolbar, tab selected/hover states, close/add controls, ellipsis, conversation width and composer controls for small/large windows. Theme colors and typography remain tokenized. Keep all existing user-accessible functionality reachable. Add functioning layout/appearance settings for supported preferences where feasible, using existing configuration infrastructure without SQL or schema changes. Do not add unsupported settings switches. No Antigravity-specific scope/remote-control/customization features.

## Ownership and verification

Implementation agent owns crates/vega, crates/vega_ui, crates/vega_theme and this delivery specification. No new dependencies, runtime/provider semantics, credentials, migrations, installed-app replacement, remote publishing or performance runs. Main owns review, integration and native CUA acceptance. Implementation must read AGENTS.md and docs/vega-exec-guide.md first. Reasonable local geometry and internal implementation choices are delegated within this contract.

Run cargo fmt --all -- --check; focused production UI/controller regression for pane coexistence, close/hide/reopen, task route switch and focus; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --locked --no-fail-fast; cargo build --workspace --locked. Preserve first failures. Existing Diff retry GitFailed is a known intermittent residual, not permission to weaken assertions. Performance explicitly deferred.

Deliver docs/vega-r9-workspace-panels-delivery.md with changed files, exact commands/log paths/results, source commit, button inventory (implemented / remaining capability), supported behavior and remaining limitations. Provide build artifact location for main to package and visually verify at compact and large sizes. Do not claim pixel parity or native acceptance before main's real screenshots.

## Confirmed real-user send preparation correction (2026-09-06)

The main agent's single real-user Ask submission reached the production worker and waited inside macOS `SecKeychainFindGenericPassword` before any durable message or provider request. The composer showed only a disabled Send button. The correction must display a truthful pending/preparing indication and a conditional system-authorization hint while preserving the draft and existing durable-echo handshake. It must not claim that a system dialog exists, inspect credentials, automate SecurityAgent, or fabricate progress/completion.

### Issue #174 revision — remove the pending row (2026-09-24)

The visible `正在准备请求…` row added by the correction above is superseded by [Issue #174](vega-issue-174-preparing-indicator.md). Keep the internal submit-pending guard, single-flight behavior, provider preflight and durable-echo handshake, but do not render a preparation label or reserve layout space for one. Keep genuine controller errors, warnings and run status visible under their existing rules.

Existing cancellation must be checked immediately after provider/credential construction returns, before either the ordinary-user or approved-plan durable/runtime entry. Synchronous Keychain waits cannot be forcibly interrupted by this UI; no misleading Cancel button is added. One bounded fault-injection regression at the existing test-only provider construction boundary must prove cancellation during deferred construction causes zero provider requests and zero new durable messages. Existing real-provider authorization and native acceptance remain owned by main/user; implementation uses only owned fixture config/DB and MockProvider.
