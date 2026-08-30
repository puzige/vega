# ✦ Vega — S5 任务卡（Sprint 5 · 写工具 + 权限门禁 + 三模式 · W9-10）

**版本** v0.2 · 2026-08-30 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt

**S5 目标**（phase1-plan §2）：write/edit/bash 工具；权限状态机（只读/变更前确认/全自动）；权限确认 UI；危险命令拦截；Ask/Plan/Execute 三模式 + Plan 产物审批流。

**Sprint DoD**：默认 Confirm 模式下，无 matching exact Always rule 的 write/edit/bash 均进入待批准态；危险命令即使有 rule/Auto 仍强制确认；Plan 产物落库且只有批准后才进入 Execute；全部工具生命周期与裁决可审计。

> 本文档合入即为 S5 的 SDD 开工门禁。代码不得先于本文及同一 PR 对 tech-spec §2/§3/§4.3/§4.4/§8、ui-spec §4.3 的契约回写。
>
> **人类裁决（2026-08-30）**：普通权限卡 `Enter=允许一次`；危险命令卡默认焦点为「拒绝」，Tab/Shift+Tab 遍历、Space 激活焦点按钮，但 bare `Enter` 无论焦点均拒绝；两类卡均为 `Cmd+Enter=总是允许`、`Esc=拒绝`。

---

## S5 最小契约闭合

### C1 · checkpoint 是文件 preimage，不是 Phase 2 回退系统

- S5 只实现 write/edit 修改前的单文件 preimage 备份；Checkpoint 列表、工作区快照、回退按钮仍属 A5-07/A5-08 Phase 2。
- 根目录沿用 tech-spec §6 数据根。project/thread/call id 各自拒绝空值或超过 120 UTF-8 bytes；合法值编码成 `id-` + 原始 UTF-8 bytes 的 lowercase hex（最长 243-byte component），禁止把原串直接 `join`。
- 布局固定为 `checkpoints/<encoded-project>/<encoded-thread>/<encoded-call>/files/<relative_path>`，reserved metadata 固定在 call root 的 `metadata.json`。existing-target write/edit 的 `metadata.json` 必须 absent，原始 preimage 只能位于 `files/`。仅当 write 的目标原先不存在时，才在创建目标前先原子落盘 exact JSON `{"metadata_version":"preimage_v1","kind":"created_new_file","path":"<normalized-relative>"}`；缺字段、多字段、错误常量或错误类型均拒绝。用户文件即使名为 `created-new-file`/`metadata.json` 也只能位于 files 下，不与控制文件碰撞。
- checkpoint 根由 conversation/app 构造后注入 `vega_tools::Tools`；`vega_tools` 不反向依赖 store。测试显式注入 tempfile 路径，不碰真实数据根。
- checkpoint 标识是 content-free、项目数据根相对且 opaque 的 strict ref，精确语法为 `preimage-v1/<encoded-project>/<encoded-thread>/<encoded-call>`；拒绝绝对路径、`..`、raw id、错误段数/前缀/编码。tool/runtime/provider 只传该 ref，禁止暴露绝对 checkpoint/data root。
- 备份内容不进入日志、事件或 SQLite。write 成功 `ToolOutput.text` exact JSON 为 `{"path":"<normalized-relative>","bytes_written":N,"checkpoint_ref":"..."}`；edit 成功 exact JSON 为 `{"path":"<normalized-relative>","bytes_written":N,"replacements":1,"checkpoint_ref":"..."}`。`N` 必须是 u64，missing/extra/wrong-type、错误 replacements 常量或非法 ref 均拒绝；失败结果只含稳定、脱敏投影。

### C2 · read/write/edit 共用路径围栏，写工具更严格

