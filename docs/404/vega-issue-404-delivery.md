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

## 取消竞态修复版本与实际失败处理

- 修复提交 `cff7707e0b846eb7b865ff4c9949b51392256e46`：typed ManagedSendEvidence、Desktop/Browser action send 边界、截图 sink 持久化确认/撤权抑制、晚到 Stop 的完成事实及有限历史兼容。新增 7 项，合计 20 项开发测试；保留原 13 项体。
- 本轮尝试整理已发布提交的命令被权限拒绝，经只读核对未修改 Git 状态；用户明确选择追加提交继续，允许本卡超过 3 提交限制。后续只追加普通提交/推送，不强推、不改写历史。
- [run 40171457328](https://github.com/puzige/vega/actions/runs/40171457328)：Clippy `121102962763`、Nextest `121102963005` 均在开发测试编译时报 E0308（Tools::new 的 Result 当作 Tools 使用），尚未发现/执行 20 项。归类 harness 初始化故障，`4771298d0a37b09ff87015c822bcd5d68a6d88ab` 只作一行构造器接线修复；生产代码和断言不变。
- [run 40172561847](https://github.com/puzige/vega/actions/runs/40172561847)，精确 head `4771298d0a37b09ff87015c822bcd5d68a6d88ab`：Clippy `121106307590` 成功；Nextest `121106307704` 20 发现/20 执行/18 通过/2 失败/0 跳过，required `121106847396` 失败。两项失败均为未交付/撤权截图取消的 `code`：实际 `control_unavailable`，断言 `cancelled`；主 Agent 复核原始日志与 feedback 分支发现 Cancelled/NotReady 合并序列化。spec 明确真实取消与未就绪分码后，由原实现者最小拆分，保留失败断言，不盲重试。
- 非实现者完成固定 `52bf4366…`→`cff7707e…` 生产增量独立审查，无新发现，确认原两项 P2 已解决；该结论仅静态，不能代替 20 项回归或最终 head CI。此前中断的增量审查不计通过。

### 可恢复的 20 项开发源码

固定 `4771298d0a37b09ff87015c822bcd5d68a6d88ab`：

- [runtime 测试（9 项）](https://github.com/puzige/vega/blob/4771298d0a37b09ff87015c822bcd5d68a6d88ab/crates/vega_runtime/src/computer_use/recovery_tests.rs)
- [conversation 测试（10 项）](https://github.com/puzige/vega/blob/4771298d0a37b09ff87015c822bcd5d68a6d88ab/crates/vega_conversation/src/agent/cua_recovery_tests.rs)
- [MCP support 与测试（1 项）](https://github.com/puzige/vega/blob/4771298d0a37b09ff87015c822bcd5d68a6d88ab/crates/vega_mcp/src/managed_recovery_test_support.rs)
- [runtime support](https://github.com/puzige/vega/blob/4771298d0a37b09ff87015c822bcd5d68a6d88ab/crates/vega_runtime/src/computer_use/recovery_test_support.rs) 与 [原始修复 patch](https://github.com/puzige/vega/commit/cff7707e0b846eb7b865ff4c9949b51392256e46.patch)、[harness patch](https://github.com/puzige/vega/commit/4771298d0a37b09ff87015c822bcd5d68a6d88ab.patch)。

## 开发回归最终通过版本

`543e2b8d54614a49fd2587f8cea91c4bc86f0388`，[run 40173293281](https://github.com/puzige/vega/actions/runs/40173293281)：

| 检查 | 实际结果 |
|---|---|
| fmt / Clippy `121108529101` | 成功 |
| Nextest `121108528961` | 20 发现、20 执行、20 通过、0 跳过 |
| 三组 Python | 5 + 15 + 40 = 60 执行通过 |
| required `121109316926` | 成功 |
| 本地 | fmt/diff 静态检查通过，无编译/测试/应用执行 |
| 独立增量审查 | `cff7707e…`→`543e2b8d…` 小增量无发现，未放宽审计或断言 |

本轮动态覆盖补足受管传输零写入/部分写入、Desktop/Browser 发送前暂停恢复、四类晚到 Stop/截图持久化及撤权抑制、磁盘重新打开/历史投影/下轮准备、有限旧 Cancelled 记录兼容。均为进程内受控依赖证据，不是真实外部桌面或原生 UI 验收。

### 固定原始源码与内容标识

以下完整版本经 GitHub contents API 读回核实可达；源码固定在 `543e2b8d54614a49fd2587f8cea91c4bc86f0388`，清理后不依赖临时文件或会过期 artifact：

| 源码 | 定义数 | SHA-256 |
|---|---:|---|
| [runtime recovery_tests.rs](https://github.com/puzige/vega/blob/543e2b8d54614a49fd2587f8cea91c4bc86f0388/crates/vega_runtime/src/computer_use/recovery_tests.rs) | 9 | `e2d262a6426151fe451a5b0b257516f0e97507d72577b6de46b2a2f4c820d134` |
| [conversation cua_recovery_tests.rs](https://github.com/puzige/vega/blob/543e2b8d54614a49fd2587f8cea91c4bc86f0388/crates/vega_conversation/src/agent/cua_recovery_tests.rs) | 10 | `822c7410187a2244fd66e2b9452880c6b97d7fe987486ef505f62bbf352868f6` |
| [MCP managed_recovery_test_support.rs](https://github.com/puzige/vega/blob/543e2b8d54614a49fd2587f8cea91c4bc86f0388/crates/vega_mcp/src/managed_recovery_test_support.rs) | 1 | `1713366b628cf86f314c6bb9f01c962bfc00d759f3933c432629d95744a1003d` |
| [runtime recovery_test_support.rs](https://github.com/puzige/vega/blob/543e2b8d54614a49fd2587f8cea91c4bc86f0388/crates/vega_runtime/src/computer_use/recovery_test_support.rs) | 支持 | `2d44b3cf191d236a6af522349c5ec05f52fab40a6f88aa661dac825dc633c444` |

原始累计变化可从 [PR #613 files/commits](https://github.com/puzige/vega/pull/613/commits) 和上述各完整 commit `.patch` 恢复。独占接线在 runtime backend/batch/mod、conversation pipeline、MCP managed/lib，最终树全部移除；生产 feedback、typed send evidence 与 screenshot sink acknowledgement 不属于删测范围。

## 清理与最新 master 合流验证

- `813e1a5408185a7b58dd5379d54a65008b88f78e` 清理 4 个开发测试/支持文件及 15 个测试门控区块（84 行接线）；代码 0 插入/2,279 删除，release 生产逻辑无替换。非实现者独立复核无清理发现，并静态扫描 420 个非 vendor Rust 文件，未见测试/支持/doctest 残留。
- 合入 master `429c6a819fc5a30f8280afd2f6ed015d4682bf41` 后固定 `c13da160c27a7c681a7cc24fc226f69c7fbb0dc3`。[run 40174268402](https://github.com/puzige/vega/actions/runs/40174268402)：Nextest `121111465462` 明确 `no Rust tests`（0 发现、0 执行），Clippy `121111465603` 和 required `121112108233` 成功，Python 60 项通过。空集门禁不冒充前述 20 项功能验证。
- 合流独立审查发现一项 P2：master 新增完整历史分叉中 `thread_fork.rs` 两处只允许 success 截图，拒绝 #404 支持的历史 Cancelled + 已持久交付截图及别名。原运行时可留下该记录；后续正常 Final 的 prefix 应可分叉，不重写取消状态或复制权限。
- 更新 spec v2 后，`b4d2e70325bf9d2169115b540e93d18a24e7b53a` 仅修 store 的两处截图门禁：实际非空附件、assistant/message/thread 归属、精确有界回执与批准/输入审计，保留原 prefix/顺序/事务和成功 first-line 兼容；原 UI/runtime/MCP 不变。独立增量审查无发现。
- [run 40177276819](https://github.com/puzige/vega/actions/runs/40177276819)：Clippy `121120594971` 成功；Nextest `121120594809` 3 发现/3 执行/1 通过/2 失败/0 跳过。两失败误从 `ContextSource.images` 读取助手截图；该集合按生产合同只包含用户模型输入图片，不应改成临时桌面观察。主 Agent 核对原始日志与 SQL 后归类观测层错误，保留图片字节/归属/历史断言。
- `652c390c59068e70af26f2a61636712b3e35c7ea` 仅改开发回归为真实 `image_attachments::for_messages` + `HistoryEntry::AssistantScreenshot` 观测；3 个定义、17 类拒绝 case、source 不变和无权限断言保留，生产字节不变。非实现者独立小增量审查无发现。
- [run 40178249738](https://github.com/puzige/vega/actions/runs/40178249738)：Nextest `121123523447` 3 发现/3 执行/3 通过/0 跳过，Clippy `121123523287`、required `121124160784` 成功，Python 60 项通过。包含 17 类错误证明/引用的原子拒绝、plain/legacy-budget/structured-budget 的公共 fork、reload、fork-of-fork、历史投影及下轮准备。
- 该批原始 [fork_screenshot_tests.rs](https://github.com/puzige/vega/blob/652c390c59068e70af26f2a61636712b3e35c7ea/crates/vega_conversation/src/agent/fork_screenshot_tests.rs) 固定在完整 green head，SHA256 `d1888784de4b7953002fbd9ac88277fbec125a8b6e64b7f19be7a538d32de82f`，GitHub contents API 已核实 blob `5ed4e7ea3e160989687b9a39d9f3f72097fb99d2` 可达。[实现原始 patch](https://github.com/puzige/vega/commit/b4d2e70325bf9d2169115b540e93d18a24e7b53a.patch)、[观测修正 patch](https://github.com/puzige/vega/commit/652c390c59068e70af26f2a61636712b3e35c7ea.patch) 可恢复测试与当时生产行为。最后清理仅删除此测试文件及 conversation agent 的两行模块接线，不移除 store 生产校验。

## 最终清理与 i18n master 合流

- `ac135a11dd807c3694502eb0122dbee2ffd3c822` 最后删除 578 行 fork 开发测试、2 行模块接线并记录证据。非实现者独立复核确认无生产变化，420 个非 vendor Rust 文件静态清点无测试/支持/doctest 残留。
- ac 的 Actions **未启动**，不可称为最终门禁通过。GitHub 状态变为 CONFLICTING；fetch 后 master 为 `f6723682f960f87941ffcd9cae477f4a5dbef777`，新增 #589 内嵌中英文界面，触及 runtime/诊断/UI/历史等本卡路径。
- 主 Agent 启动未提交 merge，实际冲突为 `agent/loop_.rs`、`computer_use/diagnostics.rs`、`conversation_stream/computer_use.rs`。不能恢复已删除的 no_progress 固定 `ComputerStopped` 收尾或旧错误/准备菜单；必须保留 master 的真实授权/暂停/恢复控件翻译与其他功能。
- 自动合流语义风险：UI `user_message` 已本地化，而结构化诊断及严格落库校验须跨语言稳定；master 已为 Executor 增加 protocol_guidance/diagnostic，#404 的非 Executor 规范文案也须与翻译展示分开。先在 spec 增加 i18n 合流不变量，再由专职实现者解决冲突和最小跨语言回归；不以任意诊断白名单绕过严格校验。

## 验证限制与后续

组件/协议/身份的各类实际故障、断连/动作后观察失败变体、完整截图容量流、权限恢复后真实预检、分页/TTL/压缩/附件竞态及原生焦点交互尚无本轮动态验收，静态覆盖与具体限制以 spec 矩阵和最终报告为准。未做真实外部进程/网络 E2E。

开发测试及独占接线必须在最终 master 树删除，保留上列原始源码/patch 和补充回归证据；最终无 Rust 测试 head 的静态清点与完整 required CI 单独记录，`no Rust tests` 不能冒充功能回归通过。最新 head required check 与独立审查通过才合并，之后 In review 等用户手测，不自动安装、关闭 Issue 或 Done。
