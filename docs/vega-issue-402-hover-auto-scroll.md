# Issue #402：悬停文案不再触发滚动

状态：2026-10-16 实施规格。关联 [Issue #402](https://github.com/puzige/vega/issues/402)、Issue #147（Markdown 选择与复制）、Issue #103（有界披露滚动）。

## 1. 问题与依据

用户在正文区域上下移动鼠标即触发滚动，不需要该行为。Issue 未附截图，诉求是“鼠标在文案区域会造成滚动”。

源码与依赖已确认的缺口（本地未编译、未运行，静态定位）：

- Vega 侧不存在 hover→滚动代码。全仓无 hover 触发的 `scroll_by`/`scroll_to`；正文内唯一的 `on_mouse_move` 位于 `message_anchors.rs`，只更新 hover 状态。
- 滚动来自 GPUI 的文本选择自动滚动（AutoScroll），由窗口级 mouse-move 驱动。`TextSelectionLayer`（`crates/vega/src/window/render.rs`）与每条消息的选择注册（`crates/vega_ui/src/conversation_stream/selection.rs`）是 Issue #147 的正常用法。
- 依赖锁定 `gpui_kit = "=0.6.0"`（`Cargo.toml`）与 `gpui-base 0.6.0`（`Cargo.lock`）。在 0.6.0 中 `WindowSelectionState::update_in_window` 仅以 `!cx.has_active_drag()` 为门槛，随后无条件执行 `update_auto_scroll`；`update_auto_scroll` 只检查 `anchor.inside`，缺少 `is_selecting` 守卫。相邻的 `update_impl` 已有 `is_selecting` 守卫，两处守卫不对称即为缺陷。
- `end()`（mouse-up）在 `did_hit_text` 时保留 anchor 以供 shift 扩展，因此“在正文单击过一次”之后，普通 hover 移动仍会进入自动滚动路径。触发带为可见裁剪区上下边缘各 16px；delta 非空时每 16ms 合成一个 `ScrollWheelEvent` 派发，由会话列表消费。

上游在 `gpui-base 0.6.1` 修复：`update_auto_scroll` 增加 `if !self.is_selecting { return; }`，并新增回归测试 `pointer_moves_after_a_click_do_not_auto_scroll`。语义与用户诉求一致：手势结束后保留 anchor 用于 shift 扩展，但只有真实拖拽才允许自动滚动。

## 2. 行为契约

- 在正文（用户消息与助手 Markdown）上移动鼠标、不按下按键时，会话列表的阅读位置不变；不得合成滚轮事件，不得启动自动滚动。
- 按住左键拖拽选择文本并移动到视口上/下边缘时，自动滚动仍然可用（Issue #147 既有能力保留）。
- 单击正文后 anchor 仍保留用于 shift 点击扩展；该 anchor 不再成为 hover 滚动的触发条件。
- 消息导航轨道、披露区内部滚动、贴底跟随、回到底部按钮、选区复制与安全投影等既有契约不变。
- 范围仅限依赖补丁升级，不新增 Vega 侧规避逻辑；`TextSelection::clear` 会连带清空用户选区，不作为方案。
- 不新增依赖，不改变 `TextSelectionRegistration` 的公开 API 用法；升级只涉及补丁版本，`gpui-base` 无公开 API 删除。

## 3. 依赖变更

- `gpui_kit`：`=0.6.0` → `=0.6.1`（`Cargo.toml`）。gpui-kit 的家族版本彼此硬绑定（`0.6.1` 声明 `gpui-base`/`gpui-component`/`gpui-kit-assets` 均为 `^0.6.1`），因此 `Cargo.lock` 中 `gpui-base`、`gpui-component`、`gpui-component-macros`、`gpui-kit-assets` 一并对齐到 `0.6.1`，家族版本保持一致。
- 依据 `docs/vega-exec-guide.md` §5，依赖白名单变更需批准；本卡按用户 2026-10-16 授权实施该补丁升级。
- `0.6.0 → 0.6.1` 公开 API 变更仅为新增；`gpui-base` 无公开项删除，`gpui-component` 的 `IconNamed` 由直接定义改为 `pub use gpui_kit_assets::IconNamed` 再导出，Vega 未引用该符号（全仓 grep 无命中）。
- `gpui-pre` 系列维持 `0.3.4`：crates.io 索引显示 `gpui-base 0.6.1` 与 `gpui-kit 0.6.1` 均以 caret `^0.3.1` 声明 `gpui-pre`/`gpui-pre-platform`，故 `0.3.4` 是合法解（非人为降级）；平台层不随本卡改动（`gpui-pre` 属 0.x minor 升级，超出本卡补丁范围）。
- `0.6.5` 已被上游 yank，不选用；`0.6.6` 则把 `gpui-pre` 改为精确 `=0.3.6`，会强制平台层 0.x minor 升级（超出本卡范围），故显式选 `0.6.1`。
- 本地仅做依赖解析（`cargo metadata --locked` 退出 0，锁文件未被改写），确认 `Cargo.lock` 与 `Cargo.toml` 自洽。
- `gpui-kit 0.6.1` 去掉了 `0.6.0` 的 `reqwest_client` 依赖，因此 `gpui-pre-reqwest-client`、`gpui-pre-http-client-tls`、`aws-lc-rs`、`syntect` 等传递依赖从锁中移除。Vega 源码不引用 `http_client`/`syntect`（全仓 grep 无命中），网络栈由 `vega_runtime` 直接依赖 `reqwest`，不受影响。
- 依赖解析只做版本选择，未在本地编译；可编译性与行为正确性由云端 `pr-check` 验证。

## 4. 验收矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 状态 |
|---|---|---|---|---|---|---|
| A1 | hover 不滚动 | 正文可滚动且已脱离贴底 | `issue402_a1_hover_in_body_after_selection_does_not_scroll`：在正文拖拽选中后，鼠标移到视口下边缘（不按键），推进时钟 6×20ms | 列表 `logical_scroll_top()` 的 `item_ix` 与 `offset_in_item` 均不变 | 生产 GPUI 测试上下文 | 待 CI 验证 |
| A2 | 拖拽滚动保留 | 同上 | `issue402_a2_drag_to_viewport_edge_still_auto_scrolls`：正文按下左键并拖到下边缘，推进时钟 | `logical_scroll_top()` 改变 | 生产 GPUI 测试上下文 | 待 CI 验证 |
| A3 | 选择能力保留 | 正文有可选择文本 | `issue402_a3_body_drag_still_selects_text`：在正文文本上拖拽选择 | 该消息 `MessageCopy::has_selected_text()` 为真 | 生产 GPUI 测试上下文 | 待 CI 验证 |
| A4 | 依赖可编译且既有回归不破 | 补丁升级后 | 云端 `cargo fmt` / `clippy` / `nextest` 全量 | 汇总 check 通过 | 云端 PR Check | 待 CI 验证 |

A1 为本次回归断言；A2 证明夹具能观测到自动滚动（否则 A1 可能因未触发路径而空过），A3 证明点击点确实命中可选中文本（否则 anchor 不会保留，A1 同样空过）。

本地已执行（仅快速检查，未编译）：`cargo fmt --all` 退出 0；`cargo metadata --format-version 1` 退出 0 且不修改 `Cargo.lock`（依赖图自洽）。`cargo check`/`clippy`/`nextest` 未在本地运行，交由云端 `pr-check` 判定。

## 5. 非目标

- 不修改选择、复制、导航轨道、披露区滚动与虚拟化行为。
- 不引入 Vega 侧对 GPUI 内部状态的规避或补丁（不 vendoring）。
- 不改动真实模型、权限、存储与上下文契约。
