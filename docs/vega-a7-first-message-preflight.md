# A7-01 · First-message readiness and honest failure

Status: SPEC FROZEN · 2026-09-17 · Owner: primary agent

## Evidence and scope

Native `/Applications/Vega.app` (SHA-256
`62928924b8a37cc2ae4e9bece2d8dabd1c660112a2fcffe56ccfe83e3d895112`)
allows a home-draft submit while its only provider is disabled and its local
credential is absent. The composer still displays `glm-5.3-flash`. One real
`⌘↩` test kept the draft, showed only `执行未完成，可安全重试`, and changed the
database from 66 threads / 18 messages to 67 threads / 18 messages; the new
thread had zero messages. This is a user-visible product failure, not evidence
that a valid provider or network request was attempted.

This task changes **first-submit readiness and failure projection only**.
It takes precedence over R69 R8's “first submit materializes” specifically
when submission cannot start because provider configuration or its credential
is unavailable. An accepted submission still follows R69's stable draft-id,
single-materialization, and no-stream-rebuild contracts.

## Behavioral contract

1. On a home draft or an existing conversation, a submission whose selected
   model has no unique enabled provider must fail *before* materializing a
   draft or starting an agent run. The original draft/input remains editable.
   Show a clear, actionable error identifying the unavailable provider/model
   configuration and directing the user to Settings → Providers; do not claim
   that a network request or agent execution failed.
2. A provider with no readable local credential likewise fails before draft
   materialization. Show the existing credential-repair guidance. Resolve the
   credential only in the approved owner-only keystore; never display, log,
   copy, or persist its value elsewhere.
3. Rejected preflight has **zero** new thread/message rows and no sidebar
   placeholder. Repeated clicks cannot create duplicates. No synchronous
   filesystem or keystore IO may be introduced on the UI render/submit path.
4. After the user repairs configuration using Vega's own Settings UI, the
   retained draft can be submitted again without restarting or editing a
   local file. On the first accepted attempt, exactly one thread is created
   with the same draft id; normal conversation persistence then owns results.
5. If execution has actually started, do **not** delete or roll back the
   thread to conceal an upstream/network failure. Preserve its messages and
   failure/retry context under existing conversation rules. Distinguish this
   from a preflight rejection in UI and tests.
6. The home composer must not imply the persisted default model is usable
   merely because its name exists in config. Keep the model name if needed to
   preserve draft preference, but submission readiness and error must reflect
   current enabled-provider authority. No fake provider, key, or success.
7. No user is asked to edit tests, config files, SQLite, or credentials by
   hand. Native live-provider acceptance needs an actual credential entered
   by the user through Vega Settings; until then it is BLOCKED, not PASS.
8. A first submit whose exact draft model is not in the current Ready pricing
   authority is also rejected **before** materialization. It opens the existing
   Settings → Pricing repair route, retains the exact composer text, project,
   model, and draft id across “Back to app”, and creates zero thread/message
   rows. After adding that model's price through Settings, retrying the same
   draft materializes exactly once and starts normally. This extends the
   first-submit ordering only; existing durable-thread pricing preflight and
   its authority remain unchanged. A pending/invalid pricing authority still
   fails closed rather than inventing a zero price.

## Boundaries

- No migration or automatic deletion of historical empty tasks.
- No changes to model pricing data or mutation policy, permission policy, branch/project menus, or
  composer geometry; no new dependencies.
- No raw key or provider response in logs/test reports. No non-test
  `unwrap()`/`expect()` and no hard-coded style values.
- Do not install, push, or merge the result from an implementation subagent.

## Acceptance

- Production-path window/controller tests with an owned temporary store and
  config cover: disabled provider, unavailable selected model, missing key,
  repeated rejected submit, and successful recovery into one materialized
  thread after readiness is repaired. Mock only the provider/network boundary
  on the successful path; label this E2E-REAL, not live provider validation.
- Assert both visible error text and authoritative DB row counts, draft id,
  retained input, and absence/presence of run start. A test must fail when
  preflight is deliberately bypassed.
- Preserve R69 regression tests. Run focused tests, then
  `./scripts/cargo-lock.sh test --workspace`, `cargo fmt --all -- --check`,
  and strict workspace clippy through `cargo-lock.sh`.
- Package a candidate, verify it in the native UI with the currently disabled
  provider: explicit guidance and no new row. The user has explicitly
  authorized sourcing the CPA credential from local Pi Agent; the primary
  agent may read that source without displaying the secret and save it through
  Vega Settings UI. Then verify a real response and one safe test-project tool
  action; record live network evidence separately from mock tests.
- A mounted production-window regression covers an enabled provider and
  credential with an unpriced exact draft model: submit → Pricing route →
  UI Settings mutation → Back to app → retained text/project/model/id → retry.
  Assert zero new rows and zero worker/provider calls before repair, then one
  materialized row under the original draft id after repair. The test must
  fail against the former materialize-before-price ordering.

## Change record

- 2026-09-17: Native E2E found a valid-provider `hy3` draft created a durable
  empty thread before the missing-price gate opened Settings; returning after
  repair lost the typed first message. Rule 8 and the mounted regression above
  freeze pricing as a first-submit readiness check before draft materialization.
