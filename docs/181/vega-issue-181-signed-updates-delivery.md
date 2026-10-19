# Issue #181：独立签名更新交付

## 范围与冻结

- 2026-09-25；分支 `feat/181-signed-updates`。
- 规格：[独立更新签名](vega-issue-181-independent-update-signing.md)，SIG01–SIG09 为验收矩阵。
- 修正用户 0.1.1 → 0.1.2 只能手动下载的反馈：ad-hoc 应用可使用固定 Ed25519 公钥认证更新，支持实际运行的系统 Applications、用户 Applications 和兼容 Documents 安装。
- 客户端和安装助手均校验签名、版本、精确大小与摘要。助手从认证 ZIP 重新解压；保留任务保护、显式重启、Apple 身份连续性及失败恢复。
- CI 在最终 ZIP 产生后签名；新发布要求四项资产。按 tag 树识别历史两资产发布，已公开资产保持不可变。
- `VEGA_UPDATE_PRIVATE_KEY` 已配置为仓库 Actions secret，应用只包含公钥。新增直接依赖 ring 0.17.14 已由主控批准，版本此前存在于锁文件。
- 与规格偏离：无。

## 本地证据

| 命令 | 结果 | 范围 |
|---|---|---|
| `cargo check -p vega -p xtask --bins` | exit 0，46.18s；最终清理分支调整后再次 exit 0，1.03s | 编译；不是运行测试 |
| `git diff --check` | exit 0 | 改动空白检查 |
| `cargo clippy -p vega -p xtask --bins -- -D warnings` | exit 101 | 既有 `vega_runtime` 的 `mcp_registry.rs:163` 触发 `large_enum_variant`；未修改无关运行时 |

最终编译原始 footer：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.03s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
```

Clippy 原始摘要：

```text
error: large size difference between variants
   --> crates/vega_runtime/src/agent/mcp_registry.rs:163:1
error: could not compile `vega_runtime` (lib) due to 1 previous error
```

本次未新增或运行本地测试；会话约束要求只在用户明确要求测试时运行。云端现有 PR 门禁按仓库配置自动执行，最终结果以 PR 页面为准。未安装、启动或强退用户应用。

## 用户手测与限制

- NOT RUN：SIG01–SIG09 的运行行为及真实 macOS 原位更新，留待用户手测；编译或云端既有测试不代表这些场景已实测。
- 0.1.1/0.1.2 必须手动安装首个内置公钥的新版本一次，再通过后续版本检查自动下载、确认重启和实际版本变化。
- 检查有活动任务时无法安装、稍后保持当前版本、不可写目录具体错误，以及失败恢复。权限不足不提权，独立签名不替代 Gatekeeper。
- 若新应用启动确认成功而旧 root-owned bundle 无法清理，保留剩余备份与 `cleanup-required.txt`；更新成功不误报安装失败。
- 发布后保留 Issue 和 worktree，合并后转 In review，等待用户手测结论。
