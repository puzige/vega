# Issue #296 UI hierarchy v2 delivery

## Candidate 1 freeze

- Contract: [UI hierarchy v2](vega-issue-296-ui-hierarchy-v2.md), H1–H6 / V1–V8.
- Implementation: `a62f3e8e5b46438ca2ca86f1f8341c4af57dd2dd`, branch `feat/296-ui-hierarchy-v2`, based on merged #297 and #295.
- Integrated source manifest: 502 build inputs, SHA-256 `2734187461240d0b6267964904c394f7a76d83388157d7b590d2fff81bd43ef6`. Inputs were compared before and after the implementation commit and are identical; documentation is excluded from this manifest.
- Platform: Darwin arm64; Rust 1.98.0 / Cargo 1.98.0 / Git 2.55.0.
- Spec deviations: none. No new dependencies, model/permission changes, database migration or external-service test data.

## Implementation and review

Eight Settings routes share a fixed 224px navigation and a centered content column capped at 720px. Persistent field labels, adjacent controls, compact switches, semantic navigation icons and lighter actions preserve the existing input entities and controller guards. Release notes default closed, use cached native Markdown, and release scrolling to the parent at their boundaries.

Conversation roles follow actual user turns; process rows use one quiet surface without nested cards. Role geometry participates in history/anchor calculations, with structural caching instead of rescanning history per token. The ordinary anchor rail is quieter and the Composer loses its permanent shadow. #295's footer removal and New Task utility branch selection remain intact. Existing 120ms interaction feedback and reduced-motion behavior remain covered.

Independent review checked event consumption before the Settings Tab fallback, disabled and exactly-once switch actions, focus without geometry shifts, complete model identifiers, conditional nested scrolling and original authorization/save guards.

## Candidate 1 integrated verification

These are targeted in-process GPUI/application checks, not external provider or OS keyboard acceptance. No local full-workspace suite was run.


### ui

```sh
cargo nextest run -p vega_ui -E 'test(issue296) | test(=issue294_existing_native_composer_has_no_branch_entry) | test(issue294_hydrated_native_and_codex_footers_keep_controls_across_theme_and_layout) | test(issue294_new_task_utility_bar_preserves_branch_selection) | test(run_activity_entries_contribute_geometry_without_duplicating_message_anchors)' --no-fail-fast
```

UTC 2026-10-11T05:50:16.282784+00:00 to 2026-10-11T05:51:50.800550+00:00 (local UTC+08:00). Exit `0`; raw log `vega296-v2-integrated-ui.log`, SHA-256 `f3a866d71bac2a71070d4aade838a683ac4cbc5afa9ee021ea52d3aaa9014767`.

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 30s
     Summary [   2.420s] 48 tests run: 48 passed, 559 skipped
```


### app

```sh
cargo nextest run -p vega -p vega_ui -E 'test(=tests::issue72_loaded_anchor::issue72_loaded_alpha_rail_click_enters_actual_viewport_once) | test(=window::workspace::tests::r49_utility_bar_mounts_above_the_card_only_on_the_new_task_page) | test(=window::workspace::tests::r49_branch_chip_opens_and_closes_the_selector_from_the_composer) | test(=conversation_stream::tests::utility_bar::issue294_existing_native_composer_has_no_branch_entry)' --no-fail-fast
```

UTC 2026-10-11T05:52:02.346043+00:00 to 2026-10-11T05:53:41.535748+00:00 (local UTC+08:00). Exit `0`; raw log `vega296-v2-integrated-app.log`, SHA-256 `7be4d29e29bf4a5ec6aae1e29a3a1141fad44aba15240e069ca792fbc1d9f62b`.

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 58.03s
     Summary [  38.080s] 4 tests run: 4 passed, 847 skipped
```


The first command's bare exact #294 name selected no test. The second command explicitly includes its full name and passes it. The combined total is 49 UI tests and 3 application tests; no claim is made that the first command alone covered all #294 cases.


### fmt

```sh
cargo fmt --all -- --check
```

UTC 2026-10-11T05:51:38.929916+00:00 to 2026-10-11T05:51:41.797359+00:00; exit `0`. Raw log `vega296-v2-integrated-fmt.log`, SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

