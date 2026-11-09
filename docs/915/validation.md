# 验证记录

## 开发版本

基线 34555cec9c0c4c4c1f7a528d739752f414e53edf；本文件随开发测试提交固定内容版本。实际工具链：huacnlee/cargo 0.102.0-9（Cargo 1.101.0 e611c7a87），rustc 1.98.0（88d9e12ae）。命令均设置 CARGO_FORK_AUTO_UPDATE=0，编译 jobs=2，未更改机器配置或 CI 工具链。

本地已执行：

- `cargo test -p vega_conversation shell_recovery_tests --locked -j 2`：9 发现 / 9 执行 / 9 通过 / 0 失败 / 0 忽略 / 0 过滤，测试 2.49 秒，增量编译 3.78 秒。
- `cargo test -p vega_ui shell_recovery::tests --locked -j 2`：5 发现 / 5 执行 / 5 通过 / 0 失败 / 0 忽略 / 0 过滤，测试 <0.01 秒，编译 70 秒。仅状态决策与回执身份回归，不代表 native 交互手测。
- `cargo check -p vega --locked -j 2`：退出 0，75 秒。
- `cargo clippy -p vega -p vega_ui -p vega_conversation --all-targets --locked -j 2 -- -D warnings`：退出 0，18.19 秒。后续新增一条服务确认回归，最终版本仍须再次核对。
- `cargo fmt --all`、`git diff --check`：已执行，最终版本另核对。

首次失败保留归因：

1. GPUI `defer` 误用两个参数回调，编译 E0593。按当前仓库 API 使用单参数 App 回调，通过弱实体 update 调度；修正接线后完整应用编译成功。
2. 首轮服务回归 5 通过 / 3 失败，全部在 blocker fixture 的 record 构造处 InvalidQuery。存储要求先 prepared 再进入未知终态；测试补齐真实生命周期后 8/8 通过，新增确认用例后 9/9 通过。未修改生产状态机或断言。
3. 一次请求后台 shell 会话返回 workspace_sandbox_required，未启动命令；改用普通阻塞执行。不是产品编译失败。

覆盖：草稿无副作用读取、同项目另一对话和重叠子路径真实阻塞、无关工作区隔离、读取失败不伪造安全、无效草稿 / 已物化 / 陈旧权限 epoch 拒绝、持久对话 revision 检查、资源未关闭拒绝 prepare、版本变化拒绝、正确确认解除真实阻塞、错误来源确认拒绝，以及新对话隐藏 / 简短入口和陈旧回执隔离。

## 剩余验证与边界

开发测试将固定可访问提交、源码和真实结果；按仓库规则合并前删除 Rust 测试及独占接线，最终空集 head 的 required CI 仅说明 no Rust tests，不当作行为测试通过。

实现独立审查、最终 required CI 尚待取得。真实窗口的浮层布局、键盘/焦点、Escape 与手动恢复仍交用户验收，不以状态测试冒充。没有升级、重启或访问修改远端机器。
