# Issue #75 · 受管桌面与浏览器控制交付记录

关联：[Issue #75](https://github.com/puzige/vega/issues/75) · [冻结规格](vega-issue-75-computer-browser-use.md)。功能点 A3-04。

## Freeze

- 本记录的核心自动验证时间：2026-10-11 UTC / 2026-10-12 Asia/Shanghai。
- 核心实施分支：`feat/75-computer-browser`；核心、UI 与打包分别独立 worktree 实施，由主代理集成单张卡的 PR。
- 环境：macOS 15.8.1 arm64，rustc 1.98.0、cargo 1.98.0、git 2.55.0。
- 发行组件固定为 Cua Driver 0.34.0、Node 24.21.0、Playwright MCP 0.0.83；资源与平台边界见规格 R29–R32。
- 真实桌面、浏览器、模型、TCC 与安装验收：NOT RUN。进程内回归不能代表真实系统集成已通过。

## 实现与证据边界

`vega_runtime::computer_use` 负责单 run 授权、单一 lease、epoch 撤权、接管/继续/停止以及受管后端。桌面发现使用只读元数据，选择后更换为单窗 bounded daemon；输入与截图只针对授权窗口。浏览器只接受扩展连接中的唯一 current http(s) 标签，允许该目标的普通点击导航，popup、多目标与不可验证结果撤销连接。

专用 managed MCP transport 限制帧、文本、结构化结果与图片；取消或协议失败后连接不可复用。进程由 Vega 直接持有和回收，停止不关闭用户应用或浏览器标签。接管先撤 epoch，直到旧执行通道关闭才进入 Paused；同一 run 的旧审批和晚到图像不能恢复动作权。

工具观察只在本次 run 保存有界缓存。模型上下文携带 Tool 来源、call ID、观察 ID、尺寸及不可信内容说明；Responses 使用函数结果内容数组，Chat Completions 在整批工具结果后投影多模态 wire 段。持久化仅保留脱敏动作摘要、输入长度与摘要值；不写截图或完整 AX/DOM 到历史。压缩会使观察失效，继续操作须重新观察。

## 核心定向回归

| 规格 | 进程内证据 | 状态 |
|---|---|---|
| CU01、CU09 | Native 模式/能力开关门禁，禁用桌面工具无 discovery 副作用，普通 MCP 仍逐次 Once | PASS |
| CU03–CU06 | 指定目标审批状态、旧 epoch 回调拒绝、暂停立即继续拒绝、停止期间其他 run 不得抢 lease、旧观察失效 | PASS（状态/控制边界） |
| CU05、CU13 | 模型尚未完成时接管会完成后台清理并阻塞 final，用户继续后运行恢复；部分写取消、错误 RPC ID 均关闭专用连接 | PASS（mock provider/内存 transport） |
| CU07 | Cua typed target 与 legacy 字段互斥；元素 token 可见且精确匹配；显示坐标映射回上游 PNG 像素 | PASS（固定合同 fixture） |
| CU08 | 仅一个 current 标签、http(s) 目标、普通导航结果，popup/多标签/空白页拒绝，DOM ref 完整匹配 | PASS（固定合同 fixture） |
| CU10、CU14 | 专用媒体解析与普通 MCP 分离，真实图片解码、MIME 伪造拒绝、缩放尺寸；缺组件、版本错、路径错、过大 manifest 拒绝 | PASS（纯函数/临时文件） |
| CU11 | Responses 工具图像数组；Chat Completions 完整工具批次后投影，保留真实 call ID | PASS（provider request fixture） |
| CU12 | 连续 20 次观察后旧上下文正文/图片清除、缓存上界、失效后晚图拒绝；相同编码长度的变图改变 fingerprint；用户图片旧估算合同保持 | PASS（runtime/context） |
| CU02、CU16 | 设置持久化、运行控制条、查看观察与关闭/删除入口 | PASS（集成后的 store/GPUI 定向回归） |
| CU15 | 固定依赖、打包路径、哈希和签名责任 | PASS（12 项纯函数测试；真实 prepare/Node MCP 握手另有证据） |
| CU17、CU18 | TextEdit 与登录 Chrome 上的真实观察、操作和接管 | NOT RUN，用户手测 |

初始 provider 失败命令：

```text
cargo nextest run -p vega_runtime issue75_provider --no-fail-fast
Summary [   0.019s] 2 tests run: 0 passed, 2 failed, 275 skipped
```

退出 100。失败原因分别为 Responses 函数结果仍是文本、Chat Completions 未生成批次后的观察段。原始日志名 `vega-75-provider-red.log`，SHA-256 `9cf8bc33d932771815457df868eac8648782633c8eb8cab2cf0e829558262b27`。

生命周期回归首次失败：`vega-75-core-final.log`，20 项中 19 通过、1 失败，退出 100。新增用例发现 monitor 在订阅后的第一次状态变更已发生时先等待 watch，导致接管一直停在 Pausing。修复为先处理当前状态，清理在 backend 锁内复核 epoch/phase，拒绝旧清理关闭新连接；随后同一断言通过。失败日志 SHA-256 `65b5074066b4606aeb97fa50d7471f746c087454fe4f5abf770dad41fb731a66`。

最终命令及原始输出摘要：

```text
cargo nextest run -p vega_runtime -p vega_conversation -p vega_mcp issue75 --no-fail-fast
Nextest run ID d9b432ed-b2c5-4758-9859-3d50a0413333 with nextest profile: default
Starting 22 tests across 13 binaries (907 tests skipped)
Summary [   2.287s] 22 tests run: 22 passed, 907 skipped
```

退出 0。原始日志名 `vega-75-core-final-4.log`，SHA-256 `a400d637d3304729899a303fcd201b4bd431e47d95901ba7bd2f0899cf8c3623`。测试时三个核心 crate 的 diff SHA-256 为 `47b5d0f0b0913a04b37ea0318ef2171ab54880e4f80be792c987d73b7987e479`；该值不包含交付文档，避免自引用。

仅执行本卡定向测试；没有本地 workspace 全量测试、全量 clippy 或真实外部进程 E2E。云端 `pr-check`、PR 与合并身份由主代理在集成后补充。

## 已接受限制与手测步骤

- macOS arm64 14+ 才启用能力；不提高 Vega 整体系统下限。Settings 的组件检测成功仅表示随包文件可用，系统权限、执行器协议和扩展连接在首次使用时检查。
- 浏览器扩展由用户安装并选择目标。首期限制的是执行器连接中的一个 current 标签，用户 Chrome 可以保留其他未连接标签。上游没有不可变 tab ID，同一 index 在两次检查之间被替换的竞态无法全部识别；不宣称浏览器级强隔离。
- 上游 `browser_tabs(list)` 调用 `ensureTab`，目标已关闭时可能先创建空白页。Vega 拒绝 about:blank 并断开，不主动关闭该页；不能保证用户关闭目标后绝不出现新空白标签。
- 桌面要求模型支持视觉；BrowserObserve 默认只将 DOM 放入模型，用户仍可查看截图，显式 include_image 才向视觉模型发送图像。页面 canvas 无 DOM 引用时交给用户接管，不提供 JS 或坐标浏览器通道。
- 截图最多最近两项、总计 8 MiB，最长边 1600；必要时 JPEG 重编码。图片 token 统计为尺寸估算，不是账单真值。截图与完整观察不恢复到历史。
- Cua fallback 与 Playwright 会使用短命文件；均限制在 owner-only run 目录，读取后清理，进程退出后清理目录。不把这一实现描述为严格无磁盘写入。
- 取消不能撤回已交给 OS/网页的事件；在途结果不可验证时标 outcome_unknown，不自动重试。接管等待执行通道回收，不在清理中允许继续或释放 lease。

用户在合并后的正式 Vega.app 手测：

1. 开启桌面能力，选择 TextEdit 的指定窗口，观察并输入无敏感测试文本；确认其他窗口未被操作、权限弹窗归属可解释。
2. 在动作期间或模型响应期间点“接管”；确认先进入 Pausing，清理后可手动操作；继续后必须先获得新观察，旧引用不得重放。
3. 安装匹配的 Playwright MCP 扩展并选一个已登录测试标签，完成 DOM 表单、普通链接导航；确认复用登录态、目标明确、用户可随时接管。
4. 打开 popup、切换连接目标或关闭目标；确认停止改控、显示可恢复的连接错误，且不会关闭用户 Chrome。关闭目标可能产生上述上游空白页残余。
5. 停止运行、关闭窗口或删除任务，确认没有新增动作；重启 Vega 后能力开关可恢复，但授权、连接与截图均不恢复。

与冻结规格偏离：无；上述限制均在 R13、R21、R24–R28 和实施裁决中明确。用户手测通过前不关闭 Issue、不标 Done、不清理本卡 worktree。

## 主线集成验证

2026-10-12：三个提交已整合到 `484fac9` 主线，没有代码冲突；UI 使用正式核心接口再次编译和验证。以下是本卡定向验证，未执行本地全量门禁。

```sh
cargo nextest run -p vega_runtime -p vega_mcp -p vega_conversation -p vega_store -p vega_ui -p vega -E 'test(issue75) | test(issue296_h3a_agents_argv_has_input_surface)'
```

退出 0，原始输出：

```text
 Nextest run ID ed822298-d5a9-4073-a576-f7504f8991b2 with nextest profile: default
    Starting 30 tests across 17 binaries (1972 tests skipped)
        PASS [   0.015s] ( 1/30) vega_mcp managed::tests::issue75_managed_wrong_response_id_poisons_connection
        PASS [   0.016s] ( 2/30) vega_mcp managed::tests::issue75_managed_media_profile_is_bounded_and_separate
        PASS [   0.017s] ( 3/30) vega_mcp managed::tests::issue75_managed_partial_write_cancel_poisons_connection
        PASS [   0.017s] ( 4/30) vega_conversation agent::tests::issue75_history_accepts_only_dedicated_safe_audit_and_session_approval
        PASS [   0.023s] ( 5/30) vega_runtime agent::context_accounting::tests::issue75_observation_fingerprint_changes_when_same_size_pixels_change
        PASS [   0.029s] ( 6/30) vega::bin/vega window::computer_use::tests::issue75_observation_previews_evict_app_assets_on_replace_clear_and_release
        PASS [   0.020s] ( 7/30) vega_runtime agent::tests::issue75_disabled_desktop_name_cannot_trigger_discovery_from_browser_grant_config
        PASS [   0.020s] ( 8/30) vega_runtime agent::tests::issue75_native_tool_gate_rejects_plan_readonly_and_disabled_without_dispatch
        PASS [   0.013s] ( 9/30) vega_runtime computer_use::backend::tests::issue75_browser_refs_are_exact_and_navigation_preserves_only_the_single_connection
        PASS [   0.022s] (10/30) vega_runtime agent::tests::mcp_registry::issue75_computer_capability_does_not_bypass_ordinary_mcp_once
        PASS [   0.013s] (11/30) vega_runtime computer_use::backend::tests::issue75_browser_tab_parser_rejects_ambiguous_or_privileged_targets
        PASS [   0.009s] (12/30) vega_runtime computer_use::control::tests::issue75_granting_browser_does_not_renew_desktop_expiry
        PASS [   0.010s] (13/30) vega_runtime computer_use::backend::tests::issue75_driver_action_maps_only_typed_window_target_and_original_png_coordinates
        PASS [   0.015s] (14/30) vega_runtime computer_use::components::tests::issue75_component_manifest_rejects_missing_version_path_and_oversize
        PASS [   0.054s] (15/30) vega::bin/vega tests::computer_use::issue75_window_cleanup_revokes_every_control_before_cancelling
        PASS [   0.054s] (16/30) vega::bin/vega tests::computer_use::issue75_late_ready_after_stop_is_revoked_and_stale_generation_is_ignored
        PASS [   0.013s] (17/30) vega_runtime computer_use::tests::issue75_audit_contains_no_typed_text_or_target
        PASS [   0.013s] (18/30) vega_runtime computer_use::tests::issue75_coordinate_projection_tracks_image_resize_and_rejects_out_of_bounds
        PASS [   0.015s] (19/30) vega_runtime computer_use::tests::issue75_observation_cache_trims_all_old_payloads_and_rejects_late_frames
        PASS [   0.013s] (20/30) vega_runtime computer_use::tests::issue75_takeover_waits_for_cleanup_and_keeps_single_lease
        PASS [   0.011s] (21/30) vega_runtime openai::tests::computer_use::issue75_provider_chat_observation_follows_complete_tool_batch
        PASS [   0.012s] (22/30) vega_runtime openai::tests::computer_use::issue75_provider_responses_observation_retains_tool_identity
        PASS [   0.012s] (23/30) vega_store config::tests::issue75_computer_use_settings_default_off_and_round_trip
        PASS [   0.085s] (24/30) vega::bin/vega tests::computer_use::issue75_confirming_background_task_deletion_revokes_its_control
        PASS [   0.117s] (25/30) vega_runtime context::tests::issue75_tool_visual_estimate_does_not_change_user_image_contract
        PASS [   0.148s] (26/30) vega_ui settings::tests::issue296::issue296_h3a_agents_argv_has_input_surface
        PASS [   0.217s] (27/30) vega_runtime agent::tests::issue75_pause_holds_final_response_until_user_resumes
        PASS [   0.190s] (28/30) vega_ui settings::tests::issue296::issue75_native_toggles_preserve_other_disk_settings_and_reject_conflict
        PASS [   0.392s] (29/30) vega::bin/vega tests::computer_use::issue75_control_bar_survives_settings_and_stop_has_epoch_and_owner_fences
        PASS [   2.287s] (30/30) vega_runtime computer_use::media::tests::issue75_observation_decode_rejects_spoofed_mime_and_retains_original_scale
────────────
     Summary [   2.328s] 30 tests run: 30 passed, 1972 skipped
```

```sh
python3 -m unittest discover -s scripts -p prepare_computer_use_tests.py -v
```

退出 0，原始输出：

```text
test_archive_extracts_only_requested_files (prepare_computer_use_tests.ComputerUsePackagingTests.test_archive_extracts_only_requested_files) ... ok
test_archive_rejects_selected_links_and_duplicate_members (prepare_computer_use_tests.ComputerUsePackagingTests.test_archive_rejects_selected_links_and_duplicate_members) ... ok
test_archive_rejects_size_and_count_limits (prepare_computer_use_tests.ComputerUsePackagingTests.test_archive_rejects_size_and_count_limits) ... ok
test_archive_rejects_traversal_absolute_and_backslash_names (prepare_computer_use_tests.ComputerUsePackagingTests.test_archive_rejects_traversal_absolute_and_backslash_names) ... ok
test_contract_has_fixed_versions_paths_and_official_sources (prepare_computer_use_tests.ComputerUsePackagingTests.test_contract_has_fixed_versions_paths_and_official_sources) ... ok
test_hash_mismatch_and_oversize_never_accept_cache (prepare_computer_use_tests.ComputerUsePackagingTests.test_hash_mismatch_and_oversize_never_accept_cache) ... ok
test_lock_rejects_version_integrity_url_and_package_drift (prepare_computer_use_tests.ComputerUsePackagingTests.test_lock_rejects_version_integrity_url_and_package_drift) ... ok
test_npm_command_disables_scripts_and_browser_downloads (prepare_computer_use_tests.ComputerUsePackagingTests.test_npm_command_disables_scripts_and_browser_downloads) ... ok
test_output_replacement_preserves_unowned_directories (prepare_computer_use_tests.ComputerUsePackagingTests.test_output_replacement_preserves_unowned_directories) ... ok
test_runtime_manifest_uses_contents_relative_paths_and_source_hashes (prepare_computer_use_tests.ComputerUsePackagingTests.test_runtime_manifest_uses_contents_relative_paths_and_source_hashes) ... ok
test_signing_inventory_rejects_hidden_native_code_and_symlinks (prepare_computer_use_tests.ComputerUsePackagingTests.test_signing_inventory_rejects_hidden_native_code_and_symlinks) ... ok
test_signs_nested_helpers_before_app_and_jit_entitlement_only_on_node (prepare_computer_use_tests.ComputerUsePackagingTests.test_signs_nested_helpers_before_app_and_jit_entitlement_only_on_node) ... ok

----------------------------------------------------------------------
Ran 12 tests in 0.009s

OK
```

日志 SHA-256：

- `vega-75-integrated-tests.log`：`901b7562ee6000952004189823812d3a1c62563ea5370b5abda4160bce5a7da6`。
- `vega-75-integrated-packaging-tests.log`：`c8f59125d64a22bea97c70f6ddfb0003c6f30cd5537c7f2aec398b42acc4a73a`。

组件准备已独立实际完成，Node 的版本及 MCP initialize/tools/list 握手已验证；没有派发浏览器或桌面操作。云端全量门禁、合并身份和用户手测状态以关联 PR/Issue 为准。本地日常 Vega.app 尚未替换。

## 云端首轮反馈修复

PR #306 首轮 run `38159970473` 的 Format 通过，Clippy 在 managed transport 的嵌套条件报错。按实际诊断合并条件，并统一 MCP 连接枚举的装箱存储；未增加 lint allow，也未放宽断言。针对受影响核心库的 Clippy 和本卡回归再次通过：

```sh
cargo clippy -p vega_runtime -p vega_conversation --lib -- -D warnings
cargo nextest run -p vega_runtime -p vega_conversation -p vega_mcp issue75 --no-fail-fast
```

两条命令均退出 0。原始输出：

```text
    Checking vega_runtime v0.1.0 (<integration-worktree>/crates/vega_runtime)
    Checking vega_conversation v0.1.0 (<integration-worktree>/crates/vega_conversation)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.29s
 Nextest run ID 66465429-9c77-4e03-bd9b-b614b49d8d72 with nextest profile: default
    Starting 22 tests across 13 binaries (907 tests skipped)
        PASS [   0.015s] ( 1/22) vega_mcp managed::tests::issue75_managed_wrong_response_id_poisons_connection
        PASS [   0.015s] ( 2/22) vega_mcp managed::tests::issue75_managed_partial_write_cancel_poisons_connection
        PASS [   0.017s] ( 3/22) vega_mcp managed::tests::issue75_managed_media_profile_is_bounded_and_separate
        PASS [   0.017s] ( 4/22) vega_runtime agent::tests::issue75_disabled_desktop_name_cannot_trigger_discovery_from_browser_grant_config
        PASS [   0.017s] ( 5/22) vega_runtime agent::tests::issue75_native_tool_gate_rejects_plan_readonly_and_disabled_without_dispatch
        PASS [   0.018s] ( 6/22) vega_conversation agent::tests::issue75_history_accepts_only_dedicated_safe_audit_and_session_approval
        PASS [   0.018s] ( 7/22) vega_runtime computer_use::backend::tests::issue75_browser_refs_are_exact_and_navigation_preserves_only_the_single_connection
        PASS [   0.019s] ( 8/22) vega_runtime agent::context_accounting::tests::issue75_observation_fingerprint_changes_when_same_size_pixels_change
        PASS [   0.019s] ( 9/22) vega_runtime agent::tests::mcp_registry::issue75_computer_capability_does_not_bypass_ordinary_mcp_once
        PASS [   0.008s] (10/22) vega_runtime computer_use::backend::tests::issue75_browser_tab_parser_rejects_ambiguous_or_privileged_targets
        PASS [   0.008s] (11/22) vega_runtime computer_use::backend::tests::issue75_driver_action_maps_only_typed_window_target_and_original_png_coordinates
        PASS [   0.010s] (12/22) vega_runtime computer_use::control::tests::issue75_granting_browser_does_not_renew_desktop_expiry
        PASS [   0.011s] (13/22) vega_runtime computer_use::components::tests::issue75_component_manifest_rejects_missing_version_path_and_oversize
        PASS [   0.010s] (14/22) vega_runtime computer_use::tests::issue75_audit_contains_no_typed_text_or_target
        PASS [   0.009s] (15/22) vega_runtime computer_use::tests::issue75_takeover_waits_for_cleanup_and_keeps_single_lease
        PASS [   0.012s] (16/22) vega_runtime computer_use::tests::issue75_coordinate_projection_tracks_image_resize_and_rejects_out_of_bounds
        PASS [   0.012s] (17/22) vega_runtime computer_use::tests::issue75_observation_cache_trims_all_old_payloads_and_rejects_late_frames
        PASS [   0.008s] (18/22) vega_runtime openai::tests::computer_use::issue75_provider_chat_observation_follows_complete_tool_batch
        PASS [   0.007s] (19/22) vega_runtime openai::tests::computer_use::issue75_provider_responses_observation_retains_tool_identity
        PASS [   0.109s] (20/22) vega_runtime context::tests::issue75_tool_visual_estimate_does_not_change_user_image_contract
        PASS [   0.222s] (21/22) vega_runtime agent::tests::issue75_pause_holds_final_response_until_user_resumes
        PASS [   2.249s] (22/22) vega_runtime computer_use::media::tests::issue75_observation_decode_rejects_spoofed_mime_and_retains_original_scale
────────────
     Summary [   2.269s] 22 tests run: 22 passed, 907 skipped
```

以上编译输出中的工作树绝对路径已脱敏；原始本地日志保留供核查。
