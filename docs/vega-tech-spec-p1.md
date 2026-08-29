# ✦ Vega — Phase 1 技术实现规格（SDD · Spec-Driven Development）

**版本** v0.2 · 2026-08-29 · 关联：[vega-phase1-plan.md](vega-phase1-plan.md) · [vega-features.md](vega-features.md) · [vega-ui-spec.md](vega-ui-spec.md)

> **SDD 工作约定**：每个 Sprint 开工前，对应模块的 spec（本文件对应章节）必须先定稿；实现以 spec 为准；实现完成后对照 spec 验收。spec 变更走文档修改 + 变更记录，不允许代码先行 spec 后补。
> 本文件覆盖 Phase 1（S1-S8）。所有 Rust 签名为**设计目标**，实现时可微调参数名，但 trait 边界、状态机、DDL 不得偏离。

---

## 1. Crate 边界与依赖图（定稿）

```
vega (bin) ──▶ vega_ui ──▶ vega_conversation ──▶ vega_runtime ──▶ vega_tools
                 │                │                    │
                 ▼                ▼                    ▼
           vega_markdown     vega_store           vega_token
                 │                ▲                    ▲
                 └── vega_theme ──┴────────────────────┘
```
- `vega_runtime` **禁止依赖 GPUI/任何 UI crate**（headless 可测）。
- UI 通过 GPUI Entity 事件订阅 `vega_conversation` 的状态变更。
- 跨 crate 共享类型（Message/ToolCall/Event）放 `vega_conversation` 的 `types` 模块，禁止循环引用。

## 2. 数据模型（A11-01 · SQLite DDL，S1 定稿）

```sql
CREATE TABLE projects (
  id TEXT PRIMARY KEY,            -- ulid
  path TEXT NOT NULL UNIQUE,      -- 绝对路径
  name TEXT NOT NULL,
  git_default_branch TEXT,
  created_at INTEGER NOT NULL,    -- unix ms
  last_opened_at INTEGER NOT NULL
);

CREATE TABLE threads (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  title TEXT NOT NULL DEFAULT '',
  mode TEXT NOT NULL DEFAULT 'execute',   -- ask|plan|execute
  permission_mode TEXT NOT NULL DEFAULT 'confirm',  -- readonly|confirm|auto
  model TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',  -- active|archived
  pinned INTEGER NOT NULL DEFAULT 0,
  unread INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX idx_threads_project ON threads(project_id, updated_at DESC);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id),
  seq INTEGER NOT NULL,           -- 线程内单调递增
  role TEXT NOT NULL,             -- user|assistant|system
  kind TEXT NOT NULL DEFAULT 'text',  -- text|plan|error|summary
  content TEXT NOT NULL,          -- markdown 原文（完整，非增量）
  status TEXT NOT NULL DEFAULT 'done',  -- streaming|done|interrupted|failed
  created_at INTEGER NOT NULL,
  UNIQUE(thread_id, seq)
);

CREATE TABLE tool_calls (
  id TEXT PRIMARY KEY,            -- 与 provider 的 tool_use_id 对齐
  thread_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  tool TEXT NOT NULL,             -- bash|read|write|edit|glob|grep|web_fetch
  input_json TEXT NOT NULL,
  output_text TEXT,               -- 截断后展示文本
  output_full_path TEXT,          -- 完整输出落盘路径（大输出）
  status TEXT NOT NULL,           -- pending_approval|approved|rejected|running|success|failed|cancelled
  approval TEXT,                  -- once|always|deny + note
  exit_code INTEGER,
  duration_ms INTEGER,
  created_at INTEGER NOT NULL,
  finished_at INTEGER
);
CREATE INDEX idx_tool_calls_thread ON tool_calls(thread_id, seq);

CREATE TABLE token_usage (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  thread_id TEXT NOT NULL,
  message_id TEXT,
  model TEXT NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  cache_read_tokens INTEGER NOT NULL DEFAULT 0,
  cache_write_tokens INTEGER NOT NULL DEFAULT 0,
  cost_microcents INTEGER NOT NULL,   -- 成本引擎计算结果
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_usage_thread ON token_usage(thread_id);
CREATE INDEX idx_usage_day ON token_usage((created_at/86400000));  -- 仪表盘聚合

CREATE TABLE permissions (        -- 「总是允许」的规则记忆
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  pattern TEXT NOT NULL,          -- 如 "bash:cargo *" / "write:src/**"
  created_at INTEGER NOT NULL,
  UNIQUE(project_id, tool, pattern)
);
```

