# Issue #81 移除 Pi Agent 相关内容：交付记录

## Freeze

- verified_at_utc: 2026-09-25 22:30 UTC / verified_at_local: 2026-09-26 06:30 CST
- branch: `feat/81-remove-pi-agent`
- task_contract: [Issue #81 规格](vega-issue-81-remove-pi-agent.md)
- scope: remove explicit Pi credential import while preserving manual Provider setup, stored credentials, user-controlled enabled state, and exact-root Skills import
- spec deviation: none
- os_arch: Darwin arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`
- git: `git version 2.55.0`

## Changes

- Removed the Pi credential import button, UI command, async dispatch/status handling, test-only Pi source override, service path resolver/parser/reader, and dedicated `PiSource` error.
- Added Provider UI and service regressions for the absent Pi action, manual key entry, stored-key retention, persisted enabled state, and explicit toggling.
- Kept generic external Skills directory preview/import and strengthened the exact-root regression with an unlinked `.pi/agent/skills` decoy.
- Added the #81 acceptance spec to the README index, marked the old A7-02 contract as historical, and updated forward-looking A7/#82 acceptance text. Historical A7-02 and #82 delivery/native acceptance records were not edited.

## Results

| requirement | evidence class | exact command | result | duration | bounded footer |
|---|---|---|---|---|---|
| Provider removal and preservation; manual credentials; enabled state; model discovery/test; generic Skills import; `.pi` no-scan | GPUI / production / service targeted | `cargo nextest run -p vega_conversation -p vega_ui -E 'test(issue81_provider_lifecycle_preserves_manual_credentials_and_enabled_state) | test(issue81_provider_settings_remove_pi_import_and_keep_credential_states) | test(pointer_credential_recovery_patch_and_reload_clear_obsolete_network_results) | test(production_form_edits_share_patch_authority_and_rollback_credentials_on_config_failure) | test(pointer_settings_uses_real_service_config_and_mock_transport) | test(issue74_native_folder_picker_previews_exact_root_before_link) | test(issue74_imported_global_auto_respects_ui_switches_without_ambient_scan) | test(issue74_vega_owned_global_requires_exact_ui_link_and_uses_config_dir_root)'` | exit 0; 8 passed, 1035 skipped | 0.334s test runtime | Nextest run `f2cb55cb-b989-489f-b5f1-ca3245821d6d` |
| Pi entry removal regression detects old behavior | GPUI targeted, expected red before implementation | `cargo nextest run -p vega_ui -E 'test(issue81_provider_settings_remove_pi_import_and_keep_credential_states)'` | expected exit 100 before removing the action; failed because `provider-import-pi` was still visible | 0.071s test runtime | Nextest run `e2b77d08-3030-45c2-b57e-ec58724ec4a2` |
| Formatting | formatter | `cargo fmt --all -- --check` | exit 0; stdout empty | 1.819s | — |
| Whitespace/conflict markers | git | `git diff --check` | exit 0; stdout empty | <0.001s | — |
| Removed production references | source scan | `rg -n 'ImportPiCredential|import_pi_credential|read_pi_provider|PiProviderCredential|PiSource|pi_models_path|provider-import-status|正在从 Pi Agent|已从 Pi Agent|从 Pi Agent 导入凭据' crates/vega_conversation/src crates/vega_ui/src/settings -g '*.rs'` | exit 1; no matches | <0.001s | no production matches |

Final targeted nextest output:

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.43s
────────────
 Nextest run ID f2cb55cb-b989-489f-b5f1-ca3245821d6d with nextest profile: default
    Starting 8 tests across 12 binaries (1035 tests skipped)
        PASS [   0.093s] (1/8) vega_conversation provider_settings::tests::issue81_provider_lifecycle_preserves_manual_credentials_and_enabled_state
        PASS [   0.103s] (2/8) vega_conversation provider_settings::tests::production_form_edits_share_patch_authority_and_rollback_credentials_on_config_failure
        PASS [   0.129s] (3/8) vega_conversation agent::tests::skills::issue74_vega_owned_global_requires_exact_ui_link_and_uses_config_dir_root
        PASS [   0.170s] (4/8) vega_conversation agent::tests::skills::issue74_imported_global_auto_respects_ui_switches_without_ambient_scan
        PASS [   0.211s] (5/8) vega_ui settings::provider_management::tests::issue81_provider_settings_remove_pi_import_and_keep_credential_states
        PASS [   0.273s] (6/8) vega_ui settings::provider_management::tests::pointer_settings_uses_real_service_config_and_mock_transport
        PASS [   0.289s] (7/8) vega_ui settings::skills::tests::issue74_native_folder_picker_previews_exact_root_before_link
        PASS [   0.332s] (8/8) vega_ui settings::provider_management::tests::pointer_credential_recovery_patch_and_reload_clear_obsolete_network_results
────────────
     Summary [   0.334s] 8 tests run: 8 passed, 1035 skipped
```

## Residuals

- `NOT RUN`: workspace-wide nextest and strict Clippy; cloud `pr-check` owns those gates.
- `NOT RUN`: installed-app Computer Use/native acceptance. This worktree was not installed over the running app; acceptance remains for the user after the PR is integrated.
- `PENDING`: pull request and cloud check.