```text
(no output; exit 0)
```

### clippy

```sh
cargo clippy -p vega_ui -p vega_theme -p vega --all-targets -- -D warnings
```

UTC 2026-10-11T05:53:57.758432+00:00 to 2026-10-11T05:54:39.365946+00:00; exit `0`. Raw log `vega296-v2-integrated-clippy.log`, SHA-256 `20c3038b8c34ad85d11fff0f74790eb02bd89b6b8553db44291cfcb85d42887c`.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 41.52s
```

## Failure-first and slice evidence

- Shared: 2 initial RED assertions exposed the old English built-in section labels; final Sidebar 16/16 and Theme 3/3 passed.
- Stream: 4 initial RED tests exposed missing role/surface/rail geometry. A separate history replacement test reproduced a stale role-cache index panic before its fix. The affected regression run passed 134 of 135; its only failure was the superseded positive-height expectation for collapsed zero-height process segments. The spec explicitly corrects that geometry while preserving anchor identity/order assertions. Final focused rerun passed 9/9. This is not described as a single 135/135 green run.
- Settings: initial 2 RED tests exposed navigation inheriting main Sidebar width and vertically separated fields. Intermediate failures exposed real nested-scroll and root Tab handling defects; both were fixed. A temporary harness-only Tab fallback was rejected and removed. Final production-path run passed 27/27 with the original harness; affected-crate Clippy passed.

Raw local logs preserve the initial and intermediate failures. Native private content was never used as an automated fixture or included in public evidence.

## Native candidate and repair loop

Candidate 1 was built from the implementation above as 0.1.66 (the stable three-part version keeps updater parsing unchanged). This is a local candidate, not the formal release. Packaging completed with exit 0; bundle `ai.vega`, arm64, ad-hoc signature and archive/bundle binary equality were verified. Binary SHA-256: `bf8929b8fff9b6f13b59396f9f1019b920abed84163083372381eff8d9cdfd70`; ZIP SHA-256: `120d105bde1ddbf648abec5127b3c24aeb1e04553a27b4dbddaaf5a781badaf2`.

`cargo xtask package --version 0.1.66` ran from UTC 05:54:56 to 05:57:46 on 2026-10-11, exit 0; log SHA-256 `ebaffb27bc27a5c3761b82cd4863ce9a6aa1aba06a0c5fb5d34476709134cc05`. Verification at UTC 05:57:50 used `codesign --verify --deep --strict`, signature display, `plutil -lint`, `lipo -archs`, archive extraction and binary hashing. The previous official 0.1.66 binary SHA-256 was `0ca03dd4f65ab1129106e68b8cb06d63a56deb98a77e09b74afa6a7efa3cb046`; its verified backup is retained locally.

The main agent inspected the active application, used normal Quit, verified process exit, preserved the previous official installation as a ZIP with identity evidence, and replaced the canonical installation. The running process path and installed binary were checked before native review. No task messages, model/permission changes or credential edits were made; no force termination was needed.

Native review used the populated light-theme application at approximately 1404×860 and 962×602 logical pixels. It covered all eight Settings routes at the normal width, narrower form layouts, collapsed/expanded native Markdown release notes, real historical conversations, nested tool-output scrolling, role/process hierarchy, the message rail, and New Task versus existing-task branch entry placement. The window size was restored after the narrow-width check. GPUI tests separately cover both themes and focus/motion behavior; these native observations do not claim OS keyboard-focus verification.

The initial candidate failed five visual/reading checks: a long Usage cost overflowed its metric, the Pricing editor lacked the current model heading, the Agents argv field lacked its input boundary, Skills actions touched adjacent content, and changing Settings pages leaked the previous page's scroll offset. H3a was written before the corresponding repair work. Private screenshots and session details remain excluded from public artifacts.

## Candidate 2 verification and native acceptance

- Final implementation: `cc0edb80ab4042493e435c5ad0f72359b2223a93`.
- Source manifest: 502 build inputs, SHA-256 `f611f9220c0478e05bf9351adea3fa9d481b302138c86388c0a1bda8768e3c1e`; identical across targeted checks, commit amendment and packaging.
- H3a delta: 7 Settings files; independent review found no blocking defect. No stream/theme/application sources changed after candidate 1.

### ui

```sh
cargo nextest run -p vega_ui -E 'test(issue296_h3a) | test(issue296_settings_eight_pages_keep_one_readable_column) | test(issue296_skills_actions_are_compact_and_agents_fields_stay_labeled) | test(issue296_v2_populated_pricing_reasoning_and_usage_keep_complete_fields) | test(pricing_actions_are_tab_reachable_and_enter_space_activate_once) | test(issue296_v2_skill_switches_keep_real_mutations_and_busy_guard) | test(issue296_usage_refresh_and_filters_keep_keyboard_actions) | test(issue296_v2_notes_are_cached_markdown_collapsed_and_keyboard_reachable) | test(issue296_v2_notes_short_content_and_scroll_edges_release_parent) | test(issue296_provider_detail_and_editor_keep_narrow_scroll_ownership) | test(issue296_general_pickers_toggle_once_from_keyboard) | test(issue296_v2_updater_switch_keeps_geometry_focus_once_and_busy_guard)' --no-fail-fast
```

UTC 2026-10-11T06:12:46.767096+00:00 to 2026-10-11T06:13:06.159394+00:00; exit `0`. Log `vega296-v2-h3a-integrated-ui.log`, SHA-256 `df316bd588eb581d3ec6fc9deb7c9a4b87d7d17d6cd28d2a5d2f6507e219e417`.

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 15.45s
     Summary [   2.270s] 16 tests run: 16 passed, 596 skipped
```

