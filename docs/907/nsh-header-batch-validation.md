# NSH header, stage, attachment and Composer batch — parent validation

## Version and scope

Development baseline is `4148291cc5b377362d0ac64aa88b285cc9e06532`. This record covers the stopped ShellChrome, stage/shimmer, Thinking header, attachment presentation and Composer action delta, including the first corrective review round, published as `0b5ea426abfd8d4358489e916273d89474fb190f`. It is not a full-migration completion record. The parent fetched origin again and confirmed current `origin/master` `34555cec9c0c4c4c1f7a528d739752f414e53edf` is already an ancestor of the development branch. No rebase or dependency update was needed. `git diff --exit-code HEAD -- Cargo.toml Cargo.lock crates/vega_ui/Cargo.toml` passed after the withdrawn test-support experiment.

The baseline's cloud run [42401257979](https://github.com/puzige/vega/actions/runs/42401257979) succeeded. The newly published exact head `0b5ea426abfd8d4358489e916273d89474fb190f` then passed [run 42408616787](https://github.com/puzige/vega/actions/runs/42408616787): Nextest, Clippy and required `check (fmt, clippy, test)` all completed successfully. This does not cover subsequent uncommitted work. The PR remains Draft, the Issue remains open/In progress, and no merge, installation, packaging or native acceptance is claimed.

## Actual local verification

Tools observed: rustc `1.98.0 (88d9e12ae 2026-08-18)`; Cargo `1.101.0 (e611c7a87 2026-11-04)` / huacnlee `0.102.0-9`; Nextest `0.9.151`, host `aarch64-apple-darwin`. No toolchain or machine configuration change was made.

- `cargo check -p vega --locked --offline -j 2`: passed during integration.
- `cargo fmt --all -- --check`: passed on the stopped final corrective tree and was checked again after review recovery.
- `git diff --check`: passed on the same tree.
- `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings`: passed; latest original log `/private/tmp/vega-907-clippy-finalbatch.log` ends with completion in 4.76s. Existing unused-manifest-dependency warnings remain visible and were not suppressed.
- `cargo nextest run --workspace --locked --offline -j 2`: run `cb12b46b-ba67-4897-87bf-1d1c399dbd4e`, 16 binaries, **203 discovered/203 executed/203 passed/0 skipped**, execution summary 0.340s; test-profile build 20.60s. Original log `/private/tmp/vega-907-nextest-finalbatch.log`.
- Preserved Python groups `python3 scripts/release_tests.py`, `python3 scripts/prepare_computer_use_tests.py` and `python3 -m unittest discover -s scripts/tests -v` passed during this batch with 5, 15 and 40 executed respectively. These do not test native UI interaction.

The 203 total consists of the prior 175 and 28 new batch regressions: stage/shimmer 5, Thinking 4, attachments 6, Composer actions 10, ShellChrome 3. Tests mount the production NSH renderer where applicable and test current authority decisions, not the operating system's focus tree, compositor or IME. They remain development-only and must be archived as recoverable source/patch with exact committed identity, then removed with their exclusive support before master; final discovery must be 0/0 rather than called a Rust-test pass.

## Failure attribution and integration repairs

- Initial full run `d7e1b42a-23ad-49d4-a932-4d694d42ad90` executed 193: 192 passed, one attachment observer failure, zero skipped. The observer incorrectly expected an ordinary Frame for synthetic NSH `ROOT_ID`; traversal now stops at that documented synthetic root. Original thumbnail coordinate, 72px size, wrapping and alignment assertions were retained. The targeted regression then passed, and all attachment tests passed in the 203-test run.
- A temporary Thinking test-support manifest experiment needed 14 additional transitive packages during offline resolution. It was withdrawn rather than changing the dependency contract. The regression instead uses the production owner/session and real selection state boundaries with the existing harness. The lockfile/package set and manifests match the committed baseline.
- Intermediate compiler diagnostics during active multi-file writes were resolved on the completed construction, not counted as successful verification. Strict lint corrections removed the now-unused private `shimmer::text` alias and unused import, used an equivalent `.then_some` expression, and named ShellChrome geometry tuple types instead of adding lint allowances. Current production `stage_text` and `StageText` APIs remain.

## Independent review status

- Thinking retained-card resume P1: independently closed statically. Trusted parent calls occur before both production RunActivity row variants; old owner sessions and selection are rejected, current cards resume without adopting unrelated ownership. Body/scroll migration is still incomplete.
- Composer four P2s: independently closed statically on the current corrected files, including `native.rs` SHA-256 `a8c80ee858962677799f38936456f809eecfdf0044fbfc7fc3ab9379a13938a8`. The review checked input-highlight reveal and display-fenced Enter, canceled focus intent, descendant keyboard handling, and settled oversized-row reveal. It did not execute tests or native interaction.
- ShellChrome inactive measurement and duplicate AX correction: independently closed statically on all four supplied file hashes, including current window wiring. Measurement does not require activation; business actions still do. Pinned AX traversal skips the hidden mirror subtree. No new incremental finding. Earlier interrupted executions yielded no valid conclusion and were not counted as passes.
- Attachment parent-Surface failure correction: independently closed statically on the recorded removal hashes. Current watch state is checked synchronously before business removal, independent of watcher consumption. No new finding; tests exercise the production callback/predicate, not the full native controller path.
- Passive stage/shimmer empty-mask input isolation and transparent Surface correction: independently closed statically. Reviewer traced ordinary and retained hit-testing, prepaint versus paint mask lifetimes, actual text paint, shared Tab state and AX traversal against 17 cached pinned dependency files verified against lockfile archives. No new P1/P2 finding. Shared `Surface::new` retains its default opaque behavior; native interaction/fidelity is not established.

## Remaining acceptance

No native app was launched for this batch. Actual mouse-up/wheel/pinch passthrough, retained row clipping, Tab/Shift-Tab/Enter/Space/AX, wrapped/narrow geometry, surface-failure recovery, glyph/bidi/fallback-font fidelity, inactive/reactivated windows and macOS/Windows/Linux interaction remain unverified. Engine layout acknowledgements are not compositor paint evidence. Pinned native IME limitations remain unresolved under the explicit keep-dependencies decision; no fork, upgrade or visible legacy fallback is introduced or represented as completion. Thinking body, full message reading, the authoritative Composer input, full tool/timeline scroll trees and remaining sidebar/native chrome still require migration.
