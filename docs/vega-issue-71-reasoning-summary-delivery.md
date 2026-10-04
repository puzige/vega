# Issue 71 · Reasoning summary delivery

Status: implementation is committed-ready in a fresh worktree for review. The issue body is empty, so the recovered scope is documented in [vega-issue-71-reasoning-summary.md](vega-issue-71-reasoning-summary.md), using the #71-only history from PR #145. The port starts at current `origin/master` `c4aefbdf74c8475d6f5e7a811b556b3fd175518d`; no #141 stack changes were included.

## Scope and compatibility

- Add an explicit Chat Completions / Responses provider setting, default old and new configurations to Chat Completions, and route Responses through the saved provider choice.
- Stream Responses reasoning summaries as a distinct event, keep them out of answer history and usage estimates, and display them with a bounded live title and preview in the thinking activity.
- Preserve the current #151 activity behavior: the newest live activity stays expanded and older activity collapses. This feature changes only the live title and displayed summary/thinking content; it does not restore default collapse behavior.
- Responses uses stateless requests and bounded parsing; failed, incomplete, cancelled, or unterminated streams cannot release partial tool calls. Incompatible reasoning profiles fail before the request. Responses remains opt-in.
- No dependency, migration, provider credential, user setting, project status, or issue status was changed. No push or PR was created.

The new provider-settings test and controller/runtime tests use in-memory mocked transports. They do not claim live provider acceptance.

## Validation

Focused feature tests passed:

```text
cargo test -p vega -p vega_conversation -p vega_runtime -p vega_store -p vega_ui --lib i71_

vega:             0 matched tests
vega_conversation: 2 passed; 0 failed
vega_runtime:     8 passed; 0 failed
vega_store:       1 passed; 0 failed
vega_ui:          6 passed; 0 failed
Total:           17 passed; 0 failed
```

Formatting and affected-crate clippy passed:

```text
cargo fmt --all -- --check
exit 0

cargo clippy -p vega -p vega_conversation -p vega_runtime -p vega_store -p vega_ui --all-targets -- -D warnings
exit 0
```

Cargo emitted its existing future-incompatibility advisory for dependency `block v0.1.6`; clippy completed with `-D warnings`. `git diff --check` also passed.

## Remaining acceptance

- A9, native application and live-provider acceptance with screenshot evidence, remains pending. It is not represented by the mocked tests.
- No application was installed or launched for live acceptance. Review, any follow-up integration, and any PR creation remain with the parent agent.
- The original #151 expansion behavior was explicitly kept and has a focused regression test. No #141-only change was ported.
