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

## 独立审查增量

首个开发提交：1a5d42d8697e978ea94956f63d5b33dc416d1c9a。非实现者发现 P2：Prepare 期间关闭浮层，迟到确认可保留；草稿随后改权不失效旧决策，可能使用旧权限快照恢复 blocker。按同一安全验收修复：关闭丢弃未提交确认并使序号失效（已提交 Resume 不伪称取消）；改权 / 模式变化重置恢复状态；回执比较权限身份；窗口提交前比较当前权威草稿及确认 source。

增量本地 UI 状态回归 7/7 通过（编译 6.62 秒）；受影响 all-target strict Clippy 退出 0（4.03 秒）。新增测试显式覆盖 pending Prepare 关闭及已提交 Resume 的区别。服务回归先前 9/9，下一最终版本再次复验。真实 native 焦点/布局仍未运行。

保留 Python 回归：`python3 scripts/release_tests.py` 5/5、`python3 scripts/prepare_computer_use_tests.py` 15/15、`python3 -m unittest discover -s scripts/tests -v` 40/40 均退出 0。

## 剩余验证与边界

开发测试将固定可访问提交、源码和真实结果；按仓库规则合并前删除 Rust 测试及独占接线，最终空集 head 的 required CI 仅说明 no Rust tests，不当作行为测试通过。

实现独立审查、最终 required CI 尚待取得。真实窗口的浮层布局、键盘/焦点、Escape 与手动恢复仍交用户验收，不以状态测试冒充。没有升级、重启或访问修改远端机器。
