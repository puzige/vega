# Issue #149 测试迁移清单

实施优先级：mock 外部边界并保留业务断言；纯 OS 契约改由手测。网络/Provider/OAuth/UI 93 项和 Artifact 11 项原测试函数迁移，零业务测试删除；第二轮另补 runtime MCP/skills 9 项，网络与 Artifact 合计 113 项。Git、Shell、控制器按 [规格与结果](vega-issue-149-remove-real-e2e.md) 迁移。

## 52 项实际删除

其中 3 项是已有 mock 用例完全覆盖的真实 Git 重复版本，其余为真实子进程/管道/PTY/沙箱、夹具环境或真实适配对照测试。混合测试中的业务断言已移到保留测试（如取消落库、崩溃恢复、终端布局与复制）。删除函数名不含仅重命名的 mock 迁移项。

- `crates/vega_conversation/src/git_workspace/tests/caps_runner.rs::commit_summary_deferred_overflow_never_eof_uses_bounded_timeout`
- `crates/vega_conversation/src/git_workspace/tests/caps_runner.rs::commit_summary_exact_argv_null_stdin_and_full_dual_drain`
- `crates/vega_conversation/src/git_workspace/tests/caps_runner.rs::commit_summary_raw_cap_is_inclusive_and_overflow_tail_is_drained`
- `crates/vega_conversation/src/git_workspace/tests/caps_runner.rs::commit_summary_stderr_overflow_is_fully_drained_then_rejected`
- `crates/vega_conversation/src/git_workspace/tests/caps_runner.rs::git_workspace_bounded_stdin_stdout_stderr_progress_concurrently`
- `crates/vega_conversation/src/git_workspace/tests/caps_runner.rs::git_workspace_runner_scrubs_git_environment_and_bounds_output`
- `crates/vega_conversation/src/git_workspace/tests/caps_runner.rs::git_workspace_stderr_cap_is_inclusive_and_plus_one_fails`
- `crates/vega_conversation/src/git_workspace/tests/snapshot.rs::git_workspace_fixture_git_scrubs_repository_targeting_environment`
- `crates/vega_conversation/src/git_workspace/tests/lifecycle.rs::git_workspace_cancel_is_typed_and_reaps_fixture_group`
- `crates/vega_conversation/src/git_workspace/tests/lifecycle.rs::git_workspace_early_parent_exit_with_inherited_pipes_fails_and_reaps_group`
- `crates/vega_conversation/src/git_workspace/tests/lifecycle.rs::git_workspace_read_timeout_is_typed_and_bounded`
- `crates/vega_conversation/src/git_workspace/tests/lifecycle.rs::lifecycle_captured_states_match_real_git`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/selection_noop.rs::ignored_mode_and_normalization_git_adapter_matches_captured_states`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/selection_noop.rs::trusted_git_empty_selection_spawns_zero_add`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/commit_proof.rs::commit_status_drift_real_git_consumes_prepared_and_spawns_zero_commit`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/commit_proof.rs::real_git_object_missing_proof_consumes_prepared_after_one_commit`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/filter_gitlink.rs::empty_blob_and_selected_delete_git_adapter_matches_captured_states`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/filter_gitlink.rs::filter_and_attributes_captured_states_match_real_git`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/selection_topology.rs::selection_topology_captured_states_match_real_git`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/runner_mutation.rs::trusted_git_mutations_use_exact_argv_and_in_memory_stdin`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/runner_mutation.rs::trusted_mutation_runner_drains_floods_while_writing_large_stdin`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/runner_mutation.rs::trusted_mutation_runner_enforces_spawn_cancel_exit_and_output_caps_for_add_and_commit`
- `crates/vega_conversation/src/git_workspace/trusted_git/tests/runner_mutation.rs::trusted_mutation_runner_times_out_cancels_and_reaps_process_groups`
- `crates/vega_conversation/src/git_workspace/branch/tests/mutation_runner.rs::trusted_mutation_cancellation_reaps_process_group_descendant`
- `crates/vega_conversation/src/git_workspace/branch/tests/mutation_runner.rs::trusted_mutation_enforces_output_caps_nonzero_and_precancel_zero_spawn`
- `crates/vega_conversation/src/git_workspace/branch/tests/branch_stub.rs::branch_captured_states_match_real_git`
- `crates/vega_tools/src/bash/tests.rs::bash_cancel_reaps_shell_and_inherited_process_group_descendant`
- `crates/vega_tools/src/bash/tests.rs::bash_custom_timeout_reaps_shell_and_descendant`
- `crates/vega_tools/src/bash/tests.rs::bash_cwd_project_write_and_stdout_stderr_merge`
- `crates/vega_tools/src/bash/tests.rs::bash_multimegabyte_no_newline_has_line_marker_and_bounded_high_water`
- `crates/vega_tools/src/bash/tests.rs::bash_output_keeps_4001_line_head_tail_with_byte_caps`
- `crates/vega_tools/src/bash/tests.rs::bash_parent_exit_with_inherited_stdout_reaps_group_before_cleanup`
- `crates/vega_tools/src/bash/tests.rs::bash_unconfirmed_reap_retains_temp_for_safe_gc`
- `crates/vega_tools/src/bash/tests.rs::full_access_cancel_and_timeout_reap_shell_descendant_and_private_temp`
- `crates/vega_tools/src/bash/tests.rs::full_access_skips_sandbox_profile_and_project_hardlink_scan`
- `crates/vega_tools/src/bash/tests.rs::full_access_writes_sibling_and_commits_while_default_remains_sandboxed`
- `crates/vega_tools/src/bash/tests.rs::sandbox_blocks_outside_git_entry_and_actual_gitdir`
- `crates/vega_tools/src/bash/tests.rs::sandbox_cleanup_rejects_replaced_root_without_touching_attacker`
- `crates/vega_tools/src/bash/tests.rs::sandbox_denies_shared_private_tmp_but_allows_call_temp`
- `crates/vega_tools/src/bash/tests.rs::sandbox_nested_temp_symlink_cleanup_never_touches_target`
- `crates/vega_tools/src/bash/tests.rs::sandbox_profile_self_test_failure_never_runs_shell`
- `crates/vega_tools/src/bash/tests.rs::sandbox_real_git_reads_and_dev_null_redirection_work_without_git_mutation`
- `crates/vega_tools/src/bash/tests.rs::sandbox_real_worktree_git_reads_but_external_gitdir_and_sibling_stay_read_only`
- `crates/vega_tools/src/bash/tests.rs::sandbox_skips_but_keeps_in_project_actual_gitdir_read_only`
- `crates/vega_tools/src/bash/tests.rs::sandbox_temp_is_private_exact_allowed_exported_and_cleaned`
- `crates/vega_tools/src/bash/tests.rs::signal_group_treats_an_already_exited_child_as_gone`
- `crates/vega_conversation/src/terminal.rs::owned_login_pty_persistent_interrupt_resize_exit_and_reap`
- `crates/vega_conversation/src/agent/tests/tool_lifecycle.rs::crash_child_runtime_fixture`
- `crates/vega_conversation/src/agent/tests/tool_lifecycle.rs::killed_child_recovers_only_displayed_content_and_reuses_running_call`
- `crates/vega_conversation/tests/stop_repair_resume_e2e.rs::stop_mid_bash_kills_the_owned_process_group_under_one_second`
- `crates/vega_ui/src/terminal.rs::production_terminal_input_handler_and_keys_reach_real_pty`
- `crates/vega/src/tests/diff.rs::diff_controller_fixture_scrubs_hook_git_environment`

## 重复业务用例对应

| 删除的真实版本 | 保留的进程内覆盖 |
|---|---|
| `trusted_git_empty_selection_spawns_zero_add` | `empty_selection_never_spawns_add_for_each_staged_delta` |
| `real_git_object_missing_proof_consumes_prepared_after_one_commit` | `commit_proof_rejects_object_missing_after_one_commit` |
| `commit_status_drift_real_git_consumes_prepared_and_spawns_zero_commit` | `commit_third_capture_mismatch_consumes_prepared_and_spawns_zero_commit` 的 status 分支 |

崩溃子进程的部分内容持久化与 running-call 恢复断言保留在 `startup_recovery_survives_reopen_and_allows_a_new_turn`。终端 UI 的布局、尺寸和复制断言保留在 R47 固定 snapshot 测试。Shell 输出收集器的纯字节边界测试继续保留；真实管道合并和 OS 回收不再作为自动化证据。
