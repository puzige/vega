# 图片预算修复：云端证据

## 测试冻结与有效 RED

- 规格 v1.1：[规格](vega-issue-547-image-context-budget.md)。先行补丁准备/审查见[检查点](vega-issue-547-test-checkpoint.md)。
- 生产基线：`871f91f246e90753743a256cee1eec315e74effc`。
- 测试先行提交：`fb0eb4d08dde64e24dc0fd27f91c6c9cf9ac9cac`，生产行为未改。
- PR：[557](https://github.com/puzige/vega/pull/557)（draft）。
- [原始测试 patch](https://github.com/puzige/vega/commit/fb0eb4d08dde64e24dc0fd27f91c6c9cf9ac9cac.patch)。原测试源码链接固定在 PR。
- [RED run 39815773545](https://github.com/puzige/vega/actions/runs/39815773545)，[Nextest job 119953866284](https://github.com/puzige/vega/actions/runs/39815773545/job/119953866284)。
- 2026-10-22 16:43:51Z：Nextest 原始输出 `40 tests run: 27 passed, 13 failed, 0 skipped`；从 17 binaries 发现/开始 40 tests，无 ignored 入口。
- 云端 fmt、Clippy 与 Python 检查成功，测试编译/发现成功。汇总门禁因目标测试失败而失败；不是最终交付绿色。
- `gh run view --job --log-failed` 原始导出 SHA256：`f6b9659fc7dbcf13aac0400a57db0380168ffd828bbc9d3c451f8936e900e857`。初次下载遇到 GitHub run 已结束/job 状态尚未同步，按五分钟节奏等日志可读，没有重新运行 CI。

### 13 个实际行为失败

| 测试/目标 | 原始结果 |
|---|---|
| actionable over-limit message | 实际仍为 Adjust context capacity；缺少减少当前输入/移除图片的恢复动作 |
| awaited-preflight cancellation | `Context(MissingHook)`，未返回 Interrupted |
| equal-pixel historical image retention | 实际图片为空；预期保留 262744 bytes 的合法 512×512 PNG |
| two-screenshot first send | `Context(NoCompactablePrefix)`，未到 provider |
| initial hard limit with automatic compaction | `Context(NoCompactablePrefix)` 而非 typed OverLimit |
| initial hard limit without hook | `Context(MissingHook)` 而非 typed OverLimit |
| initial provider setup overflow | `Context(NoCompactablePrefix)` 覆盖原 provider 失败 |
| initial provider stream overflow | 同上 |
| initial exact-hard without hook | `Context(MissingHook)`，预算内被阻断 |
| initial trigger/exact-hard with hook | 预算内意外调用 hook |
| initial trigger without hook | `Context(MissingHook)`，预算内被阻断 |
| compression-independent image estimate | 同像素两编码增量 `[17747, 17747]` 对 `[1385, 1385]` |
| nonzero geometric image cost | 1×1 实际 `[1043, 1043]`，契约 `[1025, 1025]` |

27 个通过对照包括：单首轮多组已完成工具历史仍压缩、已有历史正常/强制单次恢复、无 hook 和部分流输出不恢复、forced 超时/不减小/高于 target 均停止、hard-limit 不绕过、SourceChanged/权限投影保护、摘要期间/安装后的取消、图片全部原有限制、tool 图片估算、完整历史图片组/顺序及 v2 checkpoint 真正 SQLite 读回。

主 Agent 读取实际失败与通过清单，确认没有测试夹具/编译/runner 故障混充 RED。测试/目标/断言自本阶段冻结；实现不得追加、删改或放宽测试。

## 冻结测试内容

| 文件 | SHA256 |
|---|---|
| `crates/vega_runtime/tests/image_context_budget.rs` | `cd341ae8023aa5b349a3434f26ebcb630f71d901b0b30181767fc4db33936abd` |
| `crates/vega_conversation/tests/context_over_limit_message.rs` | `3c76c9cf630cf37f97ea6b371ded641cb1793a39a134f279535edf1d57cf8bce` |
| `crates/vega_conversation/src/agent/pipeline_image_tests.rs` | `ee42b938990d51977c63f3fa5b74da2c061d88fe57051e5896d2d96b68b98bc4` |

## 实现恢复检查点

专职实现子会话在补丁写入后、READY 回报前失败；工具已确认终止。主 Agent 检查 Git/工作区并保留现有改动，不重复实现、不丢弃成果。固定补丁基于上述测试先行提交，仅修改 3 个生产文件，35 行新增/32 行删除：

- `context.rs`：删除编码字节 token 项、保留几何与 checked arithmetic、估算器 v3。
- `agent/loop_.rs`：精确初始形态私有判定；preflight 后取消先行；普通自动摘要及两个 provider-overflow recovery 入口限定；已进入 forced 分支/硬上限/其他保护不变。
- `types/events.rs`：冻结的可操作超限提示。

生产 diff SHA256：`cefc98813590938c6648669a0572bd762810e4fad0da658c08f834cabc53c075`。冻结测试与测试接线相对 RED 提交无差异。主 Agent 对全部改动执行 `rustfmt --edition 2024 --config skip_children=true --check` 和 `git diff --check`，通过；未执行代码。写入者已停止，固定内容可交独立审查与同测试 GREEN。

## 后续阶段

- 最小生产实现：补丁已恢复并完成主 Agent 静态检查，待独立审查。
- 同一冻结测试 GREEN：待现有 Actions。
- 原测试源码/patch 与 RED/GREEN 证据固定后独立删测：待完成。
- 最终 head 全门禁与 `no Rust tests`：待完成，不能代替带测试 GREEN。
- 用户手测/应用安装：未完成；代码合并不等于应用已更新。

本地只执行快速静态、文件级格式和 diff 检查，无 Vega 执行/测试/编译/打包。
