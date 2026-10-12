# Issue #75 — 窗口观察与 live 工具卡修复

## 证据与范围

基线 6c4ce2fd。0.1.77 用户报告目录成功、指定窗口观察失败；安全诊断为 executor=cua stage=observe error_class=observation_unavailable，随后运行取消。当前聚合错误不能区分目标身份、AX 降级、截图有效性或上游捕获拒绝，不能将某个候选原因当成已验证根因。不得读取或捕获当前工具禁止访问的应用，也不得通过 Vega 或其他执行路径绕过限制。

主控在允许的 TextEdit 测试窗口完成四次成功调用，模型正确报告首末行且正常结束，UI 仍显示“含失败操作”。源码已证实 safe_runtime_tool_call 缺少 ComputerUseAudit 分支，安全 proposal 被通用未知工具规则丢弃；后续生命周期事件找不到卡片而创建 Corrupt。这是成功/失败调用共用的确定缺陷，与图像投递是否成功无关。

## 冻结合同

OF1. 四个 ComputerUse 工具的 live proposal 仅接受既有 ComputerUseAudit 严格格式，保留 call_id/tool/安全审计原文；原始参数、空对象、未知字段和未知工具均不得借此通过。使用正常工具卡展示成功、拒绝或失败，不能将合法 sanitized 参数或有限安全错误当作损坏。

OF2. 不改变历史/图像寿命，不保留额外截图、输入或窗口正文；不修改当前正常的 live observation provider 投递。所有新增诊断只使用有限枚举，不保留上游原始原因、页面、标题、路径或令牌。

## 验收矩阵

| ID | 操作 | 预期 | 层级 | 状态 |
|---|---|---|---|---|
| OF01 | 四工具合法安全 audit 经 RuntimeEvent 到卡片输入/成功/失败结果 | proposal 存在且完整匹配；正常 ComputerUse 投影 | inprocess | PASS（进程内） |
| OF02 | 原始参数、空对象、额外字段或未知名称 | live 边界拒绝，不能误放行 | inprocess | PASS（进程内） |
| OF03 | TextEdit 正常 Observe | 图像与内容投递保持，卡片为成功 | 真机 | 待新版用户手测 |

## 计划

先写 OF01/OF02 并保存旧实现失败，再修专属 live audit 分支。截图降级和有限 Observe 可恢复状态已按固定 Cua 0.34 源码核对并由主控冻结，先写失败测试再实现。仅本卡定向 nextest；无新增依赖、DDL、真实执行器自动测试或 Playwright 改动。独立只读审查后提交，PR、云端门禁与真实验收由主控完成。

OF4. Cua degraded=true 仅当 degraded_reason 与 0.34 固定 ax_tree_empty 完整字面值完全一致、elements 为空数组、精确 pid/window_id、screenshot_frame_valid=true、非空有界 snapshot_id/capture_id 及解码成功的有效图像同时成立时，发布 screenshot-only 观察。固定说明 AX 不可用，本观察只读；针对该桌面观察的所有 ComputerAct（坐标、键盘、AX等）在 dispatch 前拒绝。不能用任意前缀、任意 degraded reason 或无绑定图像通行。下一次健康 Observe 才获得正常动作能力；不延长旧观察寿命。

OF5. app_launching/window_unresolved 降级仅识别固定上游模板与数值字段，并要求精确目标身份；不发布其图片/AX。截图捕获不可用或没有观察内容同样不发布。身份与明确帧失败优先于 AX 降级分类；已知捕获不可用不得掩盖未知降级。MCP is_error=true 的明确身份/帧/未知降级冲突也先检查，再进行有限错误分类；缺少可选元数据的正常错误仍只依据固定错误码分类。未知降级、身份不符、frame mismatch/缺少有效性证明、缺失绑定、坏图、协议/策略/权限均保持失败关闭。新增固定分类区分 observation_identity_mismatch、screenshot_unverified、capture_unavailable、capture_frame_mismatch、capture_binding_failed、capture_publication_failed、ax_app_launching、ax_window_unresolved、degraded_unknown、no_observation_content，不输出上游正文。

OF6. 仅直接 ComputerObserve 的固定可恢复类 capture_unavailable/no_observation_content/ax_app_launching/ax_window_unresolved/accessibility_unavailable/accessibility_timeout 可交还模型：清空桌面当前目标、目录和全部旧桌面观察，回到可发现目录的 Running；lease 与连接保持，调用结果仍 Failed、没有自动重试、没有假成功、没有自动 Cancelled。新的显式目录/选窗 Observe 可继续。Act 的后验 Observe 不适用；未知/identity/frame/协议/策略/权限失败仍终止运行，已发动作仍 Unknown 且不重试。取消或 epoch 变化优先，恢复处理不能复活停止/接管状态。

| ID | 操作 | 预期 | 层级 | 状态 |
|---|---|---|---|---|
| OF04 | 健康/精确 ax_tree_empty + 已验证绑定PNG | 健康可Act；只读截图可交模型且AX不可用；所有Act零派发 | adapter/state fixture | PASS（进程内） |
| OF05 | 改身份、frame、snapshot/capture、原因/元素、坏图 | 全部拒绝；不发布、不放宽窗口范围 | adapter fixture | PASS（进程内） |
| OF06 | 只读截图后新健康观察 | 旧只读引用失效，新健康观察才可Act | state/adapter | PASS（进程内） |
| OF07 | 直接Observe已知可恢复错误后重新发现 | 旧目标/观察不可用；provider收到固定错误，能继续目录；不是Cancelled | runtime/provider fixture | PASS（进程内） |
| OF08 | 相同错误来自Act后验、未知/权限/协议/身份错误或停止后迟到 | 原fatal/Unknown/撤权合同保持；不重试、不复活 | runtime fixture | PASS（进程内） |
| OF09 | live成功/失败卡与历史回读 | 正常ComputerUse卡一致，无Corrupt或正文新增持久化 | conversation/UI fixture | PASS（进程内） |

