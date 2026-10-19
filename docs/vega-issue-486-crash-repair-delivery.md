# #486 · ListState 崩溃修复交付记录

2026-10-19。用户最新明确指示**“你直接实现 不用等 vega”**，覆盖已发布的 Ready/Vega 默认路由。产品需求 v1 不变，执行[崩溃修复方案 v1](vega-issue-486-list-state-borrow-crash.md) B01–B05。当前阶段：最小修复首轮 4 通过/1 失败，B04 前置布置已留证审查并仅作一处修正，待新 head 云端完整 GREEN；当前/下一执行者 Codex。Project In progress，尚未取得完整 GREEN 或发布结论。

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
| 本轮先行测试源码/hash | [先行测试阶段](vega-issue-486-crash-test-stage.md)，提交 `4b60f0b28eb1c7734d51dc1003d9141d45515872`；5 个新 GPUI 测试及外部 fixture 已冻结 |
| 真实列表行为 RED | [run 39222176469](https://github.com/puzige/vega/actions/runs/39222176469)，实际 5 发现/5 执行/1 通过/4 失败/0 跳过；B01–B04 同一 ListState BorrowError，B05 通过 |
| 冻结测试后的生产修复 | RED 已核实并[固定到新 PR #493](https://github.com/puzige/vega/pull/493#issuecomment-6262985052)后授权；[实现阶段](vega-issue-486-crash-implementation-stage.md)仅移动采样时机，源码已审查，行为待 CI |
| 同一断言 GREEN | 首次 `cfb47565b20005302a9f370cdf1f2e1f2ef01551` 验证 4 通过/1 失败，不能算 GREEN；只修正 B04 前置布置后再验证，全部断言不变 |
| 证据固定后独立删测 | 尚未执行 |
| 最新head完整空集门禁 | 尚未执行 |
| 合并/云端发布/用户手测 | 尚未执行；已安装版本仍受原崩溃影响 |

主会话通读本轮 558 行测试和 183 行外部边界 fixture，并独立复核生产投影：仅移除明确 cfg/feature 接线、只读观察及三个标记后，生产代码与 `edeed517` 基线一致。锁文件仅增加获批的十个测试包（914→924），原包身份/来源/checksum 不变。子 agent 的格式、差异、完整 offline locked metadata 均退出 0；这些静态结果不是类型检查或行为 RED。

## 本轮有效行为 RED

测试先行提交 `4b60f0b28eb1c7734d51dc1003d9141d45515872` 的 [Nextest 原始日志](https://github.com/puzige/vega/actions/runs/39222176469/job/117959818844)真实运行 5 项、14 个 binaries，0.561s；B01–B04 都在 pinned `gpui-pre 0.3.4/src/elements/list.rs:561:16` 报 `RefCell already mutably borrowed`，B05 FilePreview 全部断言通过。列表用例因 panic 未达到后续断言，不能把它们写作已验证。没有编译、依赖、runner 或缺失 API 故障替代 RED。

[Clippy job](https://github.com/puzige/vega/actions/runs/39222176469/job/117959818599) 的 fmt/全目标 Clippy 成功；Nextest job 的 Python 三组 5+15+14=34 实际执行通过，required 因真实行为失败而正确阻断。主会话先在 PR 固定原始错误、计数、不可变源码和 hash，再授权生产调整；不是根据绿色 CI 倒推测试先行。

## 首次修复验证及 B04 前置布置纠正

最小实现提交 `cfb47565b20005302a9f370cdf1f2e1f2ef01551` 的 [run 39223251294](https://github.com/puzige/vega/actions/runs/39223251294) 实际 5 发现/5 执行/4 通过/1 失败/0 跳过，14 binaries，0.807s；B01、B02、B03 的四种滚动路径及 B05 全部通过，没有再出现 BorrowError。fmt、全目标 Clippy、Python34 均通过，required 因 B04 失败正确阻断。

[原始失败及保留记录](https://github.com/puzige/vega/pull/493#issuecomment-6263165467)：B04 在原有签名断言处读到 `(0,18.5px)`，期望为 `(0,0px)`，尚未满足“实际滚动位置不变”的测试前提。主会话与执行者只读核对 pinned GPUI `ListStateInner::layout_items`：测量后到达内容底部会重新 `start_following`，短内容不足一屏时也属于底部；实际邻接物化增长后可重新贴底。生产 Render 在 list 渲染前同步 active model，邻接回复即使在视口下方也会真实物化。

[审查与精确授权](https://github.com/puzige/vega/pull/493#issuecomment-6263202623)先于任何测试修改。只将 B04 首消息的既有段落重复次数 10→40，建立足够长、顶部稳定阅读的原定前提；不改变产品、操作、计时、断言或其它四项，不 mock/直接写滚动终态，不新增测试。这属于执行总纲 §7.2 的测试自身布置修正，不补测追认实现；原始源码、有效 RED 与首次失败全部保留。测试文件将产生新 hash，不能继续声称与原字节完全相同，具体修订记录见[测试阶段](vega-issue-486-crash-test-stage.md)。

只允许本地低CPU格式、差异、静态和离线依赖元数据检查；不得本地运行Vega、编译、Clippy、测试、benchmark或打包。云端CI沿用既有fmt/Clippy/Python/Nextest/required，默认每5分钟查询，不修改门禁。不恢复上一轮42项全套，不追加实现后测试，不重试旧head换绿。最终仍须真实会话用户手测，不关闭Issue、不Done、不清理本卡分支/worktree。