迁移机制：`vega_store::migrate()` 启动时跑，`PRAGMA user_version` 记录版本，递增 SQL 文件 `migrations/0001_init.sql…`。
> 数据库文件路径（2026-08-29 人类决策，Zed 式混合布局见 §6）：macOS `~/Library/Application Support/ai.vega/vega.db`；Linux（Phase 4）`${XDG_DATA_HOME:-~/.local/share}/vega/vega.db`。

## 3. 核心类型（vega_conversation::types，S1 定稿）

```rust
pub enum ConversationEvent {           // Runtime → UI/Store 的唯一事件流
    MessageStarted { message_id: MessageId, seq: u64 },
    TextDelta { message_id: MessageId, delta: String },
    ThinkingDelta { message_id: MessageId, delta: String },
    ToolCallProposed { call: ToolCall },                  // 待权限裁决
    ToolCallApproved { call_id: CallId, approval: Approval },
    ToolCallOutput { call_id: CallId, chunk: ToolOutputChunk },
    ToolCallFinished { call_id: CallId, result: ToolResult },
    UsageUpdated { message_id: MessageId, usage: TokenUsage, cost: Microcents },
    MessageFinished { message_id: MessageId, stop_reason: StopReason },
    Error { message_id: Option<MessageId>, error: VegaError },
    Interrupted { message_id: MessageId },
}

pub enum ToolCallStatus { PendingApproval, Approved, Rejected, Running, Success, Failed, Cancelled }
// 状态机：PendingApproval → Approved → Running → Success|Failed
//         PendingApproval → Rejected（终态）；Running → Cancelled（中断）

pub enum PermissionMode { ReadOnly, Confirm, Auto }   // 对应 UI：只读/变更前确认/全自动
pub enum RunMode { Ask, Plan, Execute }               // 三模式（A2-09）

pub struct TokenUsage { pub input: u64, pub output: u64, pub cache_read: u64, pub cache_write: u64 }
pub struct Microcents(pub i64);   // 1/1_000_000 美元，杜绝浮点误差
```

## 4. Vega Runtime 规格（A3 · 核心）

### 4.1 Provider trait（A3-01）

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat_stream(
        &self,
        req: ChatRequest,          // model, messages, tools, thinking_budget, max_tokens
        cancel: CancellationToken,
    ) -> Result<EventStream, VegaError>;   // EventStream = impl Stream<Item = Result<ProviderEvent, VegaError>>
}

pub enum ProviderEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolUse { id: String, name: String, input_json: String },   // 完整聚合后发出
    Usage { input: u64, output: u64, cache_read: u64, cache_write: u64 },
    Done { stop_reason: StopReason },
}
```

- **OpenAI 兼容实现**（A3-02）：`POST {base_url}/chat/completions`，`stream: true`，SSE 解析；`stream_options.include_usage = true` 拿最终 usage；tool_calls 增量按 index 聚合。
- 重试策略：网络错误/5xx 指数退避（1s/2s/4s，最多 3 次）；429 读 `Retry-After`；**工具结果已落库的消息不重复执行**（重试只重建请求上下文）。

### 4.2 Agentic 循环（A3-03，时序定稿）

```
用户提交 ─▶ 上下文组装(system+记忆+@文件+历史窗口) ─▶ provider.chat_stream
   │                                                       │
   │◀────────── TextDelta/ThinkingDelta（转发 UI+累积）────┤
   │                                                       │
   │◀────────── ToolUse(聚合完成) ─────────────────────────┤
   ▼                                                       │
