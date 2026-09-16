# R69 实机验收报告（首页常驻真实 Composer + 惰性草稿任务）

> 日期：2026-09-15
> 受测构建：`dist/Vega.app` → `/Applications/Vega.app`，`Contents/MacOS/vega` md5 `9caa3d4633338260006873ca4122e86e`
> 基线：`master @ 1c62455`
> 签名：`codesign --verify --deep --strict` 通过
> 驱动：`scripts/native-capture.swift`（本轮新增，**零输入**捕获）+ 临时输入驱动

---

## §1 结论

**用户诉求（"composer 默认不展示、要点击才展示，把这个特性移除"）已实现并实机验证通过。**

| # | 项 | 改动前 | 改动后 |
|---|---|---|---|
| 1 | 首页 composer | 静态占位卡片「新建任务并开始输入…」，**点击后**才创建任务并显示真实 composer | **无需任何点击**即为完整可用 composer |
| 2 | 持久化时机 | 点击占位卡片即 `INSERT INTO threads` | **首次提交**才落库；未提交离开不留行 |
| 3 | 侧栏副作用 | 每次点「新建任务」/⌘N 都留一行 `未命名任务` | ⌘N 不写行，不再堆积 |

---

## §2 关键证据：零输入捕获

本轮新增 `scripts/native-capture.swift`。既有 `native-drive.swift` **必须至少传一个点击坐标**，用它产出的"首页已有 composer"证据无法排除"是刚才那一下点击触发的"。R69 的命题恰恰是"不需要点击"，所以证据必须由**不发送任何输入**的驱动产生。

驱动在激活窗口后打印 `NO INPUT SENT: this capture is the pre-interaction state`，随后只做 `screencapture -R`。

**A15**（`/tmp/r69-A15-home-no-click.png`，2808×1720 @2x，窗口 1404×860）：启动应用后零输入捕获，画面已含完整 composer：

- R49 utility bar：`r13-alpha-project` 文件夹 chip + `main` 分支 chip；
- 输入区占位「描述任务，或用 @ 引用文件」；
- R57 P2b 底行：`+`、`确认`、`glm-5.3-flash` 模型选择器、发送按钮。

即用户第二张截图的状态，**不再需要第一张截图的那次点击**。

---

## §3 惰性（不落库）的实机证明

### A2：输入不写行

```
rows before typing: 62
typed: R69 惰性草稿验收
rows after typing (must equal 62): 62
```

`/tmp/r69-A16-typed.png`：文字已进入 composer（`R69 惰性草稿验收`），发送按钮由灰变强调色（`can_send` 为真），主头部仍为 `新建任务`，侧栏**未新增行**。

**输入文本 + 发送按钮可用，但数据库零写入** —— 这是"惰性草稿"最直接的证据。

### A3：提交物化恰好一行

```
rows before send: 62
sent cmd+enter
rows after send: 63
```

新增行的绑定正确：

```
id=01M2JVNCR7MC6CXRT1X79C56V7  project_id=01M1TPSXW6KCABZ6X6WY2SSG47  project=r13-alpha-project
```

`/tmp/r69-A17-sent.png`：主头部由 `新建任务` 变为 `未命名任务`，侧栏出现**恰好一行**且为选中态。行绑定到 `r13-alpha-project`，与 utility bar 显示的项目一致（R2）。

> 该截图底部的红色提示「本地凭据缺失或无法读取，请在设置中重新填写 API Key 后重试」是本机未配置 provider key 所致，与 R69 无关；因此该行 `messages` 为 0 条（运行在 provider 构造前即被拒）。

### A14/R14：⌘N 不再堆积空任务

```
rows before cmd-n x3: 63
sent cmd-n x3
rows after cmd-n x3 (must equal 63): 63
```

**连按三次 ⌘N，行数不变。** `/tmp/r69-A12-cmdn.png`：侧栏列表无新增，主头部为 `新建任务`，composer 为空白草稿态。

