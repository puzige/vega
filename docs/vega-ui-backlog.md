# ✦ Vega — UI Backlog（待办看板）

**用途**：记录已确认、但**尚未开工**的 UI 缺陷与差距。每条含代码证据与参考实现对照，开工时无需重新调研。

**维护规则**：
- 只记**已核实**的项（有代码位置或截图证据）。未核实的猜测不进来。
- 开工后移到对应 sprint 任务卡或交付文档，并从本表删除。
- 「参考实现」指 `/Users/puzige/Workspace/vega-design-reference/`（源码）与用户提供的实机截图。**截图取色只证明视觉结果**，不用于推断内部 token。

**最近核对**：2026-09-14，`master @ 965db90`

---

## 卡片总览

| # | 项 | 严重度 | 证据等级 | 状态 |
|---|---|---|---|---|
| UI-01 | Environment 面板内容不完整（含分支入口缺失） | P0 | 代码 + 实机截图 | 待开工 |
| UI-02 | 会话中分支入口不可达 | P0 | 代码 | 待开工（含在 UI-01） |
| UI-03 | Composer 缺 `Local` 运行位置 chip | P2 | 代码 + 实机截图 | 待立项 |
| UI-04 | 右侧面板缺 Browser / 文件树 / Side chat | P2 | 代码 | 待立项 |

---

## UI-01 · Environment 面板内容不完整（P0）

### 现象

参考实现的 Environment 面板（会话右上角按钮展开）有 **6 组内容**；Vega 只有 **3 行**。

| 参考实现行 | Vega 现状 | 说明 |
|---|---|---|
| `Changes` `+90 -13` | ❌ 无 | 变更统计（文件数 + 增删行） |
| `Local` ▾ | ❌ 无 | 运行位置选择；**用户明确表示只做 local，Cloud 不做** |
| **`master` ▾** | ❌ **R49 时被删除** | **分支入口**——见 UI-02 |
| `Commit or push` | ❌ 无 | git 提交/推送 |
| `Compare branch` | ❌ 无 | 对比分支（外链） |
| `Subagents` `23 done` | ❌ 无 | 子 agent 状态分组 |
| `Sources` + 文件列表 + `View all` | ❌ 无 | 来源文件分组 |

Vega 当前行（`crates/vega/src/window/workspace.rs`）：`environment-project`、`environment-review`、`environment-terminal`。

### 分支行展开后的形态（实机截图实证）

```
🔍 Search loom branches
Branches
  ✓ master
    docs/xhs-auto-fulfillment-record
    docs/warprouter-prd
+ Create and checkout new branch...
```

即：**搜索框 + 分支列表（当前分支打勾）+ 新建分支入口**。Vega 已有 `BranchSelector` 组件（`crates/vega_ui/src/branch_selector.rs`，944 行）可支撑，含 `set_chip_chrome` 与 `set_menu_below`。

### `Local` 行展开后的形态（实机截图实证）

```
Continue in
  Local                          ✓
  New local worktree · loom
  Work locally in 1 other folder
  Connect Codex web
  Cloud
  Usage remaining
```

**用户决定：只做 Local，Cloud 相关不做。** 因此该项不作为 UI-01 的必要范围。

### 范围决定（2026-09-14）

**用户要求做到参考实现那么完整。** 但**优先级排在 UI 修改之后**——当前先专注纯 UI 调整。

### 参考实现来源

- 面板容器与行：实机截图（2026-09-14 用户提供 3 张）
- 分支选择器源码：`app/webview/assets/git-branch-picker-dropdown-content-49b5181fe45d.js`
- 环境选择源码：`app/webview/assets/worktree-environment-dropdown-e84648d210c7.js`
- 运行位置源码：`app/webview/assets/local-remote-dropdown-4db057b7481d.js`

### 约束

- 恢复 `environment-branch` 会与 R49 的 `r49_environment_card_drops_the_branch_row` 断言冲突（`workspace.rs:3166`）——须说明该 R49 决策是否被推翻。
- 不得改变 `BranchSelector` 自身的语义（open/close/switch/pending/error/focus/scroll）。

---

## UI-02 · 会话中分支入口不可达（P0）

### 现象

会话一旦有消息，用户**没有任何可达路径**切换分支。

### 两条路径同时失效（代码核对）

1. **Composer utility bar 整体消失**
   `crates/vega_ui/src/conversation_stream/utility_bar.rs:21-24`：
   ```rust
   if !self.entries.is_empty() { return false; }
   ```

2. **Environment 卡片的分支行已被删除**
   `crates/vega/src/window/workspace.rs:1907`、`:2106`、`:3176` 只剩 `assert!(shell_absent(window, "environment-branch", cx))`。

### 重要更正：utility bar 的行为**与参考实现一致**

2026-09-14 经实机核对（用户提供截图），参考实现的 utility bar **也只在新建会话出现**，已有会话中不显示：

| | 新建会话 | 已有会话 |
|---|---|---|
| utility bar | ✅ 三个 chip（项目 / Local / 分支） | ❌ 消失 |
| Environment 面板 | 存在 | ✅ **分支入口在这里** |

**因此不要改 `utility_bar_visible`**——它现在的行为是对的。

> **更正记录**：此前曾误判「参考实现的 utility bar 在会话中常驻」，并据此建议改可见性谓词。根因是只读压缩 JS，把 `showUtilityBarBranchWhen: 'always'`（只控制分支项时机）误当成整条栏的开关；真正控制整条栏的是另一个 prop `showUtilityBar`。**教训：UI 行为必须看实机，不能只读压缩代码。**

### 正确解法

在 Environment 面板恢复分支入口（即 UI-01 的分支行）。两处独立入口的分工见上表。

---

## UI-03 · Composer 缺 `Local` 运行位置 chip（P2）

参考实现的 utility bar 是**三个** chip：`loom`（项目）/ `Local`（位置）/ `master`（分支）。
Vega 是**两个**（项目 / 分支），缺中间的运行位置选择器。

**用户决定：只做 local，不管 Cloud。** 因此若实现，下拉只需 `Local` 一项，或将其置灰。

**待立项**——涉及运行位置这一新功能域，不是纯 UI。

---

## UI-04 · 右侧面板缺 Browser / 文件树 / Side chat（P2）

`WorkspaceCreateAction` 仅 2 个变体（Review、NewTerminal）。参考实现的 `+` 菜单有 5 项：Review / Terminal / Browser / Files / Side chat。

**注意**：R51 的批准范围明确不含占位入口（`docs/vega-r51-tab-controls.md`：*"No new placeholder Browser, Files or Side chat entries."*）。新增菜单占位**不算功能完成**。

**待立项**，需先定范围与验收边界。
