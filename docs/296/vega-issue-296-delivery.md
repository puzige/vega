# Issue #296 · 全应用 UI 组合交付

[规格](../vega-desktop-ui-unification.md)先于实现。主 agent 观察当前原生应用、确定边界并审查；三个专职实现 agent 在独立 worktree 和默认 Cargo target 中分别负责信息流、设置、共享交互，再合并到同一功能分支。基线为已包含 #295 的 `89e4c93842a8648fd40644f00d45bc2832fa4eed`。

## 实现结果

- 信息流：正文与 Composer 同列，锚点移入 gutter，回到底部位于固定导航行；消息、过程、错误及权限统一间距、字号和状态层级。
- 八个设置页：统一中文导航、内容列、分组、按钮、状态提示；保留提供方列表/详情、输入实体、保存与确认路径，Agents 增加持久字段标签。
- 共享控件：16px 图标居中槽；120ms hover 进入/离开，可中途反转且结束停帧；按压、选择、禁用、键盘焦点不改变几何。只读同步 macOS Reduce Motion。
- 保留 Composer 底栏不显示分支的 #294/#295 行为。无新依赖、数据迁移或模型/权限配置修改。

细分证据：[信息流](vega-issue-296-stream-delivery.md)、[设置](vega-issue-296-settings-delivery.md)、[交互与图标](vega-issue-296-interactions-delivery.md)。这些记录冻结各切片当时的结果，组合验证以下表为准。

## 首轮组合验证

组合验证在独立实现树执行，按文件清单复制集成树的全部变更源码，两个树使用相对路径、NUL、内容、NUL 的 SHA-256 校验一致；不共享 Cargo target。

首轮40个源码文件的指纹：`0231929fa87a28d8d96772353b6bd3af2895953ba26dbf39aa73e80a385033ea`。主集成树与执行验证树相同。

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| 下列联合定向 nextest | exit 0；295/295 PASS，566 skipped |
| `git diff --check` | exit 0 |
| 主程序启动/激活桥接编译 | 共享片 `cargo check -p vega` exit 0 |

精确命令（仅本卡直接相关过滤器）：

```sh
cargo nextest run -p vega_ui -p vega_theme -p vega -E 'test(issue296) | test(issue98_scroll_button) | test(issue72_message_anchor_navigation) | test(issue78_message_bubbles) | test(issue147_markdown_selection) | test(issue146_run_activity) | test(issue151_latest_activity) | test(issue70_tool_activity) | test(permissions_cards) | test(permission_card::tests) | test(issue79_composer_error) | test(utility_bar) | test(conversation_stream::tests::thinking) | test(scroll_follow) | test(conversation_stream::tests::attachments) | test(conversation_stream::tests::core_flow) | test(permission_picker) | test(model_picker_levels) | test(r64_popup_deferred) | test(r68_popup_dismiss) | test(conversation_stream::tests::composer_actions) | test(conversation_stream::tests::hydration) | test(conversation_stream::tests::timeline) | test(settings::tests::issue296) | test(issue296_usage) | test(r21_settings_shell) | test(small_provider_detail_retains_url_height_with_multiple_models) | test(issue82_provider_tooltip_remains_visible_in_narrow_windows_and_themes) | test(pricing_actions_are_tab_reachable_and_enter_space_activate_once) | test(reasoning_unknown_template_is_tab_reachable_and_enter_activates) | test(issue74_skills_settings_import_is_keyboard_reachable) | test(issue87_s21) | test(provider_submit_uses_owned_backends_and_emits_only_after_save) | test(preference_worker_preserves_new_provider_fields_and_detects_same_field_conflict) | test(settings_usage_real_render_controls_refresh_empty_and_failure) | test(issue73_mcp_settings_create_stays_disabled_and_never_launches_on_save) | test(issue296_interaction) | test(issue296_native) | test(issue296_navigation) | test(shared_search_control) | test(r31_sidebar) | test(r38_organization) | test(r41_search) | test(r48_) | test(r42_mounted_sidebar) | test(r157_) | test(menu_lists) | test(r14_real_root_sidebar_pointer_then_keyboard_preserves_toggle_and_editor_focus) | test(r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries) | test(r45_header_cluster_renders_three_stable_slots) | test(r45_composer_centering_and_geometry) | test(r46_slot_cluster_is_window_anchored) | test(r46_overlay_and_rail_start_below_header_band) | test(r46_top_band_headers_reserve_the_cluster) | test(r47_top_band_pane_actions_keep_the_slot_gutter) | test(r46_overlay_open_slot_clicks_still_work) | test(r51_workspace_tabs_keep_frozen_geometry_and_close_hitbox) | test(r51_inactive_close_follows_parent_focus_and_group_hover)'
```

