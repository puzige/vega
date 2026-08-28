# ✦ Vega — 技术难点攻坚（Tech Risks Deep-dive）

**版本** v0.1 · 2026-08-28 · 关联：[vega-tech-spec-p1.md](vega-tech-spec-p1.md)

> 逐个攻破 Phase 1 的技术难点。每篇：问题分解 → 生态调研（可验证来源）→ 方案 → 验证计划。
> 目录：**#1 流式 markdown 渲染** · **#2 GPUI 虚拟化滚动与锚定** · **#3 Agent 中断与断点续跑** · **#4 macOS 权限沙箱（Seatbelt）** · **#5 SSE 背压与事件管线**

---

## #1 流式 Markdown 渲染

### 1.1 问题分解（为什么它难）

LLM 输出是任意切分的 token 流，markdown 结构跨 chunk 是常态：`**加粗` 可能分 3 次到达，``` 代码块可能 500 个 chunk 才闭合。朴素方案有两条死路：

| 朴素方案 | 死法 |
|---|---|
| 每个 chunk 全量 reparse + 全量重渲染 | O(n²)：会话越长每 token 越贵，万行会话必掉帧；且已渲染区重排 = 视觉跳动 |
| 直接把 delta 当纯文本追加，结束再渲染 | 流式期间无格式，体验退回终端时代 |

真正的难点在三个子问题：
1. **未闭合语法怎么显示**（`**bol` 到达时，显示原文还是补全成粗体？）
2. **已确定的内容如何冻结**（不再重解析、不重排）
3. **渲染增量如何映射到 UI**（只更新变化的那个 block）

### 1.2 Web 生态怎么做（vue-stream-markdown / Vercel streamdown 拆解）

调研对象：https://github.com/jinghaihan/vue-stream-markdown（README + 提交记录）

**关键发现：它的核心不是「增量解析器」，而是独立的语法补全层 `markmend`**（@markmend/core + @markmend/ast，灵感来自 Vercel streamdown 和 remend）：

1. **Mend（补全）**：chunk 到达后，先把**尾部未闭合的语法推断并补全**再解析——`**bol` → `**bol**`（临时）、未闭合表格补齐分隔行、未闭合链接补 `](…)`。提交记录里全是这类边界 case（"preserve literal single tildes"、"complete partial triple-backtick code spans"、"incomplete table alignment"）——**这是该库 80% 的复杂度所在**。
2. **全量 reparse**：补全后对全文重新解析（JS 里 mdast 足够快，O(n) 可接受）。
3. **AST 级 diff + loading 态**：图片/表格/代码块在未完整时显示 loading 占位防跳动；代码高亮用 Shiki token 级增量（唯一真增量的地方）。

**启示**：Web 方案成立的前提是「mdast 快 + DOM diff 便宜」。GPUI 没有 DOM diff，全量重渲染 = 每帧重建元素树，O(n²) 死路照旧——所以 **Rust 侧必须做真增量**，但「mend 尾部语法」的手法可以直接借鉴。

### 1.2b markstream-vue（Simon-He95）补充调研——更成熟的师姐

vue-stream-markdown 的动画与 AST 渲染即源自此库。它比前者多了几个**可直接抄进 Vega 的设计**：

| 设计 | 内容 | 抄到 Vega 哪里 |
|---|---|---|
| **`final` 语义协议** | 流式期间未闭合构造进显式 `loading` 中间态；流结束置 `final=true` → **作废流式缓存、以最终语义完整重解析一次**，避免「永久 loading」/未闭合 token 残留 | tech-spec §5：`MessageFinished` 事件触发 pending block 终结重解析（我们消息状态机已有 streaming/done，天然契合） |
| **虚拟窗口默认值** | `max-live-nodes: 220` 滑动窗口，长文档稳态内存、**不用骨架屏** | UI spec：虚拟化窗口参考值 200 量级 |
| **帧预算批渲染** | `renderBatchBudgetMs`：每帧 CPU 预算内分批挂载新节点；突发 chunk 用 `smooth-streaming` 匀速化（防抖） | tech-spec §5.3：delta 合并（mdstream-tokio 的 CoalesceLocal 同理）+ 每帧渲染预算 |
| **fade 陷阱** | 官方明确警告：高频流式 + 节点入场 opacity 动画 = 「反复 opacity 重启」；chat 模式必须 `fade=false` | UI spec P6 补充：流式节点**禁止入场动画**（已渲染区零重排的推论） |
| **代码块双态** | 未闭合 fence 渲染为纯 `<pre>`，闭合后才切换为高亮/diff 组件 | 与我们 §5 设计一致（再次被验证） |
| **可序列化 AST** | 节点树可 JSON 化 → 支持 worker/服务端预解析 | 对应我们「解析放后台线程、channel 发回 UI」 |

另注：它的增量解析（`md.stream.parse` 带 token 缓存）证明 JS 生态也在走向真增量——流式渲染的业界共识已收敛为「**增量解析 + 冻结块 + final 终结**」三件套，正是我们的方案。深度资料：`markstream.simonhe.me/llms.txt`。

### 1.3 Rust 生态位调研结论（2026-08-28）

| 组件 | 生态位 | 结论 |
|---|---|---|
| **`mdstream` v0.2.0** | 流式 markdown 中间件：**committed + pending 双态模型**、稳定 BlockId 缓存键、Update/UpdateRef 零拷贝视图、**remend 式尾部补全（terminator）**、render-agnostic、官方文档明确面向 gpui/egui/TUI、自带 tokio coalesce glue | ✅ **首选**，与 tech-spec §5 原设计架构一致，先引入后评估 |
| `pulldown-cmark` v0.13 | CommonMark pull parser，100% spec 合规、SIMD 加速、offset 映射（可定位源区间） | ✅ 用于 committed block 完整解析；⚠️ 它**不是**增量解析器（每次对单 block 全文解析，但单 block 小，O(1) 摊销） |
| Zed `crates/markdown` | Zed 自用的 markdown 渲染（agent panel 在用） | 可读源码参考，不可直接依赖（monorepo 内部 crate） |
| `gpui-component` | 有现成 markdown/CodeEditor 只读高亮组件（tree-sitter） | 参考实现/可裁剪引入 |
| syntect / tree-sitter | 代码块高亮 | tree-sitter（与 PRD 选型一致），代码块闭合后才高亮 |

**架构对应关系**：mdstream 的 committed/pending ≡ 我们原设计的「冻结 block / 未闭合 block」；它的 terminator ≡ markmend 的 mend。**我们的设计方向被生态验证，且省掉了最脏的活（chunk 边界 + 未闭合语法处理）。**

### 1.4 最终方案（已回写 tech-spec §5）

```
TextDelta → mdstream.append()
  committed blocks（BlockId 稳定）→ pulldown-cmark 解析 → RenderNode 冻结缓存
  pending block（唯一）→ terminator 补全后轻量渲染；未闭合 code fence 内纯文本