这正是用户截图里 `未命名任务` 堆积的来源（旧 `create_task` 无条件 INSERT，且全仓库无空任务清理）。R69 后该来源被切断。

### 无重复行

```
total=63  untitled=50
materialized row 01M2JVNCR7MC6CXRT1X79C56V7 still present, bound to r13-alpha-project
```

`untitled=50` 是**历史遗留**（旧急切创建行为在本次改动前已产生的行）；R69 之后不再新增。物化行未被后续 ⌘N 覆盖或复制（R9/R11）。

---

## §4 生产测试证据（A1–A13）

`crates/vega/src/tests/r69.rs`，16 个测试。核心三条：

| # | 测试 | 判据 |
|---|---|---|
| A2 | `r69_a2_typing_on_the_home_route_writes_no_row` | 首页输入后 `threads` 行数不变 |
| A3 | `r69_a3_first_submit_materializes_the_draft_under_its_own_id` | 提交后恰好一行，且 `id` == 提交前 `OpenedThread.0.id` |
| A4 | `r69_a4_submit_does_not_rebuild_the_cached_stream` | `stream_view` 缓存键与实体身份跨物化不变 |

补充：A3b（二次提交不写第二行）、A5（草稿路由无 controller error）、A6/A6b（模型/thinking/权限改动只更新内存）、A7（草稿不启动 branch/artifact 控制器）、A8（草稿路由渲染 utility bar）、A9（头部读 `新建任务`）、A10（草稿文本跨导航保留）、A11（无项目时仍可输入）、A12（⌘N 不写行）、A13（物化失败保留草稿）。

连续 6 轮 `-p vega --bin vega r69` 均 **16 passed / 0 failed**。

---

## §5 门禁

| 门禁 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过（无 error/warning） |
| `scripts/cargo-lock.sh test --workspace` | **全绿**：`vega` bin 129 passed / 0 failed（master 为 113），`vega_ui` 305，`vega_conversation` 303，其余 crate 全 0 failed |
| 红线 grep：非测试代码 `unwrap()`/`expect()` | 无（`draft_thread` 的 `unwrap_or` 是安全兜底，非 panic） |
| 红线 grep：硬编码色值 | 无 |

---

## §6 预存偶发失败（非本轮引入，已归因）

全量门禁在多次重复运行中偶发出现下列失败，**均在干净 master 上同样复现**，不属 R69：

| 测试 | master 表现 | 归因 |
|---|---|---|
| `tests::diff::diff_refresh_intents_keep_content_during_background_and_retry` | 3 轮中 1-2 次失败 | 仓库**已记录**的既有偶发（见 `docs/vega-r6-diff-refresh-delivery.md`、`vega-r8-zcode-parity.md`、`vega-r9-workspace-panels-delivery.md`、`vega-r11-acceptance.md`、`vega-review-current-status.md`）；孤立运行恒过 |
| `tests::palette::production_root_palette_escape_preserves_composer_and_settings_action` | **每轮必失败** | 旧语义断言（⌘N 后 `OpenedThread` 为 `None`）。R69 按 R14 更新了该断言，**本轮已修复**：分支上该测试通过 |
| `artifact::tests::preview_open::open_in_uses_six_exact_raw_argv_forms` | 偶发 | 1s 上界等待，属 R52 §2.1 同类的时序敏感测试；R69 未触碰 artifact 代码 |

> `r69_a4` 的偶发失败曾列在本节，**已在本轮查清并处置**（成因不是"与 R69 无关的时序敏感"，而是末尾一个无关的 provider 等待）。见 §8。

**关于共享 `target/` 的产物串味（本轮实测再次命中）**：`target/` 是指向 `/Users/puzige/Workspace/vega/target` 的符号链接。在 R69 worktree 跑测试期间并发跑 master worktree 的测试，会得到**陈旧测试二进制**（R69 分支上 `--bin vega` 一度只报 113 个测试，即 master 的计数，而 R69 实为 129）。`touch` 变更源文件强制重建后恢复 129/16。这与 `docs/vega-r52-test-gate-delivery.md` 的结论一致：**触发条件是共享 target 被另一 worktree 并发使用，不是机器负载**。

