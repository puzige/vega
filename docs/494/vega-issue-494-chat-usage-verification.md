# Chat usage 字段优先级验证记录

对应 [规格 v4.1](vega-issue-494-chat-usage-cache-handoff.md)、[冻结测试与来源](vega-issue-494-chat-usage-frozen-tests.json)、[PR #508](https://github.com/puzige/vega/pull/508)。用户要求直接修复，由 Codex 组织专职子 agent 执行；本地仅静态检查，所有运行、编译与测试交 Actions。

## 测试先行与有效 RED

生产基线 `0d21c4ce2947c4f3e3602680d8763e94f5cccf7e`；规格先行提交 `ae7d22cdb932e335f68ccb442e062359ea5a5c9d`；测试先行提交 `fd01374536bd45a15bd7654dcdbf9ca566b917c7`。测试提交只添加 3 份局部 Rust 测试/支持源、cfg 模块、已锁 tempfile 的开发依赖边及冻结清单；`sse.rs` 与生产基线逐字节相同，没有整目录恢复旧测试。

[RED run 39380495467](https://github.com/puzige/vega/actions/runs/39380495467) / [Nextest job 118488271985](https://github.com/puzige/vega/actions/runs/39380495467/job/118488271985) 原始输出：`Starting 11 tests across 14 binaries`、`11 tests run: 6 passed, 5 failed, 0 skipped`。Format、全目标 Clippy 及三组 Python 检查成功。5 项失败均是冻结的缓存字段兼容正向目标：读优先级/显式零、写优先级、完整工具终态、渐进快照/位置、真实 HTTP→OpenAiProvider→runtime 两 Read 续轮。其余 6 项保护通过；某表行失败后的后续行不冒称已执行。

四个 parser 正向失败直接报告 typed ProviderDiagnostic Protocol；runtime 正向失败在首轮成功断言，输入同样含冲突 alias，没有实现改变。不是编译、依赖、网络或 runner 失败。原始 Nextest 日志为 61184 字节，SHA256 `4ad34d7a0aeee3e2a49632fa6937f052cd2afd697310f3f526370d0fdeacefb8`。

三份源码在 GitHub API 的 blob 身份与本地源一致：`chat_usage_tests.rs` 为 `66d4c85fb181f399f9c71bdbf5fc38f2ebc03da0`，`chat_usage_runtime.rs` 为 `257c24cb12a50d9731da1dc480a9f29ee319ff3b`，`chat_usage_http.rs` 为 `9d4a53ce4245479c45b7372718e81799ee793403`。[测试提交源码和原始补丁](https://github.com/puzige/vega/commit/fd01374536bd45a15bd7654dcdbf9ca566b917c7)可恢复完整输入/断言，不依赖本机临时文件。

[公开 RED 审查与实现授权](https://github.com/puzige/vega/pull/508#issuecomment-6284941146)在上述云端结果之后发布。其后才授权子 agent 仅修改 `usage_counts`；11 个测试、所有数据/预期和 6 个源/接线哈希保持冻结，不补测或放宽断言。

## 同一测试 GREEN

待修复提交与现有 Actions 验证；未取得结果时不标通过、不删测。

## 公开留证、独立删测和最终门禁

待同一测试完整 GREEN 后先在 PR 固定可恢复源码、完整 SHA、原始 run/job/准确计数；然后才独立删除本轮 Rust 测试和独占支持。最终最新 head 须完整 required check 成功，Rust 明确为 0 发现/0 执行、no Rust tests；空集不替代本轮行为 GREEN。

## 发布与用户手测

尚未合并或发布本轮修复。真实代理、DeepSeek/Step 及原生 UI 由用户在包含修复的发布版本中手测；Issue 保持开放，用户明确通过前不关闭或 Done。
