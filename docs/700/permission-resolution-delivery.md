# #700 实现与验证记录

## 实现

- 保留 Full access 普通工具自动批准、danger 优先与危险单次明确确认，不修改危险检测、权限 reset/fence 或沙箱。
- 将显式拒绝、真实超时、运行取消与权限界面/队列不可用分开；均拒绝执行且不记忆授权。
- 新来源在 codec、runtime、live/recovery 与历史投影中严格配对；保留旧超时记录。
- 队列 first-wins、scope 替换和子任务收尾传递实际失败类别；独立 MCP/ACP/Computer Use/范围审批授权不放宽。
- 历史校验不将正常工具输出与权限反馈的文本碰撞误判为损坏。

## 固定版本与独立审查

- master 基线：`c42148f69c9cb987901aeae56932e7ecf57a24ce`。
- 规格：`8599bf58fbab8937fc86b9a378bdd91aa16ddf7b`。
- 初次实现：`1beb98438c9728551030730cc652fb5fce4b1835`。
- 独立非实现者静态审查发现历史成功输出碰撞 P2；修复后在 `0f89ffb16e42514d62b38ff187d91d4e7ef1b8cc` 增量复审，无新增发现。静态审查不代表竞态或真实 UI 验收。

## 开发回归证据

- 源码固定标签：[evidence/700-permission-regressions](https://github.com/puzige/vega/tree/evidence/700-permission-regressions)。已读回远端标签，指向 `0f89ffb16e42514d62b38ff187d91d4e7ef1b8cc`。
- 原始测试：`crates/vega_runtime/tests/permission_outcomes.rs`、`crates/vega_conversation/tests/permission_lifecycle.rs`、`crates/vega_conversation/src/permission_history_tests.rs`，以及该版本 history.rs 的 cfg 接线。
- [Actions 40627120649](https://github.com/puzige/vega/actions/runs/40627120649)：Clippy、Nextest、required 汇总均成功。Nextest 日志确认 Rust **8 发现、8 执行、8 通过、0 跳过**；三个 Python 检查分别执行 5、15、40 项并成功。
- 覆盖普通 Full access、危险确认不可绕过与无二次提示、各类失败审计、队列取消/失联/first-wins、历史文本碰撞和非法 source/status/output/metadata。
- 限制：显式注入 Timeout 不等于验证真实计时器；决策测试不等于执行器实际未启动证明；不覆盖全部持久化/live/recovery 链路或真实桌面交互。
- 开发测试和唯一 cfg 接线合并前删除；没有新增测试依赖或 fixture。最终无测试 head 必须重新通过完整 required CI，空集记 `no Rust tests`，不能称 Rust 测试通过。

## 当前交付状态

本地仅执行格式、差异空白与静态清点，未运行 Vega、编译或测试。PR #706；最终无 Rust 测试版本的 CI 与合并待核实。Issue #700 保持开放，合并后 In review 等用户手测，不自动关闭、不置 Done。