### fmt

```sh
cargo fmt --all -- --check
```

UTC 2026-10-11T06:12:54.506826+00:00 to 2026-10-11T06:12:57.340901+00:00; exit `0`. Log `vega296-v2-h3a-integrated-fmt.log`, SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

```text
(no output; exit 0)
```

### clippy

```sh
cargo clippy -p vega_ui --all-targets -- -D warnings
```

UTC 2026-10-11T06:13:19.448677+00:00 to 2026-10-11T06:13:41.231064+00:00; exit `0`. Log `vega296-v2-h3a-integrated-clippy.log`, SHA-256 `ef5cd88b6eabeb9c3cfa0c54f475acf0daa5353fa57dac5d74e322b555caecde`.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.70s
```

### package

```sh
cargo xtask package --version 0.1.66
```

UTC 2026-10-11T06:13:47.234514+00:00 to 2026-10-11T06:14:31.981100+00:00; exit `0`. Log `vega296-v2-h3a-integrated-package.log`, SHA-256 `0ebde6b5f22200f035766257837619bb672b91fa5856f6dd910ba2cffd3dc61e`.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.43s
    Finished `release` profile [optimized] target(s) in 32.55s
```

The 16 tests comprise five H3a cases and eleven affected geometry, action/focus, switch, pricing, usage and nested-scroll checks. Prior 52-test evidence is kept separately rather than represented as a repeated final-source run.

Failure evidence: the initial H3a run failed four tests; Pricing, Agents and Skills failures corresponded to the observed defects. The initial Usage wrap requirement was too strict and its corrected TextLayout test also passed under a temporary old-layout negative check (1/1, log SHA-256 `53b4ece513560fd91251fd0af0db99b5aa7ff098398ff7bc308599a1602ba3e2`). Therefore the Usage test establishes synthetic full-text/layout constraints only; it is not claimed as automated reproduction of the original native overflow. The native before/after review below is the evidence for that specific fix. The final frozen source was restored with identical hash after the negative check.

The Settings route-scroll regression failed with a valid overflowing fixture: the first row inherited y=27px instead of the independently opened y=170px. After the repair, navigation and wheel events verified first-open top position and separate retained offsets; Provider and notes scroll ownership stayed green.

Candidate 2 verification at UTC 2026-10-11T06:14:41 passed strict/deep signature, bundle ID, version, arm64 architecture, ZIP checksum and bundle/archive binary equality. Binary SHA-256 `41d17e556670968c21e691b33f6ce7c740f84dbc63c7ca09ca4c4440596eb9ce`; ZIP SHA-256 `f4b82d3f7b657a96b3c4de3a0e739f20893d2422b388205912c73a2f2856573f`. The main agent normally quit the idle candidate, installed this verified build at the canonical path, restarted it explicitly and verified the running process and installed hash.

