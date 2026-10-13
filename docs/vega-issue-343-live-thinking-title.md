# Issue #343 · 运行中的思考标题跟随后续可读内容

来源：[Issue #343](https://github.com/puzige/vega/issues/343)。用户看到思考详情仍持续追加，但阶段标题停在较早的短语。当前 #71 提取器一旦遇到 Markdown 小标题或整行加粗，就拒绝后续普通段落；同时已有 summary 永远优先于后来到达的 thinking。这两条规则都能造成该现象，仅凭截图不能确认实际命中哪一条。

2026-10-13 用户手测修订：PR #346 的“最近可读来源优先”会让后续原始 thinking 覆盖已收到的 summary，与期望不符。本规格改为 summary 替换模式：出现可读 summary 后，标题只由 summary 更新；常规展开内容也只展示 summary。原始 thinking 仍可接收并保存在当前块，但不主动展示。该修订取代本文旧版“较晚来源优先”及 [#71 规格](vega-issue-71-reasoning-summary.md) R1–R3 中“小标题保持到下一小标题”的规则；#71 的 summary 优先原则继续适用。#329 的[前缀回退](vega-issue-329-prefix-rollback.md)继续适用：不添加固定 `Thinking ·`。其余 #71 的提取安全、资源上限及 #296 的阶段布局、折叠、工具/正文归属规则不变。

## 行为契约

- 同一来源内，新的可读 Markdown 小标题或普通段落都能更新标题。未完成的 Markdown 标记、空白、代码围栏内部内容不清除已有可读标题，也不抢占另一来源。
- summary 与 thinking content 各自增量提取预览。尚无可读 summary 时，thinking 的最新可读片段驱动标题；一旦 summary 产生可读预览，本块进入 summary 替换模式，后续 thinking 不能再抢占标题。后续 summary 继续更新标题。空白、未完成标记或代码围栏不触发标题切换；无任何可读预览时沿用「思考过程」占位，不制造思考块。
- 有非空 summary 时，常规展开内容与复制内容以 summary 替换原始 thinking，而非拼接两份内容；后续 thinking 增量不会把原文重新放回展示。summary 尚无可读预览时保留已有可读标题，避免流式标记造成标题闪空。没有 summary 时，展开内容仍为原始 thinking。当前块保留真实收到的两种文本，不虚构摘要，也不改变事件归属和存储边界。
- 标题最多 96 个 Unicode 标量；长小标题保留开头，长普通段落保留最新尾部。增量处理和 UTF-8 分片规则沿用 #71，不扫描整个思考原文，不增加新的模型请求。
- 会话阶段行与独立思考行的运行中标题使用同一当前预览；可访问名称与可见标题一致。终止后保留最后一次有效标题；原有展开/折叠、键盘行为和 shimmer 样式不变，不因 summary 到达而主动展开详细 thinking。
- 不改 provider 协议、事件、存储、思考原文、工具、正文或安装方式；公开证据只用合成文本。

## 验收矩阵

| ID | 输入与状态 | 预期 | 验证 |
|---|---|---|---|
| T1 | content 先有 `## 旧小标题`，随后普通段落分片继续 | 标题逐步转为最新可读段落，不停在小标题 | 进程内预览与 GPUI 回归；云端 PR Check |
| T2 | content → summary → content → summary | 标题先跟随 content，再切换并持续跟随 summary；后续 content 不覆盖；无固定前缀 | 进程内 GPUI 回归；云端 PR Check |
| T3 | summary 先追加空白、未完成标记或围栏代码 | 既有可读标题保留，直到 summary 产生可读预览 | 单元与 GPUI 回归；云端 PR Check |
| T4 | 超长中英文段落、跨 UTF-8 delta、标题行 | 不超过 96 标量，长普通段落显示最新尾部，标题行保留开头 | 单元与现有界限回归；云端 PR Check |
| T5 | 同一块同时收到 summary 和原始 thinking，含后续 thinking 增量 | 展开与复制只显示 summary；原始 thinking 不重复出现；无 summary 时仍显示 thinking | 进程内 GPUI 回归；云端 PR Check |
| T6 | 阶段/独立思考行在运行、终止、展开、收起时 | 可见标题与可访问名称一致；summary 不主动展开详情，交互不变 | 进程内 GPUI 回归；云端 PR Check |
| T7 | 合并后真实模型持续输出思考并返回 summary | 有 summary 后上方阶段标题只随 summary 更新，详情只显示 summary | 用户原生手测 |

## 实现计划

仅调整 `ThinkingBlock` 的展示来源选择和展开文本来源，并更新现有合成流 GPUI 回归。保留两路原始缓冲、预览上限及事件协议。先修改会在 PR #346 语义下失败的混合来源断言，再实现 summary 优先与 summary-only 展开；本地只做快速差异、格式/静态检查，完整编译和测试由云端 PR Check 执行。回滚为回退本次修订 PR，恢复 PR #346 行为。

本地仅进行快速静态检查，不编译、构建或运行耗时测试。云端 PR Check 执行格式、Clippy、nextest；不把未运行的本地测试标为通过。回滚为回退本卡 PR。
