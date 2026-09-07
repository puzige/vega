# R11 composer actions delivery

## Freeze

- verified_at_utc: 2026-09-06T02:52:16.312590+00:00
- verified_at_local: 2026-09-06T10:52:16.312762+08:00
- branch: codex/r11-composer-actions
- task_contract: docs/vega-r11-composer-actions.md
- staged_source_diff_sha256: ce1599461aa5423ab193057664315b616f059d7b1f5d173257e64f89c0e19f57
- Darwin arm64; rustc 1.98.0; cargo 1.98.0; git 2.55.0
- Remote fetched and new sibling worktree rebased before edits. No remote write.

## Delivered behavior

- The composer shows Stop during both preparation and streaming. The explicit stream/thread event cancels the existing exact worker token. Busy ownership remains until terminal drain, then input focus returns, the next draft remains and canceled status is distinct from failure. A durable success received before a late Stop retains success precedence.
- Early cancellation preserves an unacknowledged draft and creates no late durable message. Streaming cancellation retains the partial assistant with its existing interrupted status. Pending permission cancellation preserves the existing rejected/Timeout audit and executes no tool.
- The + menu offers project-file references and Ask/Plan/Execute only. Its file entry appends a separated @ token and opens the existing bounded project file selector. Slash suggestions for /ask, /plan and /execute use the same durable thread-mode handler; only the typed leading token is removed on acknowledgment, leaving the exact remaining draft. Failed persistence and later edits retain text.
- Scoped Tab/Shift-Tab navigation makes the new controls keyboard reachable. Enter/Tab/arrow/Escape command handling and discovery honor IME composition. The small public TextInput::is_composing method matches the parallel palette dependency and may be deduplicated during integration.
- Terminal disconnection releases leftover message/permission presentation, preserving partial text; it does not claim durable completion or replace existing restart repair.

## Results

| Requirement | Evidence class | Exact command | Result | Log SHA256 |
|---|---|---|---|---|
| App and UI regressions, including 4 production-root composer flows and 2 UI invariants | Mixed E2E-REAL, FAULT-INJECTION and UNIT/PROPERTY | `cargo test -p vega -p vega_ui` | PASS: 67 app + 143 UI | `1713cd7cd9fd2e14b70e4ff31353311724bc6186904113f4ee02723bcd329aab` |
| Lint | BUILD | `cargo clippy -p vega -p vega_ui --all-targets -- -D warnings` | PASS | `98d8990f15100a035f226dc56e9140323a2e5080ae004883e2629a123fe1ed28` |
| Executable | BUILD | `cargo build -p vega` | PASS | `51be80aba1818abcc8d97d46c8e48f13d6bc13ae487e402ee214a31172f71bec` |
| Formatting and patch whitespace | BUILD | `cargo fmt --all -- --check`; `git diff --check` | PASS, no output | n/a |

Fresh raw logs remain in `/private/tmp` under the table's command-specific basenames: `vega-r11-composer-final-tests.log`, `vega-r11-composer-clippy.log`, `vega-r11-composer-build.log`.

