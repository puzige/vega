# A7-03 · Runtime failure visibility after accepted submit

Status: SPEC FROZEN · 2026-09-17 · Owner: A7 E2E

## Evidence and scope

After a Pi Agent CPA credential is explicitly imported and the provider is
enabled, the first `glm-5.3-flash` message reaches the provider. The current
account rejects it with HTTP 400 `insufficient_user_quota`. Vega durably records
an assistant message with `status=failed` and empty content, but the conversation
view shows no actionable explanation. The A7-01 preflight is correctly passed;
this is a **post-start** failure, not a missing credential or failed submit.

This task only changes presentation of an existing `ConversationEvent::Error`
and an existing durable `HistoryEntry::AssistantText { status: Failed }`. It
does not alter provider requests, pricing, persistence schema, credentials, or
run/retry semantics.

## Behavioral contract

1. A runtime/provider error for an active assistant message ends that message
   and renders a visible failure line attached to that assistant turn. This
   remains visible even when the assistant emitted zero text deltas. The user
   message and durable failed assistant row remain intact.
2. The live line is selected from bounded, content-free categories. The exact
   structured provider code `insufficient_user_quota` on HTTP 400 is shown as
   an insufficient-balance instruction; 401/403, 429, other HTTP status,
   transport failures, and non-provider runtime failures use conservative
   distinct guidance. Never render raw response bodies, arbitrary error
   strings, API keys, paths, or request content.
3. The existing composer-level failure also uses the bounded category after
   app-level thread refresh; a generic completion fallback must not overwrite
   the more useful terminal reason. Pre-start failures keep their existing
   credential/preflight guidance.
4. On route reopen/restart, history only knows durable `Failed`, not the
   original provider code. Its assistant turn shows a truthful generic
   failed-run instruction, **not** a fabricated quota diagnosis. `Done` and
   `Interrupted` history keep their present display behavior.
5. Error events for a foreign/stale message cannot annotate another assistant
   turn. No raw provider diagnostic is persisted for this feature.
6. The app worker's terminal `success` must reflect `ConversationRun.failed`,
   not merely `Result::Ok`. A runtime may return `Ok(ConversationRun)` after
   durably recording a failed assistant turn; labeling that as success would
   suppress the controller's post-refresh error projection.

## Acceptance

- Production `ConversationStream::apply_event` tests cover a zero-delta quota
  failure, non-quota HTTP fallback, source-body redaction, and a foreign event.
- A typed `HistoryPage` with a failed empty assistant row renders the generic
  line after hydration; a done row does not. Inspect the mounted entry, not
  only a standalone formatter.
- App/controller production-path test uses an owned temporary store and a
  `MockProvider` at the network boundary to show accepted submit → durable
  `failed` row → visible bounded error. This does not claim live account
  credit or model success.
- Run focused tests, `./scripts/cargo-lock.sh test --workspace`,
  `cargo fmt --all -- --check`, and strict workspace clippy.
