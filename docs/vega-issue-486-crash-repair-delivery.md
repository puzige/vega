# #486 · ListState 崩溃修复交付记录

2026-10-19。用户最新明确指示**“你直接实现 不用等 vega”**，覆盖已发布的 Ready/Vega 默认路由。产品需求 v1 不变，执行[崩溃修复方案 v1](vega-issue-486-list-state-borrow-crash.md) B01–B05。记录截止独立删测阶段：完整行为 GREEN 已确认并在 PR 固定，独立清理已完成并经静态审查；当前/下一执行者 Codex。此时 Project In progress，最终空集门禁、合并、发布及用户手测待后续验证；最新结果以 [PR #493](https://github.com/puzige/vega/pull/493) 和 [Issue #486](https://github.com/puzige/vega/issues/486) 为准，不在取得前预填。

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
| 同一断言 GREEN | `02d25a72f546d0b076be9d1bd28adf4de1cbc2d0` / [run 39224636493](https://github.com/puzige/vega/actions/runs/39224636493)：5 发现/执行/通过、0 失败/跳过；含事先审查的 B04 布置纠正，全部原断言不变 |
| 证据固定后独立删测 | [完整 GREEN 与原始源码先在 PR 固定](https://github.com/puzige/vega/pull/493#issuecomment-6263358878)，之后完成[独立清理](vega-issue-486-crash-cleanup-stage.md)，最终生产仅保留批准的九行采样修复 |
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

## 完整行为 GREEN 与固定来源

纠正前置布置的提交 `02d25a72f546d0b076be9d1bd28adf4de1cbc2d0` 的 [run 39224636493](https://github.com/puzige/vega/actions/runs/39224636493) 全部成功。[Nextest](https://github.com/puzige/vega/actions/runs/39224636493/job/117967836963) 真实 5 发现、5 执行、5 通过、0 失败/跳过，14 binaries，0.813s；[fmt/Clippy](https://github.com/puzige/vega/actions/runs/39224636493/job/117967836573)、Python34、[required check](https://github.com/puzige/vega/actions/runs/39224636493/job/117969548894) 均成功。B04 原签名/物化/卡片断言完整通过，B03 四种路径及 B05 实际执行。

主会话先将完整逐项 PASS、原始 run/job、全部提交顺序、测试纠正理由及源码固定到 [PR 原始证据](https://github.com/puzige/vega/pull/493#issuecomment-6263358878)，随后才授权独立删测。GREEN 源码可从不可变提交找回，不依赖工作目录或过期 artifact：

- [GPUI 测试](https://github.com/puzige/vega/blob/02d25a72f546d0b076be9d1bd28adf4de1cbc2d0/crates/vega_ui/src/conversation_stream/link_preview_tests.rs)，SHA-256 `de18c58d1563b6eb797aa68286d2e2e4a75bdc5cbb66c916fd54048853624965`。
- [外部 fixture](https://github.com/puzige/vega/blob/02d25a72f546d0b076be9d1bd28adf4de1cbc2d0/crates/vega_conversation/src/link_preview/test_support.rs)，SHA-256 `a83eba45519905a4a2e948500734d94d9d6142b93c8810eb0d8c31a6a3a0e331`。

最终空集门禁不是上述五项行为通过的替代品。只允许删除明确独占的 Rust 测试/支持，保护采样修复及生产/benchmark/Python/CI/发布能力；不得在 GREEN 后夹带新生产行为。

## 独立清理与最终树

[清理阶段](vega-issue-486-crash-cleanup-stage.md)记录两份测试/fixture、cfg/feature/dev 接线、三个只读观察标记及十个测试独占锁条目的删除。主会话独立检查整个树相对生产基线的差异：除文档外，仅 `crates/vega_ui/src/link_preview.rs` 有批准的九行采样变更；服务、真实消息列表、FilePreview、正常清单/锁文件和全部其余路径与 `b32ad22e` 一致。

最终 UI SHA-256 `6cca823ef7c09375c9af2d9267951ac62e7ecaecedab74f7ed2bd935315c50c6`；Cargo.lock 恢复 914 包及原字节，SHA-256 `d803cdb1cd420921fa47ce091155bb0db1ce0845ecfa4027b95b46ebe2db36f9`。快速 fmt/diff/完整离线 locked metadata 均退出 0；静态测试及独占支持残留为 0，不冒充 Nextest 实际发现/执行。删测提交后的最新 head 门禁由同一 PR 原流程执行，原始输出与准确空集计数在 PR/Issue 补齐后才合并。

只允许本地低CPU格式、差异、静态和离线依赖元数据检查；不得本地运行Vega、编译、Clippy、测试、benchmark或打包。云端CI沿用既有fmt/Clippy/Python/Nextest/required，默认每5分钟查询，不修改门禁。不恢复上一轮42项全套，不追加实现后测试，不重试旧head换绿。最终仍须真实会话用户手测，不关闭Issue、不Done、不清理本卡分支/worktree。
