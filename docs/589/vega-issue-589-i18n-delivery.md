# Issue #589 — i18n implementation and verification record

- Contract: [frozen en / zh-CN specification](vega-issue-589-i18n-en-zh.md), including the issue clarification that sidebar headings are translated, not renamed.
- Starting baseline: `420a0eb9a06594173b658f7e3a4437e3d2b5432d`.
- Branch: `feat/589-i18n`.
- Latest integrated upstream: `429c6a819fc5a30f8280afd2f6ed015d4682bf41`.
- Recoverable pre-integration checkpoint: `bd356e2107b3d5a64e2ef6d28e096e296a98994c`.
- Integrated checkpoint: `a052384b4c7a7be7aa95ece22660f6ec7c129daa`.
- Stage: upstream integration complete; focused repairs and independent reviews in progress. Not yet CI-validated, merged, or user-accepted.

## Implementation boundaries

The shared translation layer is a headless workspace crate, independent of GPUI. It embeds both languages using `include_str!`, parses catalogs once, defaults to English, and exposes key lookup plus named-argument interpolation. Catalogs are split into settings/general, UI, application, and domain pairs so each migration has one writer. They form one logical catalog per language; duplicate keys, missing translations, and parameter mismatches must be checked across all pairs.

Language preference changes must use the existing serialized, conflict-aware preference save path. A failed save must not report success or discard unrelated preferences. Existing configurations without a language field must retain other preferences and default to English.

Language changes must update existing views, not only newly created windows. Review includes retained placeholders, accessibility names, generated status labels, and cached UI text. User input, model responses, command output, paths, model/provider identities, and historical business content are not translated. Sidebar identities and all existing layout/theme tokens remain unchanged.

## Risk-based verification

| Contract | Verification route | Current result |
|---|---|---|
| A01: English default and sidebar headings | Static resource/call-site review; fresh-config user walk-through | Pending |
| A02/A03: switch both ways immediately and persist | Preference save/startup call-chain review; user walk-through including already-open inputs and views | Pending |
| A04: old configuration compatibility | Serde default and preference-preservation review; user old-config check | Pending |
| A05: named interpolation | Catalog/argument static validation; implementation review; user reconnect/preview/count checks | Pending |
| A06: UI and error coverage | Per-literal static inventory with explicit non-UI classifications; bilingual surface review | Pending |
| A07: accessibility | Translated-name and keyboard-handler review; user keyboard/reader check | Pending |
| Rust compile/lint and repository checks | Latest-head GitHub Actions `pr-check` | First checkpoint failed compilation; second compiled but Clippy blocked on one lazy-evaluation warning; final head pending |
| Final Rust test inventory | Static inventory plus CI discovery/execution counts | Second cloud cycle: no Rust tests (0 discovered, 0 executed); final-head inventory still required |

No Vega code, builds, compilation, packaging, or tests are executed locally. Local checks are limited to quick formatting, diffs, and static source/resource analysis. Static checks do not establish native rendering, persistence across real restarts, screen-reader behavior, or complete UI acceptance. No Rust test is added solely for this migration's process; final required CI still applies.

## Evidence

### Initial scoped deliveries and review

| Scope | Frozen content manifest SHA-256 | Initial static evidence | Independent review |
|---|---|---|---|
| Foundation/settings | `aa13c0855341d45c017343d3453ecfadb075ed556c8719dc949360e8f42b9849` | 505 key pairs; rustfmt and resource/argument audits | In progress |
| Non-settings UI | `cd8f3d0717bfe067b2c4a19bd9da504c8e863f6b19330442b619ef2076e7e890` | 670 key pairs; 61 changed Rust files formatted; sample preservation audit | Changes requested: two static type mismatches and retained presentation gaps |
| Application | `079ec78e39164ae207e64b689d47bd0cec013cc73b24a44f65f683c078566da3` | 253 key pairs; 32 changed Rust files formatted; remaining diagnostics traced | Changes requested: domain-error refresh, inactive image statuses, Chinese composite labels |
| Domain/inventory | `1411b64d06794ac62bddb32017ab09a22f856b6edf03c0c255e2c47b92ba1fd3` | 292 key pairs; scoped formatting and catalog checks | Changes requested: privacy snapshot equality, generated-notice provenance, model-facing label reuse |

The parent ran `cargo fmt --all -- --check` successfully on the combined initial migration. This was a formatting check only, not compilation. The full static inventory initially exited 1 for unclassified remaining literals; it did not establish coverage. Later domain repairs classified swallowed updater/helper diagnostics individually, leaving a credential-placeholder comparison requiring a real source-level repair rather than suppression.

The first domain repair was frozen under content-manifest SHA-256 `5b26e8cf632a9666575fea2a2abbf293cea1cd65466f008e8034927620c9cdcf`: locale-independent privacy equality retains all stable security fields and existing guards; model-facing browser description and replay catalog identity keep baseline canonical bytes. Focused independent review resolved these findings statically; runtime evidence remains pending.

Generated-notice separation is specified in §3.1 before implementation. New provenance is additive; old messages and arbitrary model content are not inferred or translated by text matching.

### Integrated checkpoint and follow-up

`a052384b4c7a7be7aa95ece22660f6ec7c129daa` incorporates the four upstream merges through `429c6a819fc5a30f8280afd2f6ed015d4682bf41`, including native thread forking, compaction-row alignment, artifact interactions and outside-click rename cancellation. The generated-notice migration is now `0036_generated_notices.sql`, after unchanged upstream `0035_thread_fork.sql`.

