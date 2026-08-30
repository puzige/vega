# ✦ Vega — S6 任务卡（Sprint 6 · Diff 审阅 & 产物 · W11-12）

**版本** v0.13 · 2026-08-31 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt

**S6 目标**（phase1-plan §2）：git 工作区 diff 视图（高亮、hunk 导航）；产物卡片；Open in…（VS Code/Cursor/Zed/Terminal）；commit 辅助；补齐 Composer 分支选择器。

**Sprint DoD**：agent 改完代码后，用户可审阅包含未跟踪文件的 diff，通过显式点击把围栏内产物交接到 fixed allowlist 外部应用，生成并编辑 commit message，再经过 Prepare 与 Commit 两次用户确认，由不可被模型调用的 trusted Git 路径提交；不引入 shell、新依赖、新事件类型或 DDL。

> 本文档合入即为 S6 的 SDD 开工门禁。T30-T35 严格串行，每卡在前一卡 squash merge 后开工。
>
> **人类裁决（2026-08-30）**：采用 A，以 phase1-plan S6 为准，PRD v0.3.3 已闭合冲突。Phase 1 交付 Diff v1、artifact cards、fixed-allowlist Open in v1、user-confirmed commit assistance v1；Phase 2 保留 Diff v2、custom/configurable handoff、PR assistance 与 advanced polish。D1-D7 不变。
>
> **人类裁决（2026-08-30，T31 generation 契约）**：workspace refresh 的 latest request sequence 与 content generation 必须分离并在同一 mutex 下线性化。refresh 在途时保留 current；完整 private semantic identity（含 raw path、HEAD、filter/status/raw/numstat bytes、ordered private records 与 revalidated `FileIdentity`）byte-exact 不变才保留 generation/opaque IDs，真正变化或 latest refresh 失败后必须轮换且旧 ID fail closed；ABA 不得复活旧 ID。
> committed private files 以 canonical path-byte-ordered `Vec` 为唯一 authority，`WorkspaceFileId.slot` O(1) 定位后仍须 exact id/seal复验。8 MiB metadata snapshot cap 按真实 committed representation 的 checked logical-retained size 计算：`ServiceState` fixed一次、identity payload一次、fixed identity/Arc allocation，以及 public/private Vec buffers + head/labels/raw current+previous path payload；candidate handles不重复计费。这是逻辑保留量门禁，不宣称 allocator high-water。
>
> **人类裁决（2026-08-30，T32 artifact 契约）**：artifact card 为 route-owned ephemeral state，以 `(route_epoch, call_id)` 唯一标识；只接受 strict `Success && !reused` 的 write/edit result，duplicate identical 幂等，duplicate conflict 返回 typed corrupt且不产生 artifact。failed/rejected/cancelled/reused/bash/read-only均不建卡。每 route 最多 inclusive 10,000 cards，第 10,001 张拒绝。
> agent provenance 只能单向降级为 workspace change，ABA 不得升级；rename/delete/no-current-id仍保留安全 metadata label/source，但 preview/Open in 必须 stale-disabled。文本 preview 还须经过固定路径分类 allowlist、valid UTF-8、no NUL；extension 仅 ASCII case-insensitive exact `txt,md,markdown,rst,adoc,csv,tsv,json,jsonl,yaml,yml,toml,xml,html,htm,css,scss,sass,less,js,jsx,mjs,cjs,ts,tsx,rs,py,rb,go,java,kt,kts,swift,c,h,cc,cpp,cxx,hpp,hxx,m,mm,sh,bash,zsh,fish,sql,graphql,gql,proto,diff,patch,log`，basename exact `README,LICENSE,NOTICE,CHANGELOG,Makefile,Dockerfile,.gitignore,.gitattributes,.editorconfig`；其他（含 SVG、所有 image、unknown、`.env`/npmrc等潜在密钥文件）一律 metadata-only。
>
> **人类裁决（2026-08-30，T32 Open residual）**：exact `/usr/bin/open <path>` 接受 final recheck 到 LaunchServices resolve 之间的 same-user path-swap residual。实现仍必须在同一 trusted worker 内持有 root/parent/target FD，spawn 前最终重查 canonical root、parent 与 target identity，并尽可能在 spawn 后再次复验；报告不得宣称 race-free。工具 terminal 后 immediate refresh 与 Open request generation/latest-result drop 由 Stage B route controller 负责，Stage A 只提供可线性化调用的 trusted headless API。
>
> **人类裁决（2026-08-30，T32 trusted boundary）**：Vega workspace 内同进程 Rust crate caller 属 trusted boundary；public `ToolCall`/`ToolResult` DTO 可构造性不属于 provider/model 攻击面。artifact capture 仍须 strict consistency、route/project/thread/call checkpoint 与完整 input fingerprint 校验。Stage B 只能从真实 `AppAgentController` 的 `AgentBatch` proposal/terminal 配对接线并增加 integration test；禁止任何 renderer、model output 或其他旁路直接调用 capture。此边界在 Phase 1 最终集中 review 再核对。
>
> **人类裁决（2026-08-30，T32 Stage B wiring/UI）**：compact artifact card 必须紧跟 exact tool card，显示 safe label、authoritative `agent artifact|workspace change` 与 Preview 状态；固定六个 Open 按钮顺序为 VS Code/Cursor/Zed/Terminal/Default/Finder，无 custom/dropdown/fallback。Tab/ShiftTab 可达，Enter/Space 激活当前焦点；Esc 只关闭 preview/清 typed inline state，不删除历史 card。opening 期间六个 Open 按钮全部 disabled；错误只用 typed inline state，不用 modal。`ArtifactCard.preview_available` 必须由 headless raw-path classifier 投影，UI 禁止从 escaped label 反推。
>
> 生产 capture 的唯一入口是 `VegaWindow` 真实 `AgentBatch`：event move 给 `ConversationStream` 前保存 route-bound、bounded 的完整 write/edit `ToolCall`，再与 exact `ToolCallFinished` 配对；renderer/model/`WorkspaceToolTerminal` 均不得调用 capture。每个 terminal（含 bash）串行触发 artifact workspace immediate refresh以 reconcile existing cards；strict eligible terminal须在 refresh 后 capture 再 reconcile。artifact controller 独立 route-owned `GitWorkspaceService`/`ArtifactService`，即使 DiffView hidden 也工作；terminal queue 不丢 candidate，sequence/cap overflow fail closed。preview/open 分别使用 checked monotonic request sequence、cancellation 与 route/thread/project/card/current-file/latest-result fence；settings/thread/project/window/route 变化取消并丢弃晚到结果。Open 只由六个显式按钮触发，0/1 attempt 与 no fallback 仍由 headless service保证。
>
> **人类裁决（2026-08-30，T32 Stage B review hardening）**：proposal/terminal pairing 还必须绑定 exact `AppAgentController` generation；run start/finish/cancel 清空并毒化 orphan，later run 的 same call id 不得消费旧 proposal。terminal FIFO 只能保存 content-free refresh marker，或在入队前由 headless strict parser 转换并丢弃 `ToolResult.output` 的 bounded typed capture candidate；不得保留任意 raw terminal output。conflict/limit/checked overflow 关闭 route 并清 queue。artifact route currency 同时绑定 `OpenedThread`、`SelectedProject` 与 settings；删除当前 project 必须清 `OpenedThread`。任何 terminal 开始 refresh/reconcile 前保守取消 preview/Open、失效对应 fence，并尽量在 launcher 前的 cancellation point 保持 zero attempt。route close、active-none/ownership mismatch 均须让历史 card stale-disable、恢复按钮并显示 typed inline error；Open sequence overflow亦同。
> artifact card 的 Tab/ShiftTab 在边界不得 modulo wrap 形成 focus trap，须交回窗口默认 traversal或显式安全 next/previous；Preview 行语义与 headless `split_inclusive` 一致，空文件零行、trailing newline 无 phantom row、exact 10,000 行仍为 10,000 行。
>
> **人类裁决（2026-08-30，T32 Stage B retained caps/ingress）**：每个 retained write/edit proposal 的 `call_id.len + tool.len + input_json.len` checked logical sum 上限 inclusive 64 KiB，且 call id 单独上限 inclusive 120 B；normalized logical path 上限 inclusive 4096 B；paired strict-success terminal envelope上限 inclusive 64 KiB；`ArtifactCaptureCandidate` retained logical bytes上限 inclusive 8192 B。exact cap允许，+1须在 clone/parse/queue前以 `ArtifactLimit` 关闭route且不保留超限值。unpaired/non-write的大 output不受此cap限制，因为controller只生成content-free Refresh且不得clone。真实生产 `AgentBatch` ingress helper须由poll closure与app integration test共用，统一执行 AppAgentController generation match、event observe-before-move、finished poison与finish ownership。
>
> stage/commit、branch switch 与 Open in 都是当前窗口的显式用户动作，不是 `vega_tools` 工具、不注册 provider schema。模型只能生成 bounded commit 草稿，永远不能 Prepare、stage、commit、切分支或启动应用。
>
> **人类裁决（2026-08-30，T33 ref/filter authority）**：不同 local refs 允许共享同一 OID；只拒绝重复 full ref、raw short name或重复 record，current branch 只按 raw ref identity 判定，绝不按 OID 猜测。冻结 ACMRT materialized diff之外，再以相同 current/target OID和固定 read-safe prefix执行 `diff --name-status -z --diff-filter=D -M --no-ext-diff --no-textconv` 捕获纯删除 authority；D-only输出严格只接受 `D` record。R/C 的 old+new 以及 A/M/T/D 的 authority path 任一最终组件为 `.gitattributes` 都 zero switch。D-only canonical raw output/paths与 ACMRT canonical authority一起绑定 single-use permit，execute 前必须 byte-exact 重跑；D-only不加入 materialized/check-attr input。
>
> **人类裁决（2026-08-31，T34 canonical commit v0.4）**：T34 采用 three-source canonical authority：同一 immutable HEAD 下的 porcelain-v2 status、完整 stage-0 index 与 immutable HEAD tree；A capture 为 displayed authority，非空选择执行 exact one NUL-stdin add 后只能由 exact first-wins owned handoff绑定到 B，空选择必须 zero add。A 的每个 selected component 都保留结构义务，B 的每个变化必须由且仅由对应义务解释；不能用“都落在 S 内”代替 modify/delete/add/type/rename/copy/staged+unstaged 的拓扑证明。隐藏 intent-to-add、无真实 index-vs-tree delta、detached/operation state、非普通单 parent/root commit一律 fail closed。
>
> Prepare/Commit 共用 T33 `TrustedActionCoordinator` 的 commit lease；worker 在途关闭进入非视觉 `Retiring`，只取消、不 abort/drop owner future，必须在 owner uncancelled authoritative refresh + Diff/branch/artifact reconcile 后由 exact token 释放。Prepare 自身的 A→B generation 变化只能由 `(service_nonce, prepare_sequence, parent_A, exact_B, route/entity, lease)` capability 原子接管；普通 poll 只能提供 exact candidate，A→B→C、ABA、重复/旧 completion 全部毒化。
>
> staged summary 使用 fixed no-ext-diff/no-textconv cached patch命令，raw stdout 先 bounded收集再做 deterministic escaping，cap 256 KiB且 truncation marker计入；provider draft 是 exact prompts/model/`tools=[]`/`max_tokens=256`/retry=0 的单次 60s 请求，只接受 `TextDelta* Usage* Done(End) EOF`。provider/model/summary/draft/request/result/controller carrier 全部手写 redacted Debug，错误不得格式化 provider-controlled正文。T34 只支持 attached local ordinary branch；成功必须以 immutable `new_oid` 证明 exact parent、tree 与最终 raw ref，禁止 rollback/retry/amend/push。same-user 在 add/commit 前后替换 selected content/type/path、attrs/config/ref 的 TOCTOU 仍是 Phase 1 residual，不得宣称 byte-atomic。
>
> Phase 1 只做 viewer + diff 审阅（PRD D5）；不做自研编辑器/LSP、Checkpoint 回退、PR 创建或终端视图。

