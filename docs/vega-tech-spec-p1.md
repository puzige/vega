# ✦ Vega — Phase 1 技术实现规格（SDD · Spec-Driven Development）

**版本** v0.6 · 2026-08-30 · 关联：[vega-phase1-plan.md](vega-phase1-plan.md) · [vega-features.md](vega-features.md) · [vega-ui-spec.md](vega-ui-spec.md)

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
  approval TEXT,                  -- S4 裸 once|deny；S5 起为向后兼容的裁决 JSON
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

CREATE TABLE permissions (        -- 项目级「总是允许」的规则记忆
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  pattern TEXT NOT NULL,          -- S5 精确签名，不含 tool 前缀，不作 glob/regex
  created_at INTEGER NOT NULL,
  UNIQUE(project_id, tool, pattern)
);
```

迁移机制：`vega_store::migrate()` 启动时跑，`PRAGMA user_version` 记录版本，递增 SQL 文件 `migrations/0001_init.sql…`。已合入的 migration 不得改写；schema 只能通过下一编号 migration 增量演进，Phase 1 始终保持上述六张表。

S5 的 `0002_plan_review.sql` 只给 `messages` 添加三个 nullable 列：`plan_status TEXT CHECK (plan_status IS NULL OR plan_status IN ('pending','approved','changes_requested','abandoned'))`、`plan_review_note TEXT`、`plan_reviewed_at INTEGER`。非 plan 消息三列均为 NULL；该 migration 不删列、不重建表、不新建第七表，读取到非法 plan 状态/组合须 fail closed。

新 Plan 完成事务先确认 thread.mode 仍为 plan；若旧 plan approve 已抢先切到 execute，本次 completion fail closed。确认后把该 thread 除 `current_message_id` 外全部现存 pending plan 更新为 `abandoned`、`plan_review_note='superseded'`、同一 `plan_reviewed_at`，再插入或标记 current plan 为 pending；后一步失败必须回滚 supersede。Plan approve/change/abandon 则先执行带 `id + kind='plan' + plan_status='pending'` 的 conditional update，确认 affected rows == 1 后才允许改变 thread.mode 或插入 review user message；否则整个操作无后续写入。SQLite 事务串行化后：completion 先赢则旧 approval affected rows=0；旧 approve 先赢则 completion 因 mode 非 plan 失败；两个 completion 依次提交时最后完成者是唯一 pending。这是应用事务不变量，不虚称 DDL 唯一约束。

S5 起 `tool_calls.approval` 精确写四个顶层字段：`{"decision":"once|always|deny","note":null|string,"source":"…","danger":null|object}`。非 null danger 精确为 `{"rule_id":"…","decision":"once|always|deny","note":null|string}`。source 只取 `danger|readonly|run_mode|rule|auto|user|timeout|validation|readonly_tool|recovery|legacy`。danger+ReadOnly 时顶层为 deny/readonly，nested danger 保留 once|always；danger deny/timeout 时顶层为 deny/danger|timeout，nested decision=deny。读取端接受 S4 裸 `once|always|deny`，内存归一为同 decision、note=null、source=legacy、danger=null，且不重写历史行；裸值仅为 read-only compatibility，所有 S5 新写必须为严格 JSON。其他缺字段、额外字段、未知值或损坏 JSON 全部 fail closed。

S5 的 valid write/edit `input_json` 改为不含正文的 strict audit projection。write exact JSON 为 `{"audit_version":"write_edit_v1","tool":"write","path":"<normalized-relative>","content_bytes":N,"fingerprint_v1":"<64 lower hex>"}`；edit exact JSON 为 `{"audit_version":"write_edit_v1","tool":"edit","path":"<normalized-relative>","old_string_bytes":N,"new_string_bytes":N,"fingerprint_v1":"<64 lower hex>"}`。JSON key order 无语义；整数必须能严格解码为 u64（拒绝负数、小数与 overflow）；missing/extra/wrong-type、错误常量、非法规范相对 path 或非 64-byte lowercase hex fingerprint 一律 fail closed。fingerprint 输入为 ASCII domain `vega.write-edit.fingerprint.v1\0`，随后按顺序编码 tool、path、write.content 或 edit.old_string/edit.new_string，每个字段均为 `u64` big-endian length + 原始 UTF-8 bytes。实现用仓内 safe Rust 和公开 SHA-256 test vectors，不引新依赖。恢复时重算，严格解码 projection，并同时比较 DB `tool` 与 projection `tool`、规范 path、fingerprint；任一不符即 call-id conflict，绝不执行或复用。raw content 无恢复例外，永不持久化。

S5 checkpoint 对外只传 content-free、数据根相对且 opaque 的 `checkpoint_ref`，exact syntax 为 `preimage-v1/<encoded-project>/<encoded-thread>/<encoded-call>`。codec 必须校验固定前缀、恰好四段、三个 `id-` + lowercase-even-hex encoded id 均可解码为 1..=120 UTF-8 bytes；拒绝绝对路径、`.`/`..`、raw id、额外段或错误编码。ref 不得包含/泄露绝对 checkpoint/data root，tool/runtime/provider 只传 ref，不传底层路径。

created-new-file metadata 只用于目标原先不存在的 write：必须在创建目标前原子落到 call root `metadata.json`，exact JSON 为 `{"metadata_version":"preimage_v1","kind":"created_new_file","path":"<normalized-relative>"}`。existing-target write/edit 的 `metadata.json` 必须 absent，其 preimage 只能在 `files/` 下。metadata 的 missing/extra/wrong-type、错误常量或非法 path 均拒绝；不允许借此增加 checkpoint 浏览、回退或 Phase 2 恢复 API。

write 成功的 `ToolOutput.text` exact JSON 为 `{"path":"<normalized-relative>","bytes_written":N,"checkpoint_ref":"..."}`；edit 成功 exact JSON 为 `{"path":"<normalized-relative>","bytes_written":N,"replacements":1,"checkpoint_ref":"..."}`。key order 无语义；`N` 必须严格为 u64，edit replacements 必须严格为整数常量 1；missing/extra/wrong-type、非法 path/ref 或错误常量均 fail closed。成功结果不得包含正文、raw id 或绝对数据根；失败结果沿用稳定、长度有界且脱敏的工具错误投影。

write/edit 在 malformed JSON、缺失/类型错误字段、fence-invalid path 或 checkpoint id 无效时也不得持久化 raw input。此时 `input_json` 精确投影为：`{"audit_version":"write_edit_invalid_v1","tool":"write|edit","raw_input_bytes":N,"raw_input_sha256":"<64 lowercase hex>","validation_error_code":"<stable code>"}`，不得含 raw path/body。hash 输入固定为 ASCII domain `vega.write-edit.invalid-input.v1\0` + `u64` big-endian raw JSON UTF-8 byte length + exact raw bytes。稳定 code 至少包含 `malformed_json|missing_path|wrong_path_type|missing_content|wrong_content_type|missing_old_string|wrong_old_string_type|missing_new_string|wrong_new_string_type|path_absolute|path_parent|path_symlink|path_hardlink|path_git|parent_not_found|checkpoint_id_invalid`。

invalid write/edit 在任何 RunMode/permission/execution 之前直接形成 terminal rejected row，approval 精确为 `{"decision":"deny","note":null,"source":"validation","danger":null}`；随后只发不含 raw 的 observable invalid tool_result。事件、错误 Display/Debug 与 SQLite 都不得出现 raw secret/path/body。startup recovery 对遗留 pending row 新写 `{"decision":"deny","note":null,"source":"recovery","danger":null}`；裸 once/always/deny 仅可读取 legacy，所有 S5 新写路径都禁止裸值。

terminal invalid row 重放时，重新验证/计算投影，只在 tool、raw_input_bytes、raw_input_sha256、validation_error_code 全等时复用同一 rejected result；任一 mismatch 即 call-id conflict，仍然零 permission/execution。
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
//         validation pre-gate → Rejected（原子插入终态，不产生权限等待）

pub enum PermissionMode { ReadOnly, Confirm, Auto }   // 对应 UI：只读/变更前确认/全自动
pub enum RunMode { Ask, Plan, Execute }               // 三模式（A2-09）

pub struct TokenUsage { pub input: u64, pub output: u64, pub cache_read: u64, pub cache_write: u64 }
pub struct Microcents(pub i64);   // 1/1_000_000 美元，杜绝浮点误差
```

