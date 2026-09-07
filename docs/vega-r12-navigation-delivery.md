# R12 N navigation delivery

## Freeze
- verified_at_utc: 2026-09-06T04:34:39+00:00
- branch: codex/r12-navigation
- source baseline: 043c676 (A mutation bridge included as dependency)
- tracked_diff_sha256 before delivery document: a2fe3c0680044cc5292c78d5ae51dbb37864bb8d55c34a0fdca5c735309e26ae
- task_contract: vega-r12-task-navigation.md, card N and N race closure
- os_arch: Darwin arm64; rustc 1.98.0 (Homebrew); cargo 1.98.0; git 2.55.0

## Results
| Requirement | Evidence class | Exact command | Result / bounded footer | Raw log SHA256 |
|---|---|---|---|---|
| Navigation root controllers | E2E-REAL + bounded fixture invariant | `CARGO_TARGET_DIR=target cargo test -p vega navigation_ -- --nocapture` | 6 passed; 0 failed; 0 ignored; 0.42s | `4f478dda72a04839860dcb343a8c6069ecdeb8d1e8339e9e0f9ddc4586032fc1` |
| Palette service | E2E-REAL | `CARGO_TARGET_DIR=target cargo test -p vega_conversation palette::tests -- --nocapture` | 2 passed; 0 failed; 0 ignored; 0.08s | `bf53dff59db0ce8a2c30e69fe16f6212b18f70919dfd8b82b50d22796b91fe18` |
| Palette root | E2E-REAL | `CARGO_TARGET_DIR=target cargo test -p vega production_root_palette -- --nocapture` | 1 passed; 0 failed; 0 ignored; 0.60s | `f41743440e5be6d7ae2fabf95f775a2beb1911d34b83fe694f253d64d6f3e830` |
| Strict lint | STATIC | `CARGO_TARGET_DIR=target cargo clippy --all-targets -- -D warnings` | Finished dev profile; 2.68s | `df61faee53c870018ac3385584fb93df1922eb0b23108a7c8164a4207d941cd0` |
| Workspace build | BUILD | `CARGO_TARGET_DIR=target cargo build --workspace` | Finished dev profile; 3.01s | `9374bd4eecde6e4068f907aa90d87943ce2e80e43df310c4674b981ec408d8ea` |

Formatting: `cargo fmt --all -- --check` passes in commit hook; `git diff --check` has no output. Raw logs are retained under `/private/tmp/r12-n-*`; compiler logs retain the existing dependency future-incompatibility notice for block v0.1.6.

## Production evidence
- Actual root command-palette keyboard search opens an owned persisted task. Actual back mouse control, composer Cmd brackets, active platform IME input, collapsed-sidebar controls, and Settings CloseSettings handler exercise draft restoration and route changes.
- Actual Sidebar confirmation deletes the noncurrent back target between resolver dispatch and acknowledgement. The root keeps its current page/cursor, the row remains deleted, and a later back attempt skips it.
- While the read-only resolver is pending, typing a nonempty draft exceeds the retained-byte fixture budget. Final acceptance rejects navigation, keeps the exact text, and leaves the target unread/timestamp unchanged.
- The root rejects archived routes, coalesces attribute changes and project/task globals, drops stale route completions, and bounds history to 100. The byte/history bound fixtures are supplemental invariants, not a claim that 100 native tasks were clicked.
- The native-leading inset uses a theme layout token (96 px), applied to sidebar and collapsed/content controls after coordinator screenshot review; buttons still stop drag propagation.

## First failures retained
- `/private/tmp/r12-n-tests-first.log`: test compilation failed because wildcard GPUI macro import recursively shadowed the generated test attribute; fixed with explicit imports.
- `/private/tmp/r12-n-tests-second.log`: missing AppContext trait import; fixed.
- `/private/tmp/r12-n-tests-third.log`: two root tests passed; the original oversized live-line fixture spent about 50 seconds in GPUI text layout and the owned process was terminated. This is not PASS. Capacity evidence now places 1 MiB in the retained draft map plus short actual editor text; no rendering-performance assertion was weakened or added.
- `/private/tmp/r12-n-race-tests-first.log`: missing borrow for ThreadUpdate in new regression; fixed.
- `/private/tmp/r12-n-clippy-race-final.log`: let_unit_value on AsyncApp::update; removed the redundant binding, final strict lint passes.

## Residuals
- NOT RUN here: full workspace test suite and native single-instance acceptance. Coordinator owns both; existing R11 925/3/0 Git timing failures remain unresolved and are not relabeled PASS.
- NOT RUN: real provider/network/key operations, Keychain, pi, performance benchmarks, push/master changes.
- LIMIT: post-accept visit-save failure and dropped-root acknowledgement are implemented but lack a dedicated N fault-injection test. A supplies its own dropped-entity pending-release regression.
- LIMIT: Tab/Enter/Space control wiring and the corrected native inset still require coordinator native verification; root mouse/shortcut/IME paths are verified.
- No new dependencies or migrations; no whole ConversationStream or worker is retained in history/draft storage. The active editor text is not persisted. Short accepted-visit writes block competing task menu writes, never typing.

## Native focus follow-up

The consecutive-key root regression first reproduced the coordinator native finding: backward succeeded, forward after the dropped editor focus timed out (1 failed; 2.71s). Focus-independent app bindings now coexist with the more-local TextInput/IME override. No synchronous window update was added to key dispatch.

- `r12-n-focus-first.log`: RED: 1 failed; 2.71s; SHA256 `fb655b455984aa7487c8ef2d19220fa0a568bce80a5b5d427895e0e4970daa02`.
- `r12-n-focus-final.log`: PASS: 7 navigation root tests; 0 failed; 0.41s, including existing IME protection; SHA256 `c00f0d313c942699ee72d50ad140600aec2934c83980a67627807d8bb525b8cc`.
- `r12-n-focus-clippy.log`: PASS: strict all-target clippy; 2.01s; SHA256 `b5f495d96ad18fc8b662d0aea7e70e2744b7556215163603bb352c3d79c65951`.

Only the binding scope, its production-root regression, and task/delivery documentation changed. Native verification remains with the coordinator.