- 所有路径以 canonical project root 为基准；共同拒绝绝对路径、`..` 与 canonical target 跳出根目录。read 可跟随仍落在根内的 symlink，跳出根的 symlink 必须拒绝。
- write/edit 对每个已存在路径段额外拒绝 symlink；已有目标 `nlink > 1` 时保守拒绝（hardlink 的来源路径无法从 inode 安全判定，因此不区分根内外）。
- 新文件先 canonicalize 最近存在祖先并验证仍在根内；父目录必须已存在。写工具在 checkpoint 后、替换前再次校验；使用同目录临时文件 + 原子 rename。用户态 TOCTOU 残余风险保持 tech-risks §4.5 的明确记录，不虚称消除。
- write/edit 拒绝项目 `.git`、worktree `.git` 指针指向的实际 gitdir及其 hooks；Git 提交由 S6 的受信任交接路径完成。
- 不引 `cap-std`。若 std 实现无法达到上述确定性测试，按停止条件上报，不缩减测试。

### C3 · permissions 的 Phase 1 pattern 为精确签名

- `permissions.tool` 只取 `bash|write|edit`；`pattern` 不重复 tool 前缀。
- bash pattern = 原始完整命令；write/edit pattern = 围栏校验后的项目相对规范路径。比较为字节级精确相等，不折叠空白、不做 glob/regex。
- always 只对同 project + 同 tool + 同签名生效；危险命令每次仍先强制确认，persisted rule 永远不能越过 danger 步骤。
- tech-spec §2 的 wildcard 示例改为精确语义；通配规则后置且必须另写 spec。

### C4 · ReadOnly/Ask/Plan 的 bash 只读白名单为空

- read/glob/grep 是 Phase 1 唯一只读工具集；bash 一律归写类。Execute+ReadOnly 的 bash/write/edit 在 danger gate 之后拒绝，因此 rule/Auto/危险卡批准均不能绕过 ReadOnly。
- Ask/Plan 只向 provider 注册 read/glob/grep；provider 若仍返回 bash/write/edit，在进入 Execute permission engine 之前以 `source=run_mode` 拒绝并产生可审计 denied tool_result。此能力门禁不弹 danger 卡，因为该 call 根本不具备 Execute 资格。
- Execute 才注册完整六工具，再按 permission_mode 决策。
- 不用 shell 字符串解析伪造“只读命令”。未来白名单必须先补命令语法与绕过测试 spec。

### C5 · danger 规则集中且不可被弱化，Seatbelt 是 bash 真围栏

- 危险规则只放一处（`vega_tools::danger`），每条有稳定 rule id/reason；runtime/UI 不复制正则。规则可追加或加强，移除/削弱须先改 spec 并经人类批准。
- 最低覆盖：根递归删除（`rm -rf /`，含组合/拆分/换序选项与空白变体）、forced push（`git push -f|--force|--force-with-lease`）、raw device write（`dd ... of=/dev/...`）、`mkfs*`、`diskutil eraseDisk|partitionDisk|secureErase`；每条正反例单测。
- danger regex 只负责强制确认，不冒充沙箱。所有 bash 必须经 `/usr/bin/sandbox-exec`：默认 deny `file-write*`，只放行项目根与 `/private/tmp`，再 deny `.git` 与实际 gitdir；按 tech-risks workspace-write 档开放网络。
- `/usr/bin/sandbox-exec` 缺失、profile 自测失败或进程组无法可靠收拢时 fail closed，禁止裸 shell降级。
- bash 使用 `/bin/zsh -lc`、cwd 强制项目根、无 PTY；`CommandExt::process_group(0)` 建独立组，取消/超时通过 `/bin/kill` 向负 PGID 发 SIGTERM，grace 后 SIGKILL，最后 wait/reap。验收包含仍继承该 PGID 的 descendants；主动 `setsid` 脱离进程组是 tech-risks §3.5 的已知残余，不虚称 100% tree containment。不得为此引入 `libc`/`nix`。

### C6 · 裁决顺序与审计投影