---

## S6 冻结常量（所有上限均 inclusive）

| 常量 | 冻结值 | 超限语义 |
|---|---:|---|
| process IO chunk | 16 KiB | 每次读取/写入不超过该值 |
| read-only Git timeout | 10s | kill process group，typed timeout，整次请求 fail closed |
| trusted mutation timeout | 120s | kill process group，刷新真实状态，不自动重放 |
| `/usr/bin/open` timeout | 10s | kill/reap open command，exact one attempt，无 fallback |
| TERM grace / residual drain | 300ms / 500ms | TERM→300ms→KILL→wait；pipes 最多再 drain 500ms |
| read-only stdout + retained status | 8 MiB combined | 多 1 byte 即 typed too-large，snapshot fail closed |
| Git stderr | 64 KiB | 多 1 byte 即 typed too-large；正文不进入 error |
| workspace paths | 10,000 | 第 10,001 条使 snapshot fail closed |
| workspace metadata snapshot | 8 MiB | 多 1 byte fail closed；patch 必须 lazy per-file |
| IndexSnapshot status + stage inputs | shared 8 MiB / 10,000 paths | 任一输入超限、交叉不一致或 generation/HEAD 变化均 fail closed |
| per-file diff | 4 MiB / 20,000 rows / 64 KiB per line | 任一多 1 即该文件 typed too-large/truncated，不返回部分正文 |
| text/report preview | 1 MiB / 10,000 lines / 64 KiB per line | 任一多 1 即 metadata-only + typed too-large |
| image preview | metadata-only | Phase 1 禁止 decode，不留 dimensions/frame allocation 例外 |
| commit message | 32 KiB UTF-8 | empty/NUL/多 1 byte fail closed；typed `String` 无 invalid-UTF8 分支 |
| provider diff summary | 256 KiB | 确定性截断并在 cap 内含 marker，显式 `truncated=true` |
| mutation stdout / stderr | 1 MiB / 64 KiB | 多 1 byte kill group并 fail closed，随后真实状态 refresh |
| visible workspace poll | exactly 750ms | 仅 panel visible 持有 cancellable task；隐藏即取消 |

