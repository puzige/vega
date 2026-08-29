# ✦ Vega — Phase 1 技术实现规格（SDD · Spec-Driven Development）

**版本** v0.1 · 2026-08-28 · 关联：[vega-phase1-plan.md](vega-phase1-plan.md) · [vega-features.md](vega-features.md) · [vega-ui-spec.md](vega-ui-spec.md)

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
> 数据库文件路径（2026-08-29 S2-T10 补定义；同日人类决策修订为 XDG 布局）：`${XDG_DATA_HOME:-~/.local/share}/vega/vega.db`（见 §6 文件布局）。

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

### 5.0 生态结论（2026-08-28 调研，详见 vega-tech-risks.md §1）

- **首选中间件：`mdstream` v0.2.0**（Rust 流式 markdown 中间件，committed+pending 模型、稳定 BlockId 缓存键、remend 式尾部语法补全、render-agnostic、官方明确支持 gpui、有 tokio coalesce glue）。与本节原设计架构一致，先评估后决定引入还是自研。
- **解析器**：committed block 用 `pulldown-cmark`（pull parser、offset 映射、CommonMark 全合规）。
- **对标参考**：Web 生态 vue-stream-markdown / Vercel streamdown 的核心手法 = markmend/remend「未闭合语法补全」+ 全量 reparse + AST 级 diff（JS 可行因 mdast 快+DOM diff 便宜；Rust 要走真增量避免 O(n²)）。

### 5.1 管线设计（mdstream 引入场景）

```
TextDelta ─▶ mdstream::MdStream.append()
   ├─ Committed blocks（BlockId 稳定、永不变）─▶ pulldown-cmark 完整解析
   │     ─▶ RenderNode 树 ─▶ **冻结缓存（按 BlockId）** ─▶ tree-sitter 高亮代码块
   └─ PendingBlock（仅一个）─▶ mdstream display view（terminator 补全后）
         ─▶ 轻量渲染；未闭合 ``` fence 内纯文本等宽（不高亮）
block 提交事件 ─▶ 该 block 冻结 + 局部替换（禁全局重排）
Update{reset:true}（罕见，如 footnote 模式）─▶ 清空缓存重建
```

### 5.2 spike 验收（3 天时间框，不变）

① 10k 行虚拟化滚动 ≥120fps；② 1k delta/s 注入下 CPU <30%；③ 冻结区零重排（帧对比）。
任一不达标 → 降级方案 B：「按 block 分段全量重渲染」（pulldown-cmark 全量解析但按 block diff 更新视图）。
spike 第 1 天专门验证 mdstream：许可证、GFM 表格/tasklist 覆盖、reset 语义、版本锁定或 vendoring 预案（0.2.0 尚年轻）。

### 5.3 无论中间件如何、必须自研的部分

RenderNode → GPUI element 映射、冻结缓存失效策略、虚拟化滚动 + 锚定跟随（上翻不打扰）、工具卡片与 markdown 块的混排布局。

### 5.4 final 终结语义（借鉴 markstream-vue）

`MessageFinished` 到达时：作废 pending block 的流式补全态，以最终语义对该 block 完整重解析一次并冻结——避免 terminator「猜错」的补全残留在 UI 上。流式期间禁止给节点加入场 opacity 动画（markstream 官方警告：高频流式 + fade = 反复重启动画）。

## 6. 配置与密钥（A1-10/A11-05）

> **文件布局（2026-08-29 人类决策）**：遵循 XDG Base Directory，全平台一致（含 macOS；Phase 4 Linux 零返工）。配置根 = `${XDG_CONFIG_HOME:-~/.config}/vega/`，数据根 = `${XDG_DATA_HOME:-~/.local/share}/vega/`。实现为 vega_store 内零依赖环境变量解析（不引 dirs/xdg crate）。不做旧 `~/.vega/` 自动迁移（预发布无真实用户）。

- 配置：`${XDG_CONFIG_HOME:-~/.config}/vega/config.toml`：providers（name/base_url/model 列表/定价）、defaults（model/permission_mode）、ui（theme）。
- 数据：`${XDG_DATA_HOME:-~/.local/share}/vega/`：`vega.db`（SQLite）、`pricing.json`（S7 定价表：内置 deepseek/gpt/claude 主流价格，用户可加自定义模型）。
- API key **只存 Keychain**（`security` CLI 或 keyring crate，service=`ai.vega.{provider}`），config 只存引用名。

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