- capability step -1：Ask/Plan 只允许 read/glob/grep；hallucinated mutating call 先以 run_mode 拒绝，不进入下述 Execute permission engine。
- Execute 顺序固定：danger 检测/强制用户裁决 → ReadOnly 写类拒绝 → dangerous user approval 完成本次批准 → exact persisted rule → Auto → Confirm prompt；read/glob/grep 自动批准但仍审计。
- danger deny/timeout 立即拒绝；danger allow 后仍必须经过 ReadOnly，因此 ReadOnly 不可被危险卡批准绕过；非 ReadOnly 不再弹第二张 Confirm 卡。danger always 可持久化，但下次仍重复 danger 强制确认。
- `tool_calls.input_json` 对 read/glob/grep/bash 保留完整输入；valid write 只持久化 exact JSON `{"audit_version":"write_edit_v1","tool":"write","path":"<normalized-relative>","content_bytes":N,"fingerprint_v1":"<64 lower hex>"}`；valid edit 只持久化 exact JSON `{"audit_version":"write_edit_v1","tool":"edit","path":"<normalized-relative>","old_string_bytes":N,"new_string_bytes":N,"fingerprint_v1":"<64 lower hex>"}`。字段顺序无语义，所有整数必须是 u64；missing/extra/wrong-type、错误常量、非法 path/hash 均拒绝。正文只存在于当前执行内存。
- `fingerprint_v1` 固定为 SHA-256 小写 64 字符十六进制（256-bit）。输入为 domain `vega.write-edit.fingerprint.v1\0`，随后依次编码 tool、规范项目相对 path，以及 write 的 content 或 edit 的 old_string/new_string；每个字段均为 `u64` big-endian 字节长度 + 原始 UTF-8 bytes。实现使用仓内 safe Rust 与公开 SHA-256 test vectors，不新增依赖。
- recovery 对当前 provider call 重新计算 fingerprint，严格解码 projection，并同时比较 DB `tool` + projection `tool`、规范 path 与 fingerprint；任一不符即 call-id conflict，绝不执行或复用。raw content 永不持久化，也不为恢复例外放宽。
- malformed JSON、缺失/错误字段或 fence-invalid write/edit 也必须有 content-free projection，精确字段为 `audit_version="write_edit_invalid_v1"`、tool、raw_input_bytes、raw_input_sha256、validation_error_code；不得包含 raw path/body。hash 固定为 SHA-256(domain `vega.write-edit.invalid-input.v1\0` + `u64` big-endian raw byte length + exact raw JSON UTF-8 bytes)，同样输出小写 64 字符 hex。
- validation_error_code 是稳定闭集，至少区分 `malformed_json|missing_path|wrong_path_type|missing_content|wrong_content_type|missing_old_string|wrong_old_string_type|missing_new_string|wrong_new_string_type|path_absolute|path_parent|path_symlink|path_hardlink|path_git|parent_not_found|checkpoint_id_invalid`。invalid projection 直接持久化 terminal rejected，approval 为 deny/validation/danger=null；发可观察但不含 raw 的 invalid tool_result，永不进入 RunMode/permission/execution。
- terminal invalid row 重放时仅在 tool + raw_input_bytes + raw_input_sha256 + validation_error_code 全等时复用同一 rejected result；任何 mismatch 都是 call-id conflict，仍不得执行。
- approval JSON 精确为 `{"decision":"once|always|deny","note":null|string,"source":"…","danger":null|object}` 四个顶层字段；非 null danger 精确为 `{"rule_id":"…","decision":"once|always|deny","note":null|string}`。danger+ReadOnly 的顶层为 deny/readonly，nested danger 保留 once|always；danger deny/timeout 的顶层为 deny/danger|timeout，nested decision=deny。
- decision 只取 once|always|deny；source 只取 `danger|readonly|run_mode|rule|auto|user|timeout|validation|readonly_tool|recovery|legacy`。历史裸 `once|always|deny` 读取时内存归一为同 decision、null note、source=legacy、danger=null，不重写历史行，也绝不新写裸值；未知字段形状或值 fail closed。
- 生命周期：正常 call 为 pending_approval → approved → running → success|failed，或 pending_approval → rejected；running → cancelled。validation pre-gate 是唯一直接原子写 terminal rejected 的例外，不产生权限等待。每个关键状态先落库再发 UI 事件。bash 只持久化满足 C6.1 双上限的展示输出，S5 的 `output_full_path` 为 NULL。

