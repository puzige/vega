# R60 · 移除主区顶栏的项目前缀（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 4a85fc2`
> 用户决策（2026-09-14）：**顶栏前缀去掉**；**自动总结标题先不做，留着**
> 前置：无（独立 UI 改动）

---

## §1 调研结论：先纠正一个前提

用户原话："不希望它前面加文件夹前缀"。

**调研推翻了"侧栏也有前缀"这个前提**（含活库实证）：

| 位置 | 实际渲染 | 结论 |
|---|---|---|
| **侧栏会话行** | **裸标题**（`threads_block.rs:1193`） | **无前缀，不需要改** |
| 主区顶栏 | `项目名` + `/` + `标题`，三个并列节点（`render.rs:635-667`） | **有前缀，本轮移除** |

**证据**：
- 全仓库只有**一处** `" / "`，即顶栏的分隔符节点（`render.rs:648-656`）；另一处 `" / "` 在设置页的 provider/model，无关。
- 活库 `~/Library/Application Support/ai.vega/vega.db` 的 38 条 thread，**标题中无一个含 `/`**。
- 侧栏里看起来像前缀的，实际是**父子两行**：项目行（`organization/render.rs:869-884`）+ 缩进的子会话行（`render_pi_row`）。二者是独立元素。

**因此本轮范围仅为主区顶栏。**

## §2 契约

### R1（必须）移除顶栏的项目 chip 与 `/` 分隔符

`crates/vega/src/window/render.rs` 的 `render_main_header`：

1. 删 `let project_label = self.shell_project_label(cx);`（`:602`）
2. 删 `let has_project_label = project_label.is_some();`（`:603`）
3. 删项目 chip 块 `.children(project_label.map(|label| {...}))`（`:635-647`）
4. 删 `/` 分隔符块 `.when(has_project_label, |labels| {...})`（`:648-656`）
5. **保留** `main-header-title` 节点（`:657-667`）不变

### R2（必须）保留 `shell_project_label` / `Sidebar::project_label`

**不删这两个函数**——它们仍被 composer utility bar 的文件夹 chip 使用：
- `vega_ui/src/sidebar/mod.rs:480-488`（`Sidebar::project_label`）
- `crates/vega/src/window/workspace.rs:702-705`（`shell_project_label`）
- `conversation_stream/utility_bar.rs:70-107`（文件夹 chip）

### R3（必须）顶栏布局不塌陷

删掉两个子节点后，标题节点应占满可用宽度（`min_w_0().flex_1().truncate()` 已具备）。
若外层 `.flex().items_center().gap_2()` 包装（`:631-634`）变为多余，可一并清理；**但不得改变顶栏高度**（`Layout::MAIN_HEADER_HEIGHT = 46.0`）。

### R4（必须）不得影响侧栏

侧栏行的渲染**不做任何改动**。若实现过程中发现侧栏确有前缀（与调研不符），**停下来报告**，不要自行删除。

## §3 既有测试影响（必须处理，不得直接删测试）

| 测试 | 位置 | 处置 |
|---|---|---|
| `r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries` | `crates/vega/src/window/workspace.rs:1895` | 从 `shell_bounds` 存在性循环中移除 `"main-header-project"` |
| 同上，standalone 路由断言 | `crates/vega/src/window/workspace.rs:2067` | `shell_absent` 循环中的 `"main-header-project"` 需移除或改写 |

**要求**：改写须说明理由，保留测试的原意（顶栏几何 + 路由 fence）。**不得**因为控件消失就删除整个测试。

## §4 明确不做

- **自动总结标题**（用户决策：先留着，不做）—— 见 §6 备查
- 不改侧栏行渲染
- 不改 composer utility bar 的文件夹 chip
- 不改顶栏高度或标题字号

## §5 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | 顶栏不再挂载 `main-header-project`，`main-header-title` 仍在 |
| A2 | 生产测试 | 顶栏高度仍为 `MAIN_HEADER_HEIGHT` |
| A3 | 生产测试 | utility bar 的文件夹 chip **仍存在**（证明 R2 未被误删） |
| A4 | 原生截图 | 顶栏只显示会话标题 |
| A5 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败 |

---

## §6 附录：自动总结标题（本轮不做，备查）

调研已完成，机制如下，供后续立项使用。

### Codex 的实现（源码实证）

**独立模型调用**，prompt 关键行：
```
"You are a helpful assistant. You will be presented with a user prompt, and your
 job is to provide a short title for a task that will be created from that prompt."
"Generate a concise UI title (up to 36 characters) for this task."
"Fill the structured title field with plain text."
"Fill the structured description field with a compact, search-oriented summary (up to 100 characters)."
"Do not include quotes, markdown, formatting characters, or trailing punctuation..."
"- Keep it under 36 characters and under 5 words where possible."
"- If the user's prompt is already a short clear title, reuse it verbatim."
```

调用：`threadMetadataGeneration.generateTitle({hostId, prompt, cwd, ...})`。**同时生成 `description`（≤100 字符，供搜索）**。

### 三个精确数字

| 项 | 值 | 出处 |
|---|---|---|
| 临时标题截断 | **60 字符** | `fC(首条消息, 60)` |
| prompt 目标长度 | **36 字符 / 5 词** | prompt 原文 |
| 模型输入上限 | **2000 字符** | `l.slice(0, 2000)` |

截断函数（超长时 `slice(0,59)` + `…`）：
```js
function fC(e, t) {
  let n = e.trim().replace(/\s+/g, ' ');
  return n.length === 0 ? null : n.length <= t ? n : `${n.slice(0, t-1).trimEnd()}…`;
}
```

### 三层兜底

| 层 | 时机 | 内容 | 持久化 |
|---|---|---|---|
| ① 临时标题 | 首轮开始**之前**同步写入 | 首条消息截 60 字符 | **否**（`persist:false`，仅 UI） |
| ② 生成标题 | 模型返回后 | 模型总结 | 是（`source:'generated'`） |
| ③ 最终兜底 | 模型失败/无返回 | 用 ① 的 60 字符**永久落盘** | 是 |
| 默认值 | 无消息时 | 字面量 `"New chat"` | — |

**触发点**：`onFirstTurnReady` —— 模型调用与首轮**并行**跑，不等回复完成。

**哨兵机制**：`"New chat"` 同时是"未命名"标记；`onlyIfProvisional` 守卫检查"标题为空或等于 `New chat`"才允许覆盖，**避免覆盖用户手改的标题**。

### Vega 现状

**完全没有自动命名**：
- `Thread.title` 创建时为空（`vega_conversation/src/types/thread.rs:89-90`）
- 仅两个写入点：手动重命名（`threads.rs:222-232`）、`update_thread`（`:339-359`）
- 无任何模型调用用于命名

**立项时需决策**：是否接受每次新建会话一次额外模型调用的成本。