At this checkpoint the parent ran `cargo fmt --all -- --check`, `git diff --check`, and `python3 scripts/check_i18n.py --json`: formatting/diff checks succeeded; the static inventory reported 1,750 keys per language, zero catalog/reference errors and zero UI/unreviewed Chinese literals. These checks do not compile Vega or execute tests.

Independent reviews approved the repaired application boundary and domain cleanup/sidebar-error deltas at their frozen content versions. UI notice display/copy review found no additional notice-projection defect but identified a service-status equality type mismatch and the remaining Codex provenance gap. The equality repair uses a service-specific comparable failure type, preserving the status-change comparison rather than comparing translated prose. Codex metadata is specified before implementation; final integration and independent review remain pending. The upstream integration and equality-repair reviews subsequently found no new P1/P2 defect within their frozen deltas. Foundation review identified and verified fixes for the removed credential-placeholder reference and known Skill diagnostic codes; final aggregate and cloud gates remain open.

### First cloud cycle

- Draft PR: [#614](https://github.com/puzige/vega/pull/614).
- Checked head: `b26622f67d2319923f6bafc4636c8296b22ec851`.
- Run: [40172382332](https://github.com/puzige/vega/actions/runs/40172382332).
- Format and the existing Python script-check step succeeded. Clippy and Nextest's workspace discovery build failed to compile `vega_ui`; the required aggregate check failed. Rust discovery/execution counts were not obtained, so this is not a Rust test pass.
- Original failed-log SHA-256: `7cecaf287f8ad891b32742518b0d58786a17c7f39f86ce22594957bd2a517c85`.
- Product wiring causes: missing `SettingsText`/`t`/`t_args` imports in the MCP settings module; the removed placeholder constant; a sibling module calling a private scroll-restoration helper; four `String` values passed where interpolation expects borrowed strings. These are not external CI failures or acceptance changes.
- Corrections preserve behavior: imports only, existing placeholder helper, module-scoped helper visibility and temporary borrows. No assertion, gate, test, runtime policy or acceptance criterion was weakened. Corrected-head cloud results remain pending.

### Second cloud cycle

- Checked head: `36b7479f0e80db2b2b597c8aeecc43c4d34eddc6`.
- Run: [40173882170](https://github.com/puzige/vega/actions/runs/40173882170).
- Nextest workspace compilation/discovery completed successfully: **no Rust tests (0 discovered, 0 executed)**. This is an empty suite, not a claim of behavioral Rust test coverage.
- The required aggregate check still failed because Clippy rejected one `unnecessary_lazy_evaluations` instance in `sidebar_organization/mutations.rs:38`. Original failed-log SHA-256: `16c593a12b153b8e5e47cebe95f7be95acafa7a2990d6cdaaf9c97f5003632ae`.
- The correction uses `ok_or` with the identical stable error value; no warning suppression, weakened assertion or CI change. Final-head checks remain pending.

### Final Codex coverage batch

The remaining Codex-generated parameter descriptions and terminal reasons now carry typed provenance through live calls/results and persisted history. Migration `0037_codex_display_metadata.sql` adds a strict, cascading tool-call metadata table. Insert/reject/finish operations save metadata and the existing lifecycle transition in one transaction. Rendering localizes only when the explicit metadata matches the canonical preview/output and status; legacy, mismatched and external content remains unchanged. Canonical four-field input JSON, permission targets, argument length/digest and original output bytes remain stable.

The implementation froze under manifest SHA-256 `5ff7269dd22f923603361af8e03cb6bb640289eeb79c07035d16784b3adc6f61` and delta SHA-256 `4206131f069072a84c07518e736fa01c2c4a24c4f2942995bccc9115bc19a1fd`. The parent reran whole-workspace formatting, diff hygiene, the static i18n inventory and the no-Rust-test-source scan. Current static inventory: 1,764 keys per language, 1,740 literal translation calls, zero resource/reference errors and zero UI/unreviewed Chinese literals; benchmark bytes match the original hash. Independent Codex review subsequently verified these exact 14 file identities in commit `0d1f7cce7aa9b967fdb7bdd11d321a032c3b8e8d`, with no P1/P2 finding; newly created records close the required Codex coverage gap. Final-head CI remains pending.

An attempted combined amend/force-push command was denied before execution. No history rewrite occurred; subsequent delivery uses ordinary additional commits and pushes rather than retrying the denied operation. After the third cloud cycle exposed remaining UI lints, the user explicitly authorized necessary additional repair commits beyond the repository's default three-commit limit; final integration still uses squash merge.

### Third cloud cycle

- Checked head: `0d1f7cce7aa9b967fdb7bdd11d321a032c3b8e8d`.
- Run: [40176777910](https://github.com/puzige/vega/actions/runs/40176777910).
- Nextest workspace compilation/discovery succeeded again: **no Rust tests (0 discovered, 0 executed)**.
- Clippy progressed through the domain layer and reported fourteen downstream UI lint sites: unnecessary string allocations/borrows and one collapsible conditional. Original failed-log SHA-256: `f4def64266a761cfa8fd6c4ab0befe745f754afc269c119e2496d91bee7d02c4`.
- Corrections are limited to the reported expressions; translated keys, parameters, conditions, ordering and behavior stay unchanged. No lint allows, assertion changes or CI modifications are used. The next required cloud cycle must validate the corrected exact head.

The intermediate content identities above are not final-version approvals. Final versions, current static commands, independent delta review, and cloud run/job results will be recorded on the PR when available. Unknown or unrun checks remain pending; CI success and merging do not imply user acceptance.
