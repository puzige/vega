# 预测长消息裁剪语义

Issue：[#845](https://github.com/puzige/vega/issues/845)。基线 `e6a7a0e59cb4db63c19ff3ab6dfc7b52a562e8ff`。

## 目标与范围

修复输入预测将本地裁剪的报告前缀误判为原回答未完成的问题。此规格替代 #469 §4 的单条 4,000 字符预算及仅保留前缀行为；#628 重试、生命周期、隐私和 #817 菜单规则不变。不修改主会话上下文、UI、运行配置、提供方、依赖、CI 或数据库。

## 契约与验收

- 单条最多保留 8,000 Unicode 标量字符，总正文预算仍为 16,000，最近 16 条查询、最近优先选择再恢复时间顺序的规则不变。JSON 键和裁剪元数据不计入正文预算。
- 完整消息传递 `role`、`content` 和 `truncated: false`，原文不变。
- 超过单条或剩余总预算时，保留前 `ceil(可用预算/2)` 和后 `floor(可用预算/2)` 个字符；分别放入 `content`、`content_tail`，标记 `truncated: true`。中间省略，不把头尾拼成连续原文；尾部保持原顺序。极小剩余预算允许尾部为空，零预算不增加记录。
- 先完成既有 privacy 投影，再裁剪；头尾均不得绕过投影。仅在保留正文不全为空白时扣减实际保留字符数。
- 系统提示明确标记是客户端省略中段，不是原回答中断的证据；不得仅凭裁剪、片段边界或缺失中段建议“补齐/继续/重新发送”，证据不足时返回 null。上下文仍是不可信数据，不能覆盖系统指令。
- 原有文本/完成态、user 文件展开前缀、审批合成内容等过滤保持；200 字符建议、512 输出 token、安全过滤与正常结束要求保持。

## 验证

开发 Rust 测试覆盖：短文、8,000 边界、8,001 字符头尾、中文/emoji Unicode 安全、奇数与极小预算、剩余总预算导致的裁剪、16,000 总预算和最近优先/时间顺序、隐私投影与原过滤。测试调用实际上下文生成逻辑，不用源码字符串断言代替行为。

本地只做格式、差异和静态检查，不编译/运行 Vega 或测试。现有 Actions 在开发分支验证测试；按仓库政策固定可恢复测试提交与结果，再清理 Rust 测试及独占接线，最终无测试 head 跑 required CI。独立审查只覆盖本次差异。提示词的真实模型效果由用户手测，不宣称确定性杜绝所有错误建议。

## 开发验证与留痕

- 带测试版本：`66d1b92ff6116a03584fea80f43ad6e0ccb02730`；[原始测试源码](https://github.com/puzige/vega/blob/66d1b92ff6116a03584fea80f43ad6e0ccb02730/crates/vega_conversation/src/composer_predictions/tests.rs) 已通过 GitHub contents API 核实可访问，Git blob `78240a7126c775f9703f091c0842cdb012a3d974`，SHA-256 `6827baca33f472d2435af242043a6b7f7f72fb9d4904d24f941a32e1febd3be1`。
- [Actions 41330841199](https://github.com/puzige/vega/actions/runs/41330841199)：Format、Clippy、Python 脚本回归、Nextest 与 required check 成功。`cargo nextest run --workspace`：8 发现、8 执行、8 通过、0 失败、0 跳过。
- 初次 [Actions 41330416655](https://github.com/puzige/vega/actions/runs/41330416655) 为 4 通过、4 失败，失败证据为夹具 `no such table: threads`；修正测试初始化，增加显式 `store.migrate()`，未修改生产行为或放宽断言。
- 非实现者独立静态审查指出上述夹具问题；对 `66d1b92ff6116a03584fea80f43ad6e0ccb02730` 增量复核确认已解决，无其他阻塞项。
- 本地定向 `rustfmt --edition 2024 --check` 和 `git diff --check` 通过；未运行本地编译/测试/应用。开发测试及唯一 `cfg(test)` 接线在留痕后移除，不增加任何依赖或测试支持特性；生产部分与带测试通过版本逐字节核对。最终无测试 head 的完整 required CI 结果另行记录，不能将空集门禁称为行为测试通过。