固定源码：[scope 与 snapshot/capture 发布](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/platform-macos/src/tools/get_window_state.rs#L528)、[降级固定模板](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/platform-macos/src/tools/get_window_state.rs#L653)、[unresolved 优先于空AX树](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/platform-macos/src/tools/get_window_state.rs#L874)、[截图边界校验](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/platform-macos/src/tools/px_frame.rs#L65)。这证明通用只读截图分支可实现，不证明此前受限应用失败的唯一原因；不对此应用执行任何捕获验收。

## 验证证据与交付边界

本轮先行 RED（不是编译失败）：

```text
cargo nextest run -p vega_conversation issue75_observation_live
Summary [   0.022s] 2 tests run: 0 passed, 2 failed, 580 skipped
cargo nextest run -p vega_runtime issue75_observation_
Summary [   2.275s] 7 tests run: 4 passed, 3 failed, 315 skipped
```

前者复现合法 proposal 丢弃及空对象误放行；后者复现固定 AX 空树截图被拒、只读动作没有门禁、观察失败导致运行 Failed。日志 /tmp/vega-75-observation-live-red.log（SHA256 250f4afe25c339a4654f115070b7c3a1647053cb3e7737d224b9e48093378742）、/tmp/vega-75-observation-runtime-red.log（00158d345c19f24a862e43ccc1b350b379d03e251b0bb8a5ea9eeeab925ca76b）。

独立审查与收尾补充组合 RED：

```text
cargo nextest run -p vega_runtime issue75_observation_explicit_frame
Summary [   0.022s] 1 test run: 0 passed, 1 failed, 323 skipped
cargo nextest run -p vega_runtime issue75_observation_metadata_classes
Summary [   0.023s] 1 test run: 0 passed, 1 failed, 326 skipped
cargo nextest run -p vega_runtime issue75_observation_mcp_error
Summary [   0.018s] 1 test run: 0 passed, 1 failed, 327 skipped
```

分别复现明确 frame mismatch 被已知 AX 降级掩盖、未知降级被 capture_unavailable 掩盖、MCP is_error 中身份冲突被固定捕获错误码掩盖。已调整为所有明确 fatal 信号优先，再允许白名单恢复。日志与 SHA256：

- /tmp/vega-75-observation-review-red.log：799e4323e35adcd5e2e4bcf301bd047349b181296e1086f85760ee25b4af92e0
- /tmp/vega-75-observation-combined-red.log：fb9fc5c2a22e780bb74e370e95f810e100cca107b0359114a71eaa4d18a59829
- /tmp/vega-75-observation-mcp-red.log：5686fabcc85c853bd50dc4e04de3c936852ddff176d1d84a215148034a7b3d47

最终定向 GREEN：

```text
cargo nextest run -p vega_runtime -p vega_conversation -p vega_ui issue75
Summary [   2.499s] 59 tests run: 59 passed, 1512 skipped
```

日志 /tmp/vega-75-observation-final-verified.log，SHA256 ffc1483b127bc1004e1b3af7722be46fc1b3de6596e2436f19c281665e3b7945。修改文件 rustfmt --check 与 git diff --check 通过，无新增代码注释；未跑本地 workspace 全量，云端门禁由主控执行。

OF01/02/09：安全 live 转换覆盖四工具，GPUI 真实卡片事件链覆盖成功/失败，既有持久历史回读覆盖 success/failed/rejected。OF04–06：固定上游模板、合法 PNG、只读图像进入 live 消息、所有 ComputerAct 经真实 execute 路径底层派发计数为零、健康新观察替换旧观察。OF07：完整 run_agent 使用仅替代外部执行器边界的进程内 fixture，分别覆盖 is_error false/true 捕获失败；第一工具为 Failed，下一 provider 轮显式重新发现成功，第三轮正常 final，结果不是 interrupted。Chat Completions 与 Responses wire 均保留固定失败和后续目录，没有原始错误或图像泄露。OF08：Act 后验失败仅派发一次并保持 outcome_unknown/Failed，身份、帧、协议、权限、未知和政策错误保持 fatal；暂停/停止的晚到恢复不复活。

独立只读审查已确认组合优先级缺陷修复闭合，未发现剩余阻断。没有调用真实 driver、操作共享 UI 或捕获受限应用；此前特定应用的 observation_unavailable 原因仍无法由历史聚合错误唯一确定，不能声称此版已对该应用真机修复。

新版用户手测：在允许的 TextEdit 测试窗口确认目录/观察成功卡不再损坏，正确报告图像内容；如最小化确实触发可恢复固定类别，确认模型能说明原因，人工恢复后显式新目录/观察可继续。最小化并不保证捕获失败，结果以实际固定诊断为准。截图只读能力不代表可输入，无前台 fallback、无 AX 归属绕过、无自动重试未知动作。Playwright 方案保持暂停。