block 提交 → 冻结 + 局部替换（零重排）；Update.reset=true → 清缓存重建（罕见）
```

**必须自研的部分**（任何中间件都替代不了）：RenderNode → GPUI element 映射、冻结缓存失效策略、虚拟化滚动 + 锚定跟随、工具卡片与 markdown 混排。

### 1.5 验证计划（S3 spike，3 天时间框）

| 天 | 动作 | 通过标准 |
|---|---|---|
| D1 | mdstream 评估：许可证、GFM 表格/tasklist 覆盖、reset 语义、与 pulldown 的 adapter（pulldown feature）；锁定版本 + vendoring 预案（0.2.0 年轻） | 能消费 10 个真实 SSE 录制样本且输出稳定 |
| D2 | 管线打通：SSE 样本 → mdstream → RenderNode → GPUI 虚拟化列表 | 10k 行滚动 ≥120fps；1k delta/s CPU <30% |
| D3 | 边界 case 矩阵（复用 vue-stream-markdown 的 fix 列表当测试用例：单波浪号、未闭合表格、嵌套链接标签…）+ 帧对比零重排验证 | 冻结区零重排；边界 case 无渲染崩溃 |

**降级方案 B**（任一不达标）：按 block 分段全量重渲染（pulldown-cmark 全文解析 + block 级 diff 更新视图）。体验略损但不 block 里程碑。

### 1.6 残余风险

- mdstream 太新（0.2.0）：API 可能变 → pin 版本，最坏情况 vendor 进 `crates/`（它本身就是我们想写的 BlockSplitter，fork 不亏）
- GFM 表格流式场景（表格行逐行到达）是各家共同的薄弱点 → 边界 case 矩阵覆盖
- terminator 补全可能「猜错」（把用户的字面 `**` 当成未闭合粗体）→ 沿用 vue-stream-markdown 的教训：literal 保留优先于猜测

---

## #2 GPUI 虚拟化滚动与流式锚定

### 2.1 问题分解
会话流 = 不定高混排（markdown + 工具卡片）+ 流式尾部追加 + 万行规模。四块：①虚拟化容器选型；②贴底跟随状态机；③重绘成本控制；④动态高度与 CJK 测量正确性。

### 2.2 生态调研（均已确认，来源见下）
1. **`uniform_list` 要求等高行，不能用；`gpui::List` 用 SumTree 存不定高项、O(log N) 定位——正是 Zed Agent Thread 所用**（docs.rs/gpui/0.2.0/src/gpui/elements/list.rs.html；Zed `agent_ui/src/acp/thread_view.rs`）
2. gpui-component 的 `virtual_list` 需预知 `item_sizes`，流式场景不现实，弃用（docs.rs/gpui-component）
3. **`List` 硬约定：可视区外的项不得变高；变高必须 `splice(range, count)` 通知**，否则跳屏（list.rs 文件头注释）
4. `ListState::new(count, ListAlignment::Bottom, overdraw)` 原生支持底部锚定；逻辑滚动（item 索引+内偏移）防上方变高跳屏（docs.rs/gpui ListState）
5. ⚠️ **Zed v0.231.1 把 agent 流式从 bottom-up 改为 top-down + 自动跟随**（#52440），说明 bottom-anchored 流式有未明说的痛点——**开工前必须研读其最新 thread_view.rs 的 autoscroll 逻辑**（newreleases.io）
6. CJK：GPUI 字体 fallback 无显式 CJK 字体，Windows 出过垂直对齐 issue（#35878 已修），macOS CoreText 侧未见同类（推测，需实测）
7. 测试：`#[gpui::test]` + `VisualTestContext.paint()` 可 headless 驱动布局/绘制计时；无内置 FPS bench，需自测帧耗时（deepwiki gpui-ce）