> `== cap` 接受，读取到第一个超限 byte/row/path 才触发上述语义。marker 计入 cap；禁止 UI、配置或测试放宽。

---

## S6 最小契约闭合

### C1 · 私有 raw service + 唯一 bounded UI 正文通道

- Git workspace/artifact/trusted-action service 落在 headless `vega_conversation` 私有模块；`vega_ui` 不 spawn 进程、不直接读 SQLite，`vega_runtime`/`vega_tools` 不新增反向依赖或 Git wire type。
- raw `OsString`/path bytes、canonical root、Git stderr 与 runner 只留 private service。UI 只持 snapshot-scoped opaque `WorkspaceFileId`/`BranchId`/`IndexSnapshotId`、escaped label 与安全统计；generation 或 id 失效立即丢弃，不把 display label 反向变 argv。
- 允许且只允许 dedicated ephemeral bounded `DiffTextProjection` 与 `ArtifactPreviewProjection` 从 `vega_conversation::types` 交给 UI。两者不得实现 `Serialize`；不得派生正文 `Debug`（不实现或手写 redacted Debug）；不得进入 ConversationEvent/RuntimeEvent、SQLite、审计、日志、error 或 provider tool wire。
- projection 只能由 current generation + opaque id 请求，按冻结 cap 生成；generation 在完成前失效则结果丢弃。除此之外 raw path/root/stderr/文件正文一律不跨边界。
- S6 不新增 `ConversationEvent`/`RuntimeEvent` variant，不建第二业务生命周期流；workspace UI state 完全 ephemeral。

### C2 · Git runner 环境、safe prefix 与进程组生命周期

- runner 清除继承的全部 `GIT_*` redirect/config 输入（含 `GIT_DIR`、`GIT_WORK_TREE`、`GIT_COMMON_DIR`、`GIT_INDEX_FILE`、object/namespace/ceiling/config 三元组等），只重建 `GIT_TERMINAL_PROMPT=0`、`GIT_PAGER=cat`、`GIT_LITERAL_PATHSPECS=1`、`GIT_NO_LAZY_FETCH=1`、`LC_ALL=C`。正常 system/global/repo config 可按默认位置读取，但 CLI override 优先；`rev-parse --show-toplevel` 不等于 canonical project root 即拒绝。
- 所有 Git 命令共享 exact global prefix：`/usr/bin/git --no-pager -c core.fsmonitor=false -c color.ui=false -c maintenance.auto=false -c maintenance.autoDetach=false -c gc.auto=0`。trusted stage/commit/switch 再追加 `-c core.hooksPath=/dev/null`；不允许其他用户/模型 option。
- read allowlist 只含固定模板的 `status`、`diff`、`rev-parse`、`for-each-ref`、`check-attr`、`ls-files`、`ls-tree`、`hash-object`。diff 固定带 `--no-ext-diff --no-textconv`；`hash-object` 只用 `--no-filters` 且绝不带 `-w`。`GIT_LITERAL_PATHSPECS=1` 适用于所有 read/mutation，`--` 不能单独当作 literal 安全保证。
- T30 snapshot/status/diff 前，tracked snapshot raw path set 必须由 fixed `ls-files -z --cached --deduplicate` 取得，并以 bounded stdin 交 exact `check-attr -z --stdin --all`；trusted stage 必须按 C6 的 expanded selected set（含 untracked add）检查，switch 必须按 C7 的 materialized target set 检查（target tree exact顺序为 `check-attr --source=<captured_oid> -z --stdin --all`）。输出只要显式出现 attribute name=`filter`，无论 value 为何均保守拒绝。`--deduplicate`防止unmerged stage1/2/3重复路径误砖死，private parser仍拒绝其输出中的duplicate。untracked direct-read不经过 Git conversion仅限 T30 projection；未来 `git add` 仍必须对 expanded set 执行 filter 检查。preflight 前后 bytes-exact 重查；不得以 `unspecified`/`unset` 特判放行显式 filter，也不得解析或执行 filter config。
- 每个 Git child 使用 `std::os::unix::process::CommandExt::process_group(0)`。stdout/stderr 用 16 KiB chunk 并发 bounded drain；有 stdin 时 writer 与两路 drain 同时推进，禁止写完 stdin 才读输出、顺序 drain、`wait_with_output` 或无界 `read_to_end`。
- timeout/cancel/overflow 用固定 `/bin/kill` 向 negative PGID 发 TERM，等待 300ms，再 best-effort KILL，最后 wait/reap；TERM 失败也继续 KILL/wait。direct child 提前退出但继承 PGID descendant 持 pipe 时，最多 drain 500ms后同样收拢并 fail closed。主动 `setsid` 逃逸是已知 residual，报告不得宣称完整树隔离。
- read-only timeout 10s；mutation 120s；read stdout+retained status 8 MiB、stderr 64 KiB；mutation stdout 1 MiB、stderr 64 KiB。进程 IO 在 bounded worker，旧 request generation 结果不得覆盖新状态。

### C3 · bytes-first snapshot、lazy diff 与 language mapping

