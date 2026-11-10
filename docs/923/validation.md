# 后台控制工具验证记录

## 开发基线与范围

基线 `2a29adf9b2ea78eaf59d591838531b45adc7373e`。修复提案及审批来源两处漏接；控制参数限定工具名、2048 字节、ULID、wait/cursor 类型和界限，cancel 只允许 handle。恢复/取消的既有回执真实性与所有权逻辑不放宽。

失败消息不等于 worker 已停止。现有实际接线在 `app_agent::drain_agent_updates` 的 Finished/断连后进入 `window/artifact.rs` 的完成处理，调用控制器 finish、侧栏 set_thread_running(false)、完成消息状态及 composer_finished；Error 事件本身不释放控制器。此次暂未找到该链的产品缺陷，不依据历史截图强行清状态。ComposerActions 原有终态动作提取为可直接验证的方法，不增加依赖或改变取消事实优先级。

## 实际本地验证

工具链：Cargo 1.101.0（e611c7a87 2026-11-04，huacnlee/cargo 0.102.0-9，CARGO_FORK_AUTO_UPDATE=0），rustc 1.98.0（88d9e12ae 2026-08-18）。命令在本任务工作树执行，未运行远端命令、真实模型或用户未知命令。

- `cargo fmt --all -- --check`、`git diff --check`：通过。
- `cargo test -p vega_conversation -p vega_ui -p vega shell_control_tests -- --nocapture`：**13 发现 / 13 执行 / 13 通过 / 0 忽略**；服务 7、通道 3、发送状态 3。
- `cargo clippy -p vega -p vega_conversation -p vega_ui --all-targets -- -D warnings`：通过。Cargo manifest 存在既有 unused_workspace_dependencies 提示，未改依赖。
- `python3 scripts/release_tests.py`：5/5。
- `python3 scripts/prepare_computer_use_tests.py`：15/15。
- `python3 -m unittest discover -s scripts/tests -v`：40/40。

服务回归走真实内存 SQLite 的提案→审批→运行→终态，且再走 terminal_history 恢复；包含 waiting/running、succeeded、cancelled、outcome_unknown，保留原始回执。参数负对照、错误审批来源与拒绝投影不能执行的断言保留。通道测试覆盖 Error 尚待 drain 不提前完成、Failed Finished、断连、批次边界不丢终态。发送状态测试覆盖 running/stopping/focus 释放及取消事实优先级。

测试不执行真实后台 OS 进程，也不动态构造完整 AppWindow；运行时会话所有权与窗口控制器/侧栏接线仍需静态审查，实际 native 状态、鼠标键盘交互与远端现场清理仍待用户验证，不能称为端到端验证。

## 失败诊断（不更改验收）

1. edit 工具多次 target_changed，但 hash/工作树无变化；改用原文/哈希断言补丁，未覆盖并发差异。
2. 首轮 2/6 服务通过，4 项在审批阶段失败：证实第二处产品漏接 `approval_source_matches`，按 runtime ShellControl 的 ReadonlyTool + Once 修复，未改测试来源迁就实现。
3. 成功终态 fixture 的 command_result 错写 succeeded；实际契约为 exited，修 fixture 后原断言通过。
4. 通配导入引入 GPUI test 宏造成属性递归、错误事件字段需要 Arc：只修测试接线。试探性 gpui test-support 依赖在执行前撤掉，Cargo.lock/Cargo.toml 无最终差异。

## 待完成

固定开发源码与哈希、独立实现审查、开发 Rust 测试清理及清理复核、完整最终 head required CI、合并后用户手测。开发测试不会进入 master；清理后的 0 发现/0 执行标为 no Rust tests，不冒称行为测试通过。

未升级/重启远端，未重跑结果未知命令，未修改真实恢复记录或清理状态。

## 主分支集成后重验

开发初版 `0b617e0e0a6389c7c5945c1a47924911c86ecdfa` 的 PR merge-ref CI run `42784732497` Clippy job `129723013347` 报 E0061：主分支 #924 / `f2d59991` 新增 persist_runtime_event 的 WriteTransaction 参数，测试仍为旧签名。原始日志明确为测试 API 接线故障，不归因产品参数校验。

已合入 `f2d59991`，保留其数据库 writer / sealed transaction 行为，测试每次事件使用 WriteTransaction(RuntimeEvent)，成功才 commit。旧 create_thread(:memory:) 因新在线任务身份前提报 InvalidTaskIdentity，测试前置改用真实 store NewThread 行；控制提案→审批→运行→终态→历史恢复断言不变。

本轮在整合后代码重新执行上述 13 项 Rust 回归，13/13 通过；受影响三 crate 全目标严格 Clippy 通过。前版独立实现审查无阻塞发现；新增集成差异需复核后才清理开发测试。旧 CI 失败不以清理测试规避，已先修 harness 并保留本轮实际回归。

## 开发源码留痕与清理

最终可恢复开发提交：`b85a5472086865bbac3072f35c43d35b3369152c`，已通过 GitHub commits API 读回完整 SHA。原始源码地址：`https://github.com/puzige/vega/blob/b85a5472086865bbac3072f35c43d35b3369152c/<path>`。

| 路径 | SHA256 |
|---|---|
| `crates/vega_conversation/src/agent/shell_control_tests.rs` | `a62cbad16ecca329091c9411d0dc71ef6e22efc4be6c430b7cc7f3617f16bf54` |
| `crates/vega/src/app_agent_tests.rs` | `96c8da9e22e1ab96a58a19e1d34c54f92a6808adb601cfcbd497c22f38cf7688` |
| `crates/vega_ui/src/conversation_stream/composer_run_tests.rs` | `76d184b590fc65f84bcc14ce5fa993fd1998e2bf6a056f13e176c8f9c683df31` |

独立实现审查绑定 `0b617e0e0a6389c7c5945c1a47924911c86ecdfa`，主线集成复核绑定 `b85a5472086865bbac3072f35c43d35b3369152c`，均无阻塞发现。最终开发版本本地 13/13、严格 Clippy、三组 Python 已重验。开发测试清理仅移除三份测试源码及三处独占模块声明；生产参数校验、真实清理状态、终态动作和现有 Python 不删除。

清理后最终候选还需 fmt/diff、严格 Clippy、静态 Rust 清点、清理差异复核与该 head required CI。最终 Rust 空集不是行为测试通过；native 窗口和真实用户进程清理仍待用户手测，Issue 保持开放。

清理后本地复验：fmt/diff、受影响三 crate 全目标严格 Clippy 均通过；`cargo nextest list --workspace --message-format json` = 0 discovered，`cargo nextest run --workspace --no-tests=pass` = 0 executed（no Rust tests）；crates/xtask 的测试属性、cfg(test) 及本轮模块标记静态扫描无命中。独立清理复核和最终 head 云端仍待完成。