### 2.3 方案
```
容器 = gpui::list + ListAlignment::Bottom
消息/工具卡片 = 不可变 entry（一卡片一 item）；流式 token 只改尾部 item
流式（30-60ms 节流合帧）：
  entries.tail.append(chunk); state.splice(tail..tail+1, 1)
  if follow { state.scroll_to_reveal_item(last) }
上翻解除：scroll handler 中 follow = (visible_range.end >= len-2)
回底按钮：follow = true + scroll_to_reveal_item(last)
冻结缓存：按 entry id 缓存解析后的 RenderNode/样式（Element 无跨帧缓存）；代码高亮后台线程算好再进 UI
```

### 2.4 验证计划（量化）
- 10,000 条混排消息（30% CJK）：`paint()` 循环布局+绘制 p50 <8ms、p99 <16ms
- 60 tok/s 流式 5 分钟：贴底跟随零跳屏（`logical_scroll_top` 单调断言）
- 上翻 3 屏再流式：offset 漂移 <1px
- 全 CJK 消息：行高一致、无裁剪重叠
- resize（400→1600px）：无滚动位置错乱

### 2.5 残余风险
- 「可视区外变高」零容忍：图片异步加载/代码块延迟高亮若改冻结项高度，必须 splice 对应 range——工程纪律兜底
- crates.io gpui 滞后于 zed 仓库演进，pin 版本有升级成本
- macOS CJK fallback 行高未实测

---

## #3 Agent 中断与断点续跑

### 3.1 问题分解
①嵌套任务树即时取消（SSE+工具+子进程）；②进程组级强杀防孤儿；③SQLite 事件日志崩溃续跑；④悬挂 tool_use 的协议修复。

### 3.2 生态调研
1. **CancellationToken**：`child_token()` 层级取消树；`select!` 里 `recv()/StreamExt::next()/cancelled()` 取消安全，`read_exact/write_all/read_to_string` **不安全**（中途取消丢进度）→ 非安全操作移出 select!（actor 模式）（tokio.rs 官方 shutdown 指南，确认）
2. **子进程**：spawn 设 `process_group(0)` 成组长，子孙继承组；`killpg` SIGTERM→超时→SIGKILL 整树杀；tokio-process-tools crate 已实现升级策略（GitHub lpotthast/tokio-process-tools，确认）
3. **断点续跑**：Claude Code = append-only JSONL transcript，resume 重放重建上下文（deepwiki claude-code-analysis，确认）；Codex CLI = JSONL rollout + SQLite 索引双层（danielvaughan.com，确认）；**对话本身就是天然事件流，事件溯源优于快照**。SQLite WAL + synchronous=NORMAL：应用崩溃不丢已提交事务（sqlite.org，确认）
4. **悬挂 tool_use**：Anthropic 要求每个 tool_use 必须以同 id tool_result 应答，否则 400 且**会话永久卡死**；修法 = 请求前 repair pass，为无应答 tool_use 注入 `is_error` 的 tool_result（particula.tech / claudeissues.com #67094，确认）