S5 权限类型补充：

```rust
pub struct PermissionRequest {
    pub call_id: CallId,
    pub tool: String,                  // bash|write|edit
    pub display_target: String,        // bash 完整命令，或规范项目相对路径
    pub danger_rule_id: Option<String>,
    pub danger_reason: Option<String>,
}

pub enum PermissionDecision {
    Once,
    Always,
    Deny { note: Option<String> },
    Timeout,
}

pub struct ApprovalAudit {
    pub decision: Approval,             // once|always|deny
    pub note: Option<String>,
    pub source: ApprovalSource,
    pub danger: Option<DangerAudit>,
}

pub struct DangerAudit {
    pub rule_id: String,
    pub decision: Approval,
    pub note: Option<String>,
}

pub enum ApprovalSource {
    Danger, ReadOnly, RunMode, Rule, Auto, User, Timeout,
    Validation, ReadonlyTool, Recovery, Legacy,
}

pub struct InvalidWriteEditAudit {
    pub audit_version: String,          // exactly "write_edit_invalid_v1"
    pub tool: String,                   // write|edit
    pub raw_input_bytes: u64,
    pub raw_input_sha256: String,
    pub validation_error_code: String,
}

// 以下是跨边界 wire shape 示意，不改变 §1 依赖方向：
// T23 先在 vega_tools 提供 tools-local strict codec；
// T26 再由 vega_conversation 为 shared event/Store 建安全映射，runtime 不反向依赖 conversation。
pub struct WriteEditAuditWrite {
    pub audit_version: String,          // exactly "write_edit_v1"
    pub tool: String,                   // exactly "write"
    pub path: String,                   // normalized project-relative path
    pub content_bytes: u64,
    pub fingerprint_v1: String,         // exactly 64 lowercase hex chars
}

pub struct WriteEditAuditEdit {
    pub audit_version: String,          // exactly "write_edit_v1"
    pub tool: String,                   // exactly "edit"
    pub path: String,
    pub old_string_bytes: u64,
    pub new_string_bytes: u64,
    pub fingerprint_v1: String,
}

pub struct CreatedNewFileMetadata {
    pub metadata_version: String,       // exactly "preimage_v1"
    pub kind: String,                   // exactly "created_new_file"
    pub path: String,
}

pub struct WriteSuccessOutput {
    pub path: String,
    pub bytes_written: u64,
    pub checkpoint_ref: CheckpointRef,
}

pub struct EditSuccessOutput {
    pub path: String,
    pub bytes_written: u64,
    pub replacements: u64,              // exactly 1
    pub checkpoint_ref: CheckpointRef,
}

pub struct CheckpointRef(String);       // exact preimage-v1/<encoded project>/<thread>/<call>
```

