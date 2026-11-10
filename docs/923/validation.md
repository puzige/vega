# 后台控制工具验证记录

## 开发基线与范围

基线 `2a29adf9b2ea78eaf59d591838531b45adc7373e`。修复提案及审批来源两处漏接；控制参数限定工具名、2048 字节、ULID、wait/cursor 类型和界限，cancel 只允许 handle。恢复/取消的既有回执真实性与所有权逻辑不放宽。

失败消息不等于 worker 已停止。现有实际接线在 `app_agent::drain_agent_updates` 的 Finished/断连后进入 `window/artifact.rs` 的完成处理，调用控制器 finish、侧栏 set_thread_running(false)、完成消息状态及 composer_finished；Error 事件本身不释放控制器。此次暂未找到该链的产品缺陷，不依据历史截图强行清状态。ComposerActions 原有终态动作提取为可直接验证的方法，不增加依赖或改变取消事实优先级。

## 实际本地验证

工具链：Cargo 1.95.0（huacnlee/cargo fork，按机器已安装固定版本，CARGO_FORK_AUTO_UPDATE=0），rustc 1.95.0。命令在本任务工作树执行，未运行远端命令、真实模型或用户未知命令。

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
