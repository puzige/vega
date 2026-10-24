# #514 Rust test deletion evidence — READY 514-delete-v1

## Baseline and prerequisite

- Worktree: `/Users/puzige/Workspace/worktrees/vega-514-continuous-quotes`; branch `feat/514-continuous-quotes`.
- Exact baseline HEAD: `849fd67ae0c1524bba18bf6f2010484c1a3b1f44`; integrated master `83fd51d79940516029cdc1eaf8e81566870c877b`. Parent fetched/updated before delegation. Initial dirty path was only parent-owned `docs/514/vega-issue-514-continuation-evidence.md`; this writer never altered it.
- Source/lock patch SHA256, command `git diff --binary HEAD -- Cargo.lock crates`: `540e75e4e04ed116ab15a56ec0e7b57b690d2e4a134dc166133d9a3e5be3e082`. This binds the 12-file deletion scope, excluding this report and parent-owned documentation. Report SHA256 is provided separately at handoff to avoid a self-hash cycle.
- Root AGENTS, execution guide/workflow, Rust delivery lifecycle, #514 specification and relevant report evidence inspected. No nested AGENTS files found.
- No Git/index mutation, commit, push, fetch or CI query by this writer. No local Vega execution, compiler, build, Clippy, packaging, Rust test or Python execution. No acceptance changes, new tests, assertion weakening, unrelated fixes or production behavior changes.

