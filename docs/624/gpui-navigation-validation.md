# GPUI navigation development validation

This is development evidence for the #624 D4 navigation/receipt regression, not whole-card approval or native UI acceptance. All execution is in existing PR Actions; no local Vega build/test or user configuration change is permitted.

## Failure evidence and harness correction

- `f00194d215dc0cb867c7a062b51ecfa391ade799`, [run 40374004186](https://github.com/puzige/vega/actions/runs/40374004186): 19 executed, 17 passed, 2 GPUI cases failed at leaked `ConversationStream` handles, 0 skipped. Clippy rejected the unused GPUI alias. The pinned macro's entry wrapper rewrites paths to the `gpui_kit` facade.
- `3c4a3f9fc168b3eba7d57dc4220329a34f2352b2`, [run 40376422729](https://github.com/puzige/vega/actions/runs/40376422729): Clippy passed; 21 executed, 19 passed, the same 2 GPUI cases failed the newly added bounded stream-release assertion, 0 skipped. Both admission-reset development tests passed. Closing the window and draining asynchronous tasks was necessary lifetime hygiene, but not sufficient to fix the actual leak.
- Source diagnosis at pinned `gpui-pre 0.3.4`: `AnyElement::new` allocates in an arena. Calling `owner.render` outside a draw scope uses the thread-local fallback arena; dropping the returned element does not clear it. `Window::draw` establishes the application's element arena and returns `ArenaClearNeeded`, whose `clear` releases those elements. Therefore the tests must use the real window draw cycle outside a borrowed root entity, rather than manually invoke `render` and drop its return value.

The corrected harness retains both event orderings, original entity-identity assertions, full durable permission/grant snapshots, unchanged-away-route assertion, and fresh submission preparation. Each navigation uses the real window draw lifecycle. Explicit window close, bounded asynchronous cleanup and weak-handle release assertions remain; no leak check, lint, test or required CI assertion is disabled. It is not a production behavior change or a longer-timeout workaround.

The correction uses `TestAppContext::update_window` with the root entity unborrowed, followed by `Window::refresh` and `Window::draw(...).clear(...)`; assertions read the root after that draw completes. The fixture's own strong original-stream handle is explicitly dropped before checking weak-handle release. No production API or CI workflow was changed.

Local static checks: `rustfmt --edition 2024 --check crates/vega/src/window/ordinary_send/tests.rs` and `git diff --check` both exited 0. These are static checks, not test execution.

## Corrected-version evidence

- Exact version: `b40dea7ae6495f40457eb1620b3c53641873a92e`, relative to `17c5b6a43a02713189c3dcbe8ffe06d9d8dc9ed5`.
- [Run 40381304783](https://github.com/puzige/vega/actions/runs/40381304783): Clippy job `121777943588`, Nextest job `121777943089`, aggregate `121780345366` all succeeded. Nextest reported **21 discovered, 21 executed, 21 passed, 0 skipped**. Both `navigation_receipt_while_away_preserves_route_and_reloads_authority` and `navigation_return_reuses_preparing_entity_and_committed_authority` explicitly passed. The summary was emitted at `2026-10-26T18:54:53Z`.
- Non-writer scoped review of that exact two-file delta returned **no findings**. It verified the pinned draw API/borrow boundary and retained assertions; it performed no local execution or CI polling. It is not whole-card approval.
- [Recoverable development source](https://github.com/puzige/vega/blob/b40dea7ae6495f40457eb1620b3c53641873a92e/crates/vega/src/window/ordinary_send/tests.rs). Tests and exclusive GPUI support must still be removed before the final merge head under repository policy; final-head CI remains separately required.