- canonical project root 必须存在且为目录。private parser 拒绝 absolute/empty/NUL/`.`/`..` Git path；NUL-delimited raw/status/numstat 保留空格、tab、newline、non-UTF8 与 rename old/new path。所有 path argv 来自当前 snapshot并受 literal env保护。
- metadata snapshot 区分 staged/unstaged/untracked/deleted/renamed/conflicted；最多 10,000 paths/8 MiB，不携带全部 patch。面板 visible 时 exactly 750ms poll，工具 terminal 触发即时 refresh；每文件 patch 按 opaque id lazy 请求。
- tracked patch 开 rename detection，关闭 external diff/textconv；同路径 staged+unstaged 两层都保留。untracked regular UTF-8 text 合成 addition；binary、non-UTF8 content、symlink/special、identity race仅 metadata。unborn HEAD 不硬编码 SHA-1 empty-tree oid。
- `DiffTextProjection` 接受至多 4 MiB/20,000 rows/64 KiB line；超限不返回部分正文。extension mapping 固定：`.rs→rs`、`.ts→ts`、`.tsx→tsx`、`.js/.jsx/.mjs/.cjs→js`、`.py→py`，其他等宽 plain text。
- literal filename `:(glob)**`、`:!safe` 必须按 raw exact path diff/stage，不得扩大 pathspec。

### C4 · artifact provenance 与 preview

- artifact 完全来自 current ephemeral snapshot，不建表/事件。strict successful write/edit terminal 后立即 refresh；只有相对身份映射 current `WorkspaceFileId` 且 regular file `<=1 MiB`，才能计算 provenance。
- provenance 固定为 `(dev, ino, size, mtime_ns)` + read safe prefix 后的 fixed `hash-object --no-filters -- <raw-path>` content identity。命令绝不带 `-w`。同一 generation 完成验证后才标 `agent artifact`；later generation 任一 identity/digest 改变立即降级 `workspace change`。
- `(route_epoch, call_id)` 每 route 唯一且上限 inclusive 10,000；service绑定 project/thread/route，只接受真实 `ToolCall` 的完整 `WriteEditAudit`（含 fixed 64hex `fingerprint_v1`）与 strict `Success && !reused` write/edit terminal，success checkpoint ref 必须与同一 project/thread/call exact匹配。duplicate canonical fingerprint identical 幂等；same-length不同正文 fingerprint或其他duplicate conflict typed corrupt/no artifact；failed/rejected/cancelled/reused/bash/read-only不建卡，不保留 raw terminal JSON。
- bash-created、invalid/stale projection、>1 MiB、无法证明 identity 的条目只能叫 `workspace change`。agent→workspace 单向降级，ABA 不升级；rename/delete/no-current-id仍保留安全 metadata，但 preview/Open in stale-disabled。artifact 来源标签不改变 fence。
- `ArtifactPreviewProjection` 文本/报告最多 1 MiB/10,000 lines/64 KiB line，并须通过上述 fixed extension/basename allowlist、valid UTF-8、no NUL；超限/binary/unsupported/race metadata-only。Phase 1 image（含 SVG）**一律 metadata-only + Open in**，不 decode、不引依赖；`.env`/npmrc等潜在密钥文件不进 allowlist。

### C5 · Open in 六套 exact argv 与 lifecycle

- allowlist 精确为以下六套 argv；canonical absolute target/root 不可能以 option 开头，禁止 `--args`，也不添加未验证的 standalone `--`：

  ```text
  /usr/bin/open -a "Visual Studio Code" <target>
  /usr/bin/open -a Cursor <target>
  /usr/bin/open -a Zed <target>
  /usr/bin/open -a Terminal <canonical-root>
  /usr/bin/open <target>
  /usr/bin/open -R <target>
  ```

- custom/configurable app 延后 Phase 2。点击前解析 current `WorkspaceFileId`；任一 symlink segment、`.git` entry、实际 gitdir、special file、regular `nlink>1`、root escape 或 identity race都拒绝。Terminal 恒 project root，其他五项使用围栏 target。
- launcher 在独立 bounded worker；root/parent/target FD 从最终 preflight 持有到 spawn，spawn 前重查 canonical root/parent/target identity，尽可能在 spawn 后再复验。stdin/stdout/stderr 均 null；timeout 10s，wait/reap `/usr/bin/open`，结果只应用到 current request generation。成功返回后不杀已由 LaunchServices 启动的 app。exact path argv 接受 final recheck→LaunchServices resolve 的 same-user path-swap residual，报告不得宣称 race-free。
- no click、stale/unknown id、fence/identity preflight failure = 0 invocation attempt；app missing、spawn error、nonzero、timeout = exactly 1 attempt且无 retry/fallback/alternate；success = exactly 1 attempt。
- fake launcher 必须按 raw `OsString` 精确断言 space/tab/newline/leading-dash-looking component/non-UTF8 target 的六套 argv；escaped label 永不回流。

### C6 · cross-checked canonical IndexSnapshot + 两阶段 selected staging

