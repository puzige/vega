# R57 实施分解（Agent 派发计划）

> 父规格：`docs/vega-r57-composer-alignment.md`（冻结于 `41cd088`）
> 基线：`master @ 41cd088`
> 拆分原则：**按依赖链拆，不按文件拆**。理由是共享文件与构建锁两个硬约束（见 §1）。

---

## §1 拆分的两个硬约束（决定了为什么不能无脑并行）

**约束 1：共享文件 `render.rs`**
底部行（`render_compact_settings` @246、`render_mode_controls` @390、`render_permission_controls` @444）与思考 chip（`render_thinking_control` @755）**都在 `crates/vega_ui/src/conversation_stream/render.rs`**（1001 行）。两个切片同时改这一文件必冲突。

**约束 2：构建锁**
`scripts/cargo-lock.sh` 会让并行 agent 的构建排队（每次约 85 秒）。并行的收益在**思考与编辑**阶段，构建阶段会串行。这是有意的设计（避免产物串味，见 `AGENTS.md`）。

**约束 3：移除控件会造成功能回退**
`composer.rs:375` 是**唯一**发出 `ThreadSettingsRequested { permission_mode }` 的地方；`composer.rs:316` 同理用于 mode。直接删下拉 = 权限模式失去 UI 入口。**必须先有替代入口**。

---

## §2 依赖图

```
P1（权限/模式替代入口）
 └─> P2b（底部行重构）
P2a（思考滑块新组件）──┐
                       ├─> P3（接线 + 门禁 + 视觉校准）
P2b ───────────────────┘
```

- **P1 是 P2b 的前置**：没有替代入口就不能删下拉。
- **P2a 与 P2b 可真并行**：P2a 建**新文件**，不碰 `render.rs`。
- **P3 必须等 P2a + P2b 都完成**：它负责把滑块接进 composer 并跑全量门禁。

---

## P1 · 权限与模式的可达入口（前置）

**目标**：在移除底部行两个下拉**之前**，先给 `permission_mode` 与 `ThreadMode` 建好替代入口，避免功能回退。

**范围**：
1. `+` 菜单（`composer_actions.rs:346` `render_composer_actions`）增加**权限模式**分组：`只读` / `确认` / `自动`，选中项打勾。
2. `+` 菜单已有 `/ask` `/plan` `/execute` 三项承载 `ThreadMode`（`composer_actions.rs:5-9`）——**确认其可用**，无需新增。
3. 复用既有请求通路：`ThreadSettingsRequested { mode, permission_mode }`（`mod.rs:152`）。

**不做**：不改底部行渲染（那是 P2b）。

**验收**：
- 生产测试：从 `+` 菜单切换权限模式后，`permission_mode` 持久化生效。
- 既有 `composer.rs:375` 的发出路径**不删**（P2b 才决定它是否还被调用）。

**文件**：`composer_actions.rs`（+菜单）、可能 `core.rs`（状态）。

---

## P2a · 思考档位滑块组件（可与 P2b 并行）

**目标**：新建思考档位弹窗组件，**独立新文件**，不碰 `render.rs`。

**范围**：
1. 新建 `crates/vega_ui/src/conversation_stream/thinking_slider.rs`（或独立目录）。
2. 组件输入：模型支持的档位列表（`Vec<String>`，来自 `ReasoningProfile.efforts`）、当前档位、默认档位。
3. 渲染（按父规格 §4 实测值）：
   - 卡片：宽 254.5、圆角 ≈20
   - 顶部：闪电图标 / 档位名（颜色随强度）+ chevron / 重置图标
   - 中部：模型名（灰）
   - 底部：轨道 203×24、N 个圆点（N = 档位数）、knob 直径 24
   - 已填充点白色、未填充点灰色
   - 轨道样式：低档纯色、最高档渐变（父规格 §4.2 实测值）
4. 交互：拖动 knob、点击档位、重置到默认。
5. **档位数按输入动态渲染**，不是固定 7。

**不做**：不接入 composer（P3 做）、不改 `render.rs`。

**验收**：
- 单元/生产测试：给定 3 档 / 6 档 / 0 档模型，渲染的圆点数正确。
- 降级逻辑测试：当前档位不在支持列表时，降到「比它低且被支持的」最近一档（父规格 §3.1 参考实现语义）。
- 0 档时的降级形态：**父规格 §4.5 M3 未测**——实现者按「不渲染滑块」处理并在报告中标注为待校准。

**文件**：新文件 + `conversation_stream/mod.rs`（注册模块）。

---

## P2b · 底部行重构（依赖 P1）

**目标**：按父规格 §2.3 契约改造底部行。

**范围**（全部在 `render.rs`）：
1. 移除 `Execute` 下拉调用（`render.rs:162`）与 `render_compact_settings(false)`。
2. `确认` 从下拉改为**静态文本**（对齐参考实现 `⚠ Full access`：warning 色 + 图标，不可点）。
3. 移除思考 chip 调用（`render.rs:166`）与 `render_thinking_control`（@755）。
4. 最终行：`+` | 权限静态文本 | 留白 | 模型按钮 | 发送。
5. 不做语音按钮。
6. 清理随之失效的代码（`render_mode_controls` / `render_permission_controls` 若不再被调用）。

**不做**：不建滑块组件（P2a 做）、不接滑块（P3 做）。

**验收**：
- 生产测试：底部行元素数量与顺序符合上述第 4 条。
- 更新既有测试 `r21_composer_popovers_are_exclusive_and_model_labels_keep_menu_width`（`tests/core_flow.rs:78`）——它断言 `composer-mode` 与 `composer-model` 互斥；mode 菜单移除后须改写，**并在报告中说明理由**。

**文件**：`render.rs`。

---

## P3 · 接线 + 门禁 + 视觉校准（依赖 P2a + P2b）

**范围**：
1. 把 P2a 的滑块接入 composer（模型按钮旁或模型弹窗内，按父规格 §2.3 R3）。
2. 接已有数据：`ReasoningProfile.efforts` / `preference`（`vega_store/src/reasoning.rs:145-171`）与 `ComposerDefaults.thinking`。
3. 删除 P2b 遗留的死代码。
4. 全量门禁 + fmt + clippy。
5. **视觉校准**：按父规格 §4.5 补测项校准（若用户已提供截图）。

**验收**：父规格 §6 的 A1–A6 全项。

---

## §3 派发顺序与并行度

| 波次 | Agent | 说明 |
|---|---|---|
| 第 1 波 | **P1** + **P2a** | 真并行：P1 改 `composer_actions.rs`，P2a 建新文件 |
| 第 2 波 | **P2b** | 依赖 P1 完成 |
| 第 3 波 | **P3** | 依赖 P2a + P2b |

**每波内的 agent 共享同一 target**，构建会排队（约 85s/次）。派发时须告知每个 agent：**不要并行跑两个 cargo 命令**，统一走 `scripts/cargo-lock.sh`。

## §4 每个 agent 的交付要求

- 独立 worktree + `feat/` 分支（从 `master @ 41cd088` 起）
- 建完 worktree 后跑 `scripts/cargo-share-target.sh`
- 遵循 `AGENTS.md`（含合成输入事件无效的说明）
- 报告须含：改动文件、命令与结果、**未覆盖项**、与规格的偏离（如有）
- 不 push、不创建 MR
