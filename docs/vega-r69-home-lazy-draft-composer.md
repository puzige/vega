# R69 · 首页常驻真实 Composer + 惰性草稿任务（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 1c62455`
> 来源：用户 2026-09-15 实机截图 + 口头诉求「把这个特性给它移除掉」
> 取代：R8（`docs/vega-r8-zcode-parity.md:36` 的「点击才创建」折中）+ R19 §「空路由在任务创建之后才用真实 composer」（`docs/vega-r19-codex-parity.md:92-94`）

---

## §1 用户诉求与现状

用户原话：*「它下面的 composer 默认不展示，要点击才展示。我们能不能把这个特性给它移除掉？」*

截图证据（`image-8452522154f759e3a5f16af253fa6c59.png` / `image-04b8fe3dfd159fa35efcfaf2d9941e4d.png`）：首页只有一张静态占位卡片「新建任务并开始输入…」；点击后才出现真实 composer（含 R49 utility bar 与 `+ / 确认 / 模型 / 发送` 底行）。

### 现状机制

路由在 `OpenedThread` 上做硬分支（`crates/vega/src/window/render.rs:134`）：

| 状态 | 渲染 | 代码 |
|---|---|---|
| `OpenedThread == None` | 静态占位卡片（**不可输入**） | `render_empty_state`（`render.rs:679`），文案在 `render.rs:714` |
| `OpenedThread == Some(_)` | 真实 `ConversationStream` | `render.rs:135-352` |

点击占位卡片 → `empty_new_thread_clicked`（`render.rs:648`）→ `open_new_thread`（`crates/vega/src/window/session.rs:177`）→ `Sidebar::create_thread`（`crates/vega_ui/src/sidebar/mod.rs:464`）→ `create_task`（`crates/vega_ui/src/sidebar/threads_block.rs:197`）→ **立即 `INSERT INTO threads`**（`crates/vega_store/src/threads.rs:94`）。只有这一步之后 `OpenedThread` 才变成 `Some`。

**所以「点击才展示」的本质是「点击才建数据库行」。** 侧栏堆积的 `未命名任务`（用户截图可见）正是这个急切创建的副产物：`create_task` 无条件写行，且全仓库没有任何空任务清理逻辑（唯一 `delete_thread` 调用点是侧栏的显式删除确认，`crates/vega_ui/src/sidebar/mod.rs:438`）。

### 目标

首页直接渲染**真实可用的 composer**；持久化推迟到**首次提交**。未提交就离开，不留下任何数据库行。

---

## §2 参考实现（Codex 桌面版）的做法

Codex 桌面版是闭源 Electron 应用，但本地有可查证据：webview bundle（`/Users/puzige/Workspace/vega-design-reference/app/webview/assets/`）与其本地会话库（`~/.codex/sqlite/codex-dev.db`）。

### 证据

1. **客户端先生成线程 id。** bundle 里有 `client-new-thread:` 前缀常量与 `n_(e) => e.startsWith(iwt)` 判定（`app-initial-cadb12d4a15e.js`），路由态 `routeKind === 'home'` 直接携带 `clientThreadId`；服务端会话创建后再做别名映射（`p$n` / `c$n` / `g$n`）。
2. **草稿不是持久行。** `local_thread_catalog` 表（`codex-dev.db`）里 `thread_id LIKE 'client-new-thread:%'` 的行数为 **0**，且 `display_title` 为空的记录数为 **0**。452 个 rollout 会话文件中，最小的是一个只有 `session_meta` + 1 条环境上下文 user message 的 1037 字节文件——**没有任何「零用户消息」的会话**。即：Codex 不会为空任务落库。
3. **草稿提交时才创建。** `launchMode: 'start-conversation'` 在用户发送时才发出 `thread/start`（`F0t(e)` 判定集合：`thread/start` / `thread/fork` / `turn/start` / `turn/steer`）。
4. **草稿有独立的内容判定。** bundle 有 `hasDraftContent`、`isNewDraft: r == null`、`preserveDraft`、`restoreComposerDraft`、`draftThreadLocationId`，用于草稿期不写库、提交失败保留草稿、导航返回恢复。
5. **home 与会话复用同一个 composer 组件。** CSS 里 `_ComposerLayoutRoot_kbwao_2[data-composer-utility-bar-variant=home]` 与 `[data-composer-placement=home]` 只是同一组件的变体属性，不是两个不同控件。

> `data-composer-peeked` **不是**本特性开关。它只控制 overlay 在 `exiting` / `hidden` 过渡期的 `opacity`（`app-primary-7fe7c6486695.css` 中 `._Overlay_cm04e_1[data-transition-state=hidden][data-composer-peeked=false]{opacity:0}`），与「点击才出现 composer」无关。

