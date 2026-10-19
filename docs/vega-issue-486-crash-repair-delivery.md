# #486 · ListState 崩溃修复交付记录

2026-10-19。用户最新明确指示**“你直接实现 不用等 vega”**，覆盖已发布的 Ready/Vega 默认路由。产品需求 v1 不变，执行[崩溃修复方案 v1](vega-issue-486-list-state-borrow-crash.md) B01–B05。当前阶段：5 个先行测试已写齐并静态审查，待新 draft PR 云端 RED；当前执行者 Codex，下一执行者 Codex 审查/CI。Project In progress，生产尚未修复，没有新 RED/GREEN 结论。

该专职子 agent 在此前止点仅核对接口，未写测试或生产代码；现在按用户最新授权恢复。Codex 主会话负责协调、审查、集成和 PR/CI，子 agent 单独负责代码。上一交接及诊断保留为历史，不据此声称 Vega 已领取。

## 基线与边界

- 已发布且含缺陷的最新生产基线：`b32ad22e530d27925e375df00b6bd401bf9646a3`（v0.1.174）。
- 前一轮生产未改的诊断提交：`181cabf18e7b10d7cbb19e5f2467493eadbc6ab3`；本卡原分支正常 merge 了最新 master，不改写旧测试证据。本次可执行交接提交与不可变方案链接见 Issue 最新交接。
- 沿用worktree `vega-486-markdown-link-preview`、分支 `feat/486-markdown-link-preview`。原PR#491已经合并，本次修复须新draft PR，不以旧CI代替新验证。
- 子 agent 第一阶段只允许 B01–B05 与方案列明的 cfg/dev/外部边界支持；真实 ConversationStream/ListState 保持，不用普通 ScrollHandle 替代消息列表。写齐测试后先交回主会话并停止，尚不授权生产修复。
- 生产实现范围只有 `vega_ui/src/link_preview.rs` 的滚动快照采样时机；先实际BorrowError RED，再最小移动到列表prepaint返回后的窗口根层。禁止try_borrow回退、吞panic或不登记链接绕过。

## 逐阶段证据

| 阶段 | 结果/来源 |
|---|---|
| 本轮先行测试源码/hash | [先行测试阶段](vega-issue-486-crash-test-stage.md)：5 个新 GPUI 定义，真实 ConversationStream/ListState 与 FilePreview；源码/hash 已固定，未运行 |
| 真实列表行为RED | 待新draft PR的原云端门禁；不得以最小缺失API或无关故障代替 |
| 冻结测试后的生产修复 | 尚未授权 |
| 同一断言GREEN | 尚未取得 |
| 证据固定后独立删测 | 尚未执行 |
| 最新head完整空集门禁 | 尚未执行 |
| 合并/云端发布/用户手测 | 尚未执行；原崩溃仍未修复 |

主会话通读本轮 558 行测试和 183 行外部边界 fixture，并独立复核生产投影：仅移除明确 cfg/feature 接线、只读观察及三个标记后，生产代码与 `edeed517` 基线一致。锁文件仅增加获批的十个测试包（914→924），原包身份/来源/checksum 不变。子 agent 的格式、差异、完整 offline locked metadata 均退出 0；这些静态结果不是类型检查或行为 RED。

只允许本地低CPU格式、差异、静态和离线依赖元数据检查；不得本地运行Vega、编译、Clippy、测试、benchmark或打包。云端CI沿用既有fmt/Clippy/Python/Nextest/required，默认每5分钟查询，不修改门禁。不恢复上一轮42项全套，不追加实现后测试，不重试旧head换绿。最终仍须真实会话用户手测，不关闭Issue、不Done、不清理本卡分支/worktree。