---

## §7 M4 裁决落地：`created_at` 取提交时刻（2026-09-16）

架构师裁决：物化行的 `created_at`/`updated_at` 取**首次提交时刻**，不是草稿构造时刻（规格 R8a）。理由：用户可在首页停留任意时长，侧栏 `SidebarTaskSort::Created` 必须反映任务真正开始的时刻。

### 实现

`materialize_draft` 在 INSERT 前重打 `now_ms()` 时间戳，id 与其余字段仍复用草稿（R2/R9）。**不**在提交路径写 `OpenedThread`（R8b）：`created_at` 的唯一读取者是侧栏排序（投影自持久行），而运行结束的 `reload_thread_state` 本就会重读权威行；提交中途写该全局会触发 `observe_global::<OpenedThread>`（`window/mod.rs:214`），把下一帧渲染切到非草稿路径并启动 branch/artifact 控制器——这是该路由此前没有的副作用。

### 实机证据：首页停留 20 秒后提交

驱动：`/tmp/vm4`（输入 → **停留 20s** → 提交 → 截图）。

```
typed; now idling 20.0s on the home route BEFORE submitting
idle done; submitting at 1789489683068
```

| 量 | 值 | 说明 |
|---|---|---|
| 提交时刻 | `1789489683068` | 驱动打印的提交前墙钟 |
| 落库 `created_at` | `1789489683093` | 提交后 **25ms** |
| 若取草稿构造时刻 | ≈`1789489663…` | 约 20 秒更早（首页渲染时刻） |

**`created_at` 与提交时刻相差 25ms，而非 20 秒** —— 证明取的是提交时刻。`updated_at=1789489683114` 为运行开始后的活动时间，晚于 `created_at`（`updated_at` 语义是 last-activity，不是创建时刻）。

截图 `/tmp/r69-M4-submit-instant.png`：主头部变为 `未命名任务`，侧栏新增**恰好一行**且为选中态，绑定 `r13-alpha-project`。底部红色提示仍为本机缺 API Key 所致（运行在 provider 构造前被拒），与 R69/M4 无关。

### 测试

- `materialize_draft_stamps_the_submit_instant_not_the_draft_instant`（`crates/vega_conversation/src/threads.rs`）：以「构造于 60 秒前」的过期草稿物化，断言 `created_at` 落在 `[before, after]` 且不等于草稿时间戳。
- `r69_a3_...`：断言落库行的 `created_at >= 草稿.created_at`（提交不早于构造），且 `updated_at >= created_at`。

> 曾把 `created_at == updated_at` 写成断言，在套件内稳定失败：物化后的运行会经 `open_thread` 合法地推进 `updated_at`。**`created_at == updated_at` 只在无后续活动时成立**，不是 M4 的不变量。

---

## §8 预存 flake 归因修正（2026-09-16）

上一版把 `r69_a4` 的偶发失败记为「与 R69 无关的时序敏感」。本轮用探针查清了实际成因，记录如下（避免后续重复误判）：

**探针输出（失败轮次）**：`A4-PROBE reqs=0 rows=1 draft_held=false`

即失败时：**物化已成功**（行已写入、草稿已释放），只是异步 agent run 尚未抵达 provider。因此 A4 的 R9 断言（缓存键、实体 id、`owns_stream_request`）**从未失败**；失败的是末尾那个"等运行到达 provider"的等待。

**处置**：删除该无关等待，A4 只保留 R9 的三条确定性断言。理由：provider 可达性属于既有 agent-run 机制（由 `crates/vega/src/tests/agent.rs` 与 A3 覆盖），不是 A4 的命题；在并行测试负载下它偶发不达，会污染一个本来确定的断言。