- private `IndexSnapshotId` 必须从同一 request generation 与 immutable captured HEAD 下的三个 read-safe truth source **交叉构造**：`status --porcelain=v2 -z --branch --renames --untracked-files=all`、`ls-files --stage -z`、born HEAD 的 `ls-tree -rz --full-tree <captured_head_oid>`。unborn attached HEAD 必须被独立证明，tree视为空且 zero `ls-tree` spawn。三输入共享 checked 8 MiB/10,000 logical-path cap，rename 两侧都计；UI只获 opaque id和bounded safe projection，绝不读取/hash raw `.git/index`或接收 raw path/OID/tree。
- canonical private value保存完整且bytes-first排序的 HEAD/ref identity、porcelain records（rename/copy两侧）、完整 stage entries `(mode, full_oid, raw_path)`、完整 immutable HEAD tree `(mode, type, full_oid, raw_path)`，比较 full value而非只比hash。OID统一绑定 status HEAD 的40/64宽度；拒绝abbreviated/mixed/zero OID、duplicate/conflict、stage>0、unmerged、sparse-directory `040000`、special mode、corrupt/overflow。只接受index mode `100644|100755|120000`和clean unchanged `160000`；tree中前三者type必须blob、gitlink必须commit，changed/selected gitlink全部拒绝。
- porcelain HEAD-side `mH/hH/path` 与 rename/copy old side必须和 immutable HEAD tree一致；HEAD缺失只允许规范 staged add/rename-new。任何 tracked `XY=.A` 立即拒绝；还必须以 HEAD-tree cross-check识别 `add -N` 后 delete/move形成的隐藏 `.D` intent状态，均为zero add/commit。正常 staged empty `XY=A.` + stage0 nonzero empty-blob OID仍合法；tracked empty+worktree delete及staged-empty+worktree delete须保持可区分。
- 每次 Checklist/A、add immediately-before、B acceptance和Commit preflight都要求 attached raw `refs/heads/*` ordinary local branch，并复用C7完整operation marker guard（merge/cherry-pick/revert/bisect/rebase/sequencer/git-am及linked-worktree-safe git-path checks）；detached或operation state一律zero mutation。authority绑定exact raw ref与A HEAD OID/unborn state，并用fixed for-each-ref parser交叉验证。
- selected `WorkspaceFileId` 在A上建立private component ledger，绑定service nonce、A generation/seal、exact status/index/worktree kind、current/original raw path与闭包S。S bytes-first dedupe：rename/copy=old+new、delete=old、add/untracked=new、modify/type=current；staged+unstaged的forced staged部分不加入S，optional worktree part按对应闭包。任一side最终组件为`.gitattributes`、intent/unmerged/changed gitlink/duplicate/corrupt都拒绝。
- non-empty S在preflight/immediately-before/after add对同一NUL S执行exact `check-attr -z --stdin --all`，raw output必须byte-exact稳定；显式attribute name=`filter`不论value一律拒绝。mutation只执行一次exact `add -A --pathspec-from-file=- --pathspec-file-nul`，stdin为non-empty、byte-sorted、dedup、final-NUL序列，path绝不进argv。empty S只允许full index-vs-HEAD存在真实staged delta，必须zero add，fresh B canonical byte-equal A；否则Prepare disabled/headless reject。
- B除outside-S stage/status byte-exact保持外，每个变化都必须由且仅由一个selected ledger obligation解释：modify/type在B stage0存在full nonzero OID、mode/type匹配且Y/untracked清空；delete在worktree/index均缺失并形成相对HEAD的canonical staged deletion；add/untracked在B stage0存在并形成canonical add；rename old absent/new present、Y清空且pair不接触outside S；copy source保持exact A而destination形成canonical copy/add；optional-off保持A index与Y exact，optional-on只消费其selected worktree component。任何vanish/reappear/kind flip、ambiguous re-pair、extra/overlap unexplained delta都拒绝。B OID不预测，只要求Git产出的full-width/nonzero/stage0 codec authority。
- B full stage0 index与immutable HEAD tree必须有至少一个真实add/modify/mode/delete/normalized-rename delta；EOL/CRLF或ignored filemode规范化为相同tree时Prepare失败、zero provider/commit。失败不reset/restore/unstage/rollback，返回refresh后的真实状态并清selection。
- Prepare初始focus Cancel，Esc=Cancel，只有Cmd+Enter确认。successful worker返回owned handoff `(service_nonce, prepare_sequence, parent_A_IndexSnapshotId, A_generation, exact_B_IndexSnapshotId, B_generation, B_seal, route/thread/project/entity fence, shared_lease_token)`；只有exact first-wins completion能A→B。poll先/后观察exact B均可，C/多次transition/A→B→A/ABA/old completion/stale sequence毒化且无B capability；poll不得释放lease或覆盖newer fence。CommitReady前必须先reconcile Diff/branch/artifact到B。
- staged summary固定read argv为read-safe prefix后 `-c core.quotePath=true --no-optional-locks diff --cached --patch --find-renames --no-ext-diff --no-textconv --full-index --`；并发drain full stdout/stderr，stdout只retained raw 256 KiB+overflow、stderr 64 KiB、timeout 10s。raw bytes一次性确定性转UTF-8：valid UTF-8保持、invalid byte=`\\xNN`、LF/TAB外control用固定escape；chunk边界不得影响结果。cap exact通过，overflow/escape expansion预留并追加exact `\n[vega-summary truncated=true]\n`且marker在256 KiB内。summary前后以及provider前后exact B都须复验。
- provider draft只由用户click触发：60s deadline从`chat_stream`前开始覆盖setup/events/Done后EOF；exact thread model、`max_tokens=Some(256)`、exact两段prompt、`tools=[]`、real provider retry max=0。grammar只接受`TextDelta*`后`Usage*`、exact one `Done { End }`、再一次`next()==None`；cancel biased。Thinking/ToolUse/其他Done/text-after-Usage/post-Done event或error/missing/duplicate Done/early EOF/hang/provider error/empty/NUL/checked overflow/>32 KiB均直接content-free `DraftFailed`，不返回partial draft。
- **Commit**初始focus Cancel；Esc=Cancel；editor bare Enter只换行；仅Cmd+Enter第二次确认。message为non-empty/no-NUL/1..=32768-byte typed UTF-8。B authority在第一个commit-reaching await前single-use consume；第三次three-source capture须full byte-equal B且route/ref/HEAD仍匹配，否则zero commit。mutation exact为 `commit --no-gpg-sign --file=- --cleanup=verbatim`，message仅内存stdin且writer/stdout/stderr并发，无temp file/合成尾字节。
- T34只允许attached ordinary commit：born new commit必须exact one parent==A HEAD；unborn root必须zero parents。process成功后先捕获immutable `new_oid`+raw ref，proof只用explicit OID执行 `rev-parse <new_oid>^@` 与 `ls-tree -rz --full-tree <new_oid>`；new tree须exact等于B index tree，最后再次枚举并要求same raw ref仍指向new_oid且root identity不变。wrong parent/count/tree/ref moved/deleted/renamed/ABA均Failed。所有success/nonzero/timeout/cancel/ambiguous exit都执行owner uncancelled authoritative HEAD/status/index refresh及Diff/branch/artifact reconcile；禁止retry/rollback/amend/push。
- controller states固定为`Closed|Checklist(A,lease)|Preparing(A,fence,lease,candidate)|CommitReady(B,lease)|Drafting(B,fence,lease)|Committing(consumed_B,fence,lease)|Retiring(fence,lease,cancel_requested,mutation_maybe_attempted)`。route open后获取T33 exact Commit token并贯穿两阶段；click/key callback先atomic transition再spawn。in-flight close/window/thread/project/route change立即隐藏UI并进Retiring，只cancel、不abort/drop owner future；owner真实终止/reap、authoritative refresh/reconcile后才由exact token release，old completion不得清newer token。无worker close才可清route/busy后立即release。
- 所有provider/model-controlled string carrier均禁止derived raw Debug：`ChatMessage`/tool id、`ChatToolCall`、`ToolDefinition`、`ChatRequest`、`ProviderEvent`、`ScriptStep`/`MockProvider`、SSE fragment/assembler、test captured headers/body以及T34 summary/draft/request/result/fence/UI event只能手写长度/count/presence redaction或不实现Debug；provider error先映射为content-free code，禁止format/log。sentinel不得进入Debug/Display/tracing/error/event/DB/controller/UI。
- same-user在pre/post check之间替换selected worktree content/type/path、attrs/config或ref（含ABA）仍为接受的Phase 1 path-based Git TOCTOU residual；身份/hash复验只能缩窗，不能证明Git add读取瞬间的exact bytes，报告不得宣称byte-atomic或race-free。

