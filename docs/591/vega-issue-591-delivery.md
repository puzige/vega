# Composer Resume 开发验证与交付记录

## 范围与基线

- Issue：[#591](https://github.com/puzige/vega/issues/591)；PR：[#608](https://github.com/puzige/vega/pull/608)。
- 规格：[Native composer Resume](vega-issue-591-composer-resume.md)，A01–A10；Codex ACP 与 Failed 重试不在范围。
- 当前生产基线：`dee2009d8cbe93f3765ea8360a1239da9df2d972`。保留其 #632/#654 共享准备期 Interrupted 收尾、停止事件与停止行投影。
- 开发验证 head：`698e22a9b69defe6b0aad48388d5a8f7d517f114`。
- 固定基线到开发 head 的 diff SHA-256：`2f7cd38360c38c99797ff6d3bc15f6ca5ef228a55ce2d023441d561a9cdcdc8a`。

## 实际开发验证

[Actions run 40369947862](https://github.com/puzige/vega/actions/runs/40369947862) 在上述精确开发 head 完整成功：

| 检查 | 实际结果 | 原始证据 |
|---|---|---|
| fmt / 全目标 Clippy | 成功，deny warnings | [job 121737685105](https://github.com/puzige/vega/actions/runs/40369947862/job/121737685105) |
| Rust Nextest | 7 discovered / 7 executed / 7 passed / 0 failed / 0 skipped | [job 121737685869](https://github.com/puzige/vega/actions/runs/40369947862/job/121737685869) |
| 现有 Python 三组 | 5 / 15 / 40 tests，均 OK | 同一 Nextest job |
| required `check (fmt, clippy, test)` | 成功 | [job 121740355329](https://github.com/puzige/vega/actions/runs/40369947862/job/121740355329) |

实际云端命令：`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets -- -D warnings`；`python3 scripts/release_tests.py`；`python3 scripts/prepare_computer_use_tests.py`；`python3 -m unittest discover -s scripts/tests -v`；`cargo nextest list --workspace --message-format json`；`cargo nextest run --workspace`。

七项回归验证真实存储/准备/运行入口，不以源码字符串或镜像 SQL 代替生产决策：

1. 无新增 user、连续 seq、原历史保留、旧请求不能重复启动。
2. 重开 store 后恢复 Native/active/实际 Interrupted 尾行；Done、Failed、Streaming、Codex、archived、较新 user 排除。
3. pending message、当前回合凭据回执阻塞；无关旧回执不阻塞。
4. 过期身份拒绝与提交前取消回滚。
5. 已终态工具结果重建/复用且原记录不变。
6. 应用实际配置入口携带 controller 已持有 admission 启动，观察一次 MessageStarted/provider 请求、无新增 user、旧请求拒绝和占用释放。
7. 确定性在提交后、MessageStarted 前取消，观察零模型请求、原行/工具不变、唯一 Interrupted 事件与尾行身份一致，并释放占用后再次真实续跑。

测试 provider 只隔离外部模型服务；不构成真实模型、原生 UI 或外部工具系统验收。

## 可恢复原始测试与独占支持

以下源码均通过 GitHub contents API 读回并核对 SHA-256，独立于本机临时文件与会过期 artifact：

- [应用回归原始源码](https://github.com/puzige/vega/blob/698e22a9b69defe6b0aad48388d5a8f7d517f114/crates/vega/src/app_agent/resume_tests.rs)：`8eac74f7bcd163b8b7b8d08ed818922a0d85d0d8c4e76ac763e21ac086869c17`。
- [会话回归原始源码](https://github.com/puzige/vega/blob/698e22a9b69defe6b0aad48388d5a8f7d517f114/crates/vega_conversation/src/agent/resume_tests.rs)：`a1713d0ecb65b99d3dee9b49778227c0fe0cff62a48dc4a6d76e67a8cbfe282d`。
- 同 SHA 的 `crates/vega/src/app_agent.rs`、`crates/vega_conversation/src/agent/{mod,entry,persistence}.rs` 保留模块接线与 `before_runtime_start` 测试专用回调，可从该完整提交恢复。
- [Issue 实际 GREEN 及源码留痕](https://github.com/puzige/vega/issues/591#issuecomment-6423139194)。

获得实际 GREEN 后，交付树移除两份 Rust 测试、两处模块声明及回调字段/调用；不改生产逻辑、安全断言、依赖、Python tests/fixtures、workflow 或打包。无测试专用 dependency/feature 新增。

## 审查与失败归因

- 非实现者审查在早期 `b16ecde3de901f5a2b3b09868a7c98fd455daa34` 找到 admission 漏传及提交后取消写 Failed 两项产品缺陷；旧五项回归绿色未覆盖这些启动边界，不作为缺陷已解决的证据。
- `2464448bd6507e440d8d3ec5c03163587703c07d` 修复后独立复审无 blocker；因同时合入上游停止行修复导致冲突，该 head 无 Actions run，不是测试失败。
- `908131a3393ca4766b3bbd2c68e6e53fa424d23e` 整合上游后独立复审无 blocker，确认 admission、共享取消/事件和 UI 刷新兼容。
- [run 40368669620](https://github.com/puzige/vega/actions/runs/40368669620) 的编译失败归因于应用测试的通配导入遮蔽 Rust 原生 `#[test]`，触发 GPUI 同名宏递归。`698e22a9…` 仅改显式导入，生产代码与已审查整合版相同；不提高 recursion_limit，不删测试或放宽断言，随后七项真实执行通过。

## 最终门禁与手测边界

本地仅静态 diff / 格式 / locale 及 Rust 测试残留清点；未运行 Vega、编译、构建、打包或本地 Rust/Python 测试，未安装应用或操作生产数据。

本记录提交时，删测后的精确最终 head 独立审查与完整 required CI 待取得，结果在 PR / Issue 绑定完整 SHA 留痕。最终要求 Rust **0 discovered / 0 executed**，明确写 `no Rust tests`，不冒充功能测试成功。最终门禁通过才 squash merge，合并后 Issue 保持开放、Project In review。

用户仍需手测 A01–A10：真实停止/继续、无新用户消息、历史和工具结果保留、草稿/附件优先及清空、快速重复激活、重开、Done/Failed 不误判、Tab/Enter/Space/读屏和停止行兼容。CI 与合并不代表用户验收；不保证不同 call ID 的外部副作用 exactly-once。
