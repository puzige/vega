# Issue #82 Provider 说明收纳交付记录

## Freeze

- 已验证时间：2026-09-25 14:28 UTC / 2026-09-25 22:28 CST。
- 分支：`feat/82-provider-help-copy`；实现前及交付前均检查 `origin/master`，交付前未发现基线漂移。
- 任务契约：[冻结规格](vega-issue-82-provider-help-copy.md)。实现与冻结范围一致。
- 实现文件源码差异 SHA-256（不含本交付记录）：`f9b4148708ee4d583eae4f343da50ef3c433d8746e9d0a352c15f94903d301a2`。
- 环境：Darwin arm64；rustc 1.98.0；cargo 1.98.0；git 2.55.0。

## 验收矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
|---|---|---|---|---|---|---|---|
| H1 | 说明文字不再常驻；提示文本和冻结规格一致 | 已选中供应商 | 悬停发现帮助图标，再移出 | 图标附近出现同一说明，移出后隐藏；原段落已从详情布局移除 | GPUI | `issue82_provider_help_opens_on_hover_and_focus_and_keeps_credential_states` | PASS |
| H2 | 键盘用户能读取说明，焦点离开后提示隐藏 | Provider 页面已渲染 | 聚焦发现帮助图标，再把焦点移到模型帮助图标 | 两个位置均显示同一提示；原位置提示随失焦隐藏；控件有 focus-visible 样式和无障碍名称/描述 | GPUI | `issue82_provider_help_opens_on_hover_and_focus_and_keeps_credential_states` | PASS |
| H3 | 凭据状态与 Pi 导入入口保持原状 | 一个本地已存储 key 的供应商和一个缺失 key 的供应商 | 分别检查两个供应商 | 存储/缺失状态仍按原分支渲染，Pi 导入入口仍可见；不把 key 正文放入视图 | GPUI | 新增的 #82 用例；`mounted_pi_import_*` 四项回归 | PASS |
| H4 | 帮助图标不遮挡测试、编辑和删除操作 | 960px 窄窗口、至少一个模型 | 检查发现和模型行控件边界，并切换深浅主题 | 图标紧邻对应操作；按钮之间不重叠；图标在窗口内 | GPUI | 新增的 #82 用例 | PASS |
| H5 | 模型发现/测试行为与详情行布局不回退 | 固定配置与 mock provider transport | 发现模型、测试模型；在小窗口检查 URL 与滚动区 | 既有请求路径和结果保持；窄窗下 URL 行完整，内容溢出由 viewport 滚动 | GPUI | `pointer_settings_uses_real_service_config_and_mock_transport`；`small_provider_detail_retains_url_height_with_multiple_models` | PASS |

## 实现与兼容性

- 移除 Provider 详情中的常驻说明段落；在“发现模型”和各模型“测试”旁加入紧凑帮助图标。
- hover 与键盘 focus 共用相同的主题 tooltip 和冻结文案。提示不需要点击，也不启动网络请求。
- 保留凭据显示分支、Pi Agent 导入控件、发现/测试按钮顺序和请求处理。没有配置格式、持久化、密钥存储或更新协议变化。
- 影响模块：共享图标、Provider Settings 渲染/焦点句柄和对应 GPUI 测试。
- 失败路径与状态切换通过新帮助提示、stored/missing 状态断言及已有 Pi 导入成功/失败/进行中测试覆盖。
- 非目标和回滚方式：不改测试请求/Provider 网络行为；如需回滚，只需撤销本实现变更，无数据迁移。

## 实施步骤

1. 复核 #82 冻结规格、Provider 详情和现有 GPUI fixture。
2. 实现图标/提示并保持凭据和 Pi 导入分支不变。
3. 添加 hover/focus、凭据状态及窄窗布局回归；收小旧滚动回归的窗口以保留真实 overflow 情形和原断言。
4. 仅运行 #82、Provider 操作、Pi 导入和详情布局相关的定向 nextest；本地不跑全量测试。
5. 检查 rustfmt 与 diff，提交本卡并创建关联 PR；云端完整 PR gate 待 PR 页面结果。