### C7 · branch target materialization preflight

- 只列 `refs/heads/*` local refs并捕获 target OID及其bytes short branch name；raw ref/name留 private，UI只持 opaque id/escaped label。拒绝 empty/NUL/control/leading-`-` name、detached/remote guess/create/unknown/stale id/OID change；non-UTF8/ref label只展示 escaped串。
- dirty/conflict（staged/unstaged/untracked/unmerged）或 merge/rebase/cherry-pick/revert/bisect/sequencer/`git am` operation marker一律拒绝。active agent run、pending permission、pending plan review、open commit panel 任一存在也拒绝。
- current OID→captured target OID 固定执行 read safe prefix + `diff --name-status -z --diff-filter=ACMRT -M --no-ext-diff --no-textconv <current_oid> <target_oid>`，bounded bytes parser取得 materialized target path set（rename只取target/new path）；target deleted path不 materialize。若 changed set 含任意 `.gitattributes` 直接拒绝。
- materialized target paths通过 stdin交 exact `git check-attr --source=<target_oid> -z --stdin --all`；输出只要显式出现 attribute name=`filter`，无论 value 为何均拒绝。`--source` capability self-test失败也拒绝。preflight后再次复验 target OID、clean/status/operation/active guards。
- exact switch argv = mutation safe prefix + `switch --no-guess --no-overwrite-ignore --no-recurse-submodules <validated-local-branch-name>`。name 必须来自刚复验的 `refs/heads/*` enumeration；禁止 remote guess/create/detach/force/stash/reset/clean/checkout。
- 无论 exit success/failure/timeout都刷新真实 HEAD/status/branch。filter-driver repository、ignored collision、submodule recurse、target smudge/process必须在 spawn switch 前 fail closed/zero side effect。

### C8 · UI、依赖与测试 fixture

- Diff viewer 默认 unified，可切 side-by-side；文件数、+x/-y、untracked/binary/rename；逐文件折叠、next/previous hunk。新增行 `success` 8% token背景、删除 `danger` 8%、行号 `text-tertiary`、`@@` hunk `code-bg`，无硬编码颜色/字号。
- 语法高亮只复用现有 `vega_markdown`/tree-sitter；不激活/新增 `similar`。S6 零新依赖、零 DDL、Cargo.lock 不因依赖变化而改。
- 所有 filesystem/Git 测试都使用 fresh `tempfile` repo、per-repo local `user.name/user.email`、显式测试 env，不依赖 global config/current Vega repo；只清理 fixture-owned path。真实 app/provider/key/network一律不调用。
- Composer 本 Sprint 只补 A2-16。A2-12 `@引用`、A2-21 `/命令`、A2-14 模型选择器后置；`>8` 独立 inner-scroll留 S8。

---

## 卡依赖图

```text
S6 SDD PR → T30 snapshot/diff service → T31 Diff UI → T32 artifact/Open in
          → T33 branch selector → T34 commit assistant → T35 acceptance/report
```

> 每卡独立 sibling worktree/PR，严格 squash merge 后再开下一卡。

## T30 · Git snapshot/diff headless service（A5-01/A5-03）

- **范围**：`vega_conversation` private runner/snapshot/projection types；不得改 UI/DDL/event enum或注册 model tool。
- **产出/验收**：C1-C3/C8 全部；lazy patch、redacted projection、所有 exact bounds；literal `:(glob)**`/`:!safe`；恶意 env/pager/fsmonitor/external diff/textconv/lazy fetch；non-UTF8；stdout/stderr flood；PGID descendant/direct-child race/maintenance no-detach；setsid residual记录。
- **命令**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation git_workspace
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation projection_redaction
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_conversation --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && ! cargo tree -p vega_conversation | rg 'gpui(_platform)? v'
  git diff --check
  ```

- **commit**：`feat(A5-01): add bounded git workspace snapshots`（≤3 commits）。
- **停止**：std process-group/NUL parser/bounds无法闭合即 `[BLOCKED] S6-T30`。

## T31 · Diff UI + controller（A5-02/A5-03）

- **范围**：`crates/vega_ui/src/diff_view.rs`、必要 UI exports、`crates/vega/src/main.rs` controller/wiring；订阅工具 terminal refresh、路由 current project/thread/generation；不在 UI/app执行 Git IO。
- **产出/验收**：C1/C3/C8；unified/side-by-side、统计/折叠/hunk keyboard；lazy projection stale drop；exact extension map；app route可达；960×600约束；token/8% opacity。
- **命令**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_ui diff_view
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega --bin vega diff_controller
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_ui --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega --all-targets -- -D warnings
  ! rg -n '#[0-9a-fA-F]{6}|rgba?\(' crates/vega_ui/src/diff_view.rs
  git diff --check
  ```

- **commit**：`feat(A5-02): render navigable workspace diffs`；必要时 `feat(A5-03): add workspace change statistics`。

## T32 · Provenance artifact + exact Open in（A5-04/A5-05）

- **范围**：conversation artifact/launcher service、`crates/vega_ui/src/artifact_card.rs`、`crates/vega/src/main.rs` request-generation controller/wiring与app tests；无 DB/event。
- **产出/验收**：C4/C5/C8；tool terminal immediate identity/hash refresh；later downgrade；image metadata-only；六套 exact argv raw OsString awkward/non-UTF8；preflight=0 attempt，spawn/app/nonzero/timeout=1 attempt no fallback，success=1；open lifecycle/request stale drop。
- **命令**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation artifact
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_ui artifact_card
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega --bin vega artifact_controller
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_conversation --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_ui --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega --all-targets -- -D warnings
  git diff --check
  ```

- **commit**：`feat(A5-04): add provenance-bound artifact cards`；必要时 `feat(A5-05): add fixed external app handoff`。
- **停止**：不为 image/custom app加依赖；需要即 `[BLOCKED] S6-T32`。

## T33 · Guarded local branch selector（A2-16）

- **范围**：conversation branch preflight、`crates/vega_ui/src/branch_selector.rs`、`crates/vega/src/main.rs` active-state/controller wiring。
- **产出/验收**：C2/C7/C8；captured OID/NUL path set/`.gitattributes` reject/target `check-attr --source`；exact switch；all operation+active guards；filter/smudge/process recorder zero spawn、ignored collision zero overwrite、submodule no recurse；literal `:(glob)**`/`:!safe`、non-UTF8/ref label；PGID descendants/maintenance no-detach lifecycle与所有 exit refresh。
- **命令**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation branch
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_ui branch_selector
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega --bin vega branch_controller
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_conversation --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_ui --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega --all-targets -- -D warnings
  git diff --check
  ```

