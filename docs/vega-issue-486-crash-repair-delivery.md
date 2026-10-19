# #486 · ListState 崩溃修复交付记录

2026-10-19。用户在诊断后明确要求“由你继续，按 Bug 流程快速修复”；产品需求 v1 不变，执行[崩溃修复方案 v1](vega-issue-486-list-state-borrow-crash.md) B01–B05。当前阶段：等待 Vega 领取先行测试；当前执行者待领取，下一执行者 Vega。交接发布后 Project 置 Ready，尚未声称 Vega 开工；生产尚未修复，没有新 RED/GREEN 结论。

此前沿用 Feature 的 Codex 子 agent 路由已撤销；该子 agent 仅核对接口，未写测试或生产代码，现已停止。Bug 按 Vega 执行、Codex 审查及 PR/CI 的角色分工推进。

## 基线与边界

- 已发布且含缺陷的最新生产基线：`b32ad22e530d27925e375df00b6bd401bf9646a3`（v0.1.174）。
- 前一轮生产未改的诊断提交：`181cabf18e7b10d7cbb19e5f2467493eadbc6ab3`；本卡原分支正常 merge 了最新 master，不改写旧测试证据。本次可执行交接提交与不可变方案链接见 Issue 最新交接。
- 沿用worktree `vega-486-markdown-link-preview`、分支 `feat/486-markdown-link-preview`。原PR#491已经合并，本次修复须新draft PR，不以旧CI代替新验证。
- Vega 第一阶段只允许 B01–B05 与方案列明的 cfg/dev/外部边界支持；真实 ConversationStream/ListState 保持，不用普通 ScrollHandle 替代消息列表。写齐测试后先交回 Codex 并停止，尚不授权生产修复。
- 生产实现范围只有 `vega_ui/src/link_preview.rs` 的滚动快照采样时机；先实际BorrowError RED，再最小移动到列表prepaint返回后的窗口根层。禁止try_borrow回退、吞panic或不登记链接绕过。

## 逐阶段证据

| 阶段 | 结果/来源 |
|---|---|
| 本轮先行测试源码/hash | 等待 Vega 领取；尚未编写、提交或运行 |
| 真实列表行为RED | 待新draft PR的原云端门禁；不得以最小缺失API或无关故障代替 |
| 冻结测试后的生产修复 | 尚未授权 |
| 同一断言GREEN | 尚未取得 |
| 证据固定后独立删测 | 尚未执行 |
| 最新head完整空集门禁 | 尚未执行 |
| 合并/云端发布/用户手测 | 尚未执行；原崩溃仍未修复 |

只允许本地低CPU格式、差异、静态和离线依赖元数据检查；不得本地运行Vega、编译、Clippy、测试、benchmark或打包。云端CI沿用既有fmt/Clippy/Python/Nextest/required，默认每5分钟查询，不修改门禁。不恢复上一轮42项全套，不追加实现后测试，不重试旧head换绿。最终仍须真实会话用户手测，不关闭Issue、不Done、不清理本卡分支/worktree。
