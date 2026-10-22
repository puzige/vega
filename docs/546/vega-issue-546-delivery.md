# Composer 胶囊按钮交付记录

## 范围与基线

- Issue：[#546](https://github.com/puzige/vega/issues/546)；规格：[v1.1](vega-issue-546-composer-capsules.md)。
- 用户确认：只改 Compose 底栏模型选择与权限切换。其他文字按钮、图标按钮不随本卡变更。
- 开始基线：`83fd51d79940516029cdc1eaf8e81566870c877b`。
- 阶段：有效 RED 后最小实现；尚未取得同测 GREEN、尚未合并或原生手测。

## 同类样式盘点（静态事实，未改动）

| 区域 | 当前入口 | 本卡处理 |
|---|---|---|
| Composer 模型／权限触发器 | `conversation_stream/render.rs` 的 `render_model_selector` / `render_permission_status` | 本次目标 |
| Composer 上方项目／分支／后端 | `conversation_stream/utility_bar.rs` 与 `branch_selector.rs` | 已有胶囊外形，保留 |
| Composer 加号、发送／停止、上下文环 | `conversation_stream/render.rs` / `composer_actions.rs` | 保留图标控件外形 |
| 思考展开触发器 | `conversation_stream/thinking.rs` | 使用小圆角，保留 |
| 用户问答选项／动作 | `user_question_panel.rs` | 共用小圆角与交互表面，保留 |
| 权限确认卡动作 | `permission_card.rs` | 共用小圆角与交互表面，保留 |
| 侧栏顶部与组织菜单 | `sidebar/mod.rs`、`sidebar/threads_block/organization/render.rs` | 共用交互表面，保留 |
| 标题／导航与共用图标按钮 | `navigation.rs`、`icons.rs` | 共用交互表面，保留 |
| 菜单项 | `menu_list.rs` | 共用交互表面，保留 |
| 设置中的模型、服务提供方、更新及导航 | `settings/model_configuration.rs`、`model_capabilities.rs`、`provider_management.rs`、`updater.rs`、`render_impl.rs` | 共用交互表面，保留 |

这是共享样式的静态消费者分类，不表示所有这些控件都应变为胶囊，也不是全应用视觉验收。不得通过修改 `DISCLOSURE_RADIUS` 或 `InteractionExt::interactive` 全局达成本卡目标。

## 验证记录

待先行测试审查与现有 Actions 结果后补充准确提交、源码、RED/GREEN 和最终 head 证据。所有本地测试、编译与原生运行均未执行。

### 先行测试阶段检查点（未 READY）

实现写入者已停写，生产样式未改。部分测试 artifact 的组合 SHA256 为 `cf20aca19e8c0b76c32e5aaaa6474121b3fc36a751d1c6cf986ba5495d17d285`，包含 8 个源码声明的测试（尚未发现/执行）：

- `crates/vega_ui/Cargo.toml`：`3f27cebbfc927e48576b21a78e2684c05ff4da14c2a70cb56e7851cd175edde5`。
- `crates/vega_ui/src/conversation_stream/render.rs`：`ebcbfc49a85a6397817678b0658352f44e82f67407902478cd6565c6ad5d9e89`。
- `crates/vega_ui/src/conversation_stream/capsule_tests.rs`：`21ed6c243877394bb977fdc661edf92a0a6d551147d580ac897c357820697d09`。

写入者报告差异检查退出 0、只读格式检查退出 1（尚待格式化）。当前没有提交、draft PR 或 CI 结果，此 artifact 不作为可执行 RED 候选。

发现并正在审查的规格边界：基线 `model_trigger_label` 在 pending 时显示“保存中…”，而 `label_width` 在普通状态仅预留 current/Closed 与适用的 Slider 标签；短模型名进入 pending 后可能变宽。v1 要求所有 disabled 边界不动，同时要求保留原有标签宽度预留，存在解释冲突。该判断为静态证据，宽度尚未运行测量。主 Agent 已核对原始代码，并提交独立审查，不能将此既有内容变化当成胶囊外观目标 RED，也不能静默改断言求通过。

独立规格审查还明确：权限触发器没有独立 Tab/Enter 入口，pending 在请求事件边界拒绝修改；不得为测试新增产品 affordance。Full Access 模式切换不新增确认弹窗，危险操作确认属于 runtime 策略。测试与手测证据必须按真实覆盖边界陈述。

### v1.1 修订与恢复检查点

独立审查认可将 pending 文字尺寸与单纯外观状态区分，v1.1 SHA256 为 `32fdebfc00a0f125b5855c098169b0598f0993b6015fa697d57af6ba5e26512c`。同批补齐原矩阵的 content mask 可见性、真实 Tab/回焦/外点关闭/Slider→List，以及纯 runtime 危险操作策略。图标元组是生产输入证据，不冒充 SVG 像素；NoopTextSystem 不作为原生字体测量。

原写入者恢复执行时工具会话失败；主 Agent 在 `2026-10-22T16:04:14Z` 核对 HEAD 与上述三文件 SHA256 全部未变，runtime 无改动，确认旧执行终止后交给替补专职写入者。新写入者沿用相同检查点、审查结论与具体待修项，不重启已完成分析。仍未改生产外观、未取得 RED/GREEN；新版本需停写、独立审查后再进入云端。

### 修订版 Stage1 READY（待审查、未执行）

替补写入者完成 8 个 UI 测试与 1 个 runtime 纯权限测试并停写。代码 artifact 清单哈希 `ae9ee516c491ce5167d0c215cdefcccbb422215433d47a5aec69569edf8310d4`，五文件原始补丁 SHA256 `bfbc5b82d1f46b610c1c41925f21eee4dd0448c7f2239314573bea245a421d95`（45,849 bytes）。

| 文件 | SHA256 |
|---|---|
| `crates/vega_runtime/tests/full_access_permission.rs` | `4699ed5e9b3b536297edca7a5db044b382a7f50869a357fcd734af0d3f57f938` |
| `crates/vega_ui/Cargo.toml` | `3f27cebbfc927e48576b21a78e2684c05ff4da14c2a70cb56e7851cd175edde5` |
| `crates/vega_ui/src/conversation_stream/capsule_tests.rs` | `883a41319a8eb0bf81f9fbdee6a807c55c70269551a9079554be531e053584b5` |
| `crates/vega_ui/src/conversation_stream/render.rs` | `d0c1fc7b8410c13793a562beadd07b427c633757460fdadbe7d12ca332ccc038` |
| `crates/vega_ui/src/conversation_stream/thinking_slider.rs` | `ea6e522a50ddac9db10783a1ff713bca522bcd0e788a38cbf27956dc2d304d57` |

主 Agent 复核上述身份，并独立执行 `cargo fmt --all -- --check` 与 `git diff --check`，均退出 0。这些是静态检查，未编译或执行任何测试。所有生产样式和 runtime 策略未改；thinking slider 仅临时 debug selector。该版独立审查发现一个测试观察器问题：GPUI `is_contained_within` 使用右/下开区间，贴齐裁剪边缘的正常文本会误报裁剪。主 Agent 核对 GPUI 原始实现后，由写入者仅将该断言改为四边含等号比较，保留正尺寸、mask∩viewport 和全部验收含义。修复后 UI 测试 SHA256 为 `7e891183c3274291716d18fd95c4e04038b0acc54497de57e4cc1916150a67d9`，其余四文件哈希不变；逆替换可精确还原前版。主 Agent 再次执行格式/差异检查，均退出 0。该问题属于 harness，不是产品 RED；此检查点尚未运行云端测试。

### 云端有效 RED

独立短复审通过后，先行测试提交为 [`fa26cfe33c143ca7d850b8544dd5668bb6c677ab`](https://github.com/puzige/vega/commit/fa26cfe33c143ca7d850b8544dd5668bb6c677ab)，生产基线已同步 `871f91f246e90753743a256cee1eec315e74effc`（新增 Markdown quote 修复，没有改变本卡五个源码内容身份）。[Draft PR #558](https://github.com/puzige/vega/pull/558)。

- [Run 39816418068](https://github.com/puzige/vega/actions/runs/39816418068)，[Nextest job 119956009444](https://github.com/puzige/vega/actions/runs/39816418068/job/119956009444)。
- 原始命令为现有 workflow 的 `cargo nextest list --workspace --message-format json`、`cargo nextest run --workspace`。
- 原始结果：9 发现／9 执行，4 通过、5 失败、0 skipped；源码无 ignored。执行摘要 0.522s。
- 4 个通过回归：busy 请求拒绝、完整模型鼠标/键盘/菜单/焦点、权限文案/请求 guard/回焦、FullAccess 普通批准与危险操作提示。
- 4 个目标视觉失败（light/dark/disabled/narrow）在 `surface` 断言旧按钮静止填充缺失：`expected exactly one actual filled trigger quad ... got []`。
- hover 目标失败在四角断言：actual `[8.0, 8.0, 8.0, 8.0]`，expected `[14.0, 14.0, 14.0, 14.0]`。
- 独立 Clippy 失败为测试第 2 行未使用的 `gpui_kit as gpui` 导入；不是 RED 理由。只删除该导入，行为数据、fixture 和断言冻结不变，无 suppress/retry 求绿。

生产外观在上述失败实际执行前未改；原始源码与 patch 可从提交及 PR 找回。主 Agent 阅读原始日志分类后，派专职写入者修改两个触发器及专属几何 token；仍需独立实现审查与同一冻结测试 GREEN。