### 结论

Codex 的模型是：**客户端 id 先行 → 草稿只存在内存 → 首次提交才 `thread/start` 落库 → 落库后做 id 别名映射**。

**Vega 比 Codex 更省一步**：`vega_conversation::threads::new_thread_id()` 本来就是客户端生成的 ulid（`crates/vega_conversation/src/threads.rs:26-29`），`create_thread` 也是先造好 `Thread` 再 INSERT（`threads.rs:137-185`）。因此 Vega **不需要** Codex 的前缀 + 别名映射层——草稿直接携带最终 id，落库时复用同一个 id，身份全程稳定。

---

## §3 契约

### 草稿的构造与身份

**R1（必须）** 首页路由（`OpenedThread == None` 且设置关闭）渲染真实 `ConversationStream`，其 `Thread` 是**未持久化的草稿**：`OpenedThread` 置为 `Some(draft)`。

**R2（必须）** 草稿 `Thread` 的字段取值：

| 字段 | 值 | 依据 |
|---|---|---|
| `id` | `new_thread_id()`（客户端 ulid） | 与持久行同格式，落库时复用，**不得**引入新前缀或别名层 |
| `project_id` | 当前 `SelectedProject`；无选中则为空串（standalone） | `create_task` 同源（`threads_block.rs:216-221`） |
| `title` | 空串 | `create_thread_with_binding`（`threads.rs:156`） |
| `mode` | `ThreadMode::Execute` | DDL 默认 |
| `permission_mode` | `config.defaults.permission_mode` | `threads_block.rs:207` |
| `model` | `config.defaults.model` | `threads_block.rs:207` |
| `status` / `pinned` / `unread` | `Active` / `false` / `false` | DDL 默认 |
| `created_at` / `updated_at` | 当前毫秒 | `now_ms()` |

**R3（必须）** 草稿的默认值来源必须与 `create_task` **同源**（`config::load()` 的 `defaults.model` / `defaults.permission_mode`）。**不得**在 render 里读 `config.toml`（沿用 R1/A2-14 既有约束：render 只投影已加载的目录，配置 IO 走 worker）。

**R4（必须）** 草稿 id 在同一窗口内**稳定复用**：窗口持有至多一个草稿 id，重复进入首页路由复用同一个 id，直到该草稿被物化或被清空。理由：`retain_departing_draft` / `restore_navigation_draft` 按 thread id 索引草稿文本（`crates/vega/src/window/navigation.rs:278-311`、`:341-348`）。每次进入都换 id 会让导航返回时草稿文本丢失。

### 不写库

**R5（必须）** 草稿存在期间**零数据库写入**。具体地，以下既有持久化路径在草稿上必须**只更新内存**（含 `OpenedThread` 投影），不得调用 conversation 层的写函数：

| 路径 | 现状 | 草稿行为 |
|---|---|---|
| 模型选择 | `apply_thread_model_selection`（`crates/vega/src/window/session.rs:192`）写 `threads.model` | 内存更新，物化时随 INSERT 落库 |
| thinking 偏好 | `persist_composer_thinking` | 同上 |
| 线程设置（mode / permission） | `persist_thread_settings` | 同上 |
| 打开线程触碰时间 | `open_thread` / `visit_thread` | 不调用 |

**R6（必须）** 草稿仍不得启动依赖线程持久行的 artifact 控制器：`ensure_artifact_route`（`crates/vega/src/window/artifact.rs:109`）在草稿上不 begin。项目绑定的草稿**允许**启动既有 `ensure_branch_route`（`crates/vega/src/window/branch.rs:52`）：它通过 `artifact_project_root` 读取已存在的项目行来获得真实 Git 根目录，不需要草稿线程行；standalone 草稿仍由 branch route 的 `InvalidRoot` 保护拒绝。分支列出/切换继续使用既有 route、generation、busy、dirty 与授权 guards，不物化草稿。

**R7（必须）** 草稿路由**跳过**持久历史水合块（`render.rs:259-322`）。草稿按定义无历史；该块中 `recoverable_approved_instruction` 会经 `plans.rs:55` → `threads.rs:365-368` 返回 `ConversationError::NotFound`，被 `render.rs:277` 的 `?` 变成可见的 controller error。草稿直接应用空状态，**不得**出现任何错误条。

### 提交时物化

**R8（必须）** 首次提交（`ComposerSubmitted` → `submit_composer`，`crates/vega/src/window/agent.rs:540`）时，先以**草稿自身的 id** INSERT 持久行（复用 R2 各字段），再走既有 submit 路径。