- **commit**：`feat(A2-16): add preflighted branch selection`。
- **停止**：`--source`/active-state无法可靠获得即 `[BLOCKED] S6-T33`。

## T34 · Canonical two-stage commit assistant（A5-06）

- **范围**：conversation IndexSnapshot/trusted Git/provider draft、`crates/vega_ui/src/commit_panel.rs`、`crates/vega/src/main.rs` first-wins controller/wiring与app tests；零 DB/event/temp file。
- **产出/验收**：C2/C6/C8与2026-08-31 v0.4裁决全部；three-source canonical snapshot、hidden intent-to-add、mode/type/tree correlation、per-kind selected structural ledger、real index-vs-tree delta、exact NUL-stdin one-add/empty-S zero-add、owned A→B handoff与poll/ABA矩阵。另覆盖fixed summary argv与deterministic non-UTF8/cap、provider 60s strict Done+EOF grammar/retry0/max_tokens256、all carrier Debug redaction、attached ordinary parent/tree/ref proof、Retiring close/cancel/exact-token cleanup。所有prepare/add/commit成功与失败路径都在fresh temp repo验证zero/one spawn、PGID/reap、authoritative refresh/reconcile；无真实provider/key/network、temp message file、rollback/retry/amend/push。
- **命令**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation trusted_git
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation commit_redaction
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_ui commit_panel
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega --bin vega commit_controller
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_conversation --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_ui --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega --all-targets -- -D warnings
  git diff --check
  ```

- **commit**：`feat(A5-06): add canonical two-stage commit assistance`。
- **停止**：three-source/index/tree/ref、strict provider terminal或Retiring owner-lifecycle契约与实际API矛盾即 `[BLOCKED] S6-T34`，不得降级raw index hash、lossy path、temp file、early lease release或宽松provider grammar。

## T35 · S6 end-to-end acceptance + report（A5-02）

- **场景**：main temp repo 验证 agent edit/rename/untracked→diff→artifact/fake Open→dirty branch reject→Prepare→Commit→post-tree；positive branch switch使用独立 clean fixture或在commit后执行，绝不在dirty中伪造通过。
- **报告**：`docs/vega-s6-report.md` + README。只列 SDD/T30-T34 已 merged PR/squash hashes；T35 只列自身 branch commits并明确 PR/squash pending，最终 Phase 1 milestone report再补 T35 squash hash。不得自报尚不存在的 evidence。
- **精确门禁**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all -- --check
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy --workspace --all-targets --all-features -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test --workspace --all-features
  export PATH="$HOME/.cargo/bin:$PATH" && cargo build --workspace --all-targets --all-features
  export PATH="$HOME/.cargo/bin:$PATH" && cargo xtask bench
  git diff --check
  export PATH="$HOME/.cargo/bin:$PATH" && ! cargo tree -p vega_runtime | rg 'gpui(_platform)? v'
  export PATH="$HOME/.cargo/bin:$PATH" && ! cargo tree -p vega_tools | rg 'gpui(_platform)? v'
  export PATH="$HOME/.cargo/bin:$PATH" && ! cargo tree -p vega_conversation | rg 'gpui(_platform)? v'
  rg -n '\.(unwrap|expect)\(' crates --glob '*.rs'
  ! rg -n '#[0-9a-fA-F]{6}|rgba?\(' crates/vega_ui/src/diff_view.rs crates/vega_ui/src/artifact_card.rs crates/vega_ui/src/branch_selector.rs crates/vega_ui/src/commit_panel.rs
  ! rg -n 'rusqlite|Connection::' crates/vega_ui crates/vega/src/main.rs
  test "$(rg -n '^CREATE TABLE' crates/vega_store/migrations | wc -l | tr -d ' ')" = "6"
  git diff --exit-code origin/master -- crates/vega_store/migrations Cargo.toml 'crates/*/Cargo.toml' Cargo.lock
  rg -n 'enum (ConversationEvent|RuntimeEvent)' crates --glob '*.rs'
  rg -n '(/private/tmp|--force|--amend|--allow-empty|--no-verify|\b(push|reset|restore|stash|clean|checkout)\b)' crates/vega_conversation/src crates/vega_ui/src crates/vega/src/main.rs
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation projection_redaction
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation commit_redaction
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation --test s6_acceptance
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega --bin vega s6_controller
  ```

  `unwrap/expect`、event enum 与 forbidden Git verb scans 必须逐条分类：只允许测试段/既有安全值，且 Git/artifact 不新增 event variant或生产危险 argv；不能用 grep 零输出冒充。真实 key/app/UI/硬件未测项逐项标 ⚠️。

- **commit**：`feat(A5-02): close Sprint 6 diff review acceptance`。
- **停止**：只用 owned temp repo/MockProvider/fake launcher；环境失败按 `[BLOCKED] S6-T35`。

---

## S6 完成定义（DoD）

- [ ] PRD v0.3.3 与 phase1-plan S6 已闭合；T30-T35 串行 squash；master 精确门禁全绿。
- [ ] bounded ephemeral projections 是唯一 UI 正文通道；raw path/root/stderr private；无 Serialize/body Debug/event/DB/log/error leak。
- [ ] 16 KiB chunk、10s/120s、process group TERM→300ms→KILL→wait、500ms drain、maintenance disabled、literal path/lazy-fetch env与全部 inclusive cap实测。
- [ ] metadata snapshot lazy patch含 staged/unstaged/untracked/binary/rename/non-UTF8；unified/side-by-side、统计、折叠、hunk、高亮 mapping完整。
- [ ] write/edit provenance绑定 immediate `(dev,ino,size,mtime_ns)+hash-object`；later变化降级；bash-only workspace change；image metadata-only。
- [ ] Open in六套 exact argv；hardlink/symlink/gitdir/special fence；0/1 attempt语义、10s lifecycle、stale request drop；Terminal恒root。
- [ ] branch local OID/clean+active+operation guards；target `.gitattributes`/filter preflight；exact no-overwrite-ignore/no-recurse switch；任意 exit真实 refresh。
- [ ] IndexSnapshot由同一 immutable HEAD/generation 的 status + stage + HEAD tree 三源交叉构造；`.A`与隐藏`.D` intent、stage>0/zero/mode-type/tree冲突/corrupt/overflow均拒绝，staged empty正例通过。selected ledger逐kind、real delta、empty-S zero-add、NUL-stdin one-add与owned A→B generation handoff/ABA完整。
- [ ] commit 32 KiB UTF-8 stdin；provider summary exact argv/256 KiB deterministic escaping；draft exact prompts/max_tokens256/tools empty/retry0/60s Done+EOF grammar。attached ordinary parent/tree/raw-ref proof、Retiring owner refresh/reconcile、hooks/signing disabled完整；模型不能mutation；无temp/rollback/retry/amend/push/network；全部carrier手写redacted。
- [ ] fresh temp fixture/local identity/no global/current repo；零新依赖/DDL，仍六表，headless/UI边界不回退。
- [ ] S6 report只列可存在证据；ui-spec §6自动/人工/硬件分开，未测不写 ✅。