**修正前的测量**（同一 worktree，同一命令）：

| 条件 | A4 失败率 |
|---|---|
| 含末尾 provider 等待 | ~2/12（多次重复一致） |
| 去掉该等待后 | **0/15** |

> **方法论教训**：早先一次"去掉 M4 后 0/8 干净"的对照实验是**无效的**——`git stash push` 实际失败（输出 `Did you forget to 'git add'?`），那 8 轮仍带着 M4；且当时的 grep 只匹配 `FAILED` 行，**未验证测试真的跑起来**（该 worktree 曾因共享 `target/` 串味只报 113 个测试）。修正后的测量同时校验 `running N tests` 与结果计数。凡对照实验必须证明"被测代码真的执行了"。

---

## §9 与 master 合并后的验证（2026-09-16）

R69 实现完成后 master 前进了三个提交（R68 弹层：`9185ecb` 规格 / `ab2a11a` 实现 / `78faae6` 验收），合并提交 `9fc3d02`。

**R68 与 R69 的改动面直接重叠**：R68 改了 `conversation_stream/render.rs`、`utility_bar.rs`、`menu_list.rs`、`branch_selector.rs`，而 R69 改的正是 composer 的渲染路由与 `utility_bar_visible` 的调用时机。所以自动合并无冲突**不等于**语义正确，必须实测。

### 已确认无冲突的两点

| 检查 | 结论 |
|---|---|
| R68 是否改 `utility_bar_visible` 谓词 | **未改**（`git diff 1c62455..master -- utility_bar.rs` 中该函数无差异）。R69 R13 依赖此谓词不变，成立 |
| R68 的 `render_composer(window, cx)` 签名变更 | 已合并；R69 在 `render.rs` 的草稿路由分支不触碰该函数签名 |

### 发现并补上的测试缺口

**两边套件都没覆盖「R68 弹层 + R69 首页草稿路由」这个组合**：

- R68 的 10 个测试挂的是裸 `StreamHarness`（`conversation_stream/tests/r68_popup_dismiss.rs`），**绕过窗口路由**；
- R69 的 A8 只断言 utility bar 在草稿路由**被挂载**，不验证点击。

而 R69 恰恰让 composer（连同 R49 utility bar）在**任何任务存在之前**就渲染——这是用户开机即见的界面。

**新增测试** `r69_r68_project_popup_opens_and_dismisses_on_the_draft_route`（`crates/vega/src/tests/r69.rs`）：

1. 草稿路由上点项目 chip → 弹层打开（R68 A1 在草稿路由成立）；
2. 点外部 → 弹层关闭（R68 A2 成立），外部点带 `!popup.contains(&point)` 守卫，避免退化成空洞测试；
3. 弹层交互后 `thread_rows() == 0` 且 `draft.is_some()`——弹层是纯 UI，不得触发物化（R5）。

**非空洞验证**：临时给 `utility_bar.rs:253` 的 `on_mouse_down_out` 加提前 `return` 后，该测试**立即失败**；恢复后通过。证明它真的在测 R68 的关闭机制，而不是恒真。

### 合并后门禁

| 门禁 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `scripts/cargo-lock.sh test --workspace` | 3 轮中 2 轮全绿；1 轮出现 `tests::diff::diff_refresh_intents_keep_content_during_background_and_retry`（**仓库已记录的既有偶发**，见 §6，与 R69/R68 均无关） |
| `-p vega --bin vega r69` | **17 passed / 0 failed**（16 个 R69 + 1 个 R69×R68 集成） |
| `-p vega_ui r68` | **10 passed / 0 failed** |
| 实机（合并后重新打包 `9caa3d46…` 之后的构建） | 首页零输入捕获仍为完整 composer；R68 弹层在草稿路由可开可关 |

---

## §10 未决项

无。M1（导航草稿保留）、M3（file_index 在草稿路由）、M4（时间戳口径）均已落地或被测试覆盖；R68 合并组合已补测试覆盖。