权限裁决(A3-09) ─ reject ─▶ 结果(tool_result: denied) 追加   │
   │ approve                                             │
   ▼                                                     │
工具执行(vega_tools) ─▶ 输出截断(头 2k + 尾 2k 行) ─▶ 追加 tool_result
   │                                                     │
   ▼                                                     │
Usage 到达 ─▶ 成本引擎计价 ─▶ token_usage 落库 ─▶ UI 更新    │
   │                                                     │
   └── 无 ToolUse 且 stop_reason=end ─▶ 收敛；否则回到 chat_stream
```

- 单轮工具并发：同一 assistant turn 内多个 tool_use **串行执行**（Phase 1 简化，避免写冲突；Phase 2 评估只读工具并行）。
- 循环上限：单次任务最多 100 轮工具调用，超限自动收敛并提示。
- 中断：cancel token 触发 → 当前工具等待完成（bash 则 SIGTERM），message 置 `interrupted`，可恢复续跑。

### 4.3 权限门禁（A3-09，决策顺序定稿）

```
tool_call 到达
 1. 危险命令硬拦截清单命中？（regex：rm\s+-rf\s+/、git push --force、dd of=/dev…）
    → 是：无论模式，弹确认卡且默认焦点在[拒绝]
 2. thread.permission_mode == ReadOnly 且工具为写类（write/edit/bash 非白名单只读命令）
    → 直接 Rejected，原因注入 tool_result
 3. permissions 表命中（项目级"总是允许"规则）→ Approved
 4. mode == Auto → Approved（仍需过第 1 步）
 5. mode == Confirm → 弹权限卡，等待用户（超时 10 分钟视为 Rejected）
 6. 裁决结果写入 tool_calls.approval；「总是允许」写入 permissions 表
