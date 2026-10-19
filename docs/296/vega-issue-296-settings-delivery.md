# Issue #296 Settings UI 交付记录

本记录覆盖 [统一 UI 规格](../vega-desktop-ui-unification.md) R5、R7 的设置页切片和 T9–T13。八个路由共用 section/action/status 表达；通用分组、提供方列表与详情、思考/定价卡片、用量页头、MCP/Skills 动作与状态、Agents 持久字段标签均向已有 token 收敛。保留输入实体、配置与 controller 语义、FocusHandle、保存/取消及授权文案；未操作运行中的 Vega 或真实用户配置。与冻结规格偏离：无。

## 冻结身份

- 验证完成：2026-10-11 11:16 Asia/Shanghai。
- 基线：`89e4c93842a8648fd40644f00d45bc2832fa4eed`，开始修改前已 fetch 并从最新 `origin/master` 创建隔离分支；包含 #295。
- 分支：`feat/296-settings-ui`；独立默认 Cargo target，无共享构建目录。
- 设置源文件指纹：`1265d4496e3b0106c521377f3f2f117a389e837359c87ff4d32847e13f9ab07a`。算法为按路径排序，对每个变更 Settings 文件的相对路径、NUL、文件内容、NUL 依次计算 SHA-256，含新增 `settings/tests/issue296.rs`。
- 工具：rustc 1.98.0 (`88d9e12ae` 2026-08-18)，cargo 1.98.0 (`797e8a9bc` 2026-08-05)。
- Settings 补丁依赖同卡共享 theme token 与 `interaction` 适配器；这些文件由各自唯一 owner 交付，不重复包含于本切片。独立旧基线编译所用旧滚动常量兼容项也不交付。

## 验收映射

| ID | 合成前置状态与操作 | 结果与证据 |
|---|---|---|
| T9 | 八页、Light/Dark、1403×860/1200×760/960×600；提供方在365px rail下包含八个长模型ID并进入编辑态 | `issue296_settings_eight_pages_keep_one_readable_column` 和 `issue296_provider_detail_and_editor_keep_narrow_scroll_ownership` 通过；保留两滚动区与180px列表，详情/输入/动作有界可达 |
| T10 | 真实 GPUI hover/leave 绘制、导航 focus、General 两选择器 Enter/Space、Usage 筛选/刷新键盘、busy guard | `issue296_settings_hover_and_focus_preserve_control_geometry`、`issue296_general_pickers_toggle_once_from_keyboard`、`issue296_usage_refresh_and_filters_keep_keyboard_actions` 通过；动作一次、禁用不重发；已有 Pricing/Reasoning/Skills 键盘回归通过 |
| T11 | 多行中文错误、MCP 保存禁用与不启动进程、Usage 加载/空/失败、Skills intro | `issue296_settings_long_status_aligns_icon_to_first_line` 及既有 MCP/Usage/Skills 定向测试通过；16px图标居中首行行盒，状态保持中性边界和完整文案 |
| T12 | 窄窗 Agents 原输入实体和 Skills 动作组 | `issue296_skills_actions_are_compact_and_agents_fields_stay_labeled` 通过；三个持久标签、按钮最小28px、刷新随内容收缩 |
| T13 | 打开设置及 rail 宽度、提供方保存事件与配置并发保护、tooltip、价格/思考键盘 | 既有 `r21_settings_shell`、provider save、preference worker、tooltip 等定向回归通过；测试使用合成数据与进程内替身 |

## RED → GREEN

初始命令：

```sh
cargo nextest run -p vega_ui issue296
```

`/private/tmp/vega-296-settings-red-fixed.log` 的有界原始输出：

```text
Starting 3 tests across 1 binary (563 tests skipped)
FAIL ... issue296_skills_actions_are_compact_and_agents_fields_stay_labeled
refresh must hug its label
FAIL ... issue296_provider_detail_and_editor_keep_narrow_scroll_ownership
Summary ... 3 tests run: 1 passed, 2 failed, 563 skipped
```

Skills 失败证明旧实现的刷新按钮撑满内容列。第二个失败是测试夹具只有两个模型而无法形成列表滚动；改为八个长模型ID，仍保留原滚动断言，不作为产品 RED 证据。更早的测试编译错误（动态 debug selector 生命周期）在执行断言前修正，日志为 `/private/tmp/vega-296-settings-red.log`。

实现后基础3项 GREEN；随后补齐状态、绘制、键盘及保存回归。过程中修复的测试夹具借用错误及旧 theme 常量缺失均在独立树完成，未改变业务断言或增加重试。最终综合命令：