### C6.1 · bash 输出同时受行数与字节数约束

- stdout/stderr 合流后以固定 16 KiB chunk 读取；禁止 `read_to_end`/`wait_with_output`。当前行只保留可渲染的头尾，总长度上限 64 KiB（65,536 bytes，含稳定的 line-middle marker）。
- 整体同时满足最多 head 2,000 + tail 2,000 行，以及 head 4 MiB + tail 4 MiB rendered bytes；line/output markers 都计入对应字节预算，任一先到即截断。tail 使用有界 ring，总 retained output 不超过 8 MiB。
- 实现的峰值 owned payload 上界为 8 MiB retained + 64 KiB current-line + 16 KiB read chunk + 常数级索引/marker；测试接口暴露或内部断言 high-water，不用 RSS 猜测。
- 必测多 MiB 且完全无换行的输出，断言单行 marker、最终 payload/行数上限及 high-water 上界；截断不能破坏 UTF-8 展示（无效字节按稳定 replacement 规则渲染）。

### C7 · Plan 用 messages 增列，不建第七表

- 新增递增迁移 `0002_plan_review.sql`，只给 messages 增 `plan_status`、`plan_review_note`、`plan_reviewed_at` 三个 nullable 列；plan_status 带 `CHECK (plan_status IS NULL OR plan_status IN ('pending','approved','changes_requested','abandoned'))`。
- 完成一个新 Plan 时，事务先确认 thread.mode 仍为 plan；若旧计划 approve 已抢先切到 execute，则本次 completion fail closed。确认后把该 thread 除 current_message_id 外所有现存 pending plan 更新为 abandoned、`plan_review_note='superseded'`、同一 `plan_reviewed_at`，再插入/标记 current plan pending；后一步失败则整笔回滚，不留下“旧计划已废弃但新计划缺失”。
- Plan 最终 assistant message 为 `kind='plan'`、`status='done'`、`plan_status='pending'`；读取到不合法组合/值 fail closed。审批更新必须带 `WHERE id=? AND kind='plan' AND plan_status='pending'`，且 transaction 观察 affected rows == 1 后才允许 mode change/user message，防重复/旧计划审批。
- 批准在同一事务先完成上述 conditional update，确认恰好一行后才把 thread.mode 置 execute、插入可审计 user 指令；要求修改/放弃同样先夺取 pending 单次终态，再执行各自后续写入。
- plan_status 只取 `pending|approved|changes_requested|abandoned`；不复用 messages.status，不滥用 tool_calls。0001 保持不可变，0002 只增列，新旧 DB 均恰好六表。

---

## 卡依赖图

```text
S5 SDD PR
  ├─▶ T23 write/edit + preimage + fence
  └─▶ T24 bash + Seatbelt + danger
          └─▶ T25 permission engine + persistence
                  └─▶ T26 runtime/conversation wiring + audit
                          ├─▶ T27 tool/permission cards
                          └─▶ T28 Ask/Plan/Execute + Plan review
                                  └─▶ T29 end-to-end acceptance + report
```

> 工作流仍严格串行：每卡独立 sibling worktree 与 PR，squash merge 后才开下一卡。

## T23 · write/edit + preimage checkpoint + 写路径围栏（A3-07）

