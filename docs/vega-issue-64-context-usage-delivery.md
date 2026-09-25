# Issue #64 — 上下文占用指示交付记录

## 需求与范围

- 用户场景：在 Composer 模型选择器旁只读查看当前会话输入上下文估算。
- 数据来源：复用当前 `ConversationStream` 的输入估算及 `ContextSettings.context_limit`。
- 容量未知时保留中性圆环并说明未配置；没有估算时隐藏。
- Composer 的分支入口依据 #191 规格保留，本卡不改它的位置、可见性、交互和 BranchSelector 行为。
- 不新增 provider 请求、数据库字段、供应商容量发现或自动压缩行为；不存取 prompt 正文。

## 验收矩阵

| ID | 风险/需求 | 前置状态 | 操作 | 预期可观察结果 | 测试层级 | 状态 |
|---|---|---|---|---|---|---|
| C64-01 | 已知用量与容量 | 135000 估算、258000 设置上限 | 渲染并悬停 | 圆环可见；同一提示显示估算、52%、135k / 258k 和设置上限来源 | 纯函数 + GPUI | PASS |
| C64-02 | 容量未知 | 有估算、无有效配置上限 | 悬停或聚焦 | 显示估算及“容量未配置”；无百分比或猜测分母 | 纯函数 + GPUI | PASS |
| C64-03 | 估算未知 | 无输入估算 | 渲染 | 不挂载指示器，不显示 0 或旧会话数据 | GPUI | PASS |
| C64-04 | 超过设置上限 | 估算大于正上限 | 计算并渲染 | 提示保留真实超限百分比；圆环填充封顶 100% | 纯函数 | PASS |
| C64-05 | 整数边界 | 零、最大整数、极小正上限 | 计算与格式化 | 不除零、不溢出、不产生非有限数；输出稳定 | 纯函数 | PASS |
| C64-06 | 可访问性与焦点 | 指示器可见 | Tab 聚焦并移开鼠标 | Tab 可达；hover 与 focus 展示同一提示，辅助文本包含完整数字及来源 | GPUI | PASS |
| C64-07 | 主题和尺寸 | Light / Dark | 分别渲染 | 圆环和提示使用主题 token；控件不增加 Composer 高度 | GPUI | PASS |
| C64-08 | 更新与隔离 | 会话 A 有估算 | 切换模型或更新投影 | 仅显示当前 owner 的值；清除估算后不残留 | GPUI + 投影回归 | PASS |
| C64-09 | 运行行为 | 会话可发送 | 查看、悬停、聚焦 | 只读交互无 provider 请求，不改模型、发送状态或自动压缩 | GPUI | PASS |

## 实现计划

1. 在 `vega_ui` 新增纯显示模型，覆盖容量有效性、四舍五入百分比、超限圆环封顶和紧凑数字格式。
2. 从当前上下文控制器状态派生模型，不引入共享跨 crate 类型或新依赖。
3. 在模型选择器左侧挂载 16px 圆环与唯一的多行 tooltip；用主题 token，复用 hover / focus 状态，并设置完整 aria 文本。
4. 保持当前模型 selector popup 锚点、Composer 行高及 #191 分支入口逻辑不变。
5. 执行本卡 Nextest、fmt、diff-check、`vega_ui` Clippy。真实桌面 hover、Tab、Light/Dark 检查留待用户安装该版本后完成。

## 验证与交付

- 基线：`origin/master` `2f38c5b`（v0.1.19，包含 #157、#146、#191）。
- 分支：`feat/64-context-usage`；任务专属 worktree。
- 功能测试：`cargo nextest run -p vega_ui issue64_context_usage_` — rebase 后 PASS 11/11（504 tests skipped）。
- 格式：`cargo fmt --all -- --check` — PASS。
- Clippy：`cargo clippy -p vega_ui --all-targets -- -D warnings` — PASS。
- Diff：`git diff --check` — PASS。
- 窄布局：360px GPUI 测试确认焦点提示在视口内，且不与模型选择、发送、Composer 分支入口相交。
- Spec 偏离：无。
- 用户桌面验收：NOT RUN；需在合并后的版本确认 hover/focus、已知/未知容量、会话切换和主题。
