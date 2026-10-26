# Permission reset recovery — development stage

This supplements the D4 recoverable reset protocol. It does not reduce full-card acceptance: initiating reset still requires the application-wide pause, active-run Stop and trustworthy cleanup evidence; Bash and remaining UI remain outstanding. No local Vega execution, compilation, test, migration or user configuration change is permitted. Existing Actions execute development tests; native integration remains user hand-test.

## Recovery service contract

- Resume only an existing, valid pending journal. No journal means no reset; a completed journal does not initiate another reset. A missing/inconsistent journal fails closed.
- Acquire the authority coordination guard and exact database admission pause, then require all existing owners released. This is an application-owner check, not proof of external-process exit. Persisted streaming messages, active automation claims, active/unreleased delegated executions, unresolved effect-capable calls, unknown credential continuations or unknown command-scope results block recovery without normalizing those records.
- Validate and revoke automation authorization through typed tasks. Verify JSON identities/revision match their indexed columns; pause active tasks, clear authorization on all affected statuses, increment affected revision once, preserve payload/usage/blocker history, and never finish an uncertain claim to pass reset.
- Open an existing canonical configuration under its writer lock. Missing/invalid configuration or a changed canonical source fails; never initialize a missing configuration. Accept only the journal's exact raw permission source or its already-reset target, preserve unrelated preferences, save Confirm/no grant/exact target revision atomically, and read back before completing the journal.
- Recovery is idempotent across the DB/config boundary: any failure after pending remains pending; retries use the same journal id, epoch and target revision. Configuration conflict does not overwrite a competing edit. Completion/commit uncertainty is not a success receipt.
- Startup must check pending reset before ordinary automation recovery can normalize active claims. If recovery cannot complete, automatic scheduling stays unavailable and subsequent execution continues to fail closed. The startup call must not claim to have stopped native processes or repaired earlier FullAccess effects.

## Required development checks

Use isolated database/config fixtures and the real recovery service. Cover source and already-target recovery, retained unrelated preferences, missing/malformed/conflicting config, automation authorization on active and paused tasks, indexed-data mismatch/revision overflow, pending startup refusal before claim normalization, and unsettled-work preservation. No real credentials or external process/network execution.

## Current evidence

- Persistent-journal stage `4514968e4a4d47fe66f773f47b89f6225ae3cf32` passed [run 40383352630](https://github.com/puzige/vega/actions/runs/40383352630): Clippy `121785079577`, Nextest `121785079960`, aggregate `121787583590` all succeeded. **26 discovered, 26 executed, 26 passed, 0 skipped**, summary `2026-10-26T19:08:37Z`. Scoped non-writer review of that exact increment returned no findings; it did not execute code or query CI.
- Recovery service implements existing-pending continuation, strict existing-config edit, typed automation revocation, configuration readback, and conservative unsettled-work refusal. Ordinary automation command/startup paths reject pending before normalizing claims. Six new isolated service regressions await Actions and scoped independent review.
- Startup does not yet invoke the recovery service. Its earlier generic config initialization and application-wide execution/error projections must be integrated with the reset controller; otherwise a missing configuration could be recreated before recovery checks. No completed reset UI or downgrade-safe result is available yet. Initial reset/Stop/cleanup proof, in-memory invalidation and full startup wiring remain required—not waived by this increment.
