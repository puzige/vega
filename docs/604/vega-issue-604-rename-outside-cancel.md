# #604 — 任务重命名外部点击取消

## 方案 v1 / 基线

- Issue：<https://github.com/puzige/vega/issues/604>
- 基线：`420a0eb9a06594173b658f7e3a4437e3d2b5432d`。
- 分支：`feat/604-rename-outside-cancel`。
- 本卡仅修复侧边栏任务标题的行内重命名取消，不改项目/分组编辑、持久化、排序或视觉样式。

## 根因与最小实现

`crates/vega_ui/src/sidebar/threads_block.rs` 中，`start_rename` 创建独立草稿输入实体；`render_rename_editor` 仅连接 Enter 的 `ConfirmRename` 和 Esc 的 `CloseSettings`，未连接外部点击。`cancel_rename` 已通过 `editing.take()` 丢弃草稿并通知刷新，且不调用持久化。

在 `ThreadsBlock::render_rename_editor` 的编辑器容器上增加 `on_mouse_down_out`，通过现有 `cx.listener` 调用 `cancel_rename`。不停止传播、不主动切换焦点、不提交草稿、不新增全局监听或依赖。保留原有 Enter/Esc action 接线。

所用 GPUI `0.3.4` 的 `Interactivity::on_mouse_down_out` 在 capture 阶段检查指针位于元素 hitbox 外部，并将事件交给回调；该 API 已在本仓其他编辑器/弹窗使用。只清除重命名状态，不截获目标的鼠标事件。内部点击不符合 outside 条件，选字/移动光标保持现状。选择 mouse-down 而非 blur，可覆盖不获取键盘焦点的空白区域。

## 验收矩阵

| ID | 操作 | 预期 |
|---|---|---|
| A01 | 未改名称，点击侧边栏空白处 | 退出编辑，原名称不变 |
| A02 | 修改草稿，分别点击空白处、另一任务行、主会话内容 | 退出编辑，保留原名称，草稿不落库 |
| A03 | 取消后再次触发重命名 | 输入值为当前原名称，不残留被取消的草稿 |
| A04 | 点击另一任务或其他可操作目标 | 取消当前重命名，同时目标操作仍执行，不吞点击 |
| A05 | 点击输入框内部、移动光标、选择文字 | 不取消编辑，不影响输入 |
| A06 | Enter 提交有效新名称 | 沿用现有提交路径，保存一次，无重复提交 |
| A07 | Esc 或 Enter 空白标题 | 沿用现有取消语义，不保存草稿；Esc 不关闭设置 |
| A08 | 双击标题/操作菜单触发重命名 | 编辑器正常打开，不被同次打开手势立即取消 |

## 验证、风险与回滚

- 本地禁止运行 Vega、编译、构建和测试。仅检查差异、格式及调用链。
- 本次是既有取消方法的一处 UI 事件接线，无新增业务决策；不为流程临时添加 Rust 测试。由非实现写入者独立审查 outside bounds、事件传播、草稿销毁与原有键盘路径。
- 最终 PR head 必须执行现有 Actions：fmt、Clippy、Python 检查和 nextest 清点/运行。Rust 空集如实记为 `no Rust tests`（0 发现/0 执行），不称 Rust 测试通过。
- 主要剩余风险是 GPUI 原生事件时序、真实焦点与目标操作组合；A01–A08 由用户在合并后的版本纯手测。CI/静态审查不替代用户验收。
- 回滚仅撤销本卡新增的外部点击监听，不改变数据 schema，无数据迁移。

## 交付记录

待实现、独立审查和最终 head 云端验证；未执行项不标通过。Issue 合并后保持开放，置 In review 等用户手测。