- `PermissionRequest` 是 conversation→UI 的脱敏投影，write/edit 不得携带 content/old_string/new_string 正文；字节数只进入审计投影。
- S5 起 shared `ConversationEvent::ToolCallProposed.call.input_json` 与 Store 使用同一安全投影：read/glob/grep/bash 可保留完整输入，valid/invalid write/edit 只能携带 §2 的 `write_edit_v1|write_edit_invalid_v1` projection。runtime-local raw input 绝不直接映射到 UI 事件。
- runtime 为保持 headless 与依赖方向，持有 runtime-local permission facts/decision，由 `vega_conversation` 单向映射为上述共享 UI/Store 类型；这沿用 S4 已裁决的 `RuntimeEvent → ConversationEvent` 双层边界，不允许 UI 直接消费 runtime-local 类型。
- `PermissionMode` 与 `RunMode` 正交：前者决定 Execute 中的授权方式；Ask/Plan 的工具暴露限制不能通过切换 permission mode 绕过。
- 未知/损坏的 `permission_mode`、decision 或 approval JSON 必须 fail closed 并呈现错误，不可回退 Auto。
- `ApprovalAudit` 的持久化 JSON 必须恰好对应 §2 四字段形状；Rust 字段增加 `deny_unknown_fields` 等等价严格解码。legacy 裸值只在独立兼容分支接受，不能让宽松 JSON 解码掩盖损坏审计。
- `InvalidWriteEditAudit` 同样 strict encode/decode，且其 String 字段在构造前已按 §2 固定闭集/格式验证。raw provider JSON 只在当前调用内存用于 hash/验证，不进入 conversation shared event；UI/Store 只看到脱敏 projection 与稳定 error code。
- `WriteEditAuditWrite|Edit`、`CreatedNewFileMetadata`、`WriteSuccessOutput|EditSuccessOutput` 与 `CheckpointRef` 都是 strict codec 边界：实现须用 `deny_unknown_fields` 或等价机制，构造/反序列化时验证所有常量、path/hash/ref 与 u64；不能靠宽松 `Value` 取字段后忽略额外输入。T23 在 tools-local 完成 schema/roundtrip/negative tests；T26 负责把相同安全 wire shape 接到 shared event、DB、recovery 与 provider tool_result，任何边界损坏均 fail closed。

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
 -2. write/edit 输入能否解析且通过 path fence？
    → 否：持久化 write_edit_invalid_v1 projection + deny/validation，发脱敏 invalid tool_result 后终止本 call
 -1. thread.run_mode == Ask|Plan 且工具为 write/edit/bash？
    → source=run_mode 直接 Rejected；不进入 Execute permission engine，不弹 danger 卡
 0. read/glob/grep？→ 自动 Approved（仍走完整审计生命周期）
 1. danger 集中规则命中？
    → 是：先弹危险确认卡，默认焦点[拒绝]；deny/10 分钟超时立即 Rejected
    → once/always 只确认危险性，不得跳过下一步 ReadOnly；不再弹第二张 Confirm 卡
 2. thread.permission_mode == ReadOnly 且工具为 write/edit/bash
    → Rejected，原因注入 tool_result（若步骤 1 已确认，两个裁决都写入 approval JSON）
 3. 步骤 1 已获用户确认？→ Approved 本次；always 可存规则但下次 danger 仍回步骤 1
 4. permissions 表命中 project + tool + exact pattern？→ Approved
 5. permission_mode == Auto？→ Approved
 6. permission_mode == Confirm？→ 弹普通权限卡，等待用户；10 分钟超时 Rejected
 7. 每个裁决与状态推进先写 tool_calls；always 幂等写 permissions
