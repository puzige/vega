# Final-version gates — NSH production migration

## Exact source and retained evidence

- Development source and complete Rust regressions: `ef3dddd6791e3607e40b1b1bd8adb60b48979752`.
- Remote recovery tag: `evidence/907-nsh-development-ef3dddd6791e`.
- Public source: https://github.com/puzige/vega/tree/ef3dddd6791e3607e40b1b1bd8adb60b48979752
- Published validation records: `evidence/907-development/validation-records.tar.gz`, with raw-local and sanitized-public hashes in its sibling manifest. Raw local files were retained. Only exact local workspace/home prefixes were replaced in the public copy.
- Cleanup commit: `792d69b58ea9ae543426e04ab20571d572cd4c5b`.
- Source gate commit: `c5cd4ffeceda9665dace2602f1d2429fbf9849b1`.
- Integrated master: `2a29adf9b2ea78eaf59d591838531b45adc7373e` (#922).
- The merge records upstream ancestry while preserving every production-source byte from the cleanup commit. Only `docs/915/draft-shell-recovery.md` and `docs/915/validation.md` were added. Existing NSH rendering and operation/source authority fences were retained rather than restoring upstream GPUI helpers.

The implementation-review baseline `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc` is historical. Both P2 fixes received behavior validation and non-author incremental review; the exact paste baseline was recovered and hash-verified before that review concluded. See `review-app-settings-workspace.md`, `review-single-line-paste-final-delta.md` and `nsh-paste-baseline-recovery.md`.

## Current-cycle development regression, before cleanup

Toolchain: rustc/Cargo 1.98.0; auto-update disabled, offline/locked, central build concurrency 2; target directory `target/official-nsh-validation`. Local Nextest is 0.9.151, not the CI-pinned 0.9.146.

The exact stopped source manifest is retained as `target/nsh-final-gates-precleanup-content.json`, identity `24af621a501ddf6fa60a57d5a3657a6425758395247b13845ced384a1d3f24fe`. The published archive includes it and the command stdout/stderr/JSON records. Additional concurrent documentation did not mutate its recorded source bytes.

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets --locked --offline -j 2 --target-dir target/official-nsh-validation --features vega_ui/nsh-reading-integration,vega/nsh-workspace-integration -- -D warnings`: exit 0, 18.43s.
- Combined UI/application test-artifact build: exit 0, 85s; exact `profile.test` artifacts were selected.
- All 14 actual artifacts completed with exit 0; no watchdog timeout. Counts: UI 362; application 31; reading/rename 15; native host 36; geometry 24; scroll 16; icons 3; settings 4; recovery 3; message anchors 6; command palette 3; ordinary controls 4; workspace mounted 6. The remaining application-library artifact had an empty set and is not counted as behavior coverage.
- Ordinary workspace lib/bin Nextest: 393 passed, 0 skipped.
- Python: release 5/5, computer-use preparation 15/15, script tests 40/40.
- Pre-cleanup i18n inventory had 3 stale fixture classifications and 44 unreviewed development literals. Exit 0 alone was not accepted as passing. After actual fixture removal, only the corresponding 60 fixture classification entries (132 occurrences) were removed; final structured checking has 0 errors.

These are development/in-process results, not OS-native acceptance or final-head cloud CI.

## Final production source, after cleanup and baseline integration

All task-added Rust tests, exclusive runners/helpers, development features and diagnostic seams were removed only after the remote development commit/tag were verified. Production capability patches, original upstream vendored tests/dev-dependencies, Python tests/fixtures, benchmarks and safety assertions were preserved. Cargo removed 14 unreachable development packages; no registry identity was added or upgraded.

Actual checks on source commit `c5cd4ffeceda9665dace2602f1d2429fbf9849b1`:

| Check | Actual result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked --offline -j 2 --target-dir target/official-nsh-validation -- -D warnings` | exit 0, 5.33s |
| `cargo nextest list --workspace --locked --offline --target-dir target/official-nsh-validation --message-format json` | 0 discovered |
| `cargo nextest run --workspace --locked --offline --target-dir target/official-nsh-validation --no-tests=pass` | exit 0; 0 executed, 0 skipped — **no Rust tests**, not Rust-test success |
| `python3 scripts/release_tests.py` | 5 passed |
| `python3 scripts/prepare_computer_use_tests.py` | 15 passed |
| `python3 -m unittest discover -s scripts/tests -v` | 40 passed |
| `python3 scripts/check_i18n.py --json` | 0 errors, 181 classified literals |

Raw records use the `target/nsh-final-head-*` prefix. A mistakenly attempted unsupported `check_i18n.py --check` returned an argument error; that invocation did not validate anything and was followed by the actual supported structured interface. Two cleanup-created `let_and_return` warnings were repaired with equivalent return expressions; the first failed Clippy log remains retained.

## Independent cleanup review and cloud status

Both non-author cleanup-delta reviews completed against stopped source `c5cd4ffeceda9665dace2602f1d2429fbf9849b1`, with **no actionable findings**:

- [Application/UI cleanup review](review-final-cleanup-ui.md): 187 scoped files, including both retained P2 fixes, production rendering/authority/measurement paths, manifests and exact fixture-allowlist removals.
- [Backend/Cargo cleanup review](review-final-cleanup-backend.md): six retained capability additions, original upstream tests/dev dependencies, native editing/reading/scroll/rendering invariants and the exact 14-package lockfile removal. No new package identity or resolved edge.

The [final backend provenance](nsh-authorized-backend-patches.md#final-post-cleanup-source-provenance-supersedes-preceding-current-state-records) records 231 files, 13 changed originals plus six production additions, zero missing originals, and no retired development feature occurrences. Its current JSON manifest is `nsh-backend-current-manifest.json`; combined tree SHA-256 is `fb8603eb20796b571b3a38865904f22e052649b495ecadbfc4443ae068e74998`. These are static/provenance results, not native acceptance.

The PR branch was pushed and remotely read back at `c5cd4ffeceda9665dace2602f1d2429fbf9849b1`. [Source-gate run 42730844715](https://github.com/puzige/vega/actions/runs/42730844715) completed successfully at `2026-11-10T00:53:40Z`, verified by query at `00:55:38Z`:

| Cloud job | Job ID | Result |
|---|---:|---|
| Clippy, including fmt and deny-warnings workspace/all-targets Clippy | 129548123287 | success |
| Nextest, including all three Python groups | 129548122828 | success; Python 5/15/40; **no Rust tests (0 discovered, 0 executed)** |
| required `check (fmt, clippy, test)` | 129551187474 | success |

The actual raw Nextest log records pinned `cargo-nextest 0.9.146`, rustc 1.98.0, and the zero-execution summary. This is not a Rust behavior-suite pass. The older run 42408616787 covers only `0b5ea426abfd8d4358489e916273d89474fb190f` and is not reused.

The documentation-only evidence commit containing this ledger will have a new HEAD. Its exact-head required CI must be checked separately after push; the source-gate result above does not pre-certify that new commit. Source reviews remain bound to unchanged production bytes. Final cloud status is recorded in the PR/Issue with the complete final commit and run URL, avoiding a self-referential documentation-only commit loop. CI queries remain at least two minutes apart absent a completion notification.

## Native acceptance and delivery boundary

See `nsh-native-acceptance-blockers.md` for all nine existing native acceptance scenarios.

- macOS ARM64 is available, but the full native OS/IME/clipboard/AX/graphics and actual production-window matrix has not been executed. The documentation agent's prohibition on app launching was a delegated-task restriction, **not** a user-wide prohibition. No completed native acceptance is claimed.
- No authorized available Windows GUI execution route is present in this tool/context. A Windows graphics session, native build/artifact and acceptance operator are required.
- No authorized available Linux GUI execution route is present in this tool/context. X11/Wayland graphics sessions, native build/artifact and acceptance operator are required.
- A macOS CI test job or Ubuntu check aggregator does not validate Windows/Linux production UI. Cross-compilation would not satisfy native behavior either.
- The pre-existing lack of OS HTML clipboard acquisition is not classified as a new migration regression or silently added implementation requirement.

All three platform native acceptance columns remain incomplete. User sign-off remains incomplete. No merge, release, packaging, installation, Issue closure or Done transition has occurred. Do not merge this task as a fully accepted migration while required native evidence is missing.