- **前置/参考**：S4 T22 + S5 SDD PR；tech-spec §4.4；exec-guide §3；tech-risks §4.3；C1/C2/C6。
- **范围**：`vega_tools` 的 write/edit/fence/checkpoint/error/output/tools；仅复用白名单依赖，不动 runtime/conversation/UI/DDL。
- **产出**：write 支持新建/覆盖，checkpoint 后同目录原子替换；edit 按字节唯一匹配，0 或多匹配失败并返回有限周边上下文；父目录不隐式创建；工具结果不泄露正文；仓内 safe Rust SHA-256 fingerprint_v1/invalid raw-input hash 与公开 test vectors；tools-local strict codec 覆盖 `write_edit_v1`、created-new-file metadata、checkpoint ref 与 write/edit success output。
- **安全验收**：覆盖绝对路径、`..`、不存在尾段、外跳/根内 symlink、hardlink、`.git`/worktree gitdir；checkpoint id 的空/121-byte 拒绝、120-byte 编码恰为 243-byte component、不同 UTF-8 id 不碰撞、用户 `metadata.json` 路径隔离；checkpoint 先于修改，失败目标不动；new-file metadata 在目标创建前原子落盘且 exact，existing-target write/edit metadata absent、preimage 只在 files/；checkpoint ref 精确为 `preimage-v1/<encoded-project>/<encoded-thread>/<encoded-call>` 且拒绝 absolute/`..`/raw id；valid/invalid projection、成功/失败 output 均不含 raw content/path 或绝对数据根。所有 strict JSON 做 roundtrip，并对 missing/extra/wrong-type、负数/小数/u64 overflow、错误常量、非法 lower-hex/ref 做 negative tests；hash 对 byte/字段边界敏感且 error code 稳定。
- **命令**：`cargo test -p vega_tools`；`cargo clippy -p vega_tools --all-targets -- -D warnings`。
- **commit**：`feat(A3-07): add fenced write and edit tools`（≤3 commits）。
- **禁区/停止**：不做 checkpoint 浏览/回退，不创建表，不放宽 T21 read fence；若 std 无法满足围栏验收则 `[BLOCKED] S5-T23`。

## T24 · bash + Seatbelt + danger matcher（A3-05/A3-09）

- **前置/参考**：T23；tech-spec §4.3/§4.4；tech-risks §3/#4；C4/C5/C6。
- **范围**：`vega_tools` 的 bash/danger/sandbox/output/tools；只激活既有 tokio/tokio-util/regex，无 PTY/白名单外 crate。
- **产出**：cmd + timeout_ms（默认 120_000，0/溢出拒绝）；cwd 只能为 canonical project root；16 KiB 流式合流；64 KiB 单行、head/tail 各 2,000 行与各 4 MiB 三重上限；exit_code/duration/truncated；process-group TERM→KILL→reap；Seatbelt fail closed；集中 danger rule id/reason。
- **验收**：`cargo test -p vega_tools bash`；`cargo test -p vega_tools danger`；`cargo clippy -p vega_tools --all-targets -- -D warnings`；macOS 集成覆盖 cwd、timeout、4001+ 行、多 MiB 无换行/high-water、合流、cancel/timeout 后 shell 与继承 PGID descendants 均退出、项目外/.git 写失败、项目内普通写成功、危险正反例；setsid 逃逸只记录残余，不伪造全树断言。
- **commit**：`feat(A3-05): add sandboxed cancellable bash tool`；必要时 `feat(A3-09): centralize dangerous command rules`。
- **禁区/停止**：不做 PTY/A6/SSH；不以 regex 替代 Seatbelt；sandbox-exec/profile/process-group 环境不可用则 `[BLOCKED] S5-T24`。

## T25 · 权限纯引擎 + permissions 持久化（A3-09/A11-04）

