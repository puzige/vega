# Issue 71 · Reasoning summary delivery

Status: PR [#236](https://github.com/puzige/vega/pull/236) is open for review from `feat/71-reasoning-summary`. Its implementation commit `3b4f710` starts at current `origin/master` `c4aefbdf74c8475d6f5e7a811b556b3fd175518d`. The issue body is empty, so the recovered scope is documented in [vega-issue-71-reasoning-summary.md](vega-issue-71-reasoning-summary.md), using the #71-only history from PR #145; no #141 stack changes were included.

## Scope and compatibility

- Add an explicit Chat Completions / Responses provider setting, default old and new configurations to Chat Completions, and route Responses through the saved provider choice.
- Stream Responses reasoning summaries as a distinct event, keep them out of answer history and usage estimates, and display them with a bounded live title and preview in the thinking activity.
- Preserve the current #151 activity behavior: the newest live activity stays expanded and older activity collapses. This feature changes only the live title and displayed summary/thinking content; it does not restore default collapse behavior.
- Responses uses stateless requests and bounded parsing; failed, incomplete, cancelled, or unterminated streams cannot release partial tool calls. Incompatible reasoning profiles fail before the request. Responses remains opt-in.
- No dependency, migration, provider credential, or user setting was changed. Issue #71 remains In progress until the PR merges; the Project card will move to In review after merge and stay there for A9 acceptance.

The new provider-settings test and controller/runtime tests use in-memory mocked transports. They do not claim live provider acceptance.

## Validation

Focused feature tests passed:

```text
cargo nextest run -p vega -p vega_conversation -p vega_runtime -p vega_store -p vega_ui --lib i71_
17 tests run: 17 passed, 1397 skipped
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

- A9, native application and live-provider acceptance with screenshot evidence, remains pending. The mocked tests do not represent a live provider request or native application acceptance.
- After merge, build an exact candidate and record its archive/executable hashes; obtain action-time approval before replacing or launching that package. Do not bypass Gatekeeper or other macOS protections.
- The original #151 expansion behavior was explicitly kept and has a focused regression test. No #141-only change was ported.