### 3.3 方案
```
状态机：Running → Cancelling(SIGTERM) → Killing(SIGKILL) → Stopped(落 cancel 事件)
        任一时刻崩溃 → 启动 Recovering → Resumable

on_stop():
  token.cancel()                      // SSE/工具循环 select! 立即退出
  killpg(pgid, SIGTERM); 300ms 未退 → killpg(pgid, SIGKILL)
  db.append(ToolCancelled)            // 同步 commit，不经 select!
每次 LLM 请求前 repair(messages):
  无 tool_result 的 tool_use → 注入 {is_error:true, "interrupted by user"}
on_boot(): 扫未结会话 → 重放事件流 → repair → Resumable
SQLite：WAL + synchronous=NORMAL，单写连接 + mpsc 写队列（写库走 actor 串行，绝不放 select! 分支）
```

### 3.4 验证计划（量化）
- Stop 延迟 p99 ≤1s（按钮→进程组全灭）；SSE 中断 ≤100ms
- kill -9 主进程 ×100 次：孤儿进程 0
- 随机时刻 SIGKILL ×100：恢复后 messages 100% 通过 API 校验（无 400）
- WAL 事件写入 <1ms/条；崩溃 0 库损坏

### 3.5 残余风险
- 孙进程 setsid 逃逸 → 按 PGID/端口扫描兜底
- 整机断电可能丢最后事务 → 关键事件局部 FULL sync
- bash 中断留半截写文件 → 事件记录，resume 时告知模型
- 同会话并发 resume → 文件锁

---

## #4 macOS 权限沙箱（Seatbelt）

### 4.1 问题分解
权限门禁 UI（已有设计）只是第一道防线；真边界要 OS 级沙箱：agent 发疯也只能动项目目录。对标 Codex CLI 的 sandbox-exec 方案。

### 4.2 生态调研
1. **sandbox-exec 自 macOS 10.13 起 man page 标 DEPRECATED 但未移除**，Chrome/Firefox/Claude Code/Codex CLI/SwiftPM 都在用；社区判断 Apple 不会移除，但自定义 SBPL 随 OS 升级可能失效（simonwillison.net + HN，确认）
2. **Codex 的 Seatbelt 实现**（`codex-rs/core/src/seatbelt.rs`，确认）：基线 `(allow default)` + `(deny file-write*)`，再 `-D WRITABLE_ROOT_N` 逐条放行项目目录；`.git` 重新 deny 只读（防改 git hooks）；网络默认禁、可 `(allow network*)`；进程加固 PT_DENY_ATTACH + 剥离 DYLD_*（deepwiki.org/openai/codex）
3. **只沙箱 spawn 的子命令，主 agent 循环不沙箱**——GPUI 主进程无需进沙箱（确认）
4. Rust 调用：直接 `Command::new("/usr/bin/sandbox-exec")`，无成熟封装 crate（生态空白）
5. **危险命令正则必被绕过**（base64/变量拼接/`$()`）——业界共识：正则只做 UX 兜底，真边界靠 OS 沙箱；清单参考 Cline 官方博客（确认）
6. Linux 等价物：bubblewrap + Landlock + seccomp（codex 同款，Phase 4 再说）

### 4.3 方案
Seatbelt profile 骨架（workspace-write 档）：
```lisp
(version 1)
(allow default)
(deny file-write*)
(allow file-write* (subpath (param "WRITABLE_ROOT_0")))  ; 项目目录
(allow file-write* (subpath "/private/tmp"))
(deny file-write*  (subpath (param "GIT_DIR")))          ; .git 只读
(allow network*)                                        ; 构建模式；只读档删此行
```
```rust
Command::new("/usr/bin/sandbox-exec")
  .arg("-p").arg(profile).arg("-D").arg(format!("WRITABLE_ROOT_0={ws}"))
  .arg("--").args(cmd).spawn()
```
write/edit 工具围栏（不经 bash 的直接 FS 写）：`canonicalize(target).starts_with(canonicalize(root))` 逐段校验防 symlink 逃逸（或用 `cap-std` capability API）；额外 deny `~/.ssh`、`.git/hooks`。

### 4.4 验证计划（量化）
- 逃逸测试集 ≥30 条（`$HOME`/`/etc`/symlink 链/hardlink/`..` 穿越）：拦截率 100%
- 禁网模式 curl 出站 100% 失败；联网模式 `cargo build` 成功率 ≥95%（10 个真实 crate 回归）
- sandbox-exec 启动增量 <50ms/命令
- 混淆危险命令语料（编码/拼接各 20 条）逃逸率 <5%
- CI 跑 macOS 最新 + N-1 双版本（防 OS 升级破坏 profile）

