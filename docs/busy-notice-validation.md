# 忙碌提示修复验证

基线：`25443251c954e1790d868f30ef901f9803ade4be`。
最终开发提交：`aa2d1f70cd0d6741e1a6c16993486e505e4f54c7`。

## 实现与审查

忙碌提示为独立 AgentBusy 类型，保留原文案；入口不覆盖真实错误或清除 controller_runtime_error。受运行身份围栏保护的终态仅清忙碌类型。准备出口在 attempt、lease generation、stream 核对通过后清理，仅在无运行时生效，不释放或改变运行状态。

独立审查初版 `39c10ded474599cde84b7eb0519e7d43194a04be` 发现准备失败/取消/断连不进入运行终态的遗漏；上述最终开发提交补齐准备出口，增量复审确认 P2 闭合，无新增阻塞。审查为静态检查，未重复动态测试。

## 本地实际验证

Cargo 1.101.0（huacnlee/cargo），rustc 1.98.0；CARGO_FORK_AUTO_UPDATE=0，CARGO_BUILD_JOBS=4。

- `cargo test -p vega -p vega_ui busy_ -- --nocapture`：9 发现 / 9 执行 / 9 通过，0 忽略；最终一次执行构建 4.88 秒，测试约 0.01 秒。vega lib 空集不计行为测试。
- `cargo clippy -p vega -p vega_ui --all-targets -- -D warnings`：通过，3.71 秒；既有 workspace unused tracing-subscriber 提示不属于本次修改。
- `cargo fmt --all -- --check`、`git diff --check`：通过。
- Python release_tests、prepare_computer_use_tests、scripts/tests：此前本轮实际执行分别 5/5、15/15、40/40。后续增量只涉及 Rust 提示状态和开发测试，不修改 Python。

准备测试初次编译因 fixture 把 owner 映射键误写为 SubmissionId（实际为 thread String）失败，只修测试接线，拒绝断言不变；随后上述 9 项全部执行通过。

覆盖忙碌到终态、取消/焦点语义、真实错误及同文案普通错误保留、真实错误替换忙碌、无提示终态、准备完成保留新运行、旧 generation/thread/stream 拒绝、旧 attempt 不移除新 owner。身份测试直接使用生产判定；名为 appended_message 的测试仅验证身份不依赖消息 ID，没有动态构造追加消息。准备测试使用真实注册接口生成身份，但未动态串联完整窗口回调/lease/stream链。

## 测试来源与清理

源码可从开发提交的以下路径恢复（推送后需核验远端可达）：

| 路径 | SHA256 |
|---|---|
| crates/vega/src/busy_owner_tests.rs | 9c8afb4cdf7935aa3b39a61f7643539f71e376ed29417cd850d0abea18cd2f09 |
| crates/vega/src/window/busy_preparation_tests.rs | 635b232ea813be8d1f08b8d1ecf9eb5b42a97aad465fd40b12c7271290cd88c6 |
| crates/vega_ui/src/conversation_stream/busy_notice_tests.rs | 9c19b2e1aec119947d5d68491d58122ebc02cd22294613ff90de18c3724f4e38 |

按仓库规范，交付树移除三份 Rust 测试与三处专用模块声明，保留生产状态方法和身份判定。清理后独立复核、最终严格检查及 required CI 尚待完成；最终 Rust 空集应记录为 no Rust tests，不替代上述行为回归。

未访问远端应用或数据，未部署、重启、重试用户任务。新 PR 保持开放等待用户决定合并；本地验证和静态审查不能证明截图现场的所有权状态，也不替代 native 手测。原始压缩故障与本任务无关，不宣称被本修复解决。

清理后复验：fmt/diff、受影响两 crate 全目标严格 Clippy 通过（4.84 秒）；Nextest workspace 构建 11.72 秒，0 发现 / 0 执行（no Rust tests）。独立清理复核确认生产字节未变、三份测试及模块接线完整移除、9 项源码哈希与开发提交一致，无阻塞。required CI 与远端源码可达性将在 PR 中回填，不把本地结果写作云端通过。
