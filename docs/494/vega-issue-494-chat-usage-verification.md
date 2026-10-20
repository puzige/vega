# Chat usage 字段优先级验证记录

对应 [规格 v4.1](vega-issue-494-chat-usage-cache-handoff.md)、[冻结测试与来源](vega-issue-494-chat-usage-frozen-tests.json)、[PR #508](https://github.com/puzige/vega/pull/508)。用户要求直接修复，由 Codex 组织专职子 agent 执行；本地仅静态检查，所有运行、编译与测试交 Actions。

## 测试先行与有效 RED

生产基线 `0d21c4ce2947c4f3e3602680d8763e94f5cccf7e`；规格先行提交 `ae7d22cdb932e335f68ccb442e062359ea5a5c9d`；测试先行提交 `fd01374536bd45a15bd7654dcdbf9ca566b917c7`。测试提交只添加 3 份局部 Rust 测试/支持源、cfg 模块、已锁 tempfile 的开发依赖边及冻结清单；`sse.rs` 与生产基线逐字节相同，没有整目录恢复旧测试。

[RED run 39380495467](https://github.com/puzige/vega/actions/runs/39380495467) / [Nextest job 118488271985](https://github.com/puzige/vega/actions/runs/39380495467/job/118488271985) 原始输出：`Starting 11 tests across 14 binaries`、`11 tests run: 6 passed, 5 failed, 0 skipped`。Format、全目标 Clippy 及三组 Python 检查成功。5 项失败均是冻结的缓存字段兼容正向目标：读优先级/显式零、写优先级、完整工具终态、渐进快照/位置、真实 HTTP→OpenAiProvider→runtime 两 Read 续轮。其余 6 项保护通过；某表行失败后的后续行不冒称已执行。

四个 parser 正向失败直接报告 typed ProviderDiagnostic Protocol；runtime 正向失败在首轮成功断言，输入同样含冲突 alias，没有实现改变。不是编译、依赖、网络或 runner 失败。原始 Nextest 日志为 61184 字节，SHA256 `4ad34d7a0aeee3e2a49632fa6937f052cd2afd697310f3f526370d0fdeacefb8`。

三份源码在 GitHub API 的 blob 身份与本地源一致：`chat_usage_tests.rs` 为 `66d4c85fb181f399f9c71bdbf5fc38f2ebc03da0`，`chat_usage_runtime.rs` 为 `257c24cb12a50d9731da1dc480a9f29ee319ff3b`，`chat_usage_http.rs` 为 `9d4a53ce4245479c45b7372718e81799ee793403`。[测试提交源码和原始补丁](https://github.com/puzige/vega/commit/fd01374536bd45a15bd7654dcdbf9ca566b917c7)可恢复完整输入/断言，不依赖本机临时文件。

[公开 RED 审查与实现授权](https://github.com/puzige/vega/pull/508#issuecomment-6284941146)在上述云端结果之后发布。其后才授权子 agent 仅修改 `usage_counts`；11 个测试、所有数据/预期和 6 个源/接线哈希保持冻结，不补测或放宽断言。

## 同一测试 GREEN

生产修复提交 `530aec4ac68ab7c437b4f7de8161e0deae85643a` 只改 `usage_counts`，新增 5 行、移除 19 行。缓存读按 nested → DeepSeek → top-level 选第一个非 null 的数字；写按 nested → top-level。eager `.or(number(...)?)` 仍验证所有已识别非 null 数值；保留 selected read/write/总量及独立 miss 范围校验，去掉 alias 全等与跨来源 read+miss 强制分割。六个冻结测试/支持/接线文件 SHA256 全部未变。

[GREEN run 39381186494](https://github.com/puzige/vega/actions/runs/39381186494) / [Nextest job 118490424303](https://github.com/puzige/vega/actions/runs/39381186494/job/118490424303)：`Starting 11 tests across 14 binaries`、`11 tests run: 11 passed, 0 skipped`；0 失败/重试，run ID `db5bfd66-3c05-44be-9b42-5e0fddb8c43b`。所有表行、真实 provider/runtime 的两个 Read 和结果回传、续问复用、typed 错误/零执行及取消均通过。Format、全目标 Clippy、Python 5+15+14=34 和 required check 成功。

原始 Nextest 日志 52788 字节，SHA256 `89d7b5543a8b25fc3ea3e8665a5250b69ea488188a599664212fd093715a3738`；Clippy/format 原始日志 72247 字节，SHA256 `3d24f85918597e1f6211c42d7e94bd63186a7614698a36be6b8b9c06a6dc963f`。[完整 GREEN 与源码恢复记录](https://github.com/puzige/vega/pull/508#issuecomment-6285041628)在独立删测授权前发布，原输入/断言保持不变。

## 公开留证、独立删测和最终门禁

GREEN 和公开留证之后才授权子 agent 独立清理六路径：删除 `chat_usage_tests.rs`、`chat_usage_runtime.rs`、`chat_usage_http.rs` 三份专用源；将 `Cargo.lock`、runtime `Cargo.toml`、`openai/mod.rs` 的开发独占接线恢复至生产基线。只去掉 tempfile 的开发依赖边，包与生产依赖不变。`sse.rs` 修复 SHA256 必须保持 `2815518a30c65e08c5873501d5d9220484faf21dc1a9a23cd0203ad4c6854d4f`。

本记录随独立清理提交写入，最终最新 head 的完整门禁仍需 Actions 证明。准确清理提交、原始 run/job、Rust 0 发现/0 执行和 required check 结果在 [最终状态固定记录](https://github.com/puzige/vega/pull/508#issuecomment-6285049121)读回。空集不替代上述 11/11 行为 GREEN，也不证明真实服务已通过。

## 发布与用户手测

合并 SHA、发布时间及用户手测状态以 [最终状态固定记录](https://github.com/puzige/vega/pull/508#issuecomment-6285049121)和 [Issue 494](https://github.com/puzige/vega/issues/494)为准。真实代理、DeepSeek/Step 及原生 UI 由用户在包含修复的发布版本中手测；Issue 保持开放，用户明确通过前不关闭或 Done。