The user delegation and parent continuation report record exact-head GREEN [run 39792089235](https://github.com/puzige/vega/actions/runs/39792089235): **38 discovered / 38 executed / 38 passed / 0 failed / 0 skipped**. Nextest job `119870428987` (14:01:21–14:08:13Z, test execution 1.181s), fmt/Clippy `119870428092`, Python 5+15+40 = 60, and required job `119873746804` all succeeded. Parent retrieved complete success at 2026-10-22 14:22:37Z. These are parent-verified cloud results, not an independent writer CI query.

Original test-first history, full SHA and source/evidence are fixed at [PR #529 comment 6343126549](https://github.com/puzige/vega/pull/529#issuecomment-6343126549). All 38 test sources remain accessible in the exact baseline commit. Earlier #514 reports retain RED/implementation history; this removal does not rewrite or independently re-certify that history. Prior UI and current integration independent approvals were supplied by the parent. This is not independent approval of the deletion.

## Deletion inventory: 38 to static 0

| Location | Removed tests |
| --- | ---: |
| `crates/vega_markdown/src/stream_boundary_tests.rs` | 12 |
| `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs` | 22 |
| `crates/vega_ui/src/conversation_stream/list_marker_behavior_tests.rs` | 3 |
| `thinking_body.rs::quote_lifecycle_tests` | 1 |
| Total | 38 |

Removed the three test files, embedded lifecycle module, `quote_test_body` helper/re-export, and cfg(test) wiring in markdown lib and conversation_stream mod. Removed only exclusive UI observation hooks in inline_code, selection, render_rows and thinking_body; removed gpui_kit test-support dev dependency and exclusive lock closure.

Static audit covered all 429 remaining tracked Rust files (432 before), all 13 workspace members including xtask, and vendor. Test attributes, test cfg, test-support features, dev-dependency sections and explicit test targets have no remaining matches in repository Rust/manifests. This is **static 0, not cloud discovery/execution**. Documentation scan found only the explicit `text` fence in markdown `stream.rs:6–14`; no executable Rust doctest found. Existing vendor doctest=false settings retained.

A broad helper search still finds vendor/mdstream `observe_text` in reference-definition parsing. It is production code, not UI test observation, and remains unchanged. MockReplay/split_deltas, benchmark/probe instrumentation, real IDs, checks/assertions, fixtures, Python and generic CI/test-runner capability remain intact.

## Production cfg projection equivalence

For every surviving edited Rust file, derive the baseline non-test projection by dropping actual cfg(test)-guarded fields/items/statements/arguments and removing cfg(not(test)) attributes. Format the projection with rustfmt via stdin and compare it byte-for-byte with the final file. All seven comparisons passed. This is a focused static source projection for known edits, not a general Rust parser or runtime test.

- lib.rs, mod.rs, thinking.rs: test wiring/re-export only.
- inline_code.rs: exact former production `text.into_any_element()` path retained; production shaping, hitboxes, text runs and links untouched.
- selection.rs: only test `observed_runs` field/locals/arguments and observations removed. Actual append parameters match the former production cfg projection. StyledText, paint_runs, real selection runs, logical/content ranges, IDs/order, layout registrations, link ownership and interactions are unchanged.
- render_rows.rs: retain exact former non-test marker and checkbox expressions. Two redundant nested block-expression wrappers left by cfg removal were stripped before comparison. Text, indentation, controls and layout unchanged.
- thinking_body.rs: remove test model injection, helper and lifecycle module only. Real sync/preparation/render/selection path unchanged.

Diagnostic command: `ruby -EUTF-8 /tmp/514-static-audit.rb`. Script SHA256 `d96f9d8bc2ebe5bc2f10c5e911f7f3c8ea228867516aae597c0c65398fed9d78`. Temporary diagnostics are not the durable source of original tests: baseline SHA and public PR evidence above are.

## Lock package and edge identity

Offline Cargo metadata updated the lock; final offline locked metadata succeeded without compilation/network. Compare exact `(name, version, source)` package keys and checksums, resolving dependency strings to package identities before comparing edges. Version qualification disappearing after deduplication is not a package upgrade.

- Packages: **924 → 914**, zero additions, zero retained identity/checksum changes.
- Edges: **2908 → 2882**, zero additions, 26 removals.
- Only edges removed from retained packages: gpui-pre 0.3.4 → backtrace 0.3.76 and proptest 1.11.0. Backtrace itself remains for other consumers. Other 24 removed edges originate in removed packages.
- Removed exclusive packages: bit-set 0.8.0, bit-vec 0.8.0, convert_case 0.11.0, proptest 1.11.0, proptest-macro 0.5.0, quick-error 1.2.3, rand_xorshift 0.4.0, rusty-fork 0.3.1, unarray 0.1.4, wait-timeout 0.2.1.
- Cargo-shortened bit-set/bit-vec/convert_case/quick-error dependency strings still resolve to the same retained identities.
- gpui-kit resolved features: assets,component. gpui-pre: default,font-kit,wayland,windows-manifest,x11. Neither has test-support enabled. Facade test-support forwards to GPUI/platform/base/component; gpui-pre test-support includes proptest and leak-detection.
- **mdstream remains 0.3.0**, source null/local manifest `vendor/mdstream/Cargo.toml`. Root Cargo.toml and entire vendor tree match baseline bytes; mdstream and block path patches preserved.

## Protected paths

Captured all tracked path hashes before source edits, hashing symlink target bytes rather than dereferencing directory links. **1,091 tracked paths** outside the 12-file source/lock scope and parent-owned continuation file compare equal, including root manifests/config, xtask, vendor, workflows, Python, fixtures, migrations, resources, benchmarks and release/packaging.

Sorted protected `SHA256  path` inventory digest: `f85804142c232477ee3477b693aaf40b7b0da0679331506910d8e168ebe0f9d1`. Parent-owned continuation capture/current hash also matched: `fcbd5845a56d3b863cd1ef1b7edbfb4cdf1aa84b256b47cfdd268612b75dc479`; it was never written by this writer and may continue changing under parent ownership.

## Commands, UTC, elapsed and outcomes

All dates 2026-10-22 UTC. Exact elapsed was not captured for exploratory reads/searches and initial scoped edits; none are claimed as runtime validation.

| Command/operation | UTC | Elapsed | Exit/result |
| --- | --- | --- | --- |
| Baseline hash capture via Ruby | before 14:29 | not captured | First exit 1: symlink directory EISDIR; corrected to hash link target. Next capture succeeded but inventory printing exited 1 on US-ASCII; UTF-8 retry succeeded. No source mutation in failures. |
| Scoped test/support deletion via Ruby | before 14:29:16 | not captured | 0 |
| `cargo metadata --offline --format-version 1` | 14:29:16 | 0.95s | 0 |
| `cargo fmt --all` | 14:29:36 | 1.70s | 0 |
| Lock identity/edge static Ruby analysis | between 14:29 and 14:31 | not captured | 0 |
| First projection audit | 14:31:35 | 0.22s | 1: diagnostic parser stopped multiline call at argument comma. Corrected harness to track parentheses; no production change or weakened equality assertion. |
| Projection/protected/static audit | 14:31:50 | 0.58s | 0; seven byte-equal projections |
| `cargo fmt --all -- --check` | 14:32:17–14:32:19 batch | 1.70s | 0 |
| `cargo metadata --offline --locked --format-version 1` | same batch | 0.63s | 0 |
| `git diff --check` | same batch | 0.01s | 0 |
| Test/support rg scan | before report | not captured | 1 = no matches |
| Broad observation/helper rg scan | before report | not captured | 0 = two production vendor observe_text occurrences, retained |
| Initial report-generation Ruby command | after source validation | not captured | 1: source encoding parse failure before report write; report then written directly |

An edit-tool historical observation reported file_not_read. Current render_rows was read before successful targeted brace removal; read-before-edit was not bypassed. Broad documentation output was narrowed to doctest candidates rather than treating all comments as tests.

## Pending and stopped boundary

No unexpected production issue identified; specification deviation: none. Writer stops at READY with exact source/lock patch identity above. Parent owns separate deletion commit/integration; independent reviewer must review this stopped version. Final committed head still requires existing complete cloud gate: fmt, all-target Clippy, Python, Rust discovery/build with **0 discovered / 0 executed, no Rust tests**. These post-deletion cloud results are **pending, not passed**. Empty-set discovery does not substitute for pre-deletion 38-test GREEN. Formal PR, merge and user manual acceptance remain parent-owned workflow.

## Exact baseline/final file SHA256 inventory

| Path | Baseline | Final |
| --- | --- | --- |
| `Cargo.lock` | `d7c701e424bae4802ca725b937899e3875d0d0293b7d3be5558ff707849ab888` | `71388bbb23bdeddcf884a02731f1190f22f32c889521ce431f933a834d12ce9d` |
| `crates/vega_markdown/src/lib.rs` | `a07bc52bc9927491251a0ee6381c011b481725b3d24f3b0d050f7d124b6fe5b9` | `99b01a98171a3660eb9101411b447c94b7c5ee2d3566f889f7a9bf226bedc2aa` |
| `crates/vega_markdown/src/stream_boundary_tests.rs` | `58af94904c5a86dde60992627dc4d0749adbacc13e70f6bf9fe5fcc15a83a837` | DELETED |
| `crates/vega_ui/Cargo.toml` | `3f27cebbfc927e48576b21a78e2684c05ff4da14c2a70cb56e7851cd175edde5` | `45f54ccb5909337fc1e31329bf85aaa254a829225e3e952d44ffa3fc5950787d` |
| `crates/vega_ui/src/conversation_stream/inline_code.rs` | `79c1ee767cc41f30d4c12dea63acce076e181a9216d4b2036e344d3370bd5a62` | `0599cb9e18f4cabcbca3bcc8e97f26fa37aa84b3f43514999a6cdb650bbbf723` |
| `crates/vega_ui/src/conversation_stream/list_marker_behavior_tests.rs` | `dc9938517dae1a9df334791939324c58ec7fead3850ac1dd1da9969395f73e72` | DELETED |
| `crates/vega_ui/src/conversation_stream/mod.rs` | `88c9f0b7fbd3c1f746e94c49ec1a9de8cbc681c4f8b79890bb307fffed7a88c4` | `5774e655493845fa154d8e81bdccbceeaa52807cf81406199ddf0ffad9e8d278` |
| `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs` | `f314d6828b845abadf08e960f069f84982d46873bd89bb559f177441c320e726` | DELETED |
| `crates/vega_ui/src/conversation_stream/render_rows.rs` | `eb1ac7f33231dd54fd149ef613eea45f9db71ed7f4d338c1c7adc9dd7ad395f6` | `eb410c95cc502df861810068f736cc54a091c24d39e611e981032683fc30c5a3` |
| `crates/vega_ui/src/conversation_stream/selection.rs` | `243d067beb12db799a0b0656d8c6a481818252f95b1d83a916cca1395977844a` | `e57e59b17307303b6a754a0841a598bc3ba8d3441a7d99fdf87fbb137067a2e7` |
| `crates/vega_ui/src/conversation_stream/thinking.rs` | `3e77613194495f21a473254e24138193629befca506f967b914ff9dbf0918680` | `8e9711de6bd2189d1b1d5e42252dc7c0a0f9c15e16bec6d75adf01c58b8a954c` |
| `crates/vega_ui/src/conversation_stream/thinking_body.rs` | `985ae28b1ac4b51a97a2ccac9d30e0bfbed5a4e06b0bf884b1f9eb5d2c663e0c` | `82e017785db0f4c19b8d1809287d7def818ef925a94c471c650e87004f628b10` |

## Original test name inventory

### `crates/vega_markdown/src/stream_boundary_tests.rs` (12)

- `list_fences_keep_following_prose_outside_code_across_chunk_schedules`
- `list_fences_preserve_internal_and_terminal_blank_code_lines_verbatim`
- `top_level_fences_after_lists_remain_separate_blocks`
- `list_fence_snapshots_keep_unrelated_committed_nodes_frozen`
- `late_reference_definitions_update_only_the_affected_committed_block`
- `dedented_fences_after_wide_ordered_markers_keep_heading_syntax_as_code`
- `ordered_list_fence_continuations_preserve_code_blank_lines`
- `list_fence_bodies_keep_block_syntax_literal_in_pending_and_final_snapshots`
- `pending_prose_repair_keeps_backtick_fences_on_their_original_line`
- `pending_prose_repair_keeps_tilde_fences_on_their_original_line`
- `closed_strikethrough_before_list_fences_preserves_code_and_real_blank_lines`
- `quote_first_list_continuations_preserve_owned_quote_paragraphs_across_chunks`

### `crates/vega_ui/src/conversation_stream/quote_behavior_tests.rs` (22)

- `paragraph_rails_cover_preserved_gaps`
- `independent_quotes_do_not_decorate_intervening_body`
- `nested_siblings_share_only_the_outer_rail`
- `wrapping_and_resize_use_remaining_quote_width`
- `streamed_intermediate_and_finished_frames_match_fresh_projection`
- `mixed_quote_children_keep_content_layout_and_copy_separators`
- `quote_links_request_original_destination`
- `thinking_rails_survive_offscreen_first_paragraph_and_gap_only_mask`
- `thinking_first_paragraph_is_beyond_culling_overscan`
- `soft_wrapped_unicode_copy_has_only_source_paragraph_breaks`
- `quoted_code_horizontal_scroll_moves_code_not_quote_or_paragraph`
- `unquoted_body_and_code_have_no_quote_rails`
- `sibling_quotes_in_one_stream_block_remain_distinct`
- `mixed_children_preserve_ordinary_layout`
- `unicode_nested_selection_tracks_append_and_resize`
- `list_owned_quote_rails_start_at_the_list_content_column`
- `nested_list_owned_quote_rails_keep_both_list_insets`
- `outer_quote_rails_do_not_follow_descendant_list_insets`
- `inner_list_owned_quote_rails_preserve_the_outer_quote_origin`
- `ordinary_quote_first_items_keep_markers_outside_the_quote`
- `selectable_quote_first_items_keep_marker_order_and_content_columns`
- `thinking_quote_first_items_keep_marker_order_and_content_columns`

### `crates/vega_ui/src/conversation_stream/list_marker_behavior_tests.rs` (3)

- `heading_first_list_items_keep_visible_marker_and_heading_style`
- `code_first_list_items_keep_marker_physical_lines_and_horizontal_scroll`
- `nonparagraph_first_list_items_copy_markers_in_visible_order`

### `crates/vega_ui/src/conversation_stream/thinking_body.rs` (1)

- `thinking_publication_retains_source_anchored_selection_and_rejects_stale_layout`