首轮日志的有界原始输出：

```text
 Nextest run ID 08538e92-d8e9-408b-b237-779961ff790d with nextest profile: default
    Starting 295 tests across 4 binaries (566 tests skipped)
     Summary [   2.996s] 295 tests run: 295 passed, 566 skipped
```

原始日志 `vega-296-integrated-nextest-final.log` SHA-256：`95f9cd89f4a10a5c810bfd572f32318c8289433a2a4a17510bce3842d283f989`。日志仅本地保留，公开摘录不包含编译器打印的本机路径。

首轮相同过滤器为294 PASS/1 FAIL：根窗口旧 `r45_composer_centering_and_geometry` 仍以16px计算内缩，实际R1已规定36px。仅更新该测试的精确公式与两侧最小内缩断言，保留原容差、居中、宽度一致、非重叠与最小高度检查。修复后完整相同集合295/295通过；不删除、ignore或重试测试。


## CI 修复后的组合验证

最终42个源码文件在集成树与独立验证树的指纹一致：`610a887a2350c2f43e02f49eacd10b5527ec5e4637a00ce3bb5a40c000ebbd52`。本次命令包含首轮集合及云端检出的直接相关路径：

```sh
cargo nextest run -p vega_ui -p vega_theme -p vega -E 'test(issue296) | test(issue98_scroll_button) | test(issue72_message_anchor_navigation) | test(issue78_message_bubbles) | test(issue147_markdown_selection) | test(issue146_run_activity) | test(issue151_latest_activity) | test(issue70_tool_activity) | test(permissions_cards) | test(permission_card::tests) | test(issue79_composer_error) | test(utility_bar) | test(conversation_stream::tests::thinking) | test(scroll_follow) | test(conversation_stream::tests::attachments) | test(conversation_stream::tests::core_flow) | test(permission_picker) | test(model_picker_levels) | test(r64_popup_deferred) | test(r68_popup_dismiss) | test(conversation_stream::tests::composer_actions) | test(conversation_stream::tests::hydration) | test(conversation_stream::tests::timeline) | test(settings::tests::issue296) | test(issue296_usage) | test(r21_settings_shell) | test(small_provider_detail_retains_url_height_with_multiple_models) | test(issue82_provider_tooltip_remains_visible_in_narrow_windows_and_themes) | test(pricing_actions_are_tab_reachable_and_enter_space_activate_once) | test(reasoning_unknown_template_is_tab_reachable_and_enter_activates) | test(issue74_skills_settings_import_is_keyboard_reachable) | test(issue87_s21) | test(provider_submit_uses_owned_backends_and_emits_only_after_save) | test(preference_worker_preserves_new_provider_fields_and_detects_same_field_conflict) | test(settings_usage_real_render_controls_refresh_empty_and_failure) | test(issue73_mcp_settings_create_stays_disabled_and_never_launches_on_save) | test(issue296_interaction) | test(issue296_native) | test(issue296_navigation) | test(shared_search_control) | test(r31_sidebar) | test(r38_organization) | test(r41_search) | test(r48_) | test(r42_mounted_sidebar) | test(r157_) | test(menu_lists) | test(r14_real_root_sidebar_pointer_then_keyboard_preserves_toggle_and_editor_focus) | test(r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries) | test(r45_header_cluster_renders_three_stable_slots) | test(r45_composer_centering_and_geometry) | test(r46_slot_cluster_is_window_anchored) | test(r46_overlay_and_rail_start_below_header_band) | test(r46_top_band_headers_reserve_the_cluster) | test(r47_top_band_pane_actions_keep_the_slot_gutter) | test(r46_overlay_open_slot_clicks_still_work) | test(r51_workspace_tabs_keep_frozen_geometry_and_close_hitbox) | test(r51_inactive_close_follows_parent_focus_and_group_hover) | test(issue64_context_usage) | test(issue64_context_settings) | test(issue72_loaded_anchor) | test(conversation_stream::tests::composer_actions::issue150) | test(issue69_environment_selection) | test(utility_menu_coordination) | test(issue73_mcp_bearer)'
```

原始输出（exit 0）：

```text
 Nextest run ID ad69f518-2462-431a-bdea-f82774ac429f with nextest profile: default
    Starting 323 tests across 4 binaries (541 tests skipped)
     Summary [  39.784s] 323 tests run: 323 passed, 541 skipped
```