```

### 4.4 内置工具 I/O 契约（A3-05~08，摘要）

| 工具 | 输入要点 | 输出契约 |
|---|---|---|
| bash | cmd, timeout_ms(默认 120s) | stdout/stderr 合并，头 2000+尾 2000 行截断；exit_code；cwd 强制=项目根 |
| read | path, offset, limit | 带行号；单行>2k 截断；二进制检测拒绝 |
| write | path, content | 写前备份到 checkpoint；返回成功字节数 |
| edit | path, old_string, new_string | 唯一匹配校验，失败返回周边上下文；0 或多匹配=错误 |
| glob/grep | pattern, path? | 尊重 .gitignore；结果上限 500 条 |

## 5. 流式 Markdown 管线（A2-02 · S3 spike 验证目标）

### 5.0 生态结论（2026-08-29 T14 spike 尽调更新，取代 2026-08-28 调研；详见 vega-tech-risks.md §1）

- **选定路线：引入 `mdstream` 0.3.0（精确锁定 `=0.3.0`）+ `pulldown-cmark` 0.13 做已提交块解析。** spike 实测确认 committed+pending 模型满足全部验收（见 §5.2 数据），方案 B（全量重渲染）CPU 超预算 2.4 倍起，不采用。
- **License**：`MIT OR Apache-2.0` 双许可（crates.io + 仓库 LICENSE-MIT/LICENSE-APACHE 均核验），**可引入私有商业项目**。
- **0.2.0 → 0.3.0（2026-07-07）变化**：唯一 breaking 是 `mdstream::pending` / `mdstream::syntax` 模块路径内部化（改为从 crate root 导入 `TerminatorOptions` / `terminate_markdown`）；新增 `MdStreamBuilder`；修复表格分隔符跨 chunk 边界 bug、tag 分析误判；移除可避免 panic 路径。核心 API（`MdStream`/`append`/`append_ref`/`Update`/`BlockId`）不变。我们只用 crate root 导出，不受影响。
- **committed+pending 模型与 BlockId 稳定性（源码确认）**：`BlockId(u64)` 单调递增（`next_block_id` 计数器，起始 1），committed 块一旦产出永不变更（README："safe for UI to cache by BlockId"）；`Update { committed, pending, reset, invalidated }`；`invalidated: Vec<BlockId>` 用于后置引用定义等文档级语义波及，消费方按需重解析对应块。spike 实测 48k delta 流：committed 内容零变更、缓存零重复解析。
- **GFM 覆盖**：committed 块由我们用 pulldown-cmark 解析，GFM 扩展（tables/tasklists/strikethrough）按需全开（adapter 暴露 `PulldownOptions`）；pending 尾部 terminator 覆盖未闭合 code fence/emphasis 四变体/strikethrough/inline code/katex/link-image/setext 保护；0.3.0 已修表格分隔符流式 bug。tasklist 无专门 terminator（未闭合 `- [ ]` 降级为普通行文本渲染，提交后完整解析）——可接受。
- **reset 语义（源码确认）**：`Update.reset=true` 仅在 scope 驱动转换时出现（如进入 footnote 单块模式），语义 = 消费方丢弃全部缓存按本条重建；另有 `MdStream::reset()` 手动重置。频率极低（spike 48k delta 样本中 0 次），走"清空缓存重建"路径即可，无需优化。
- **gpui 支持成熟度（修正原结论）**：0.3.0 README 仅将 gpui/Zed 列为 render-agnostic 的**目标集成场景**（"helps downstream UIs (egui, gpui/Zed, TUI)"），**不存在 gpui 适配器**，"官方明确支持 gpui"的原表述不成立；上游 0.4 README 更是明确声明不发布任何渲染器。对本项目无实质影响（RenderNode→GPUI 映射本就自研，见 §5.3），但依赖预期需修正。
- **版本锁定与 vendoring 预案**：crates.io 0.3.0 = tag v0.3.0，锁定 `mdstream = "=0.3.0"`（`mdstream-tokio = "0.3.0"` 的 coalesce 胶水按需引入，MSRV 1.88 与工具链兼容）。⚠️ 上游 main 已是破坏性 0.4 API（`MdStream`→`StreamEngine`，**committed/pending+BlockId 模型整体移除**，fallible append + `mdstream_protocol::Reducer` ChangeSet）——0.3.0 是目标模型的最后一版，上游不会再修 0.3.0 bug。预案：出问题即 fork/vendor 进 `crates/`（双许可、约 4.5k 行、无深度依赖树，fork 成本低）。
- **维护活跃度（风险知悉）**：单维护者（Latias94），10 stars / 3 forks / 0 open issues，232 commits，最后 release 2026-07-07、最后 commit 2026-07-30；下载量 87k 中 86.7k 集中在 0.2.0（0.3.0 仅 266，来源存疑），社区规模极小。结论：当作"买断一段源码"使用，靠锁定版本 + fork 预案对冲，不指望上游维护。
- **解析器**：committed block 用 `pulldown-cmark` 0.13（pull parser、offset 映射、GFM 扩展全开）。
- **对标参考**：Web 生态 vue-stream-markdown / Vercel streamdown 的核心手法 = markmend/remend「未闭合语法补全」+ 全量 reparse + AST 级 diff（JS 可行因 mdast 快+DOM diff 便宜；Rust 要走真增量避免 O(n²)）。
- **⚠️ 白名单增补待人类批准**：`mdstream =0.3.0`（及可选 `mdstream-tokio 0.3.0`）不在现行依赖白名单内，需人类批准后 T15 才可落 workspace 依赖。

### 5.1 管线设计（路线 A：mdstream 0.3.0，spike 实测定稿）

```
TextDelta ─▶ mdstream-tokio coalesce（CoalesceLocal 攒批，8ms/4KB 先到先 flush）
   ─▶ mdstream::MdStream.append_ref()（UI/解析线程持有 stream，零拷贝 borrowed update）
   ├─ Committed blocks（BlockId 稳定、永不变）─▶ pulldown-cmark 完整解析（仅首次，按 BlockId 冻结缓存）
   │     ─▶ RenderNode 树 ─▶ tree-sitter 高亮代码块（T16：闭合块才高亮）
   └─ PendingBlock（仅一个）─▶ display view（terminator 补全后）─▶ 轻量解析
         ─▶ 未闭合 ``` fence 内纯文本等宽（不高亮）
block 提交事件 ─▶ 该 block 冻结 + 局部替换（禁全局重排）
Update.reset=true（罕见，spike 48k delta 样本 0 次）─▶ 清空缓存重建
Update.invalidated（后置引用定义）─▶ 按 BlockId 重解析指定块
```