```sh
cargo nextest run -p vega_ui -E 'test(settings::tests::issue296) | test(issue296_usage) | test(r21_settings_shell) | test(small_provider_detail_retains_url_height_with_multiple_models) | test(issue82_provider_tooltip_remains_visible_in_narrow_windows_and_themes) | test(pricing_actions_are_tab_reachable_and_enter_space_activate_once) | test(reasoning_unknown_template_is_tab_reachable_and_enter_activates) | test(issue74_skills_settings_import_is_keyboard_reachable) | test(issue87_s21) | test(provider_submit_uses_owned_backends_and_emits_only_after_save) | test(preference_worker_preserves_new_provider_fields_and_detects_same_field_conflict) | test(settings_usage_real_render_controls_refresh_empty_and_failure) | test(issue73_mcp_settings_create_stays_disabled_and_never_launches_on_save)'
```

退出码0，原始日志 `/private/tmp/vega-296-settings-final.log`：

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 9.14s
Starting 21 tests across 1 binary (552 tests skipped)
PASS ... issue296_settings_long_status_aligns_icon_to_first_line
PASS ... issue296_skills_actions_are_compact_and_agents_fields_stay_labeled
PASS ... issue296_settings_hover_and_focus_preserve_control_geometry
PASS ... issue296_provider_detail_and_editor_keep_narrow_scroll_ownership
PASS ... issue296_general_pickers_toggle_once_from_keyboard
PASS ... issue296_usage_refresh_and_filters_keep_keyboard_actions
PASS ... issue296_settings_eight_pages_keep_one_readable_column
Summary [0.570s] 21 tests run: 21 passed, 552 skipped
```

最后将 General 原布尔开关的表面反馈接入共享 motion 后，仅重跑受影响的设置矩阵与 shell：

```sh
cargo nextest run -p vega_ui -E 'test(settings::tests::issue296) | test(r21_settings_shell)'
```

退出码0，原始日志 `/private/tmp/vega-296-settings-switch-final.log`：

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 6.27s
Starting 7 tests across 1 binary (566 tests skipped)
PASS ... r21_settings_shell_opens_general_and_tracks_sidebar_width
PASS ... issue296_settings_long_status_aligns_icon_to_first_line
PASS ... issue296_skills_actions_are_compact_and_agents_fields_stay_labeled
PASS ... issue296_settings_hover_and_focus_preserve_control_geometry
PASS ... issue296_provider_detail_and_editor_keep_narrow_scroll_ownership
PASS ... issue296_general_pickers_toggle_once_from_keyboard
PASS ... issue296_settings_eight_pages_keep_one_readable_column
Summary [0.518s] 7 tests run: 7 passed, 566 skipped
```

原始日志 SHA-256：

| 日志 | SHA-256 |
|---|---|
| `vega-296-settings-red-fixed.log` | `3f57e3e67a5e6d4b934d8acf36fc7b99ac83507b9a2cc828e9a1f8a403a78e25` |
| `vega-296-settings-final.log` | `6b878c0712896bd6a58c2627aae427a23337d47c4e564e2325504b911660dc1c` |
| `vega-296-settings-switch-final.log` | `da7ada7829c780438f5d05911fbc6ee70f4af31a0bc2ac7168649ca6427714a2` |

构建唯一附带提示为既有传递依赖 `block v0.1.6` 的 future-incompatibility 警告。未运行 workspace 全量测试；完整门禁由云端 PR check 承担。

## 剩余验证与风险

- 上述结果是进程内 GPUI 布局、绘制、事件及合成状态证据，未作为新构建真实原生像素或系统焦点体验的验收。
- 用户在任务空闲并更新后手测：八页长内容滚动、Light/Dark、窄窗、hover进入/离开、键盘 focus、系统 Reduce Motion、真实保存/取消及返回原任务。当前运行的 Vega 未重启、未停止、未安装替换。
- 共用 motion 的计时/反转/停帧和系统桥接 mock 由 interaction owner 单独交付。此切片通过适配器接入，不复制计时器或更改 controller。
- 提供方列表和详情仍各自滚动；本轮仅统一布局和动作层级，不改变其信息架构。真实超长本地化文案和字形光学对齐仍需手测。
- 回滚按同卡整体 revert，避免只撤 Settings 而留下共享契约的不匹配。补丁尚待主 agent 集成、云端检查和合并；不声称已发布。
