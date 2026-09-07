# R4b 权限真实应用入口验收补充

v0.1 · 2026-09-05 · test-only bounded acceptance

## 目的

补充 `docs/vega-r4b-permission-order-sdd.md` 的运行级证据边界。测试必须从
`ConversationStream::submit_message` 发出真实 `ComposerSubmitted`，经
`VegaWindow` 的订阅与 `submit_composer`/`start_agent_run` 进入现有 worker，不能
直接构造 `PermissionRequest` 或直接调用 `run_agent_worker` 后宣称覆盖应用入口。

## 冻结夹具

- 项目目录、SQLite 数据库和测试配置均由 `tempfile` 创建；配置通过现有
  `cfg(test)` provider/config seam 注入。测试不读取用户配置、Keychain、真实文件
  或真实 provider。
- 线程使用 `Execute` + `Confirm`，模型使用内置定价 authority 中的固定测试模型。
  `MockProvider` 是唯一替换的 provider/network 边界。
- 第一轮脚本只提出一个 `write` 调用并结束为 `ToolUse`；第二轮必须收到工具结果后
  返回文本并以 `End` 结束。写入路径是项目内相对路径，内容为固定测试 sentinel。

## 必须证明的链路

1. 真正提交后，worker 先通过现有 `PermissionQueue` 请求权限；在
   `ToolCallProposed` 进入 app poll/stream ingress 前，请求保持 pending。
2. 精确 proposal 进入当前 stream 后，只出现一个对应的 PermissionCard；测试通过
   实际 UI 键盘 action 选择“允许一次”，不直接响应 queue latch。
3. 文件在批准前不存在，批准后内容完全匹配；唯一 call id 的持久化生命周期以
   `success` 终结。worker 继续发起第二次 MockProvider 请求，该请求包含对应的
   tool-result 消息，最终文本事件进入当前 stream 并完成 run。
4. 断言 `MockProvider` 收到 2 个请求、该 call 只有一个成功的持久化 audit、active
   run 和权限卡均终结。`write` 是覆盖语义，因此本测试不把最终字节内容冒充底层文件
   syscall 的 exactly-once 计数；R4b 既有 recorder 测试继续负责其可观测的单次副作用
   证据。现有 R4b 的拒绝、超时、重复、terminal-first、取消和身份不匹配反例继续
   保留；本补充不改变权限生产语义。

## 允许的测试 seam 与边界

只允许复用 `VegaWindow` 已有的 owned provider/config override、GPUI test window
和 `MockProvider`。若需要判断卡片确实由 stream 显示，只增加不暴露 payload 的
只读 presence 查询；不得把 queue pending 状态单独当作卡片显示证据。拒绝/超时
矩阵仍以既有 R4b UI 测试为准，不扩展为新的权限状态机。

## 验收

测试名和精确命令在交付文档中记录，先保留首次失败 raw log，再执行该测试的定向
回归。随后由 root 分配共享 Cargo 后执行 `cargo fmt --all -- --check`、定向
`cargo clippy --all-targets -- -D warnings` 和必要 build；workspace 全量由联合树
统一安排。性能测试、soak、真实 provider 和 CUA 均不属于本补充。

## 变更记录

- v0.1：冻结真实 submit/app ingress、request-first 卡片、Once 审批与成功终态
  tool-result 证据边界；文件覆盖写入不宣称底层 syscall 计数。
