# #907 Composer selector notification convergence

## Scope and acceptance (before implementation)

Implementation/development validation lane, not independent review. Only `crates/vega_ui/src/conversation_stream/composer_selectors.rs`, its children and this document are writable. Settings remain frozen. Parent coordinates compilation, runtime validation, shared wiring and baseline integration; this lane does not run Cargo/rustc, network or Git mutation.

- Initial production ConversationStream mounting must return from `open_window`; the existing two mounted recovery cases retain every assertion and their 2-second / 512-tick drain and 10-second readiness budgets.
- A Host notification may propagate through GPUI keyed state to its parent. An owner notification with an unchanged selector projection must not revoke publication/intent or emit another Host notification.
- A genuinely changed selector source must be installed and previous publication/activation revoked **before** notifying Host observers. The resulting parent notification must see that installed source and converge, without waiting for a frame. Subsequent real changes, including source ABA, must still refresh and revoke stale actions.
- Keep existing selection, model/provider identity, busy/save ownership, presentation epoch, route/visibility, composition, geometry/mount/native revision, keyboard and accessibility authority checks. No per-frame unconditional notification, notification cap, timeout extension or removal of real-change notifications.
- Add a small observable Host regression using production synchronization and real GPUI observation. The existing full-host recovery fixture remains the required mounted regression; no mirror algorithm or fake external-service success.

## Confirmed cause and minimal plan

Preserved composer source SHA-256 before writes: `d7ef058d19b29707ff397c9400ba828412f4b2f69c0c004d2bffef70063a2b9a`.

The original `target/nsh-recovery-diagnostic.stderr` has unmatched `new.open_window`, while Root rendering, SQLite setup and service request capture returned. `target/nsh-recovery-diagnostic.sample.txt` lines 52–73 place 537/777 samples in `apply_notify_effect`, including the composer owner observer and `Context<Host>::notify`; this is a notification feedback loop, not a service deadlock or fixture readiness timeout.

The locked GPUI Fast 0.1.9 implementation establishes the reverse edge: `Window::use_keyed_state` calls `observe_keyed_state`, which forwards state notifications to the current parent view. Composer Host observes that ConversationStream and unconditionally changes identities and calls `notify`, closing the cycle. `App::apply_notify_effect` removes the pending-notification marker before invoking observers, so queue deduplication cannot terminate it.

Plan: centralize installation of an actual captured Snapshot in Host, compare the existing `same_source` contract, revoke/install only when it changes, and call `notify` from the owner observer only after that installation. Reuse synchronization on the normal parent-render path. Unchanged-source rendering must preserve keyboard selection, fresh activation intent and native publication identities. Keep live `current`/`valid` checks unchanged.

## Implementation and development regression

`Host::sync_snapshot` now uses the existing full `same_source` contract. A changed source stops the previous Surface, clears publication/activation/height, and installs the new actual Snapshot. An equivalent source retains the current publication identity, intent, selected keyboard row, activation, geometry, mount and native revision. Both parent-render synchronization and the owner observer use that same function. The observer emits `notify` only after a changed source is installed, making the keyed-state return notification a no-op. No live `current`, `valid`, `accepts`, route, keyboard, AX or business dispatch guard was changed; no unconditional frame notification was added.

One development-only GPUI regression, `owner_notifications_converge_and_real_source_aba_revokes_published_intent`, creates a real ConversationStream and Host using the production owner subscription. A real GPUI observer feeds Host notifications back to that owner, matching the observed keyed-state feedback edge. It checks observable notification counts together with preserved keyboard publication/intent, actual captured next-model changes, cleared stale publication/focus/height, and pointer/keyboard/AX stale-snapshot rejection after ABA. The bounded counter fails on recurrence rather than silently dropping notifications. This test is an observation-boundary regression, not a mounted/native-font or external-service claim; the existing two-case production recovery runner remains mandatory. Existing five selector regressions and all recovery assertions/budgets were retained.

## Static validation and stopped handoff

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18) --edition 2024 --config skip_children=true --check` on the two changed Rust files: exit 0.
- Python SHA-256, trailing whitespace/newline, no ordinary code comments and no lint suppressions in the two Rust files: passed.
- No Cargo/rustc, build/test/native execution, network or Git mutation ran. Implementation is **uncompiled and unrun** by this lane. No PASS is claimed for the new regression or recovery cases.
- No shared registration changes are needed: the private `#[cfg(test)] mod tests` and the feature-gated `nsh_recovery_integration` harness already exist. Parent owns all Cargo/shared files.

Changed Rust SHA-256:

| Path | SHA-256 |
|---|---|
| `crates/vega_ui/src/conversation_stream/composer_selectors.rs` | `04d769bd42d3b0e12f57d3cb7f43feee4545cc77604acfc72b9dc5af88ad844f` |
| `crates/vega_ui/src/conversation_stream/composer_selectors/tests.rs` | `223e50ae628e072d97c715f871529790df9b7b9f01097ad309027dd6ca62e1e9` |

Read-only recovery artifact hashes at handoff:

| Path | SHA-256 |
|---|---|
| `crates/vega_ui/src/conversation_stream/shell_recovery/mounted_regressions.rs` | `c77184684377f91199483d1345f8404f84117efeabf5bc60e4939822a03c3c3b` |
| `crates/vega_ui/tests/nsh_recovery_integration.rs` | `1c7bb60deb24323afc88bf19175e99961128c5c8c2b959c834ee7669eaf5337b` |

## Parent next verification

Run the selector regressions with the coordinated toolchain, recording source/toolchain identities and fresh logs:

```sh
cargo test -p vega_ui --features nsh-reading-integration --lib \
  conversation_stream::composer_selectors::tests --offline --locked -j2 \
  --target-dir target/official-nsh-validation
```

Compile the existing recovery runner without executing it:

```sh
cargo test -p vega_ui --features nsh-reading-integration \
  --test nsh_recovery_integration --offline --locked -j2 \
  --target-dir target/official-nsh-validation --no-run
```

Then parent executes the newly compiled exact recovery binary once using its existing 25-second watchdog and bounded diagnostic sampling, retaining fresh stdout/stderr. Require both original cases to pass with the two-pass footer and `open_window` to return; a compiler error, watchdog kill or partial run is not PASS. Do not blindly rerun the old binary. No fixture assertion or 2-second / 512-tick / 10-second budget may be enlarged. Native accessibility, full Composer mounted interactions and external-service acceptance remain separately pending; this implementation report does not substitute for consolidated independent review or final-head CI.