## Results

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
|---|---|---|---|---|---|
| #82 hover/focus、凭据状态、窄窗主题和相邻操作 | GPUI targeted | `cargo nextest run -p vega_ui -E 'test(issue82_provider_help_opens_on_hover_and_focus_and_keeps_credential_states) | test(mounted_pi_import_) | test(pointer_settings_uses_real_service_config_and_mock_transport) | test(small_provider_detail_retains_url_height_with_multiple_models)'` | exit 0；7 passed，476 skipped | 0.192s 测试运行时间 | Nextest `3be52311-94df-4bc7-aa44-3a5c84c72753` |
| 格式 | formatter | `cargo fmt --all -- --check` | exit 0；stdout 空 | — | — |
| 空白/冲突标记 | git | `git diff --check` | exit 0；stdout 空 | — | — |

定向 nextest 原始输出：

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.30s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 3be52311-94df-4bc7-aa44-3a5c84c72753 with nextest profile: default
    Starting 7 tests across 1 binary (476 tests skipped)
        PASS [   0.066s] (1/7) vega_ui settings::provider_management::tests::mounted_pi_import_failure_is_visible_and_does_not_enable_provider
        PASS [   0.067s] (2/7) vega_ui settings::provider_management::tests::mounted_pi_import_has_explicit_reachable_action_and_idle_does_not_read_source
        PASS [   0.076s] (3/7) vega_ui settings::provider_management::tests::small_provider_detail_retains_url_height_with_multiple_models
        PASS [   0.076s] (4/7) vega_ui settings::provider_management::tests::mounted_pi_import_success_shows_setup_status_enables_provider_and_keeps_key_blank
        PASS [   0.088s] (5/7) vega_ui settings::provider_management::tests::mounted_pi_import_in_progress_disables_action
        PASS [   0.106s] (6/7) vega_ui settings::provider_management::tests::issue82_provider_help_opens_on_hover_and_focus_and_keeps_credential_states
        PASS [   0.192s] (7/7) vega_ui settings::provider_management::tests::pointer_settings_uses_real_service_config_and_mock_transport
────────────
     Summary [   0.192s] 7 tests run: 7 passed, 476 skipped
```

格式与 diff 检查命令均返回 0，原始 stdout 为空。

### 失败复现与修复记录

- 首次 `cargo fmt --all -- --check` 返回 1，仅报告实现文件的格式化差异；运行 `cargo fmt --all` 后复查通过。
- 首次定向编译因 gpui-kit 0.6.0 未提供 `IconName::CircleHelp` 而返回 101：`error[E0599]: no variant, associated function, or constant named 'CircleHelp' found for enum 'IconName'`。改为项目既有 Lucide SVG 约定的内联问号圆形图标，定向编译通过。
- 首次运行 `small_provider_detail_retains_url_height_with_multiple_models` 失败：`overflow must expand the scrollable flow: flow=Size { 412px × 475px }, viewport=Size { 412px × 475px }`。说明移除常驻段落后，该 600px 窗口的内容不再发生滚动溢出。将测试窗口高度减至 560px，保留原来的完整 URL 行和 overflow 断言；复测通过。
- 流程偏离：新增 GPUI 回归用例是在实现改动后补入，而非先运行失败用例再实现；最终定向回归集全部通过，未对断言放宽。
- `cargo clippy --workspace`、workspace 全量 nextest、真实 Provider 网络和安装后桌面 Computer Use 未在本地运行；云端 PR gate 和 master 集成后的真实桌面验收分别依既有流程完成。

## Residuals

- `NOT RUN`：云端 PR gate，等待 PR 建立后执行。
- `NOT RUN`：master 集成后的桌面 Computer Use 和真实服务验收；依仓库验收规程由主控/用户在 master 完成。
- `NOT RUN`：workspace 全量测试和全量 Clippy；本地按规程只执行本卡定向 nextest，PR gate 承担完整门禁。
- spec 偏离：无。
