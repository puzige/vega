# Credential continuation delivery

## Plan

Implement design §8 using a durable receipt bound to the original user and assistant, source version/fingerprint, and a unique recovery operation. Recovery claims the receipt in the same transaction that creates a continuation assistant; it never inserts a user message or executes historical tool arguments. Existing Plan approval remains a separate path. Every resumed model request rebuilds credential projection using current owner state.

## Test matrix

| Requirement | Evidence planned |
| --- | --- |
| Duplicate recovery clicks | SQLite CAS permits one claim and one continuation assistant |
| Restart | Temporary file database close/reopen preserves receipt and original user association |
| Source edits | Source version/fingerprint conflict rejects claim |
| Completed side effects | Retained tool results passed as history; simulated side effect counter remains one |
| Unknown side effects | Incomplete or recovery-cancelled tools reject automatic continuation |
| Credential failure | Receipt persisted with failure; recovery has independent entry |
| Success/cancellation | Receipt lifecycle records consumed operation; no implicit replay |
| UI | Local continue/settings actions, no new user draft |

## Validation

Local compilation, tests, Clippy and builds are prohibited. Only formatting and static diff checks run locally. Functional tests must run in cloud PR Check. Real UI and process integration remain user manual acceptance.

## Implemented

- Migration 0023 stores content-free receipt identity, original user, source version/fingerprint, claim operation and continuation assistant. Claim and insertion share one immediate transaction. Messages finish and receipt completion share one transaction.
- A dedicated continuation entry rebuilds history and current credential authority, preserving completed tool results. It creates a linked assistant segment without inserting a user turn or resubmitting prior tool arguments. Plan approval remains unchanged.
- Startup recovery only reconstructs an interrupted claimed continuation when its assistant was streaming and its user remains the latest user. Any unfinished/cancelled operation in that user's execution interval blocks automatic continuation; earlier turns do not permanently disable later turns.
- History hydration restores reason and actions. Source conflicts reject continuation with guidance to inspect history and send a new instruction. Consumed receipts stop offering continuation. Settings uses the existing settings route.

## Evidence

Local `cargo fmt --all -- --check` and `git diff --check` are the only executed checks. No compilation, Clippy, nextest, external process or UI acceptance has run locally. Cloud PR Check must validate the new tests before delivery is considered accepted.

New automated cases: six temporary SQLite transaction/reopen tests; one conversation mock-provider/tool-executor test that persists a completed side effect, blocks the follow-up, reopens SQLite, resumes and asserts the side-effect count and user count remain one; GPUI hydration and notice tests for recovery availability. These are pending CI, not passing evidence.

## Residual boundaries

A source conflict or unknown execution outcome requires the user to review existing tool records and issue a new instruction; the application does not guess whether an interrupted mutation ran. Transparent API and environment authorization are supplied by the Broker workstream. Credential grants and projection are rebuilt on each continuation instead of reusing an old projection or injection permission. Real keyboard/UI behavior and host process isolation remain manual acceptance.
