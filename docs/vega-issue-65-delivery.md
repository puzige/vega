# Issue 65 — automatic conversation titles

Implementation status: ready for main-agent review and full acceptance gates; not installed, committed, pushed, or claimed native-verified by the implementer.

## Scope and mechanism

- R1/R4: migration 0008 persists `auto_title_state` and `auto_title_claim`. Existing nonempty/populated conversations become legacy; empty untitled conversations remain eligible. First accepted user insertion, fallback title, and claim share one transaction, before the assistant placeholder. A failed transaction rolls all three back. A persisted claim is never retried.
- R2: app worker freezes original composer text before `@file` expansion. Input normalizes whitespace and is bounded at 2,000 Unicode scalars; fallback is the first 40, or `图片对话`. No image, expanded reference, tool, history, credential, or existing system context enters the auxiliary prompt. Generated text is whitespace/quote normalized and bounded at 40 scalars; blank/incomplete/tool output fails closed.
- R3: the accepted run's provider authority/model is reused for exactly one non-tool request, maximum 512 output tokens, with retry disabled only for that request. A separate OS worker owns a separate Tokio runtime, has a 15-second deadline and an 8 KiB local text bound, ignores reasoning, and cancels the stream on completion/failure. It survives normal main-runtime teardown and route cancellation. Known usage is stored with no message ID; absent usage produces no fabricated row; unknown price has NULL pricing provenance, not a priced-zero claim. Fixed tracing diagnostics never include raw errors, content, credentials, or paths.
- R5: both manual title writers persist manual provenance, including same-text and ABA changes. Completion conditionally changes only the original claimed thread's title metadata, without `updated_at` changes or resurrection.
- R6: a separate notification lifetime reads title metadata on the background executor and projects only title onto the still-current originating thread. The mutation epoch fences older sidebar snapshots and asynchronous title reads; epoch conflicts coalesce a fresh read, including after sender disconnection. Read errors retry at most three times, then emit a fixed diagnostic. No navigation, draft, model, permission, or active-run replacement.

The existing workspace `tracing` dependency was explicitly approved for direct conversation-crate use; no new library/version was introduced. Historical migration fixtures now construct the real version-6/version-7 schemas rather than pretending a partially downgraded current schema is historical.

## Changed source areas

- `crates/vega_store`: migration registration/0008, title claim/completion/manual provenance, migration and race/rollback guards; exact current schema-version assertions update 7→8, table inventory unchanged.
- `crates/vega_runtime`: `Provider::chat_stream_once`, OpenAI per-call zero-retry override, local HTTP retry-policy regression.
- `crates/vega_conversation`: private automatic-title service and canonical type re-export, persistence config/first-turn transaction integration, read-only title facade, controller and normalization/failure/deadline regressions; tracing manifest/lock entry.
- `crates/vega`: original-text/provider wiring, independent notification consumer, existing worker-call compatibility, production app-worker + recorded HTTP + GPUI regressions.

## Focused evidence

All commands ran in the #65 feature worktree through the cargo lock. Logs live in `/tmp/vega-issue65-focused.9EaFno`.

| Command | Result | Log |
|---|---|---|
| `./scripts/cargo-lock.sh check -p vega_conversation -p vega` | passed after compile-adapter fixes | initial tool transcript, sessions 2791/83183/3966 |
| `./scripts/cargo-lock.sh test -p vega_store -p vega_conversation -p vega_runtime -p vega automatic_title -- --nocapture` | 16 passed, 0 failed: app 5, conversation 6, runtime 1, store 4 | `final-focused-2.log` |
| `./scripts/cargo-lock.sh test -p vega_store --lib` | 103 passed, 0 failed | `store-tests-rerun.log` |
| `./scripts/cargo-lock.sh clippy -p vega -p vega_conversation -p vega_store -p vega_runtime --all-targets -- -D warnings` | passed | `clippy-rerun.log` |
| `./scripts/cargo-lock.sh fmt --all` | passed | `fmt-freeze.log` |
| `git diff --check` | passed | tool transcript at freeze |

Source frozen and cargo returned to main. SHA-256 of the changed-source manifest:
`2b9183a57e74afe7430e88f3a7e7941010c8598d5bbfb719c86d7ce8d8eb4910`.
Reproduction: `git ls-files -m -o --exclude-standard crates Cargo.lock | sort | xargs shasum -a 256 | shasum -a 256`.
This excludes main-owned specification/handoff/review documents and this delivery report.
After the 16-test run, the existing `at_reference_rejection_keeps_provider_at_zero_calls`
regression gained a title-notification zero-call assertion; strict clippy compiled it,
but its execution is explicitly deferred to main's full suite. The config builder
change fixed only the clippy production-layout warning and was also compiled by
the successful strict rerun. Formatting made no behavioral changes.

Representative production proofs:

- `automatic_title_survives_primary_runtime_and_route_cancel_without_reference_leak_or_retry`: gated naming finishes only after main runtime returns and route token is cancelled; original `@file` text only, exact two visible rows, separate unpriced known-usage row, restart retention and no second-turn request.
- `automatic_title_production_worker_records_real_http_wire`: production app worker → real OpenAI provider → bounded loopback server captures exactly primary + title requests, frozen model, two title messages, no expanded-file sentinel/tools, durable title, and no fabricated unknown usage.
- `automatic_title_real_notifications_refresh_current_after_other_task_epoch`: actual watcher/temporary DB/notifications for two tasks; current task eventually refreshes and late other-task completion cannot replace route/header.
- `automatic_title_notification_recovers_after_owned_transient_read_error`: real query failure recovers through watcher retry rather than being mistaken for a deleted task.
- `automatic_title_controller_rolls_back_claim_with_rejected_assistant_insert`: injected assistant-insert failure leaves no user row, title, claim, notification, or request.
- `automatic_title_image_only_fallback_has_no_image_request_and_empty_draft_no_claim`: pure-image accepted turn gets neutral fallback and no image-bearing title request; empty payload does not claim.
- Store regressions cover once/reopen, wrong claim, manual same-value/ABA through both writers, populated-history exclusion and deletion; migration-7 upgrade preserves legacy title/provenance.
- Collector regressions cover scalar limits, redacted Debug, blank/error/unsupported/partial/overflow/cancellation, discarded reasoning, usage observed before error, and the actual 15-second production deadline.

## Original failures (not hidden)

- Initial check found wrong `ChatRole` argument type, borrowed `NewTokenUsage`, and cloning a reference to non-Clone `OpenedThread`; corrected to existing APIs. These early outputs exist in the tool transcript, not separate disk logs.
- `app-title-tests.log`: first recorded-HTTP test compile failed because the app has no direct serde_json dependency and `OpenAiProvider::new` returns Result. Reworked the owned wire test to inspect UTF-8 JSON wire strings without a dependency addition and unwrap only in test code. `app-title-tests-rerun.log` passed all four tests then present.
- `store-tests.log`: cargo-lock correctly refused overlap with the still-exiting preceding focused command. No concurrent cargo ran; after it exited, `store-tests-rerun.log` passed 103 tests.
- `clippy.log`: strict clippy rejected a production struct update because all other fields are test-only. Replaced it with the config builder method; final rerun results are appended below.

## Remaining acceptance and limitations

Main agent owns full workspace gates, package/build, real-provider native UI verification, integration, and card status changes. No native visual claim is made here. The auxiliary network request can succeed while its subsequent persistence fails; fixed diagnostics and persisted fallback remain, with no request retry. After three transient UI read failures the consumer stops retrying that notification; later notification/navigation/reload can refresh durable state. Continuous unrelated metadata mutation can defer header projection until an epoch-stable read, without blocking the UI. The separate worker is bounded but is not joined on route changes; process shutdown terminates it and the durable claim preserves fallback without restart retries.

## Full-suite fixture adaptation (main-authorized follow-up)

Main's first full-suite run stopped in app tests with 140 passed / 15 failed:
`/private/tmp/vega-issue65-acceptance.ap67AY/workspace-tests.log`.
Fourteen failures shared one concrete fixture limitation: existing injected
`MockProvider` instances used one script/call counter for primary and auxiliary
requests. The new `chat_stream_once` default delegated into that same fixture's
`chat_stream`, consuming scripted primary tool/answer rounds, moving original
request indexes, and raising exact primary request counts from one to two.
This explained missing permission/stop states, wrong reasoning answer rounds,
reference requests unexpectedly lacking expanded text, and image requests being
mistaken for the text-only title request. The fifteenth failure was r69's old
empty-title expectation after an accepted first turn, now replaced by R1's
explicit first-message fallback.

Approved adaptation is test-only: `tests/title_fixture.rs` wraps the existing
primary provider and routes `chat_stream_once` to an independent recorded
auxiliary script (Done with no usable text, therefore fallback). Thirteen existing
window provider injection sites use the wrapper. Existing primary counts,
response, permission, stop, privacy, and model assertions remain exact and
unchanged; production `Provider`, `MockProvider`, and title enablement are not
modified. All #65 dedicated app/HTTP/watcher tests remain unwrapped. The sole
changed existing outcome assertion is r69 title `""` → `"materialize me"`, while
the draft identity, row count and other metadata assertions stay intact.

Adaptation verification results and replacement source freeze hash follow below.

- `./scripts/cargo-lock.sh test -p vega --bin vega -- --test-threads=1`: **156 passed, 0 failed, 0 ignored**, 47.82 s. Log `app-all-fixture-adapted.log`. This also executes the enhanced preflight-rejection assertion previously deferred.
- `./scripts/cargo-lock.sh fmt --all`: passed, `fixture-fmt.log`.
- `./scripts/cargo-lock.sh clippy -p vega --all-targets -- -D warnings`: passed, `fixture-clippy.log`.
- `git diff --check`: passed.
- Replacement source manifest SHA-256 (same command as above): `d9edf3bab9badb34917e8798880f69c7ff7238adfadeb76a78ab542b25833677`.

Source re-frozen and cargo returned to main. No production behavior changed during this fixture follow-up.