## ui-spec §6 Sprint 末检查矩阵

| 检查项 | 自动化最低证据 | 人工/硬件边界 |
|---|---|---|
| token | component token/opacity tests + exact scan | 真实字体观感人工 |
| Light/Dark | 双 theme state tests | 真实切换无闪烁人工 |
| CJK | CJK/emoji/escaped non-UTF8 不 panic | fallback/豆腐块真实窗口 |
| keyboard | diff→Open→Prepare→Commit focus tests | 完整真实窗口链路人工 |
| 960×600 | layout constraints | 像素截图人工 |
| P1-P8 | `xtask bench` 原始值/unsupported | ProMotion/首帧/RSS留 S8 |
| competitor | 无自动化替代 | Codex/ZCode截图未做即 ⚠️ |

## 已知偏离、兼容限制与 residual（原样进入报告）

1. filter driver repository：所有 relevant `check-attr --all` 输出只要显式出现 attribute name=`filter` 即一律拒绝；same-user 在preflight后改 attributes/config是已知 TOCTOU residual，不宣称原子隔离。
2. 所有 Git child收拢 inherited PGID descendants；主动 `setsid` 逃逸是 residual。
3. hooks 与 signing固定关闭；依赖它们的repo需在终端提交。
4. Phase 1 image metadata-only；Open in仅六个fixed targets。custom/configurable handoff、PR assistance、Diff v2留 Phase 2。
5. Composer @引用、/命令、模型选择器与 >8独立inner-scroll后置。
6. T35 report无法包含自身尚未产生的 PR/squash hash；只列 branch commit + pending，Phase 1最终报告补 hash。
7. fake launcher/MockProvider不等于真实 app/LLM/key/费用；真实 UI/CJK/960×600/竞品截图/ProMotion/P1-P8逐项留人类/S8。
8. T34 path-based Git mutation接受同一用户在复验与Git实际读取/更新之间替换selected content/type/path、attrs/config/ref的TOCTOU；实现以three-source/identity/hash/ref前后复验缩窗，但不宣称byte-atomic。

## 未决阻塞检查

- 当前无未决 spec/API blocker；review确认本机支持 `check-attr --source`、stdin commit与所需GPUI API。
- T30/T33/T34 若实际系统行为与本 frozen contract矛盾，立即 `[BLOCKED]`，不得降级 shell/lossy path/temp file/宽权限。

## 变更记录

- v0.1 (2026-08-30) S6 初始 SDD：T30-T34 与基础安全边界。
- v0.2 (2026-08-30) 人类批准 A：PRD v0.3.3、raw path private、artifact provenance、two-stage stdin commit，重排 T30-T35。
- v0.3 (2026-08-30) 最终 executable hardening：bounded projections、literal pathspec、PGID/maintenance、exact caps/Open argv、target filter preflight、porcelain-v2 + stage-entry cross-checked logical IndexSnapshot（显式拒绝 `XY=.A` intent-to-add、允许 `XY=A.` staged empty file）、controller ownership、copyable gates与报告 evidence timing定稿。
- v0.4 (2026-08-30) 人类修订 filter 契约：所有 relevant `check-attr` 固定 `--all`，显式 attribute name=`filter` 无论 value 一律拒绝；same-user preflight TOCTOU保留为 residual。
- v0.5 (2026-08-30) 人类冻结 T31 request/content generation 契约：mutex 线性化 latest-wins；unchanged private identity 保留 opaque IDs；change/failure/ABA fail-closed 轮换；8 MiB cap覆盖真实 committed metadata snapshot。
- v0.6 (2026-08-30) 人类冻结 T32 artifact 契约：fixed text path allowlist；strict non-reused write/edit success；route/call幂等与 10,000 card cap；agent provenance单向降级、ABA不升级；rename/delete保留安全 metadata但禁用 preview/Open in。
- v0.7 (2026-08-30) 人类接受 exact Open path 的 final-recheck→LaunchServices same-user swap residual；实现仍持 root/parent/target FD并在同一 worker内 spawn前后尽力复验；immediate refresh/open request generation明确移交 Stage B controller。
- v0.8 (2026-08-30) 人类冻结 trusted in-process crate boundary：capture 仍做 strict route/checkpoint/fingerprint consistency；Stage B 仅允许从真实 AppAgentController AgentBatch proposal/terminal 接线并补 integration test，最终 Phase 1 集中复核。
- v0.9 (2026-08-30) 人类冻结 T32 Stage B compact inline card、六个显式 Open 控件、headless preview eligibility、真实 AgentBatch 唯一 capture 入口、terminal 串行 refresh/capture/reconcile，以及 preview/open/route latest-result fence。
- v0.10 (2026-08-30) review hardening：Agent generation pairing、content-free terminal FIFO、SelectedProject/route invalidation、terminal-before-open cancellation、fail-closed historical card、无焦点陷阱与 exact preview 行语义。
- v0.11 (2026-08-30) controller final hardening：冻结 proposal/id/path/terminal/candidate retained caps及exact/+1语义，并冻结production/tests共用的真实 AgentBatch ingress helper。
- v0.12 (2026-08-30) 人类冻结 T33 shared-OID refs/current-by-raw-ref 契约，并增加 D-only authority capture与 R/C old+new `.gitattributes` 零切换、permit前后 byte-exact 重放。
- v0.13 (2026-08-31) 冻结 T34 canonical commit v0.4/P0/P1：status+stage+immutable HEAD tree三源、selected structural ledger、hidden intent与real-delta门禁、owned A→B generation handoff、attached ordinary parent/tree/ref proof；补fixed summary/strict 60s provider grammar、全carrier redacted Debug、Retiring lease生命周期及same-user TOCTOU诚实边界。