**spike 实测（2026-08-29，10k 行合成文档 277KB，48k 个 3-8 字节 delta）**：
- 增量管线合计 **1.23 µs/delta**（append 0.87 + committed/pending 解析 0.36）→ 1k delta/s 下 **1.23 ms CPU/s（0.12% 单核）**；committed 块 2,667 个全部只解析一次（0 次重复解析），committed 内容 0 次变更。
- 对照降级方案 B（每 delta 全量 reparse + 块级 diff）：10k 文档稳态 **712 µs/delta → 712 ms CPU/s（71% 单核）**，超 30% 预算 2.4 倍，且总量随文档线性增长（O(n²) 总量，全程估算为路线 A 的 290 倍）——**CPU 预算项上方案 B 直接出局**，仅保留为 mdstream 彻底不可用时的应急降级（届时必须再做分帧摊销设计）。

### 5.2 spike 验收（3 天时间框，不变）

① 10k 行虚拟化滚动 ≥120fps；② 1k delta/s 注入下 CPU <30%；③ 冻结区零重排（帧对比）。
任一不达标 → 降级方案 B：「按 block 分段全量重渲染」（pulldown-cmark 全量解析但按 block diff 更新视图）。
spike 第 1 天专门验证 mdstream：许可证、GFM 表格/tasklist 覆盖、reset 语义、版本锁定或 vendoring 预案（0.2.0 尚年轻）。

**spike 实测（2026-08-29，Apple Silicon Mac，外接 4K@60Hz 显示器）**：
- ① gpui `uniform_list` + 10k 行（混合 markdown 样本含 CJK）程序化滚动：fps 稳定 60（**被 60Hz 显示器 vsync 封顶**）；每帧 build 耗时 p50 7-12µs / p99 ≤22µs，对比 120fps 的 8.33ms 帧预算有 ~400 倍余量 → **判定 120fps 达标可行性成立**，字面 120fps 数字需在 ProMotion 目标硬件上复测（T17 验收时执行）。
- ② 1k delta/s（实际 960-976/s）注入 12s：进程 CPU 均值 16.5%、峰值 23.1%（`ps -o %cpu` 采样）→ **达标**。
- ③ 冻结区零重排：渲染计数器分区对照——流式注入期间冻结块重建计数 = 0（**CLEAN**）；对照组（每批 delta 全量重渲染 10k 块）冻结重建 10,512,288 次、CPU 29.7%，证明计数器方法有效区分两种路线。

### 5.3 无论中间件如何、必须自研的部分

RenderNode → GPUI element 映射、冻结缓存失效策略、虚拟化滚动 + 锚定跟随（上翻不打扰）、工具卡片与 markdown 块的混排布局。

### 5.4 final 终结语义（借鉴 markstream-vue）

`MessageFinished` 到达时：作废 pending block 的流式补全态，以最终语义对该 block 完整重解析一次并冻结——避免 terminator「猜错」的补全残留在 UI 上。流式期间禁止给节点加入场 opacity 动画（markstream 官方警告：高频流式 + fade = 反复重启动画）。

## 6. 配置与密钥（A1-10/A11-05）