Bounded exact footers:

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 4.45s
test result: ok. 67 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.94s
test result: ok. 143 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.61s
Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.76s
Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.04s
```

### Evidence boundaries

- `r11_composer_stream_stop_retains_partial_and_next_draft`: E2E-REAL, owned DB/project + production key submit and Stop click + actual worker; MockProvider only replaces network. Durable interrupted partial and unsent next draft survive; zero delayed tool proposals and no late marker file.
- `r11_composer_context_and_slash_keyboard_use_real_mode_and_file_handlers`: E2E-REAL for production +, @index and slash keyboard paths, real mode persistence and zero provider/worker invocation. The owned connection's query-only rejection is a FAULT-INJECTION subcase proving failed persistence preserves the prefix.
- `r11_composer_stop_revokes_pending_permission_without_tool_execution`: E2E-REAL through the actual permission queue and Stop UI; no marker, no outstanding prompt, exact rejected/Timeout denial. It uses the existing real-worker test scheduler parking allowance.
- `r11_composer_preparation_stop_preserves_draft_and_prevents_late_start`: FAULT-INJECTION at the existing provider-construction gate; actual root/worker/DB outside that bounded delay. Stop retains draft; zero provider requests and zero durable rows after gate release.
- `r11_composer_mode_ack_preserves_later_edits_and_terminal_precedence` and `r11_composer_marked_text_is_not_a_command`: UNIT/PROPERTY/UI-handler invariants, including marked text via the real EntityInputHandler. They do not claim native OS candidate-window acceptance or an actual OS thread-spawn failure.

## First failures retained

- Initial test compilation found fixture selector lifetime and mutable VisualTestContext mistakes; fixed fixture code. Log `vega-r11-composer-tests.log`, SHA256 `7ca3c164e6468fc3e43582b229c23bfee53b92f8c9b62d10eee52e82432bc072`.
- Keyboard-navigation compilation needed the Focusable trait import. Log `vega-r11-composer-tests-3.log`, SHA256 `6c25b21ab1c8214e72a2d46423f800eb8f23987172b69918b6e44d28faac5da4`.
- The new permission test initially asserted cancelled tool status, contrary to the existing cancelled_permission contract (rejected plus Timeout audit). Corrected the new assertion to that documented contract; runtime unchanged. Log `vega-r11-composer-tests-5.log`, SHA256 `36eedb22a972d1b1ca2fb1d1b2cdafbb4c0edfaa2ff79add79d7006c02759d1d`.
- A subsequent permission test hit the GPUI deterministic scheduler's cross-thread wake check. Reused the existing production permission E2E's allow_parking setup; no production bypass. Log `vega-r11-composer-tests-6.log`, SHA256 `9b9e04438255aebfc07e887b9c4682452f5af004c58678e8299b20cdf590f736`.
- First combined regression run: 66 app tests passed; existing `tests::diff::diff_refresh_intents_keep_content_during_background_and_retry` failed with retry terminal `refresh_error=Some(GitFailed)`. No diff code changed. Log `vega-r11-composer-regression.log`, SHA256 `4baf52a3f919dfa122c0ad67ed97097c951c57845955df4b3a7ce6f7b08db0c4`. Exact isolated rerun (`cargo test -p vega tests::diff::diff_refresh_intents_keep_content_during_background_and_retry -- --exact --nocapture`) passed once in 0.91s, log `vega-r11-existing-diff-retry.log`, SHA256 `8b62c25ae7193021a3ff23bed3c74ef2277936178a58503b88fb9d4cce557d2b`. Final post-cleanup combined suite passed; the first failure is preserved, not diagnosed as a proven flake.
- Root review found the initial spawn-error reset on the wrong worker branch. It was moved from history loading to the actual agent spawn failure before final verification; no history worker now changes run state.

## Residuals and integration

- NOT RUN here: workspace-wide suite, native CUA/IME candidate-window checks, real provider/network/key interaction. Root owns integrated suite and native acceptance.
- LIMIT: disconnected/panicked-worker durable rows still use existing restart repair; this slice only releases presentation ownership. Normal explicit cancellation is covered through actual durable interrupted state.
- LIMIT: no image/file upload, /goal, /compact, extensions or skill menu is advertised. + is project-file context; mode commands never submit a model request.
- ACCEPTED: pre-existing `block v0.1.6` future incompatibility advisory remains; lint emitted no project warning.
- Shared integration hunks: `window/render.rs` adds only the stop subscription beside composer submit; `vega_ui/src/lib.rs` adds component-scoped bindings; `text_input/state.rs` adds the same is_composing method as the palette slice. No workspace/sidebar/settings or data-accounting edits.

## Native follow-up: file popup placement

- Verified at 2026-09-06T03:19:28.500352+00:00. Root native review found the file candidate popup's `bottom: 0` covered the draft. The only production edit is `conversation_stream/render.rs`: anchor its bottom to the input row's top (`relative(1.0)`), keep a spacing gap, and defer/occlude the popup at foreground priority. Candidate selection and retry/key contexts are unchanged.
- `cargo build -p vega`: PASS, 3.85s. Raw `vega-r11-file-popup-build.log`, SHA256 `9ab2ff8185a752382df37c9276f5b9e3707f03e93c2d8a732c8315d2b2fd1323`.
- Existing production keyboard regression `cargo test -p vega r11_composer_context_and_slash_keyboard_use_real_mode_and_file_handlers`: PASS, 1 passed, 0 failed, 0.24s. Raw `vega-r11-file-popup-keyboard.log`, SHA256 `ed7003192d0c7e1d543a1a72a973c37f089c5af258355072eb905b454661b2a2`.
- `cargo fmt --all -- --check` and `git diff --check`: PASS. No new layout test; root owns the combined native recheck.