```

validation step -2 是 permission 前置安全边界：invalid write/edit 不产生 PermissionRequest、不泄露 raw input、不执行，并在脱敏 terminal row 持久化完成后才把 invalid tool_result 加回 provider 上下文。capability step -1 随后拦 Ask/Plan 的 valid mutating call；Ask/Plan 只注册 read/glob/grep，hallucinated write/edit/bash 以 `source=run_mode` Rejected，不弹 danger 卡。danger-first 顺序只适用于具备 Execute 资格的 valid mutating calls。Execute 内步骤不可交换：rule/Auto 永远不能绕过 danger，危险卡批准也永远不能绕过 ReadOnly。

**Phase 1 exact rule**：`permissions.tool` 为 `bash|write|edit`；bash pattern 是原始完整 cmd，write/edit pattern 是围栏校验后的规范项目相对路径；字节级精确匹配，不折叠空白、不作 glob/regex。通配权限后置，必须另行 spec。

**danger 最低规则**：单一模块输出稳定 rule id/reason，至少覆盖 `rm -rf /` 的组合/拆分/换序选项、`git push -f|--force|--force-with-lease`、`dd ... of=/dev/...`、`mkfs*`、`diskutil eraseDisk|partitionDisk|secureErase`。规则可追加/加强；移除或弱化须先改 spec 并经人类批准。regex 只决定强制确认，不能代替 §4.4 的 OS 沙箱。

权限等待使用可取消的手工 BoxFuture/通道，不引 `async-trait`；生产超时固定 600 秒，测试可注入短时钟。卡消失、切线程、关窗与 cancel 都 fail closed；同一 call id 的重复 UI 决策只有第一次生效。

### 4.4 内置工具 I/O 契约（A3-05~08，摘要）

| 工具 | 输入要点 | 输出契约 |
|---|---|---|
| bash | cmd, timeout_ms(默认 120s) | stdout/stderr 合并；行/字节双上限；exit_code；cwd 强制=项目根 |
| read | path, offset, limit | 带行号；单行>2k 截断；二进制检测拒绝 |
| write | path, content | 写前备份到 checkpoint；成功返回 exact path/bytes_written/checkpoint_ref JSON |
| edit | path, old_string, new_string | 唯一匹配校验；成功返回 exact path/bytes_written/replacements=1/checkpoint_ref JSON；0 或多匹配=脱敏错误 |
| glob/grep | pattern, path? | 尊重 .gitignore；结果上限 500 条 |

#### 4.4.1 直接文件写工具

- write/edit 与 read 共用 project-root 围栏：绝对路径、`..`、canonical target 外跳均拒绝。read 可跟随仍在根内的 symlink；write/edit 对任一已存在 symlink 路径段都 fail closed。
- write/edit 对已有目标的 Unix link count `nlink > 1` 保守拒绝；hardlink 无法从 inode 安全恢复“来源路径”，不得猜测根内/外。已有文件在 checkpoint 后、替换前再验一次。
- 新文件只允许父目录已存在；canonicalize 最近存在祖先并逐段拒绝 symlink。最终写入使用目标同目录临时文件 + atomic rename。用户态 TOCTOU 残余风险仍按 tech-risks §4.5 记录。
- `.git`、worktree `.git` 文件指向的实际 gitdir 与 hooks 对 write/edit 全部只读。Git commit 只能走 S6 受信任交接，不向模型暴露绕过入口。
- write 目标存在时先复制原始字节到 `files/<relative_path>`，且 call-root `metadata.json` 必须 absent；目标不存在时不伪造 preimage，必须在创建目标前先原子写 §2 exact `created_new_file` metadata。edit 仅接受已有目标，call-root metadata 同样 absent；先按字节验证 old_string 恰好一次，0 次或多次均不改文件，并返回长度有界的周边上下文。
- checkpoint 失败时目标必须保持逐字节不变。checkpoint 内容、content/old/new 正文不得进入 error Display/Debug、tracing、事件或 SQLite。
- write/edit 的 content-free audit projection 与 `fingerprint_v1` 严格按 §2 生成；仓内 SHA-256 必须通过 NIST/标准公开向量以及字段顺序/长度分隔测试。恢复只在 DB tool、projection tool、规范 path、fingerprint 全等时复用，任何 mismatch 均为 conflict 且零执行。
- checkpoint 对外标识与成功 ToolOutput 严格按 §2：只传 opaque `preimage-v1/...` ref，不传真实 data root；write/edit success JSON 在 tools、runtime、ConversationEvent、Store、provider/UI 边界均 strict decode，缺/多/错字段、非法 u64/replacements/ref 必须 fail closed。UI 只消费安全成功/失败投影，不接触 raw input、checkpoint path 或 preimage。
- 解析/字段/path fence 任一失败时改走 §2 `write_edit_invalid_v1` 投影；计算 exact raw JSON hash 后立即丢弃 raw input，持久化 deny/validation terminal row。invalid tool_result 只含 tool + stable code（例如 `Tool error: invalid write input (malformed_json)`），不得回显 path、body 或原 JSON，也不得进入 permission hook/dispatcher。

#### 4.4.2 bash

- bash 仅接收 `cmd` 与可选 `timeout_ms`；调用方不存在 cwd 参数。timeout 缺省 120_000ms，0 与不可表示值拒绝。执行固定为 `/bin/zsh -lc`，cwd 固定 canonical project root，无 PTY。
- 所有生产 bash 都由 `/usr/bin/sandbox-exec` 启动。workspace-write profile 基线 deny `file-write*`，只放行 project root 与当次 Vega-owned temp exact subpath，再 deny `.git` 与实际 gitdir；禁止 broad-allow 共享 `/private/tmp`，网络按 tech-risks §4 workspace-write 档开放。sandbox-exec 缺失/profile 自测失败必须 fail closed，禁止裸 shell。
- 每个 bash call 在 spawn 前以独占创建方式于 canonical `/private/tmp` 下建立不可预测的专用目录，权限必须收紧为 0700；记录其初始 dev/inode，并在 profile 参数化前验证 `symlink_metadata` 为目录、不是 symlink、canonical path 仍位于 `/private/tmp`、dev/inode 未变。`TMPDIR`、`TMP`、`TEMP`、`TEMPDIR` 全设为此 exact path；不得把真实路径写入 tool wire、output、event、SQLite 或普通错误文本。
- Seatbelt 是 path-based，不能阻止任一可写根内预存 hardlink 修改其他路径的同一 inode。每次 spawn 前必须对 canonical project root 与专用 temp dir 执行 no-follow 扫描：project 覆盖 hidden/ignored entry，只跳过 profile 已强制只读的 `.git` entry 与已发现实际 gitdir；temp dir 不跳过任何 entry。任一普通文件 Unix `nlink > 1`，或目录遍历、`symlink_metadata`/metadata 读取失败，均以 hardlink preflight failure 终止且不得创建子进程。不得用 canonicalize 跟随 entry symlink 做此扫描。
- 成功、命令失败、cancel、timeout 均在 child 完成并 reap 后清理专用 temp dir。cleanup 必须锚定创建时记录的 canonical root/dev/inode，递归时不跟随 symlink；根身份/containment 不符时禁止递归删除。cleanup 失败返回脱敏 tool failure，路径留待后续安全 GC，不得暴露绝对 temp root 或放宽 profile。pre-spawn 任一步失败时同样尝试上述安全 cleanup，但仍保证零 command spawn。
- dual-root 扫描只关闭 launch 时已存在的 hardlink；外部并发进程在 scan 后、sandboxed command 打开文件前创建或替换 hardlink 仍是用户态 TOCTOU 残余。Phase 1 接受并在报告列明；不得据此宣称 inode 级 100% containment，也不得跳过扫描。
- 子进程用 `std::os::unix::process::CommandExt::process_group(0)` 建独立 process group；取消/超时以系统 `/bin/kill` 向负 PGID 发 SIGTERM，短 grace 后 SIGKILL，并 wait/reap。测试须证明 shell 与仍继承该 PGID 的 descendants 均退出；主动调用 `setsid` 的后代可逃逸，是 tech-risks §3.5 已知残余，不得宣称 100% process-tree containment。不得为信号引入白名单外 `libc`/`nix`。
- shell 内 `exec 2>&1` 合并 stdout/stderr；用固定 16 KiB chunk 流式读取，禁止取消不安全/无界的 `read_to_end`、`read_to_string`、`wait_with_output`。单条 rendered line 最多 65,536 bytes（含稳定 middle marker）；整体同时限制 head/tail 各最多 2,000 行与各 4 MiB rendered bytes，所有 line/output marker 均计入预算；tail 为有界 ring，总 retained output ≤8 MiB。
- 峰值 owned payload 上界为 8 MiB retained + 64 KiB current-line + 16 KiB read chunk + 常数级索引/marker；测试用内部 high-water 计数断言，不用 RSS 推测。多 MiB 无换行输入也必须保持此上界，并用稳定 replacement 规则避免截断产生无效 UTF-8 展示。返回 `exit_code`、`duration_ms`、`truncated`；S5 不落完整输出文件，`output_full_path` 保持 NULL。
- ReadOnly/Ask/Plan 的 bash 只读白名单在 Phase 1 是空集；read/glob/grep 提供只读能力。未来若开放 bash 只读命令，须先定义语法与混淆绕过测试。

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
- 数据：macOS `~/Library/Application Support/ai.vega/`：`vega.db`（SQLite）、`pricing.json`（S7 定价表：内置 deepseek/gpt/claude 主流价格，用户可加自定义模型）、`checkpoints/`（S5 write/edit preimage）；Linux（Phase 4）同构换 XDG 数据根。
- S5 preimage id 规则：project/thread/call id 的原始 UTF-8 长度必须为 1..=120 bytes；编码为 `id-` + 每个原始 byte 的 lowercase hex，得到 5..=243-byte collision-free path component。空值/超长值直接拒绝，禁止把 provider 原串 join。
- S5 preimage 布局：`checkpoints/<encoded-project>/<encoded-thread>/<encoded-call>/files/<relative_path>`；call root 的 `metadata.json` 是唯一 reserved 控制文件。它只在目标原先不存在的 write 中、创建目标前原子落盘，内容 exact 为 `{"metadata_version":"preimage_v1","kind":"created_new_file","path":"<normalized-relative>"}`；existing-target write/edit 的 metadata 必须 absent，preimage 只在 files/ 下。用户目标永远位于 files 下，因此名为 `metadata.json` 或 `created-new-file` 也不碰撞。checkpoint root 由 conversation/app 构造后注入 tools；tools 不依赖 store。该目录不是可浏览/可回退的产品 Checkpoint，A5-07/A5-08 仍属 Phase 2。
- S5 `checkpoint_ref` exact syntax 为 `preimage-v1/<encoded-project>/<encoded-thread>/<encoded-call>`；它是 content-free、相对、opaque 的 wire value，禁止绝对 data root、`.`/`..`、raw id、错误前缀/段数/hex。tool/runtime/provider/UI 不得暴露或推导底层 checkpoint 路径。
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
| 单元 | runtime 循环（mock provider 回放 SSE 脚本）、权限决策矩阵全组合、danger 正反例、edit 唯一匹配、markdown 切块 | cargo test |
| 集成 | 六表 DDL CRUD、0001→0002 保数据迁移、中断恢复、write/edit preimage、Plan 审批恢复、bash process-group 收拢 | tempfile 库 |
| Golden | 10 个真实 SSE 录制样本 → 事件序列断言 | insta snapshot |
| UI | 权限卡交互、滚动锚定 | #[gpui::test] |
| 性能 | P1-P8 准线 | xtask bench |

S5 必测矩阵与安全语料：

- Ask/Plan hallucinated mutating call 在 capability step -1 以 run_mode 拒绝且不弹 danger 卡；Execute 再覆盖三种 PermissionMode × write/edit/bash × danger/non-danger × rule/no-rule × once/always/deny/timeout，包含 danger+ReadOnly 双裁决、danger+Auto/rule 仍弹卡、重复决策单终态。
- approval JSON 精确四顶层字段/三 nested danger 字段；legacy 裸 once/always/deny 归一且不重写、所有新写路径永不发裸值；startup pending recovery 精确写 deny/recovery/danger=null；缺/多字段、未知 enum、损坏 JSON fail closed。
- absolute/`..`/不存在尾段/根内与外跳 symlink/hardlink/`.git`/worktree gitdir；checkpoint id 的 0/120/121-byte 边界、hex collision-free 与 files/metadata 隔离；new-file metadata 在目标创建前原子落盘且 exact，existing-target write/edit metadata absent、preimage 只在 files/；checkpoint 失败时原文件逐字节不变。
- SHA-256 公开向量、domain/length-prefix 字段边界；valid write/edit audit exact shape 与正文缺席；recovery 的 DB tool/projection tool/path/fingerprint 任一 mismatch 都 conflict 且零执行。
- checkpoint ref 与 write/edit success output 的 exact roundtrip；对全部 strict schema 做 missing/extra/wrong-type、负数/小数/u64 overflow、错误常量、非法 path/lower-hex/ref negative tests。T23 覆盖 tools-local codec/metadata 原子性；T26 覆盖 runtime→ConversationEvent→Store→recovery/provider wire roundtrip；T27 只消费安全成功/失败投影；T29 端到端证明 DB/event/provider/UI 无正文、raw id 或绝对数据根。
- malformed JSON（含 secret-like 文本）、write/edit 缺失/错误类型字段、absolute/`..`/symlink-invalid path：断言 `write_edit_invalid_v1` exact shape、raw byte hash/error code deterministic、raw path/body 在 DB/event/error/tool_result 全 absent、approval=deny/validation/danger=null，且零 permission/execution。
- bash 默认/自定义 timeout、4001+ 行头尾、多 MiB 无换行、64 KiB line/8 MiB retained/high-water、stdout/stderr 合流；cancel/timeout 后 shell 与继承 PGID descendants 退出。共享 `/private/tmp` 不可写；专用 temp 可写且四个 temp env 精确指向它。预存 project→outside 与 temp→outside hardlink、hidden/ignored hardlink、dual-root scan traversal/metadata failure 必须在 spawn 前拒绝并以 test-only spawn probe 证明零子进程；普通单链接文件不误拒。success/failure/cancel/timeout 与 pre-spawn reject 均清理；symlink entry 不被跟随，根被替换时禁止递归清理且错误脱敏。setsid escape 与 dual-scan 后竞态作为已知残余记录，不伪造通过；sandbox/profile/temp-root 任一步不可用时 fail closed。
- Plan 重启仍 pending，approve 后才首次执行，change/abandon 不预执行；0002 CHECK 拒绝非法值、corrupt read fail closed。连续/重启/并发 completion 后仅最后提交者 pending，旧项 abandoned+superseded+reviewed_at；completion-vs-old-approve 两种提交顺序分别断言旧 approval=0 或 completion fail closed；新 plan 写失败时 supersede 同事务回滚。审批竞态只有 affected-rows==1 的赢家能切 mode/插消息；新旧 DB 均恰好六表。
- 权限卡 GPUI 测普通/危险两套焦点与键盘；危险卡 Tab/Shift+Tab wrap、Space 激活三种焦点、bare Enter 在全部焦点始终 deny、Cmd+Enter always、Esc deny；另测附言、重复提交、超时、切线程/关窗 fail closed。不能自动化的 ui-spec §6 项逐项记录人工走查证据。

S5 的“checkpoint”测试只验证修改前 preimage；Checkpoint 列表、工作区快照与回退属于 Phase 2，禁止用本节把它们提前纳入实现。

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
- v0.3 (2026-08-30) S5 SDD 契约闭合：§2 固定六表 add-only 的 0002 Plan CHECK/事务赢家/旧计划 supersede、严格 approval/recovery JSON、valid fingerprint_v1 与 invalid input hash projection；§3 补权限/invalid audit UI 投影与双层 headless 边界；§4.3 增 validation step -2、RunMode capability step -1，并固定 Execute 的 danger→ReadOnly→rule→Auto→Confirm 顺序；§4.4 定稿 write/edit preimage/围栏/脱敏验证、SHA-256 recovery identity，以及 Seatbelt bash 的行/字节双上限、process-group 与 setsid 残余；§6 固定 checkpoint id 编码/files+metadata 布局并区分 Phase 2 Checkpoint；§8 增加权限、路径、recovery、bash、Plan supersede 与 UI 验收矩阵。
- v0.4 (2026-08-30) 人类批准 S5 strict wire schema：§2/§3 固定 valid `write_edit_v1`、created-new-file metadata、opaque `checkpoint_ref`、write/edit success output 与 strict codec 类型；§4.4/§6 固定 metadata 落盘时序、existing-target absence 与只传 ref 的边界；§8 分配 T23/T26/T27/T29 roundtrip/negative/e2e 验收。
- v0.5 (2026-08-30) 人类批准 T24 hardlink 补充契约：§4.4.2 固定 bash spawn 前 no-follow 扫描可写树，普通文件 `nlink > 1` 或扫描失败均 fail closed/零 spawn；§8 增预存/hidden/ignored hardlink 与扫描失败测试，并明确 scan 后并发替换为 Phase 1 TOCTOU 残余。
- v0.6 (2026-08-30) 人类批准 T24 temp-root 补充契约：§4.4.2 移除 broad `/private/tmp` allow，固定每 call 0700 exact temp subpath、四 temp env、project/temp dual-root scan 与 reap 后身份校验/no-follow cleanup；§8 增 shared tmp 拒绝、temp hardlink 零 spawn、全终态 cleanup 与根替换保护。