> **文件布局（2026-08-29 人类决策，Zed 式混合）**：配置走 XDG——`${XDG_CONFIG_HOME:-~/.config}/vega/`（全平台一致）；数据按平台——macOS `~/Library/Application Support/ai.vega/`（Bundle-ID 命名空间，与 Keychain service `ai.vega` 一致），Phase 4 Linux `${XDG_DATA_HOME:-~/.local/share}/vega/`。实现为 vega_store 内零依赖解析（`cfg!(target_os)` 门控，不引 dirs/xdg crate）。不做旧 `~/.vega/` 自动迁移（预发布无真实用户）。

- 配置：`${XDG_CONFIG_HOME:-~/.config}/vega/config.toml`：providers（name/base_url/model 列表/定价）、defaults（model/permission_mode）、ui（theme）。
- 数据：macOS `~/Library/Application Support/ai.vega/`：`vega.db`（SQLite）、`pricing.json`（S7 定价表：内置 deepseek/gpt/claude 主流价格，用户可加自定义模型）；Linux（Phase 4）同构换 XDG 数据根。
- API key **只存 Keychain**（`security` CLI 或 keyring crate，service=`ai.vega.{provider}`），config 只存引用名。
- 远期预留：缓存 `~/Library/Caches/ai.vega`（macOS）/ `${XDG_CACHE_HOME:-~/.cache}/vega`（Linux）；日志 `~/Library/Logs/ai.vega` / `${XDG_STATE_HOME:-~/.local/state}/vega`。

## 7. 错误模型（统一 VegaError）

```rust
pub enum VegaError {
    Provider { status: Option<u16>, message: String, retryable: bool },
    Io(std::io::Error),
    Store(rusqlite::Error),
    Tool { tool: String, message: String },
    Cancelled,
}
```
UI 呈现：Provider 错误 → 会话内内联条+[重试]；Store 错误 → 启动期阻断+导出建议；Tool 错误 → 工具卡片失败态（不阻断会话）。

## 8. 测试策略（SDD 验收的一部分）

| 层 | 内容 | 工具 |
|---|---|---|
| 单元 | runtime 循环（mock provider 回放 SSE 脚本）、权限决策矩阵全组合、edit 工具匹配、markdown 切块 | cargo test |
| 集成 | 六表 DDL CRUD、中断恢复（杀进程→重启→续跑）、checkpoint 回退 | tempfile 库 |
| Golden | 10 个真实 SSE 录制样本 → 事件序列断言 | insta snapshot |
| UI | 权限卡交互、滚动锚定 | #[gpui::test] |
| 性能 | P1-P8 准线 | xtask bench |

## 9. Sprint → Spec 映射

| Sprint | 依据章节 |
|---|---|
| S1 脚手架/CI/schema/bench | §1 §2 §3 §8 |
| S2 侧边栏/项目模型 | §2 + UI Spec §1 §4.1 |
| S3 流式渲染 | §5 + UI Spec §5 |
| S4 Runtime 核心 | §4.1 §4.2 §4.4（只读工具） |
| S5 写工具+权限+三模式 | §4.3 §4.4 §3 |
| S6 Diff/产物/Open in | UI Spec §4.5 + §2 |
| S7 Token v1 | §2(token_usage) §6 + Features A10 |
| S8 打磨/里程碑 | UI Spec §6 Checklist 全过 |

---

## 变更记录
- v0.1 (2026-08-28) 初版定稿。
- v0.2 (2026-08-29) T14 spike 结论回写：§5.0 尽调更新（mdstream 0.3.0 锁定、license/gpui 结论修正、0.4 移除 committed/pending 模型的 vendoring 预案、白名单增补待人类批准）；§5.1 路线 A 定稿附实测数据（1.23µs/δ vs 方案 B 712µs/δ）；§5.2 三项验收实测（60Hz 显示器下 120fps 余量成立、CPU 16.5%、冻结区 CLEAN）。