原始日志 `vega-296-final-ci-nextest.log` SHA-256：`bea18caedbcdd02d646582aadb2c7454bcacc18fc47e718009f996c5f1b55ccb`。此命令中 issue150 的命名空间未匹配根窗口用例，因此另以精确测试名补跑；不把未被选择的用例计为已验证。

精确补跑（同一42文件指纹，exit 0）：

```sh
cargo nextest run -p vega -E 'test(issue150_running_stop_button_is_brand_blue_and_send_is_unchanged)'
```

```text
 Nextest run ID 42481680-6404-4ded-af91-6a6264424435 with nextest profile: default
    Starting 1 test across 2 binaries (243 tests skipped)
     Summary [   0.290s] 1 test run: 1 passed, 243 skipped
```

`vega-296-final-ci-issue150.log` SHA-256：`acec6278dc70208319dfad2d29f2f5b4a26da2d13db1c732d5928fac2b328f5e`。最终定向证据为323项联合集合加1项精确补跑，共324项通过。

最终同指纹限定静态检查：`cargo clippy -p vega_ui -p vega --all-targets --no-deps -- -D warnings` exit 0，21.57s；`cargo fmt --all -- --check` 与 `git diff --check` 均 exit 0。Clippy原始日志 `vega-296-final-ci-clippy.log` SHA-256：`020113f2526ba72106619799d4bf686e4bf6bc0a3fa6a5c8cc753ae3bc09d920`。仅有既有传递依赖 `block v0.1.6` 的 future-incompatibility 提示。完整workspace门禁仍由云端PR检查承担。

## 审查与验收边界

云端 Clippy 先检出双向迭代器的 `.last()`，中间改为 `.next_back()` 后又检出 `filter_next`；最终合并为等价 `.rfind()`。断言不变，限定 `vega_ui` Clippy 与直接 Native 用例通过。完整 nextest 在 `af33502` 检出8项失败，均先在独立树复现再修复，保留失败日志，不自动重试。

| 路径 | 根因与修复 | 独立定向结果 |
|---|---|---|
| 发送可用态、Environment取消选择、Light/Dark分支菜单键盘高亮 | 动效层把语义变化当作hover过渡，并覆盖调用端显式背景；语义变化现在即时更新并取消在途动画，后置背景作为rest保留 | 四个原失败断言不改、不加等待；生产root 2/2、共享及菜单12/12、原生入口/导航5/5 PASS |
| 窄窗上下文用量提示、Composer高度 | 旧29px用量外框与新28px底栏不一致；固定288px提示越出窄窗。统一外框token并用原生anchored约束到窗口，保留deferred及hover桥接 | 上下文提示14项与真实锚点路径共15/15 PASS；Settings往返4/4 PASS |
| 已加载历史锚点 | 旧测试仅按7px识别选择标记；新规格中选择与当前位置均6px | 按精确token、accent及绘制次序识别两枚标记；真实消息、视口进入、请求次数等断言全部保留 |
| MCP Bearer保存 | 新分组间距使按钮位于滚动视口下方，旧测试点击不可见坐标 | 真实滚动后点击；新增960×600 Dark/365px侧栏下Tab→Enter；2/2 PASS，保留遮罩、清空、凭据与SQLite无secret断言，并验证保存不启用服务器 |

共享层修复经另一专职agent独立只读审查，无阻断项；不更改模型、权限或持久化逻辑。两条旧几何测试按冻结新规格更新，未删除、ignore或放宽业务与安全断言。

独立审查确认 rail/预览、历史定位、尾随恢复、权限所有选项与 fail-closed guard、Composer send/stop/权限可达性以及过程折叠状态链路保留。交互审查检出的重复 hover、焦点边框位移、原生 Button 拖出按压色残留、tracked FocusHandle 未进入 Tab 顺序均已加入实际绘制/事件回归。

本地仅运行本卡直接相关的定向测试，不运行 workspace 全量。完整 fmt、Clippy、nextest 由 PR 的云端 `pr-check` 承担，结果以 GitHub check 页面为准。与冻结规格偏离：无。

当前原生观察来自安装的 0.1.64，只证明旧界面问题。未停止、重启、安装替换或向真实任务发送消息；测试未修改用户运行时数据。公开材料不包含原始会话内容、截图、服务地址、凭据或用量。

合并后 Issue 保持开放，Project 进入 In review。用户在真实任务空闲后更新并手测：长混合会话滚动与追尾、Light/Dark、窄窗八个设置页、hover/按压/Tab、系统 Reduce Motion、真实保存/取消和权限提示。新代码的原生像素、字体光学对齐与系统集成在用户验收前不标为通过；本卡 worktree 与本地分支保留。回滚为 revert 整个 UI 提交，无数据迁移。
