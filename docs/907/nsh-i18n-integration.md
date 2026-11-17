# NSH migration i18n classification integration

## Scope and author status

**Author READY — stopwriting after this handoff. Implementation lane, not independent review.**

Only `scripts/i18n_allowlist.json` and this document were written. Checker, tests, production sources, catalogs, dependencies and configuration were not modified. No Git, network, Cargo, native application or delegation operations were performed. The parent-supplied branch is `feat/907-nsh-full`; the parent supplied the HEAD and fetched-master ancestry, which this lane did not independently query. The preserved dirty migration and other lanes remain parent-owned.

This implements the cross-cutting i18n boundary in [nsh-full.md](nsh-full.md#验收先行), without changing checker semantics or accepting untranslated production UI. No nested `AGENTS.md` was found in the worktree, including scripts, crates and docs.

## Original failure evidence

The handoff reported 3 stale entries plus 48 unclassified findings. The actual first execution of **`python3 scripts/check_i18n.py`**, using Python **3.15.2**, returned **exit 1**:

```text
Catalog keys: {'en': 2062, 'zh-CN': 2062}; Rust files: 700; literal calls: 1692
Remaining Chinese literals by classification: {'benchmark': 5, 'fixture': 10, 'internal': 55, 'model_prompt': 4, 'protocol': 87, 'stored_content': 6, 'unreviewed': 145}
Static result: catalog/reference errors=3; UI/unreviewed findings=145
```

- Original stdout SHA256: `ea36feb9aa419890a463482094fc066676f207e599645520a80420ce21b20689`.
- Original stderr was empty.
- Original allowlist SHA256: `ceeff2bee52edbefa6b290823121933f30b9bf530653390dce2b47ae3d4e6d1a` (163 entries).
- Checker SHA256 before and after: `e2746a5cabb8b21b62ae266642dc2779b96c7e941639d0dc33390bba401b4ee2`.
- Findings are **literal occurrences**, not distinct classification identities. The 145 original unreviewed occurrences comprise 73 distinct exact `(path, decoded literal)` identities, enumerated by source in the table below. Each newly added allowlist entry preserves its exact literal, occurrence count and classification evidence. No original `ui` classifications were present.

All three original errors were `stale classification ... expected 1 occurrences, found 0`:

| Original path | Original literal / identity | Confirmed disposition |
| --- | --- | --- |
| `crates/vega_store/src/config.rs` | Entire embedded string beginning `# Vega configuration file.\n#\n# [[providers]]: one block per OpenAI-compatible provider.`, including `projects_collapsed` / `sessions_collapsed` comments containing `项目` / `会话` | Removed only after reading current config loading and finding no exact current Rust string anywhere under crates. `load_from` uses `begin_edit`; `read_from` uses `yaml_config::read` / `decode`. No relocated equivalent of this old embedded template is present. |
| `crates/vega/src/updater/install.rs` | `启动确认超时；保留运行应用和原版备份，需手动恢复` | No exact current Rust string anywhere under crates. Current `finish_startup` retains recovery artifacts on an unconfirmed launch and emits the new diagnostic listed below; it does not implement the old timeout message. |
| `crates/vega/src/updater/install.rs` | `新版本未正常启动，已恢复原应用` | No exact current Rust string anywhere under crates. Current `finish_startup` retains the candidate/backup/recovery records after exit or timeout instead of claiming an automatic rollback. Separate pre-launch rollback paths do not contain this literal. |

The final validation explicitly lexed every current Rust file to confirm these removed strings have no relocated occurrence. The old multiline template identity is also pinned by the original allowlist hash, rather than being recreated as an exception elsewhere.

## Implementation and source evidence

**Removed 3 stale identities; added 73 exact identities covering 145 occurrences:** fixture 131, internal 13, protocol 1. Final allowlist: **233 entries**. Existing non-stale entries were retained. No file-wide suppression, wildcard, category/schema expansion or checker/test modification was introduced.

### Production-source findings

- `crates/vega/src/updater/install.rs`: four **internal** diagnostics:
  - `finish_startup`, line 311: `新应用身份已变化，保留恢复记录`.
  - `finish_startup`, line 326: `工作区启动尚未确认；保留新版、原版备份与恢复记录，禁止自动降级`.
  - `verify_retained_backup`, line 447: `恢复备份身份已变化，保留记录`.
  - `retained_backup`, line 460: `恢复备份缺失或冲突，保留记录`.
  - `helper` failures terminate at `run_helper_if_requested` logging. Receipt failures from `acknowledge_startup` are logged in `startup_controller::start` and mapped to `StartupError::WorkerUnavailable`; `acknowledge_workspace` logs failures. Raw Chinese diagnostics are not the UI error. Recovery/cleanup files contain separate English instructions; those files were not changed.
- `crates/vega_conversation/src/types/automation.rs`: one **protocol** occurrence, `AutomationReason::PermissionReset` in `protocol_reason`, line 93: `权限已重置；请检查任务后手动恢复`. `set_system_reason` / `set_pause_reason` retain the canonical bytes in task data. `reason_message()` uses `message()` when `system_reason` is present; the existing `domain.automation.reason.permission_reset` key has both English and Chinese catalog values (`domain.en.toml` / `domain.zh-CN.toml`, line 42). This is not an untranslated UI label; arbitrary stored/model reasons are not text-matched or rewritten.
- `crates/vega_ui/src/conversation_stream/composer_actions.rs`: nine **internal** search aliases in `COMMANDS`, lines 40–94: `询问`, `问答`, `计划`, `执行`, `压缩`, `目标`, `定时`, `周期`, `分叉`. `ComposerCommand::matches()` consumes aliases; `ComposerActionRow::label()` displays `vega_i18n::t(id.command().label_key)`. Chinese search remains available regardless of UI locale. These aliases are not displayed labels.

### Development fixture findings

Every newly classified fixture was inspected at its actual input/assertion site before allowlisting. The per-entry reasons identify the relevant behavior, not just a filename. The table preserves the original unreviewed identity/occurrence counts and the inspected source version. Paths below are relative to `crates/vega_ui/src/` unless stated otherwise.

| Source | Identities / occurrences | Actual role |
| --- | ---: | --- |
| `automation_editor/nsh_editor/tests.rs` | 3 / 5 | UTF-8 byte-limit `界` repetitions; untrimmed `keep` and failed-draft save/retry inputs and expectations. |
| `command_palette/presentation/tests.rs` | 2 / 5 | `日本語` composition settlement and `界` scalar-count paste limits. |
| `commit_panel/tests.rs` | 3 / 4 | Synthetic changed/generated multiline messages containing `中文 body`; exact draft preservation/freeze expectations. |
| `conversation_stream/conversation_status/tests.rs` | 7 / 7 | Synthetic location, empty title/subtitle, omitted-thinking, error and compaction snapshots for wrapping/style/height measurements, including combining characters and emoji. |
| `conversation_stream/newshimmer/tests.rs` | 5 / 11 | Shaped-cluster and ellipsis samples; harness snapshot accessibility labels and mounted text replacement expectations. |
| `conversation_stream/resume_tail/tests.rs` | 1 / 1 | Test `snapshot(dark, language)` explicitly supplies `回到底部` paired with `Back to bottom`; not the production label source. |
| `conversation_stream/selection_menu/body_opacity_tests.rs` | 3 / 9 | Synthetic Markdown, expected rendered text and `😀中` UTF-8/UTF-16 range probes. |
| `conversation_stream/selection_menu/body_tests.rs` | 1 / 1 | `A😀中é` surrogate-interior/overrun offset probes. |
| `conversation_stream/selection_menu/source_anchor_tests.rs` | 2 / 3 | Synthetic Markdown source and surrogate/nonfinite anchor rejection probes. |
| `conversation_stream/selection_menu/tests.rs` | 1 / 2 | Harness projection explicitly switches to `复制原文` and asserts locale/identity updates. |
| `conversation_stream/stream_fade/tests.rs` | 1 / 8 | `native_ranges_require_exact_part_text_and_preserve_multibyte_append_boundaries` supplies `A😀中` source-part/opacity probes. |
| `settings/agents/nsh_agents.rs` | 1 / 2 | Inline `#[cfg(test)]` profile-claim test edits/asserts synthetic `新名称`. |
| `text_input/editing_core_tests.rs` | 7 / 24 | Synthetic edit receipts, failed local drafts, composition settlement, UTF-16 selections and invalid mutations. |
| `text_input/native_adapter.rs` | 6 / 14 | Inline `#[cfg(test)]` conflict/overflow draft preservation and CRLF paste payload/settlement assertions. |
| `text_input/nsh.rs` | 11 / 26 | Inline `#[cfg(test)] recovery_tests` mention replacement, IME, undo/redo, failed-draft reload/restore and directed paste fixtures. Every classified occurrence is after the test-module boundary, not in editor UI construction. |
| `text_input/nsh/native_integration.rs` | 5 / 9 | Feature-gated `nsh-reading-integration` harness `mounted_native_recovery_history_ax_and_submit_close_production_loop(TestAppContext)` uses `Fixture` for AX/IME edits and recovery/history/continued-typing expectations. This is a development integration harness, **not** a `#[cfg(test)]` module and not ordinary product labels. |

Fixture classification does not certify these Rust tests ran, nor authorize retaining development Rust tests/harnesses in the final delivery. Parent-owned test cleanup must remove newly stale exact classifications along with their sources, then rerun the unchanged checker.

## Validation and exact snapshot

A first post-edit run returned exit 0 (0 errors, 0 UI/unreviewed findings). Two evidence descriptions were subsequently corrected to the exact source symbols `ComposerActionRow::label()` and `native_ranges_require_exact_part_text_and_preserve_multibyte_append_boundaries`; classifications, literals and counts did not change. The **final-version** run below supersedes that intermediate result.

Final execution at **2026-11-08T09:11:05.954950+00:00**, Python **3.15.2**, elapsed **0.952 s**:

```text
$ python3 scripts/check_i18n.py
Catalog keys: {'en': 2062, 'zh-CN': 2062}; Rust files: 700; literal calls: 1691
Remaining Chinese literals by classification: {'benchmark': 5, 'fixture': 141, 'internal': 68, 'model_prompt': 4, 'protocol': 88, 'stored_content': 6}
Static result: catalog/reference errors=0; UI/unreviewed findings=0
exit 0
```

- Final stdout SHA256: `6a7121596e4dda90cb35afba5fc732fa75424e95064b6e014d21bb6f2a3939e1`; stderr empty.
- Final allowlist SHA256: `8c2c8911bf2c5c2887cdd57baf036f90a4a1cb21cb36a711c2ed54e12c00579e`.
- Checker unchanged SHA256: `e2746a5cabb8b21b62ae266642dc2779b96c7e941639d0dc33390bba401b4ee2`.
- Full check-input manifest SHA256: `56b6c70795ed55bdbd8d69e92a3c947755239bc0852a8d8c800d9908be0b87e8`.
- Manifest covers **710 files**: all 700 Rust sources, 8 locale TOMLs, checker and allowlist. Construction: sort relative `Path` objects, append `path + "\t" + sha256(file bytes) + "\n"` for every file, then SHA256 the UTF-8 concatenation. Bytes were identical immediately before and after the checker execution. This is a validated input snapshot, not a claim that concurrent writers have stopped.
- JSON readback confirmed 233 entries and 233 unique exact identities. The existing checker enforced schema/categories/reasons/positive exact counts and stale-count detection.
- Translation calls changed **1692 → 1691** between initial and final observations while other authorized lanes were active. This lane made no source edits and does not attribute that change or claim the original full source snapshot remained unchanged. Do not repeatedly recalculate unrelated moving strings; parent reruns on the integrated version.

### Inspected source SHA256 values

```text
crates/vega/src/updater/install.rs 763cdfbf7b2127dd6daa6d6619d1969fb5d21918e35aa2ec7fba50f69acc607b
crates/vega_conversation/src/types/automation.rs 3b6c8a0783f953068d5d8d4b4c2ea6b873c1e85639d36678f389589ec7809a83
crates/vega_ui/src/conversation_stream/composer_actions.rs 67e7354488d4a643d754e2ca29aef644f6dff1b7cbf8d7702dbeef12bc8179ff
crates/vega_ui/src/automation_editor/nsh_editor/tests.rs da676b8d0baf625b084c27f6f554fcfde0e0ced740adc6f583e28b0b2651062d
crates/vega_ui/src/command_palette/presentation/tests.rs 55598e94e86fc674ff137ed15b8912201ac58c0387589b41a5ed7c17d4e89283
crates/vega_ui/src/commit_panel/tests.rs 5fcd81219e156d4732418b33f1c8cc2a0b8f723f134858090c51dd555f7f0c05
crates/vega_ui/src/conversation_stream/conversation_status/tests.rs 8204fc3b390c5fef708a1dad413826b50941ce2b5ce03aebb3bc6710384d0e15
crates/vega_ui/src/conversation_stream/newshimmer/tests.rs 6d04f68e83a5a0b160bbc0ac3cca425fbea209796c6ca01a1bb8799f6356a95d
crates/vega_ui/src/conversation_stream/resume_tail/tests.rs 75ab30f33786ad48bb4f991986c1758d9af05103030ce7325e73c9b73d6556f2
crates/vega_ui/src/conversation_stream/selection_menu/body_opacity_tests.rs 29ff8d0955db7ab1f26fa0443dda536a16e05249998d31d1f5928c7a3a38b9ab
crates/vega_ui/src/conversation_stream/selection_menu/body_tests.rs 75c2ca5ed0f5f4ebe041fd67be2fa7c71423f1ab1c35341ce23d79ea491aac56
crates/vega_ui/src/conversation_stream/selection_menu/source_anchor_tests.rs 2eb7c657098a4ad058ca7b1ee0a961ce72dd3b06f269140cd71d6cb1b55315d7
crates/vega_ui/src/conversation_stream/selection_menu/tests.rs ac0dd5329d62555887ea9ef694b993527e270dccb96c3a711c725116e85c4b11
crates/vega_ui/src/conversation_stream/stream_fade/tests.rs 1da0d93044790b6ad7ab92bee79f235c7568efc1a978bcc805d9514b2caefabd
crates/vega_ui/src/settings/agents/nsh_agents.rs 148e3c4564ea8c50b7ee9f2dd33d9d97644640ba30818c9820d5c9b16e715522
crates/vega_ui/src/text_input/editing_core_tests.rs 3ac79c53cd6319fb116284bf3c28956a1b7ec68cad1d3529279c489040b9236c
crates/vega_ui/src/text_input/native_adapter.rs 3b0d4b661c6e6a45ae225d0d2617e5fca95776af405ded9fe43ba572a85967e6
crates/vega_ui/src/text_input/nsh.rs fa42a98fd2c25a4ff3b9f04570dc5054067cc81bd87195c800b51dfd520ce4e9
crates/vega_ui/src/text_input/nsh/native_integration.rs d196df8a3182441c0a8fe032baf905770a2086adfb30258cbddd0f7e1bd136c1
crates/vega_store/src/config.rs bad572976cdeeaa5f7e344ec2251724b22a4ad33ce629a175350f331aa773104
crates/vega/src/startup_controller.rs 6bd299865d25ddf10a6ecab93ef37c9f53dca556c954aff3e81555b084454902
```

## Unresolved items and parent handoff

- **Unresolved true production Chinese UI from these findings: none.** No source symbol/catalog-key translation request is outstanding from this classification implementation.
- This static pass does not establish complete English coverage, dynamic-message correctness, language switching/persistence, accessibility, or the checker's A01–A07 limitations. No Rust compilation/tests, Python unit suite, native application, CI, three-platform run or user handtest was executed in this lane.
- Centralized non-implementer independent review is **pending**, per the requested implementation-first ordering; this document is author evidence, not review approval.
- Parent must rerun `python3 scripts/check_i18n.py` after integrating other writers and again after development-test cleanup. Required CI and user/platform acceptance remain independent pending gates. Do not edit the checker or weaken counts to accommodate later changes.

**READY — author stops writing the two owned files; parent owns integration and subsequent revalidation.**
