# Issue #57 · Recents 惰性加载（去除 Show More）— 规格（冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ da4cc48`
> 来源：[puzige/vega#57](https://github.com/puzige/vega/issues/57)（用户截图 + 一句需求）
> 取代：[vega-r29-sidebar-progressive-lists.md](vega-r29-sidebar-progressive-lists.md) 中 **Recents** 的 `Show More / Show Less` 交互；R29 的 **Projects** 部分（5 条 + Show More）保持不变。
> 涉及：`crates/vega_ui/src/sidebar/mod.rs`、`crates/vega_ui/src/sidebar/threads_block.rs`、`crates/vega_ui/src/sidebar/threads_block/organization.rs`、`.../organization/render.rs`、`.../organization/tests.rs`

---

## §1 需求与现状

### 用户诉求（Issue 原文）

> 列表不需要主动展示 Show More 按钮，但实际上能按照惰性加载去实现。如果一个列表能展示的完全部，那就直接展示，展示不完就使用惰性加载，往下下滑就进行持续加载。

### 现状（R29）

`render_recents_pi`（`organization/render.rs`）用 `recents_expanded: bool` + `take(visible_count)` 截断：未展开固定 10 行，超过 10 行时渲染一个 `Show More` 文本行，点击/回车展开为全部并变成 `Show Less`。

问题：Recents 是侧栏里最长的列表，却要求用户先点一次按钮才能继续看；用户要的是**滚动即加载**，且**装得下就不要截断**。

---

## §2 冻结契约

### R1 · 移除 Recents 的 Show More / Show Less

- Recents 不再渲染 `organization-recents-show-more` / `organization-recents-show-less`（两个 selector 恒不存在）。
- `Show More / Show Less` 只保留给 Projects（R29 行为不变，selector 不变）。
- Recents 的 `recents_expanded`、`recents_progressive_focus` 状态删除。

### R2 · 初始批次

- Recents 初始渲染 `RECENTS_PAGE = 10` 行（与 R29 的紧凑上限一致）。
- 总行数 ≤ 10 时不做任何截断。

### R3 · 视口填充（装得下就全展示）

- 若 Recents 内容底部**没有**超出 Sidebar 滚动视口（含完全无溢出的情况），且仍有未渲染的 Recents 行，则继续追加一页，直到「全部渲染」或「内容溢出视口」。
- 这条同时满足「如果一个列表能展示的完全部，那就直接展示」与「展示不完时用惰性加载」。

### R4 · 滚动到底部继续加载

- 当 Sidebar 滚动位置到达底部时，追加一页。
- 判定用 Sidebar 滚动句柄的当前值：`offset.y + max_offset.y <= 0`（`offset.y ∈ [-max_offset.y, 0]`，因此该式等价于「已到底部」；内容无溢出时 `max_offset.y == 0`、`offset.y == 0`，同样为真）。

### R5 · 稳定排序、只追加

- 排序沿用 `projections::sorted_threads`（pinned 优先 → 时间倒序 → `id` tie-break）。
- 加载只增加渲染行数，**不重排**已渲染行，不改变 `Pinned` / `Projects` 的顺序或内容。

### R6 · 内存态

- 可见行数只在内存中，不复用 R29 的 `Show More` 记忆；新的 `ThreadsBlock` 从初始批次开始（与 R29 的 non-persistence 一致）。

### R7 · 触发点必须在外层滚动容器上

- 判定与触发放在 `Sidebar` 的 `sidebar-scroll` 容器上（`track_scroll` + `on_children_prepainted`），而不是 `ThreadsBlock::render`：
  - 滚动时 `cx.notify` 只标记 `Sidebar` 及其祖先为 dirty，`ThreadsBlock` 的 `ViewElement` 子树被 prepaint 缓存复用，`ThreadsBlock::render` **不会**被调用；
  - `track_scroll` 的 `max_offset` / `bounds` 在容器自身 prepaint（`clamp_scroll_position`）中更新，**早于** children prepaint 与 `on_children_prepainted` 回调，因此回调读到的是**本帧**布局；`render` 阶段只能读到上一帧的值。
- 触发时用 `App::defer` 在当前 effect cycle 末尾增长并 `notify`：prepaint 期间 `notify` 会被 `draw_phase != None` 丢弃，`defer` 保证同一轮 `flush_effects` 内产生下一次 draw，从而收敛（无需 `on_next_frame` 的额外帧泵）。

---

## §3 实现要点

### 3.1 `Sidebar`（`sidebar/mod.rs`）

- 新增字段 `sidebar_scroll: gpui_kit::ScrollHandle`（`ScrollHandle::new()`）。
- `sidebar-scroll` div 追加 `.track_scroll(&self.sidebar_scroll)` 与 `.on_children_prepainted(...)`。
- 回调逻辑：

```rust
if scroll.offset().y + scroll.max_offset().y > px(0.0) {
    return; // 未到底部：不加载
}
let sessions = sessions.clone();
cx.defer(move |cx| {
    sessions.update(cx, |block, cx| {
        block.grow_recents(cx); // 变化时内部 notify
    });
});
```

### 3.2 `ThreadsBlock`（`sidebar/threads_block.rs`）

- `recents_expanded: bool` → `recents_visible: usize`（初始 `RECENTS_PAGE`），新增 `recents_total: usize`（初始 0，渲染时更新）。
- 新增：

```rust
/// Recents 惰性加载：追加一页。返回是否发生变化（变化时 notify）。
pub(crate) fn grow_recents(&mut self, cx: &mut Context<Self>) -> bool {
    if self.recents_visible >= self.recents_total {
        return false;
    }
    self.recents_visible = self
        .recents_visible
        .saturating_add(RECENTS_PAGE)
        .min(self.recents_total);
    cx.notify();
    true
}
```

### 3.3 `render.rs`

- `render_recents_pi` 改为 `&mut self`，记录 `self.recents_total = standalone.len()`，用 `take(self.recents_visible)` 渲染，删除 `has_hidden` / `render_progressive_control` 的 Recents 调用。
- `render_organization` 需要先结束对 `self.organization` 的不可变借用（把 `org.projects.read(cx).error.clone()` 提前取出）再调用 `&mut self` 的 `render_recents_pi`。
- 渐进控件收敛为 Projects 专用：`render_progressive_control` / `toggle_progressive_section` 的 Recents 分支删除，selector（`organization-projects-show-more|less`、`organization-projects-progressive-label`）保持不变。
- `RECENTS_PAGE` 放在 `organization.rs`（`pub(crate) const RECENTS_PAGE: usize = 10;`），供 `threads_block.rs` 与测试引用。

---

## §4 验收矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 层级 | 证据 |
|---|---|---|---|---|---|---|
| T57-1 | R1 无 Show More | 任意 | 渲染含 >10 条 Recents 的侧栏 | `organization-recents-show-more` / `-show-less` 恒不存在 | 挂载测试 | selector 断言 |
| T57-2 | R3 装得下就全展示 | 视口可容纳全部 | 12 条 Recents、高窗口 | 12 行全部渲染，无 Show More | 挂载测试 | 行 selector 计数 |
| T57-3 | R2/R4 滚动加载 | 视口不足 | 30 条 Recents、矮窗口，滚轮滚到底 | 初始 10 行；到底后 20 行；再到底 30 行 | 挂载测试 | 行 selector 计数 |
| T57-4 | R5 稳定排序 | 同 T57-3 | 逐页加载 | 追加行序与 `sorted_threads` 一致，首行为最新 | 挂载测试 | id 顺序断言 |
| T57-5 | 回归：Projects Show More | R29 | 9 个项目 | 仍为 5 条 + Show More/Less 可切换 | 挂载测试 | R29 既有用例 |
| T57-6 | 回归：Pinned 视口 | R29 | 有 pinned | Pinned 5 行视口与既有几何不变 | 挂载测试 | R35/R38 既有用例 |
| T57-7 | 真实 E2E | 安装包 | 真实 Vega：注入多条独立任务，滚动侧栏 | 滚动持续加载、无 Show More、排序稳定 | E2E | 截图 + 像素/几何 |

---

## §5 非目标

- 不改 Projects 的 5 条 + Show More（R29 保持）。
- 不改 Pinned 的 5 行视口、行高、字号、缩进阶梯（R48）、分组间距（R50）。
- 不做真正的数据分页 / keyset：`sidebar_organization` 快照仍是完整 metadata，本卡只做**渲染窗口**的惰性增长。
- 不改排序规则、schema、持久化、provider、runtime、composer。

---

## §6 变更记录

- 2026-09-22 初版冻结（Issue #57）。
