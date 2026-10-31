# #785 交付与测试清理证据

## 实现与首轮审查

- 基线：`c3475642d5f031f282c108e39cc11c9506615d0a`。
- UI/单草稿修复 head：`0f34591e9c22447693d94cca33cf95b163a7b33c`，PR [#786](https://github.com/puzige/vega/pull/786)，Issue [#785](https://github.com/puzige/vega/issues/785)。
- 非实现者独立只读审查绑定上述 base/head：无可操作发现。核对侧栏无替代 UI、单草稿导航、普通/Codex/自动化物化、权限重置、思考默认保存与迟到回执隔离。不能替代云端或用户手测。
- 本地实际执行：修改 Rust 文件 `rustfmt --edition 2024 --check`、`git diff --check`、两份 locale TOML 解析；侧栏两个文件及导航文件与 `fe0fdef3880e50b1e17b8ebf3b9118ef738cf768` 内容精确相同。未运行本地 Vega 代码、编译、测试或打包。

## 清理前实际云端结果

[Actions run 41149631801](https://github.com/puzige/vega/actions/runs/41149631801) 全部成功：

- [Clippy job 124382794254](https://github.com/puzige/vega/actions/runs/41149631801/job/124382794254)：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 成功。
- [Nextest job 124382793969](https://github.com/puzige/vega/actions/runs/41149631801/job/124382793969)：Python 三组 unittest 分别执行 5、15、40 项并通过；`cargo nextest list --workspace --message-format json` 发现 10 项，`cargo nextest run --workspace` 实际执行 10 项、通过 10 项、跳过 0 项、失败 0 项。
- [required check job 124383692004](https://github.com/puzige/vega/actions/runs/41149631801/job/124383692004) 成功。

上述 10 项为基线已有 Rust 测试，并非本卡新增，覆盖提交诊断而非本卡原生 UI 交互；不能将它们声称为单草稿真实行为验收，也不满足最终空集交付要求。用户已明确授权本次清理这些测试及专属接线。

## 可恢复源码

以下完整提交已推送，在清理前逐项通过 GitHub Contents API 读回确认可访问；源码保留在提交/PR 历史，不复制进最终 master 文件树。原始修复 patch：[0f34591e.patch](https://github.com/puzige/vega/commit/0f34591e9c22447693d94cca33cf95b163a7b33c.patch)。

| 原始文件（完整源码） | SHA-256 |
|---|---|
| [app_agent.rs](https://github.com/puzige/vega/blob/0f34591e9c22447693d94cca33cf95b163a7b33c/crates/vega/src/app_agent.rs)；末尾 1 项测试及其模块 | `58a6e6b824c254f0e9dad24305cd49e498484fc35bf21b817e05c9505771e53f` |
| [agent/ordinary.rs](https://github.com/puzige/vega/blob/0f34591e9c22447693d94cca33cf95b163a7b33c/crates/vega_conversation/src/agent/ordinary.rs)；末尾 1 项测试及其模块 | `241c941b229bae1351407b73ee01b421d725ec3efb09a25b92960d62576b71f2` |
| [submission_diagnostics.rs](https://github.com/puzige/vega/blob/0f34591e9c22447693d94cca33cf95b163a7b33c/crates/vega_conversation/src/submission_diagnostics.rs)；末尾测试模块声明 | `b248337264d48c67c72b2ee2d65066468f20dd72984bbadddf5b5c3a7195cbd2` |
| [submission_diagnostics/tests.rs](https://github.com/puzige/vega/blob/0f34591e9c22447693d94cca33cf95b163a7b33c/crates/vega_conversation/src/submission_diagnostics/tests.rs)；8 项测试及独占 helper | `b4bb98373b3d27b7fe17b595c9670f2fbb2b784983404569f924bbc63290a5c9` |

## 最终版本待验证

清理增量审查及最终 head 的完整 required CI 尚未取得；最终需确认 `no Rust tests`（0 发现、0 执行），空集不是 Rust 测试通过。未安装或重启远端应用、未改数据库，用户 UI/真实请求手测尚未进行，Issue 保持开放。
