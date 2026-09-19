# Issue 63 · UI/application slice evidence

## Freeze

- Verified UTC: 2026-09-18T16:48:49Z; local: 2026-09-19 00:48:49 +0800.
- Contract: `vega-issue-63-image-attachments.md`, R1–R7.
- Branch: `feat/issue-63-image-attachments`; shared integration worktree,
  uncommitted implementation. Final tree/hash and whole-workspace gates are
  recorded by the main integrator after both implementation slices freeze.

## Changes

- `vega_ui/text_input`: opt-in Composer clipboard intent; normal inputs stay
  text-only. Image bytes are shared into the worker instead of copied on UI.
- `vega_ui/conversation_stream/attachments.rs`: explicit paste, external drop,
  native picker, bounded atomic import, generation retirement, cached image
  previews and accessible mouse/keyboard removal. History preview preparation
  also runs in the background; placeholders retain the same 72px geometry.
- Composer menu, stream state/submission/render/hydration: frozen attachment
  IDs survive rejected preflight and only durable ACK consumes matching IDs;
  pure-image submissions work. Empty Composer mounts no attachment surface.
- `vega/app_agent.rs`, `window/agent.rs`: immutable text/image payload crosses
  readiness, lazy materialization and worker boundary without rereading UI.
- `vega_theme`: shared 72px thumbnail token; existing colors and chrome reused.
- UI handler tests and `vega/tests/r69.rs` application E2E added; existing
  exhaustive test classifiers extended for the new semantic image row without
  weakening any existing assertions.

## Results

| Evidence | Command | Result |
|---|---|---|
| Production compile | `scripts/cargo-lock.sh check -p vega -p vega_ui` | PASS |
| UI production handlers | `scripts/cargo-lock.sh test -p vega_ui -p vega issue63 -- --test-threads=1` | 6 passed, 0 failed |
| Application E2E-REAL; mock provider boundary | same command | 1 passed, 0 failed |
| Whitespace | `git diff --check` | PASS |
| Strict workspace lint (follow-up) | `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings` | PASS, 7.36s |
| Workspace format (follow-up) | `scripts/cargo-lock.sh fmt --all -- --check` | PASS |

Latest raw bounded test footer:

```text
test tests::r69::issue63_standalone_first_image_submit_crosses_app_worker_and_persists ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 149 filtered out; finished in 0.50s

test conversation_stream::tests::attachments::issue63_external_drop_and_remove_use_rendered_handlers ... ok
test conversation_stream::tests::attachments::issue63_invalid_clipboard_batch_is_atomic_and_route_retires_import ... ok
test conversation_stream::tests::attachments::issue63_mixed_clipboard_preserves_text_and_freezes_image ... ok
test conversation_stream::tests::attachments::issue63_native_picker_cancel_and_selected_file ... ok
test conversation_stream::tests::attachments::issue63_paste_image_only_submit_reject_and_ack_preserve_new_identical_image ... ok
test conversation_stream::tests::attachments::issue63_route_change_discards_late_picker_and_empty_composer_has_no_attachment_surface ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 351 filtered out; finished in 1.09s
```

The standalone app test drives the real Paste action and production submit,
rejects a disabled owned-fixture provider before creating any thread, confirms
the preview remains, repairs through the settings service, resubmits image-only,
and verifies exact bytes at the mock provider, one durable attachment row and
the consumed Composer preview. It does not claim real network/model vision.

The UI tests exercise actual rendered menu/drop/remove handlers, clipboard
action, mixed text+image, native picker cancellation, invalid atomic batch,
pending import, an identical image added after submission surviving ACK, and
`OpenedThread` route changes retiring picker results. The invalid-batch test's
second half uses explicit cancellation; the separate route test proves the
actual global observer wiring.

## Initial failures and limitations

- First focused test compile failed: three existing exhaustive test classifiers
  lacked `UserImages`, and the dynamic debug selector needed a static test
  lifetime. Added the missing variants and owned test selector; no production
  behavior/assertion was weakened. Subsequent focused runs passed.
- Main's first strict lint found an introduced nested `collapsible_if` in the
  mixed clipboard text guard. Replaced it with a let-chain without lint allows;
  subsequent strict workspace/all-target lint and format check passed. Main
  retained the first failure log with the integration evidence.
- One attempted test invocation correctly refused the held repository cargo
  lock; the later serialized invocation ran successfully.
- Existing upstream `block 0.1.6` future-incompatibility warning remains.
- LIMIT: macOS GPUI selects filesystem paths first, then plain text, then image
  representations. When it returns only text despite additional platform image
  formats, Vega cannot see the omitted image. Mixed entries that GPUI does return
  are handled; ordinary text paste is preserved.
- LIMIT: TIFF-only clipboard content is intentionally unsupported by R1 and
  produces the actionable PNG/JPEG/WebP error, not silent omission. Finder copied
  files are explicit attachment intents in Composer; unsupported files produce
  that error rather than being pasted as implicit file references.
- NOT RUN by this slice: full workspace gates, native pixel/live vision test,
  package/install, remote push. These belong to main integration. No user
  credentials, configuration or workspace files were changed.