**R8a（必须，2026-09-16 架构师裁决）** 物化行的 `created_at` / `updated_at` 取**首次提交时刻**，不是草稿构造时刻。理由：用户可在首页停留任意时长，侧栏 `SidebarTaskSort::Created`（`crates/vega_ui/src/sidebar/threads_block/organization/projections.rs:648`）必须反映任务真正开始的时刻。

**R8b（必须）** 物化**不得**在提交路径中改写 `OpenedThread`。`created_at` 的唯一读取者是侧栏的 "Created" 排序（投影自持久行），而运行结束时的 `reload_thread_state` 本就会重读权威行；在提交中途写该全局会触发 `observe_global::<OpenedThread>`（`crates/vega/src/window/mod.rs:214`），把下一帧渲染切到非草稿路径（进而启动 branch/artifact 控制器），这是该路由此前没有的副作用。

**R9（必须）** 物化**不得**改变路由身份：`OpenedThread.0.id` 与 `stream_view` 的缓存键都不变。因此 `owns_stream_request`（`agent.rs:27-38`）继续成立，已缓存的 `ConversationStream` 实体（连同 composer 文本与焦点）**不得**被重建。

**R10（必须）** 物化失败（store 不可用 / 写失败）必须走既有失败语义：保留草稿与草稿文本、显示错误、**不**留下半写状态。不得静默吞掉。

**R11（必须）** 物化只能发生一次。重复提交 / 竞态不得产生第二行。

### 路由与外壳

**R12（必须）** 主头部标题：草稿路由显示 `新建任务`（与 `OpenedThread == None` 的现行为一致，`render.rs:591-597`），**不得**显示 `未命名任务`。

**R13（必须）** R49 utility bar 在草稿路由**照常渲染**：草稿绑定当前项目且 `entries.is_empty()` 时 `utility_bar_visible` 为真（`crates/vega_ui/src/conversation_stream/utility_bar.rs:23-33`）。这是用户截图里首页该有的样子。**不得**改 `utility_bar_visible` 的谓词（`docs/vega-ui-backlog.md:115` 明确要求）。

**R14（必须）** 侧栏「新建任务」（⌘N）与命令面板的新建入口改为**导航到首页草稿路由**，不再急切建行。理由：这正是用户截图里 `未命名任务` 堆积的来源；与 Codex 的 New chat 行为一致。`Sidebar::create_thread`（`sidebar/mod.rs:464`）的急切 INSERT 语义随之取消。

**R15（必须）** 无项目选中时的首页：仍渲染真实 composer（可输入，物化为 standalone 任务），同时保留现有引导文案（`先添加一个项目` / `添加一个文件夹后，就可以创建任务并开始工作。`）与 `显示侧栏` 入口（`render.rs:687-695`、`:737-752`）。

### 不得改变

**R16（不得）** 不得改 `Layout` / `Typography` 的任何 token：composer 宽度 736、圆角 20、最小高 100、包裹列 12/16 padding、utility bar 的 inset/高度/圆角，以及 R49/R57/R61/R62/R64/R66/R67 的全部几何与配色结论。

**R17（不得）** 不得改 R57 P2b 冻结的底行结构：`+` | 权限状态（静态文本）| spacer | 模型 | 发送/停止。

**R18（不得）** 不得改 `OpenedThread` 的类型（仍是 `Option<Thread>`）。草稿/持久的区分**不得**塞进 `Thread`（`crates/vega_conversation/src/types/thread.rs:83-107` 明确「field-by-field with the `threads` DDL」）。区分由窗口持有。

**R19（不得）** 不得引入新依赖。

**R20（不得）** 不得把 `unwrap()` / `expect()` 写进非测试代码（exec-guide §3 红线）。

---

## §4 改动点（供实现者定位，非穷举）

| 文件 | 改动 |
|---|---|
| `crates/vega/src/window/render.rs` | 首页分支由 `render_empty_state` 改为构造/复用草稿并渲染 `ConversationStream`；草稿跳过水合块；`render_empty_state` 的占位卡片与 `empty_new_thread_clicked` 移除（引导文案与 `显示侧栏` 保留，R15） |
| `crates/vega/src/window/session.rs` | `open_new_thread` 改为进入草稿路由；新增草稿构造 / 复用 / 物化辅助 |
| `crates/vega/src/window/agent.rs` | `submit_composer` 前置物化（R8-R11） |
| `crates/vega_ui/src/sidebar/threads_block.rs` | `create_task` 的急切 INSERT 取消（R14）；默认值解析保留给草稿复用 |
| `crates/vega_ui/src/sidebar/mod.rs` | `create_thread` 语义调整 |
| `crates/vega_conversation/src/threads.rs` | 新增「以给定 id 物化草稿」的入口（复用 `create_thread_with_binding` 的构造与 INSERT 逻辑，id 由调用方给出） |
| `crates/vega/src/app_palette.rs` | 新建入口对齐 R14 |