- **前置/参考**：T23/T24；tech-spec §3/§4.3；C3-C6。
- **范围**：runtime permission 纯逻辑、store permissions/tool_calls/**recovery.rs**、conversation shared permission types；不改 UI/DDL。
- **产出**：Ask/Plan capability step -1 与 Execute 固定顺序的纯决策引擎；runtime-local facts/decision 经 conversation 单向映射给 UI，不反转依赖；permission request 不携带 write/edit 正文；permissions exact list/insert/match、重复 always 幂等、project 隔离；approval JSON 严格四字段/嵌套 danger 形状并兼容 S4 裸值；未知 mode/JSON fail closed。
- **验收**：`cargo test -p vega_runtime permission`；`cargo test -p vega_store permissions`；`cargo test -p vega_store recovery`；`cargo test -p vega_conversation`；覆盖 3 modes × 3 mutating tools × danger/non-danger × rule/no-rule 与 once/always/deny/timeout，并含 danger+ReadOnly、四字段序列化/legacy 解码/损坏 JSON；startup pending row 必须新写 deny/recovery/danger=null 严格 JSON，绝不再发裸值。
- **commit**：`feat(A3-09): implement ordered permission decisions`；可拆 `feat(A11-04): persist exact project permission rules`。
- **禁区**：不混淆 RunMode/PermissionMode，不做 wildcard，不允许 mutating dispatcher 绕过 gate。bash readonly whitelist 为空须进报告。

## T26 · Runtime/Conversation 写工具接线 + 全量审计（A3-03/A3-04/A3-12）

- **前置/参考**：T25；tech-spec §3/§4.2-§4.4；S4 双事件类型裁决；C1-C6。
- **范围**：runtime agent/provider/permission、conversation agent/types、store tool_calls/permissions/messages/**recovery.rs**；仅既有依赖，不改 UI。
- **产出**：Execute 注册六工具，Ask/Plan 只注册三只读工具；手工 BoxFuture permission hook，production timeout 600s（测试可注入）；状态先持久化后可见；denied/timeout 作为 tool_result 继续循环；write/edit checkpoint/fingerprint 隔离；shared event/DB/recovery 只承载 strict valid/invalid audit projection 与 strict success/failure output，checkpoint 只传 opaque ref；bash 元数据完整；取消待批立即拒绝、bash 收拢进程。
- **验收**：三个 crate 全测；mock 覆盖 Confirm once/always/deny/timeout、Auto、ReadOnly、danger in Auto + rule、danger+ReadOnly、write→edit→bash 串行、逐状态落库/正文脱敏；重启相同 fingerprint 复用，DB tool/projection tool/path/fingerprint 任一不同均 conflict；valid audit 与 success output 在 runtime→ConversationEvent→Store→recovery/provider roundtrip 保持 exact schema，missing/extra/wrong-type、非法 u64/hash/ref/常量 fail closed，事件/DB/provider 无正文、raw id 或绝对数据根；malformed secret-like JSON、缺/错字段、absolute/`..`/symlink-invalid 输入均写 deterministic invalid projection + rejected validation JSON，事件/DB/tool_result 无 raw 且零 permission/execution；startup recovery 新写严格 recovery JSON。
- **commit**：`feat(A3-04): register write edit and bash tools`；`feat(A3-12): persist permission-gated tool lifecycles`。
- **禁区**：不接真实 API/key，不并行同轮工具，不计价，不让 runtime 依赖 store/UI/GPUI；ConversationEvent 仍是 UI/Store 唯一事件流。

## T27 · 工具卡 + 权限卡（A2-05/A2-06/A2-07/A2-08）

- **前置/参考**：T26；ui-spec §4.2/§4.3/P5/P6/§6；顶部人类裁决。
- **范围**：vega_ui tool_card/permission_card/conversation_stream/text_input；只用 theme token，不直读 SQLite。
- **产出**：工具状态/耗时/折叠，bash 全命令等宽、输出默认收起、退出码；write/edit 卡只消费 strict 安全成功/失败投影，成功显示规范相对路径与字节/替换摘要，不展示 checkpoint ref，strict decode 失败呈 fail-closed 损坏结果；invalid write/edit 只显示 stable code 的 rejected card 且无权限按钮；warning 权限卡与拒绝附言；普通 Enter once；危险默认 Reject，Tab/Shift+Tab 循环焦点、Space 激活焦点按钮（含 Allow Once），但 bare Enter 无论焦点始终 deny；两者 Cmd+Enter always/Esc deny；scoped key context、重复提交幂等、卡消失/切线程/关窗 fail closed。
- **验收**：`cargo test -p vega_ui permission_card`；`cargo test -p vega_ui tool_card`；`cargo clippy -p vega_ui --all-targets -- -D warnings`；工具卡覆盖 write/edit strict success/failure shape、missing/extra/wrong-type、非法 u64/replacements/ref，断言 UI 无正文/raw id/绝对数据根；GPUI 测两套键位、危险卡全焦点位置的 Enter=deny、Tab/Shift+Tab wrap、Space 三按钮、附言、重复、超时、关闭；色值 grep 零新增。
- **commit**：`feat(A2-08): add fail-closed permission cards`；必要时 `feat(A2-05): render audited tool call cards`。
- **禁区**：不做 S6 完整 diff，不加模态/装饰动画/硬编码视觉值；危险卡 Enter 绝不允许。

## T28 · Ask/Plan/Execute + Plan 审批（A2-09/A2-10/A2-15）

- **前置/参考**：T27；tech-spec §2/§3/§4.2；ui-spec §4.4/§6；C4/C7。
- **范围**：`0002_plan_review.sql`、store messages/threads、conversation plans/agent/types、UI plan_card/composer；不引依赖。
- **产出**：0001→0002 保数据且仍六表；plan_status CHECK + corrupt-read fail closed；typed mode 更新并重启恢复；Ask/Plan 零写；新 Plan completion 原子 supersede 全部旧 pending 后才建立唯一最新 pending；批准/修改/放弃先 conditional update 且 affected rows==1 后才做后续事务写；批准后才切 Execute 并启动新 turn；Composer 补模式/权限控件、1~8 行与历史 ↑。
- **验收**：store、conversation plans、UI plan_card 测试；老库迁移/CHECK 拒绝/人工注入 corrupt row 的 fail-closed read；连续/重启/并发完成新 Plan 后只有最新 pending，旧项为 abandoned+superseded+reviewed_at且不可批准；新 plan 写失败时 supersede 回滚；completion 先赢则旧 approval=0，旧 approval 先赢并切 Execute 则 completion fail closed；approve/change/abandon 竞态只有赢家能改 mode/插 user message。
- **commit**：`feat(A2-09): persist Ask Plan Execute modes`；`feat(A2-10): add durable plan approval flow`；最多再 `feat(A2-15): expose thread permission modes`。
- **禁区/后置**：不新增 plans/artifacts 表，不做 Phase 2 rollback；@引用、/命令、模型选择器后置，分支选择器归 S6。

## T29 · S5 端到端验收 + 报告 + README（A3-12）

- **前置/参考**：T23-T28 均已 squash merge；phase1-plan S5 DoD、exec-guide §3/§7、ui-spec §6、tech-spec §8。
- **场景**：Confirm write once/checkpoint、edit always 后同 exact rule 不再弹卡但全程审计、bash deny(note)；new-file exact metadata 与 existing-target metadata absent/preimage、opaque checkpoint ref、valid audit/success output strict roundtrip，且缺/多/错字段与非法 u64/hash/ref fail closed；write/edit valid recovery identity 与 malformed/field/path-invalid secret-like 输入脱敏拒绝；startup pending recovery 写严格 JSON；Auto + rule + danger 仍弹；危险卡完整键盘；ReadOnly 三写经 danger gate 后拒绝；Plan 连续/重启/完成竞态 supersede 旧 pending，旧 plan 永不可批准，再测 approve/change/abandon 审批竞态；timeout/cancel/关窗 fail closed；bash 无换行内存有界且继承 PGID descendants 可收拢。
- **门禁**：fmt、clippy、workspace test/build；runtime/tools cargo tree headless；UI 色值、production unwrap/expect、六表、migration add-only、key/正文审计 scans。
- **报告**：`docs/vega-s5-report.md` + README；列出 SDD/T23-T29 PR 与 merge commit、原始门禁与测试数、S5 DoD、ui-spec §6、红线、偏离/后置、真实 key/费用/dogfood 未执行。
- **commit**：`feat(A3-12): close Sprint 5 with audited acceptance`。
- **禁区/停止**：mock 不冒充真实 API；未自动化 UI/P1-P8 必须如实标注；环境失败按 `[BLOCKED] S5-T29` 停止。

---

## S5 完成定义（DoD）

- [ ] SDD PR 先于代码；C1-C7 与危险卡键位已回写 tech/ui spec。
- [ ] T23-T29 均 squash merge；master 四门禁全绿。
- [ ] Confirm 中无 matching exact Always rule 的 write/edit/bash 均确认；rule 命中不弹卡但每次仍完整审计；once/always/deny/note/timeout 可审计。
- [ ] danger 先于 ReadOnly/rule/Auto，危险卡按人类裁决，集中规则未削弱。
- [ ] write/edit fence/preimage/唯一匹配/fingerprint recovery；new-file metadata、existing-target metadata absent、opaque checkpoint ref、valid audit 与 success output exact schema 全过；raw content/raw id/绝对数据根零持久化或传播；approval JSON 精确且 legacy fail closed。
- [ ] invalid write/edit 输入以 deterministic content-free projection + deny/validation 严格 JSON 终结；零 permission/execution；startup recovery 只新写 deny/recovery 严格 JSON。
- [ ] bash cwd/120s/16KiB streaming/64KiB 单行/头尾各 2k 行与 4MiB/process-group/Seatbelt 全过。
- [ ] 危险卡 Tab/Shift+Tab/Space 可达；bare Enter 在任意焦点均拒绝；Cmd+Enter/Esc 固定。
- [ ] Ask/Plan 零写；Plan 持久化且批准后才 Execute；change/abandon/重复审批正确。
- [ ] 新 Plan 原子 supersede 旧 pending；重启/竞态后仅最新可批，旧项带 stable superseded note。
- [ ] 仍恰好六表；0002 只增列；runtime/tools headless；UI 不直连 SQLite；ConversationEvent 唯一。
- [ ] ui-spec §6 逐项记录；P1-P8 不回退（120Hz 字面复测仍留 S8）。
- [ ] S5 报告/README 已更新，偏离与后置无隐瞒。

## 已知偏离与后置（原样进入 Sprint 报告）

1. ReadOnly 的 bash 只读白名单为空；只读能力由 read/glob/grep 提供。
2. permissions Phase 1 只做精确签名；wildcard 规则后置。
3. S5 checkpoint 仅文件 preimage；列表/工作区快照/回退仍属 Phase 2。
4. Composer 的 @引用、/命令、模型选择器后置；分支选择器归 S6。
5. 危险卡 `Enter=拒绝` 是 2026-08-30 人类安全裁决，对普通卡键位作 override。
6. 真实 LLM、账单对比与 dogfood 属人类活动；执行 agent 只准备路径。
7. process-group 可收拢 shell 与继承 PGID descendants；主动 `setsid` 逃逸是 tech-risks §3.5 已知残余，S5 不虚称 100% 树隔离。
8. fingerprint_v1 因依赖白名单采用仓内 safe Rust SHA-256；必须以公开向量与边界测试守护，后续若改用外部加密 crate 仍须人类批准。

## 未决阻塞检查

- 当前无未决 spec 阻塞；hardlink 采用 `nlink > 1` 保守拒绝，process-group 信号固定走系统 `/bin/kill`，均不引新依赖。
- T24 开工先实测 sandbox-exec profile 与 process-group；只能 fail closed，不能裸 bash 降级。

## 变更记录

- v0.1 (2026-08-30) S5 开工 SDD：T23-T29、C1-C7、安全红线与 DoD 定稿；contract review 随同一开工 commit 补定 valid/invalid content-free fingerprints、approval/recovery 严格 JSON、bash 字节/行双上限、危险卡完整键盘语义、RunMode capability 前置门禁、Plan CHECK/事务赢家/旧计划 supersede、checkpoint id/layout 与 setsid 残余。
- v0.2 (2026-08-30) 人类批准 schema 补充：固定 valid `write_edit_v1`、created-new-file metadata、opaque `checkpoint_ref` 与 write/edit success output 的 exact JSON；补齐 T23 tools-local codec、T26 shared event/DB/recovery、T27 安全投影 UI、T29 e2e 与 DoD 的 strict negative tests。