Native recheck passed all five H3a observations: the populated Usage cost remains complete, left-aligned with its own label and clear of adjacent metrics at normal and narrow widths; Pricing shows the edited model name with separation and Cancel preserves the unchanged draft; Agents argv has its neutral input boundary; Skills actions and source diagnostics have visible spacing; General-to-Reasoning navigation starts the new page at the top and returning restores General’s own position. Window dimensions were restored afterward. No raw private UI images or values are published.

The candidate remains version 0.1.66 for local acceptance; official cloud CI, merge and release installation are recorded separately in the PR/Issue after completion. User subjective acceptance is still pending.

## Cloud correction

First cloud run [38117663952](https://github.com/puzige/vega/actions/runs/38117663952), head `ea3f45800b2927fda4e44f776bad480df92c15da`: Clippy succeeded; Nextest ran 2141 tests with 2139 passed, 2 failed and 5 skipped. The failures were `i76_context_settings_real_inputs_persist_and_reopen` (persisted policy not observed after clicking Save) and `issue103_tool_group_is_bounded` (352px group versus an old 348px formula that omitted the frozen 4px gap). Required check failed; no merge or blind retry was performed. Dedicated slices investigate each cause. H5 now explicitly states the 28px header + 4px gap + 320px scroll body geometry; the corrected test must separately verify each part, retain the 320px/240px body/detail caps and preserve nested/outer scrolling assertions. The persistence timeout was a missed click: at the 1200×900 fixture, the Provider detail viewport ended at y=876px while the Save button center was y=889px. A model-editor-only helper now sends one calculated wheel event if needed, asserts the entire control lies inside the viewport, and clicks once. No timeout, storage assertion or production behavior changed; invalid-input coverage additionally verifies the visible error state.


Both original failures were reproduced before correction. The stream slice then passed six relevant bounds/scrolling tests; the application slice passed persistence/reopen, invalid-input and stale-ACK cases. The following combined run verified both patches after integration:

```sh
cargo nextest run -p vega -p vega_ui -E 'test(issue103_) | test(=tests::composer_actions::context_compaction::i76_context_settings_real_inputs_persist_and_reopen) | test(=tests::composer_actions::context_compaction::i76_context_invalid_input_never_persists_or_calls_provider) | test(=tests::composer_actions::context_compaction::i76_context_stale_settings_ack_retires_pending_without_applying_values)' --no-fail-fast
```

UTC 2026-10-11T06:34:04.075519+00:00 to 2026-10-11T06:34:27.987444+00:00; exit `0`. Log `vega296-v2-ci-integrated-tests.log`, SHA-256 `ca058960860b0682dec40b2d2dfae15df0020a4f2480200f9d81a09271ce82d9`.

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 19.39s
     Summary [   1.286s] 9 tests run: 9 passed, 847 skipped
```

```sh
cargo fmt --all -- --check
```

UTC 2026-10-11T06:34:08.758493+00:00 to 2026-10-11T06:34:11.571923+00:00; exit `0`. Log `vega296-v2-ci-integrated-fmt.log`, SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

```text
(no output; exit 0)
```

```sh
cargo clippy -p vega -p vega_ui --all-targets -- -D warnings
```

UTC 2026-10-11T06:35:04.029372+00:00 to 2026-10-11T06:35:08.983869+00:00; exit `0`. Log `vega296-v2-ci-integrated-clippy.log`, SHA-256 `61b173ec2e7ac59d5b638d23c2ccf1f30047a2c9511ab6101c860fbfd0a5adc3`.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.88s
```

Independent review confirmed the original persistence, invalid/stale-ACK and scrolling guards remain intact. Comparison against candidate 2’s 502-input manifest found exactly two changed test files; all production inputs and the installed candidate binary were unchanged. No new local package was needed for these test-only corrections; the formal release will still be installed and reviewed after the new cloud gate.

## Residuals

- Full workspace gate: cloud `pr-check`, pending.
- OS focus cannot be established through synthetic native key injection; production GPUI event/focus tests are the behavior evidence.
- The issue stays open for the user's subjective visual review; worktrees are retained. This record does not claim user acceptance.