---

## §5 验收

验收遵循 exec-guide §7 的 E2E-first：优先真实 production 入口。测试包装器：`./scripts/cargo-lock.sh test -p <crate> <filter> -- --nocapture`。

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | 首页路由（`OpenedThread == None`）渲染出 `composer-shell`（`render.rs:144` 的 debug selector），且可输入文本 |
| A2 | 生产测试 | 首页输入文本后，store 的 `threads` 行数**不变**（R5） |
| A3 | 生产测试 | 首页提交后，store 出现**恰好一行**，其 `id` 等于提交前 `OpenedThread.0.id`（R8/R9：id 稳定）；且该行 `created_at` 不早于草稿构造时刻（R8a：提交时刻口径） |
| A4 | 生产测试 | 提交前后 `stream_view` 的缓存键与实体身份不变（R9：不重建 stream） |
| A5 | 生产测试 | 草稿路由上 `controller_error` 为 `None`（R7：无 NotFound 错误条） |
| A6 | 生产测试 | 草稿路由上改模型 / thinking / 权限：内存与 `OpenedThread` 更新，store 行数不变（R5） |
| A7 | 生产测试 | 项目绑定草稿上 branch controller begin 并通过真实项目根列出/切换；不 begin artifact controller；standalone 草稿不 begin branch（R6） |
| A8 | 生产测试 | 草稿路由 + 已选项目 → `utility_bar_visible == true`（R13） |
| A9 | 生产测试 | 主头部标题为 `新建任务`（R12） |
| A10 | 生产测试 | 首页输入 → 导航到别的任务 → 返回首页，草稿文本仍在（R4） |
| A11 | 生产测试 | 无项目选中时首页仍有 `composer-shell` 且可输入（R15） |
| A12 | 生产测试 | ⌘N / 命令面板新建后，store 行数**不变**且进入草稿路由（R14） |
| A13 | 生产测试 | 物化失败注入 → 草稿文本保留、显示错误、无半写行（R10） |
| A14 | 回归 | R49/R57/R61/R62/R64/R66/R67 既有测试全绿；`crates/vega/src/window/workspace.rs` 的 `r49_utility_bar_mounts_above_the_card_only_on_the_new_task_page` 与 `navigation/tests.rs` 的 `navigation_settings_returns_to_empty_without_database` 按新语义更新（注明是 R69 的有意变更） |
| A15 | 实机像素 | 启动应用进入首页：**无需任何点击**即见完整 composer（utility bar + 底行 + 可输入） |
| A16 | 实机像素 | 首页输入文字后切走再切回，文字仍在 |
| A17 | 实机像素 | 首页输入并发送：侧栏**只出现一行**任务，标题正常 |
| A18 | 门禁 | `cargo fmt --all -- --check`、`cargo clippy --all-targets -- -D warnings`、`scripts/cargo-lock.sh test --workspace` 全绿 |

**A2 / A3 / A4 是本轮核心证据**：它们把「惰性」从「看行为差不多」变成可证伪的断言——A2 证明未提交不写库，A3 证明物化复用草稿 id，A4 证明提交不重建 stream。

---

## §6 待实测与已知风险

| # | 项 | 状态 |
|---|---|---|
| M1 | 首页路由渲染 stream 是否会与 `sync_navigation` 的草稿保留逻辑打架 | **未测**。R4 的稳定 id 是设计上的对策，实现后必须由 A10 证伪 |
| M2 | 草稿上的模型选择是否需要额外 UI 反馈 | **未测**。默认沿用现行为（选择器即时显示），物化时才落库 |
| M3 | `file_index` 在草稿路由是否可用 | **未测**。它按 project 目录工作（`crates/vega/src/window/file_index.rs:69-77` 只比较 `project_id`），预期可用；若实测报错则按 R6 同法跳过并上报 |
| M4 | 物化后 `threads.created_at` 应取「首次提交时刻」还是「进入首页时刻」 | **已裁决（2026-09-16）**：取**首次提交时刻**，见 R8a/R8b。实现为在 `materialize_draft` 内重打时间戳并复用草稿 id，且不写 `OpenedThread`。 |

---

## §7 变更记录

- 2026-09-15：冻结。取代 R8 的「点击才创建」折中与 R19 的「创建之后才有真实 composer」。用户诉求：首页 composer 常驻。