### 4.5 残余风险
- macOS 升级静默破坏 SBPL → 双版本 CI + 升级 runbook
- `(allow default)` 基线宽松，IPC/mach 面未收紧；fork bomb 不在防护范围
- 网络二值化（放行=全放行），域名级控制需叠 HTTP 代理（Phase 3+）
- 用户态写围栏有 TOCTOU 窗口；高保障场景把写操作也路由进沙箱子进程

---

## #5 SSE 背压与事件管线

### 5.1 问题分解
链路：LLM SSE（峰值 ~1k delta/s 推测）→ tokio → 三个消费者：UI（GPUI 主线程 120fps）、SQLite（rusqlite 同步 API）、计价。防：UI 线程淹没、落库阻塞、背压失控内存膨胀。

### 5.2 生态调研
1. **SSE 客户端**：绕开 `reqwest-eventsource` 自动重试（POST body 不可 clone 直接报错），用 `eventsource-stream` 包 `bytes_stream()` 自管退避重连（确认）
2. **背压两层合并**：bounded mpsc + 中间 coalesce 任务（8ms 或 4KB 先到先 flush）→ UI 通道压到 cap 8 帧对齐；DB 走 200ms/500 条攒批。合并后 1k delta/s 降到 UI ≤120 notify/s、DB ≤5 事务/s（方案设计值）
3. **rusqlite 高频写**：专用 OS 线程（tokio-rusqlite 模式）延迟最稳，避免 spawn_blocking 的 P99 抖动；WAL + synchronous=NORMAL（确认）
4. **GPUI 线程模型**：`cx.spawn` 前台任务内 `WeakEntity.update + cx.notify()`，失效天然按帧合并，不需要额外 vsync 定时器（确认）
5. **延迟预算 <16ms 可行**，前提是落库/计价完全移出上屏关键路径（确认）；⚠️ 1k delta/s 峰值是推测，公开数据单流 ~100 tok/s，**开工先用真实 provider 抓包校准再定通道容量**

### 5.3 方案（管线结构）
```
SSE stream (eventsource-stream + bytes_stream, 自管重连)
  → [parse task] → bounded mpsc(256)
  → [coalesce task] 8ms/4KB flush
  → 三路分发：
     UI: cx.spawn → WeakEntity.update + notify（帧合并，cap 8）
     DB: 专用写线程，攒批 200ms/500 条一事务（WAL）
     计价: 无锁累加器，usage 到达后校准（与 UI 计数器同通道展示）
背压策略：UI 慢→CoalesceLocal 本地缓冲续合（不丢文本只合并）；DB 慢→队列上限+批量增大；永不阻塞 SSE 读取
```

### 5.4 验证计划（量化）
- 抓包校准真实 delta 频率分布（OpenAI/DeepSeek/CPA 各 10 任务）
- 2× 校准峰值注入 5 分钟：UI 帧率 ≥120fps、内存曲线平坦（无队列膨胀）
- 端到端 token 到达→上屏 p99 <16ms；落库延迟对上屏 0 影响（关键路径隔离验证）
- kill -9 后已收 delta 0 丢失（DB 攒批窗口内除外，<200ms）

### 5.5 残余风险
- 真实峰值若远超 1k/s（多工具并发回灌），coalesce 参数要重调
- eventsource-stream 对非标准 SSE（注释行/心跳）兼容性需实测各 provider
- GPUI WeakEntity.update 在窗口关闭竞态下的失败处理（静默丢 vs 落盘标记）

---

## 总结：难点全景与开工顺序建议

| # | 难点 | 方案状态 | 最大不确定点 | 验证时机 |
|---|---|---|---|---|
| 1 | 流式 markdown | ✅ mdstream+pulldown 已定 | mdstream 成熟度 | S3 D1 spike |
| 2 | 虚拟化滚动锚定 | ✅ gpui::List+Bottom 已定 | Zed 为何转 top-down（需读源码） | S3 D2 |
| 3 | 中断/续跑 | ✅ 事件溯源+进程组杀 | 悬挂 tool_use repair 的各 provider 差异 | S4-S5 |
| 4 | 权限沙箱 | ✅ Seatbelt 对齐 codex | OS 升级破坏 profile；TOCTOU | S5 |
| 5 | SSE 背压 | ✅ 两层合并管线已定 | 真实 delta 峰值未校准 | S4 前抓包 |

**结论：五大难点全部有已验证的对标实现（Zed/Codex/Claude Code/markstream），无无人区。剩余不确定性都可在对应 Sprint 的 1-3 天 spike 内闭环。具备开工条件。**

