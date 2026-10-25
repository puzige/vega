# Issue #404 — 实施与验证记录

## 版本与范围

- Issue：[404](https://github.com/puzige/vega/issues/404)；Draft PR：[613](https://github.com/puzige/vega/pull/613)。Issue 保持开放，未视作用户手测通过。
- 基线：`b03d0d5f2c7da0efab147a4fec0d26e2adefc4e3`。
- [规格 v1](https://github.com/puzige/vega/blob/1ce4b8a2c1fddcf5ab0a9ec29c58c386b1b085fe/docs/404/vega-issue-404-cua-error-recovery.md)：`1ce4b8a2c1fddcf5ab0a9ec29c58c386b1b085fe`，包含 #75 旧条款覆盖说明。
- 初始开发实现：`35e93540f07969093cbfdb01895a33c862365970`。两个实现者分别持有 runtime/会话与 UI，不重叠；主 Agent 统一提交。
- 实现分离能力不可用和真正取消；保留冻结错误、实际派发状态与无进展有限诊断；去掉 runtime 固定收尾及专用错误菜单；保留真实授权/接管/恢复/停止、普通工具错误和历史结果兼容；内置指引升至 v6。无新依赖/迁移/CI 修改，无用户数据/权限/安装变更。

## 首轮开发验证（非最终门禁）

精确 head `35e93540f07969093cbfdb01895a33c862365970`，[Actions run 40161424306](https://github.com/puzige/vega/actions/runs/40161424306)：

| 检查 | 实际结果 |
|---|---|
| 本地 `cargo fmt --all -- --check`、`git diff --check` | exit 0，仅格式/静态检查 |
| 云端 fmt | 成功 |
| [Nextest job 121073618989](https://github.com/puzige/vega/actions/runs/40161424306/job/121073618989) | 11 发现，11 执行，11 通过，0 跳过；不是最终空集 head |
| 同 job 三组 Python 检查 | 成功 |
| [Clippy job 121073618816](https://github.com/puzige/vega/actions/runs/40161424306/job/121073618816) | 失败：`progress.rs:485,493` 的 `clippy::question_mark`，未放宽 lint |
| required `check (fmt, clippy, test)` | 失败，未合并 |

本地未运行 Vega、编译、构建、打包、cargo check、Clippy 或测试。开发动态证据仅为云端进程内替身，不代表真实桌面/浏览器操作或用户验收。

### 可恢复的原始开发测试源码

均固定完整提交 `35e93540f07969093cbfdb01895a33c862365970`，不是可过期 artifact：

- [runtime recovery_tests.rs](https://github.com/puzige/vega/blob/35e93540f07969093cbfdb01895a33c862365970/crates/vega_runtime/src/computer_use/recovery_tests.rs)，8 个定义，SHA256 `cc5c9c1a2146b1ae22f5540062633fe9caedcc8b8f0509b396091517b70709ad`。
- [conversation cua_recovery_tests.rs](https://github.com/puzige/vega/blob/35e93540f07969093cbfdb01895a33c862365970/crates/vega_conversation/src/agent/cua_recovery_tests.rs)，3 个定义，SHA256 `afb5e99b2b49cc7049a9740cad191a7ffb253fbec0cd7c8c10a0a1222bc982aa`。
- [完整原始 patch](https://github.com/puzige/vega/commit/35e93540f07969093cbfdb01895a33c862365970.patch) 包含测试专用 backend driver 接线、模块声明和当时生产代码。

已执行回归覆盖：三项权限组合的下一实际 provider request 包含关联 JSON 错误并产生脚本化助手说明；混合非 CUA 与同批/后续冻结调用零新增派发；第三次重复目录返回真实诊断且模型继续；交错目录/窗口不重置目录计数；已派发动作超时并伴随接管时 unknown 不重放；真实 Stop；恢复后的旧 epoch 不派发；截图容量诊断、已交付回执保持；内存 SQLite 投影与有限伪造审计拒绝。

## 独立审查与修复诊断

- UI 原固定内容经非实现者独立静态审查，无可操作发现。5 文件 `+11/-317`，相对规格提交的 UI 二进制 diff SHA256 `f3c2a92807a01297adb578a5fa09064f548fbd5557a6a7c12b9c357ea54e12b8`。保留控件/键盘接线的结论为静态，不是 UI 实测。
- 首轮 runtime 集成审查中断，没有最终批准；主 Agent 核对原始源码后确认两项产品/审计缺陷，不将部分报告当作审查通过：
  1. `TargetChanged` 的合法错误诊断未在 `valid_error_output` 候选中，可能被当作损坏结果；补有限候选及回归，不放宽任意 JSON 校验。
  2. 观察成功后因模型投影容量转成 Failed 时仍保留 success-only `truncated`，被 `validate_runtime_terminal` 拒绝；修结果转换与 live audit 回归，不修改审计断言。
- Clippy 是新增 Option 返回路径的 lint 问题，按原诊断使用 `?`，不加 allow，不盲重试。
- 修复由原实现者持有原范围；完成后固定新 head、独立审查和 Actions 重验。原 11 项回归保留，不以已有通过替代缺陷修复后的验证。

## 第二轮开发验证与取消竞态审查

精确 head `52bf436653d35e418d740197ccaee4dcebb3b634`，[Actions run 40165878090](https://github.com/puzige/vega/actions/runs/40165878090)：fmt、Clippy job `121086692366`、Nextest job `121086692465`、required 汇总 `121087367740` 全部成功；13 发现/13 执行/13 通过/0 跳过，三组 Python 检查成功。本地仍仅 fmt/diff 静态检查。

- 新增两项回归：生产反馈生成的有限错误经过 live terminal/recovered/history/card 审计；生产 agent + `persist_runtime_event` 的已执行观察容量错误返回下一 provider request 并由模型解释。保留原 11 项断言；未放宽 terminal 校验。
- 可恢复补充源码：[runtime 测试](https://github.com/puzige/vega/blob/52bf436653d35e418d740197ccaee4dcebb3b634/crates/vega_runtime/src/computer_use/recovery_tests.rs)、[临时跨 crate support](https://github.com/puzige/vega/blob/52bf436653d35e418d740197ccaee4dcebb3b634/crates/vega_runtime/src/computer_use/recovery_test_support.rs)、[conversation 测试](https://github.com/puzige/vega/blob/52bf436653d35e418d740197ccaee4dcebb3b634/crates/vega_conversation/src/agent/cua_recovery_tests.rs)、[完整 patch](https://github.com/puzige/vega/commit/52bf436653d35e418d740197ccaee4dcebb3b634.patch)。临时 support 包括 backend、batch、mod 接线，均须在最终树清除。
- 新非实现者完成完整静态审查，要求修改两项 P2，不以 CI 绿冒充审查通过：
  1. backend 在 ManagedClient 真正写入前设置 input_started，发送前取消可被误记为 outcome_unknown 并冻结已暂停控制。主 Agent 核对 managed.rs 的发送前取消与 biased select、backend 的标志设置，确认修复应在实际发送边界提供类型化事实，而非放宽未知结果保护。
  2. 晚到 Stop 把成功 CUA 状态改成 Cancelled 但保留完成摘要/截图回执，新增 validator 拒绝该组合；旧实现也可留下这类历史记录。主 Agent 核对 live sink 调用链后确认结果生产与有限历史兼容都需处理，不声称未交付截图已 delivered。
- [规格 v2](vega-issue-404-cua-error-recovery.md#取消竞态的实施细化v2) 细化原 C404-06/08/11；原实现者修复，写入范围最小扩展到受管 `vega_mcp` 发送事实接口，不调整普通 MCP 权限、协议、重试，不新增依赖。发送前暂停零写入可恢复、发送后未知不重放、晚到 Stop 经 live/reload/history/下轮准备的回归均待补充云端结果。

## 验证限制与后续

组件/协议/身份的各类实际故障、断连/动作后观察失败变体、完整截图容量流、权限恢复后真实预检、分页/TTL/压缩/附件竞态及原生焦点交互尚无本轮动态验收，静态覆盖与具体限制以 spec 矩阵和最终报告为准。未做真实外部进程/网络 E2E。

开发测试及独占接线必须在最终 master 树删除，保留上列原始源码/patch 和补充回归证据；最终无 Rust 测试 head 的静态清点与完整 required CI 单独记录，`no Rust tests` 不能冒充功能回归通过。最新 head required check 与独立审查通过才合并，之后 In review 等用户手测，不自动安装、关闭 Issue 或 Done。
