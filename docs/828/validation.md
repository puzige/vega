# Registry deadlock validation record

## Versions and static audit

- Baseline: `b2c18b2c6d95c146d8abbaafa38cf93818699707` (PR #824).
- Spec-first commit: `258c4dd092eb3ac2b5a759b0f29b33a0592b0c1a`.
- Successful development head: `0fdd6dbc0ff63e2a732edf79425c2ee08a32f94e`.
- Final production delta: only `drop(state)` after offer collection and before sorting/credential checking in `step_up_offers`. No dependency, workflow, migration, UI or other production changes.
- Directly affected paths inspected: `store`, `reject_owner_secret_in_ui` callers (discovery/preparation/begin/step-up), CRUD/credential rotation and `reconcile_pending` callers, `test_connection`, `ready_for_run`, `ensure_ready`, run credential readers, challenge/admission callbacks. Other paths already release the registry with explicit `drop` or scope boundaries before re-entry; callbacks do not invoke store while holding that registry. No additional same-root re-entry found.
- Independent non-implementer review of `4ddc46441dde61606cbd75869760ce95a78adc6f`: no actionable findings. Subsequent changes only fix temporary fixture setup, then remove tests; production `drop(state)` unchanged. Final-head verification is recorded in PR #831 / Issue #828.
- Local `cargo fmt --all -- --check` and `git diff --check` passed after fixture formatting. Initial format-only check failed and was corrected before committing code. No local Vega execution, compile/build/check/Clippy/test/package. No running MCP settings inspection or real user config/credential/database access.

## Development cloud validation

Existing PR workflow only; no Actions modification or weakened assertion.

| Version / Actions | Actual result | Disposition |
|---|---|---|
| `4ddc46441dde61606cbd75869760ce95a78adc6f` / [41297733680](https://github.com/puzige/vega/actions/runs/41297733680) | Compilation failed, E0599: temporary fixture used unavailable `Ulid::new`; Rust discovery/execution not reached | Corrected fixture to repository API `Ulid::generate`; no production or assertion change |
| `3a2e45f25966374228a3f3913762dc079156569e` / [41298323277](https://github.com/puzige/vega/actions/runs/41298323277) | Clippy passed; Rust 4 discovered/4 executed/0 passed/4 failed/0 skipped, all during initial fixture reconciliation (`Invalid`) | Canonicalized newly-created temp root to satisfy symlink-ancestor protection on macOS; no security check or assertion relaxed |
| `0fdd6dbc0ff63e2a732edf79425c2ee08a32f94e` / [41299222030](https://github.com/puzige/vega/actions/runs/41299222030) | Rust 4 discovered/4 executed/4 passed/0 failed/0 skipped across 16 binaries; Python 5+15+40 passed; fmt, Clippy and required aggregate success | Valid development regression evidence |

Successful jobs: [Nextest 124826488821](https://github.com/puzige/vega/actions/runs/41299222030/job/124826488821), [Clippy 124826488916](https://github.com/puzige/vega/actions/runs/41299222030/job/124826488916), [required 124827300140](https://github.com/puzige/vega/actions/runs/41299222030/job/124827300140).

### Recoverable original test source

- Retained ref: `evidence/828-mcp-progress-0fdd6dbc` at full development SHA above.
- [Original Rust test source](https://github.com/puzige/vega/blob/0fdd6dbc0ff63e2a732edf79425c2ee08a32f94e/crates/vega_conversation/src/mcp_settings/registry_progress_tests.rs), verified accessible with GitHub Contents API before deletion.
- Source SHA256: `8d036e688921575228faa6d58a9a004dda7fb97b9847bfb44d7922f1b6ca015e`; blob `65bb215c9a404f7aee822d857bc39dc99d191a73`, 6093 bytes.
- The earlier failed fixture source is retained at `evidence/828-mcp-progress-3a2e45f2` / full SHA in table.
- Final cleanup removes the entire development-only Rust module and its `cfg(test)` connection; no fixture, helper, dev dependency or feature remains. Final required CI must confirm `no Rust tests` (0 discovered/0 executed). Final run/head evidence belongs in PR/Issue to avoid a documentation-only new head after validation.

## Limits and manual acceptance

These isolated tests exercise repeated empty settings reads, owner credential rejection/rotation, concurrent empty readiness plus owner credential reads, synthetic OAuth cancellation authority, and YAML declaration-change/error revocation. They do not exercise a live model/MCP server, actual tool-description/result projection, a secret-bearing nonempty OAuth offer, or cancellation of an active real conversation. No external network server or process was used by the tests.

Latest final head required CI, merge and installation are separate facts. User manual acceptance remains pending; follow the task spec. Keep Issue #828 open, In review after merge; do not claim CI proves native integration or that the running app was updated.
