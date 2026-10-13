# Issue #329 · 交付记录

任务：[Issue #329](https://github.com/puzige/vega/issues/329)；规格：[思考阶段标题固定前缀](vega-issue-329-thinking-prefix.md)。本卡来自 2026-10-13 用户对实际 Vega 会话的反馈；公开记录只保留抽象后的现象。

## 实现前验收矩阵

| ID | 需求/风险 | 前置状态 | 操作 | 预期可观察结果 | 层级 | 证据 | 状态 |
|---|---|---|---|---|---|---|---|
| T1 | content-only 标题不明 | 正在流式且仅有 thinking content | 追加两段普通文本及新标题 | `Thinking ·` 固定，预览变化 | GPUI | `issue329_thinking_prefix_tracks_content_and_summary` | 通过 |
| T2 | summary 优先级 | 已有 content 预览 | 追加 summary，再追加 content | 前缀不变，预览改为 summary 并保持 | GPUI | 同 T1；原文断言 | 通过 |
| T3 | 空态伪标题 | 有思考事件但预览不可读 | 发送空白/不完整 Markdown/围栏 | 只显示 `Thinking`，无空分隔符 | GPUI | `issue329_empty_preview_has_only_fixed_label` | 通过 |
| T4 | 长文本截断 | 中英文长段落 | 跨 delta 追加文本 | 总长 ≤96 标量，尾部内容可更新 | GPUI | `issue329_thinking_prefix_respects_unicode_limit_and_latest_tail` | 通过 |
| T5 | 交互与归属回归 | 思考相邻工具、正文或运行终止 | 收起/展开、检查原文和历史恢复 | 原文不变；无思考时无行；交互不回退 | GPUI | T1 展开原文；思考模块 15/15 与活动状态 12/12 | 部分：历史恢复未单独重跑 |
| T6 | 真实应用 | 合并后的 master 应用 | 用户用真实模型手测流式阶段 | 前缀稳定、动态更新、最终收起 | 原生手测 | 用户反馈 | 待用户手测 |

## 实现计划

1. 先新增可失败的定向 UI 测试，核对阶段行与思考行在 content-only、summary、空态和长文本下的可见标题。
2. 在 `vega_ui` 的预览/标题投影处形成统一标题，复用现有增量提取；阶段渲染与思考行使用同一投影，保持可访问名称一致。
3. 仅跑本卡定向测试，记录命令、退出码和原始输出摘要。云端 PR check 负责全量门禁。
4. PR 合并后交用户原生手测；手测前保持 Issue 开放且卡片为 In review。

无协议、存储或配置迁移；回滚为回退本卡 PR。

## 验证记录

- 新增 3 项回归后、实现前：`cargo nextest run -p vega_ui issue329_` 退出码 100，3 项均失败。分别检出缺少固定前缀、不可读预览仍显示「思考过程」、长标题未给前缀留预算。
- 实现与审查修订后：`cargo nextest run -p vega_ui issue329_` 退出码 0，原始输出摘录：

```text
    Starting 3 tests across 1 binary (731 tests skipped)
        PASS [   0.112s] (1/3) vega_ui conversation_stream::tests::thinking::issue329_thinking_prefix_respects_unicode_limit_and_latest_tail
        PASS [   0.141s] (2/3) vega_ui conversation_stream::tests::thinking::issue329_thinking_prefix_tracks_content_and_summary
        PASS [   0.178s] (3/3) vega_ui conversation_stream::tests::thinking::issue329_empty_preview_has_only_fixed_label
     Summary [   0.180s] 3 tests run: 3 passed, 731 skipped
```

- 相关回归：`cargo nextest run -p vega_ui conversation_stream::tests::thinking::` 退出码 0，15/15；`cargo nextest run -p vega_ui issue296_single_activity_` 退出码 0，12/12。只覆盖进程内行为，不代表真实模型或原生像素验收。
- PR #332 首轮云端 Clippy 通过，Nextest 在 2,425 项中有 2,424 项通过、1 项失败：#296 shimmer 测试仍断言旧文案 `Reviewing changes`，实际为本卡要求的 `Thinking · Reviewing changes`。修订该旧断言后，`cargo nextest run -p vega_ui issue296_run_clock_and_shimmer_stop_when_not_painted_or_terminal` 退出码 0，1/1；`cargo nextest run -p vega_ui issue329_` 退出码 0，3/3。最终云端门禁以 PR check 结果为准。
- 代码格式、diff 空白与新增代码注释检查通过；未跑本地全量门禁。真实 provider 与原生 UI 等待用户在合并后的 master 手测。
