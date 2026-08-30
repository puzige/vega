# ✦ Vega — S6 任务卡（Sprint 6 · Diff 审阅 & 产物 · W11-12）

**版本** v0.2 · 2026-08-30 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt

**S6 目标**（phase1-plan §2）：git 工作区 diff 视图（高亮、hunk 导航）；产物卡片；Open in…（VS Code/Cursor/Zed/Terminal）；commit 辅助；补齐 Composer 分支选择器。

**Sprint DoD**：agent 改完代码后，用户可在 Vega 内审阅包含未跟踪文件的工作区 diff，通过显式点击把围栏内产物交接到 fixed allowlist 外部应用，生成并编辑 commit message，最终经过 Prepare 与 Commit 两次用户确认，由不可被模型调用的 trusted Git 路径提交；全过程不引入 shell、新依赖、新事件类型或 DDL。

> 本文档合入即为 S6 的 SDD 开工门禁。T30-T35 严格串行，每卡在前一卡 squash merge 后开工。
>
> **人类裁决（2026-08-30）**：采用 A，以 phase1-plan S6 为准并修订 PRD v0.3.3。Phase 1 交付 Diff v1、artifact cards、fixed-allowlist Open in v1、user-confirmed commit assistance v1；Phase 2 保留 Diff v2、custom/configurable handoff、PR assistance 与 advanced polish。此前 PRD 冲突已闭合，D1-D7 不变。
>
> stage/commit、branch switch 与 Open in 都是当前窗口的显式用户动作，不是 `vega_tools` 工具、不注册 provider schema。模型只能生成 bounded commit message 草稿，永远不能 Prepare、stage、commit、切分支或启动外部应用。
>
> Phase 1 只做 viewer + diff 审阅（PRD D5）；不做自研编辑器/LSP、Checkpoint 回退、PR 创建或终端视图。

---

## S6 最小契约闭合

### C1 · Git service 是私有 headless 边界，不新增事件流

- Git workspace/artifact/trusted-action service 落在 headless `vega_conversation` 私有模块；`vega_ui` 不 spawn 进程、不直接读 SQLite，`vega_runtime`/`vega_tools` 不新增反向依赖或 Git wire type。
- raw `OsString`/path bytes 只存在于 private service。跨 UI 的共享类型只暴露 opaque、单 snapshot 有效的 `WorkspaceFileId`、escaped display label 与安全状态/统计；禁止把 raw path、绝对 root、stderr 或文件正文塞入 shared event、`Debug`、日志或 SQLite。`WorkspaceFileId` 只能由当前 snapshot 解析，过期/未知 id fail closed。
- S6 workspace/artifact/branch/commit UI 是 ephemeral view state，不新增 `ConversationEvent`/`RuntimeEvent` variant，不建事件旁路；既有 ConversationEvent 唯一业务流不变。
- 全部 Git 调用只用 `std::process::Command` 启动固定 `/usr/bin/git`，`current_dir` 固定 canonical project root；禁止 shell、命令字符串、Git alias、自定义 executable 或从模型输入构造 option。production 代码无 `unwrap/expect`。

### C2 · 固定进程环境、并发 drain 与可收拢生命周期

- runner 清除继承的 Git redirect/config 环境，包括 `GIT_DIR`、`GIT_WORK_TREE`、`GIT_COMMON_DIR`、`GIT_INDEX_FILE`、`GIT_OBJECT_DIRECTORY`、`GIT_ALTERNATE_OBJECT_DIRECTORIES`、`GIT_NAMESPACE`、`GIT_CEILING_DIRECTORIES`、`GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_*`/`GIT_CONFIG_VALUE_*`、`GIT_CONFIG_SYSTEM`、`GIT_CONFIG_GLOBAL`、`GIT_CONFIG_NOSYSTEM` 及其他 `GIT_*` 输入；随后只重建集中 allowlist：`GIT_TERMINAL_PROMPT=0`、`GIT_PAGER=cat`、`LC_ALL=C`。正常 system/global/repo config 仍由 Git 按默认位置读取，但 CLI 安全 override 优先，且 `rev-parse --show-toplevel` 不等于 canonical project root 时 fail closed。
- read-only subcommand allowlist 只含固定模板的 `status`、`diff`、`rev-parse`、`for-each-ref`、`check-attr`；diff argv 明确带 `--no-ext-diff --no-textconv`。任何未列 verb 都不能从 workspace refresh 路径执行。
- 每个 argv 固定禁 pager/color/prompt/fsmonitor；diff 固定禁 external diff/textconv。不得依赖用户 alias、pager、prompt、fsmonitor hook、external diff 或 textconv 产生结果。
- stdout/stderr 必须同时以固定 chunk 并发 drain，各自及合计都有集中 bytes 上限；禁止先等 child 再顺序读另一 pipe、`wait_with_output` 或无界 `read_to_end`。超限、timeout、cancel 都 kill 后 wait/reap；无论 exit 结果都收拢 child 并返回 typed、脱敏错误。
- 进程 IO 离开 UI 上屏关键路径，在 bounded blocking worker 中运行；同项目只接受最新 refresh generation，旧结果不得覆盖新 snapshot。工具完成后即时 refresh，面板可见时再用有界低频 polling 捕获外部改动。

### C3 · project-root fence 与 bytes-first snapshot/diff

- project root 必须存在、为目录且 canonicalize 成功。private service 对 Git 返回路径拒绝 absolute、空路径、NUL、`.`/`..` component；所有 path argv 来自当前 snapshot，前置 `--`，禁止 provider/raw markdown 注入。
- status/raw/numstat 使用 NUL-delimited bytes parser，保留空格、tab、换行、non-UTF8 与 rename old/new path。escaped label 只用于显示，永不反向用于 Git/Open in。
- snapshot 区分 staged、unstaged、untracked、deleted、renamed、conflicted；默认聚合 HEAD→working tree 审阅，同时保留 staged/unstaged layer，不能丢失同路径双层变更。unborn HEAD 不硬编码 SHA-1 empty-tree oid。
- tracked patch 固定 rename detection，并关闭 external diff/textconv。untracked regular text 经同一 private fence 读取并合成 addition hunk；binary、非 UTF-8 内容、symlink/special file、过大或读取前后 identity 变化只呈 bounded metadata placeholder。binary numstat `-` 不伪造行数，deleted 只用 Git preimage。
- command timeout、per-stream/per-command bytes、per-file lines/bytes 与 total snapshot 上限在 T30 固定为集中常量并做边界测试；触顶显示 truncated/too-large，不由 UI 放宽。

### C4 · artifact 来源可证明且只做 bounded preview

- artifact model 完全来自当前 ephemeral workspace snapshot，不建 artifacts 表、不新增事件。只有 strict successful write/edit projection 能解析出当前 `WorkspaceFileId`，且 projection 的相对身份与当前 snapshot exact 一致时，才标记 `agent artifact`；stale/invalid projection 不标记。
- bash 创建/修改的文件及无法关联 strict write/edit projection 的条目只能叫 `workspace change`，不得推断为 agent artifact。artifact 来源标签不改变路径围栏或权限。
- 卡片覆盖 file/report/image 安全投影：opaque id、escaped label、类型、bounded size/line/dimension/frame metadata 与 preview 状态。文本只在 bytes/lines 上限内预览；binary/unsupported/竞态显示 metadata-only。
- image preview 只有在 decode **之前**可证明 encoded bytes、dimensions、frame count 与 decoded allocation 都受固定上限约束时才能启用；若 GPUI 现有 API 无法证明，Phase 1 固定 metadata-only + Open in。不得为图片 preview 引入新依赖。

### C5 · Open in 是 fixed allowlist 用户交接

- allowlist 精确为 VS Code、Cursor、Zed、Terminal、default app、Finder；custom/configurable app 延后 Phase 2。统一使用固定 `/usr/bin/open` + 固定 app name/argv，不搜索 PATH 中的 `code`/`cursor`/`zed`，不接受自定义 executable/arguments。
- 用户点击前重新解析当前 `WorkspaceFileId`。目标路径任一 symlink segment、`.git` entry、实际 gitdir、special file 或 regular file `nlink > 1` 均保守拒绝；canonical target 必须在 canonical root 内且 identity 未变。
- VS Code/Cursor/Zed/default/Finder 接收已围栏目标；Terminal 恒定只打开 canonical project root，绝不把 artifact path 当 cwd/argument。所有 path 参数都在 option terminator 后。
- 失败/app 缺失/目标消失显示内联错误，不降级其他程序。测试用 injectable recorder/fake launcher；真实外部应用启动只接受人类实测证据。

### C6 · commit assistance 冻结为两阶段 selected staging

- trusted mutation 的 exact safe prefix 固定为 `/usr/bin/git --no-pager -c core.hooksPath=/dev/null -c core.fsmonitor=false -c color.ui=false`。commit message 是 bounded、非空、无 NUL 的 UTF-8 in-memory value；固定 commit argv 为该 prefix + `commit --no-gpg-sign --file=- --cleanup=verbatim`，message 经 child stdin 写入后关闭；**不创建 `/private/tmp` 或任何 message file**。hooks disabled 与 `--no-gpg-sign` 是 Phase 1 固定兼容限制，进入报告。
- 所有 trusted mutation（stage/commit/switch）固定带 `-c core.hooksPath=/dev/null` 与 C2 安全配置；禁止 `--amend`、`--allow-empty`、`--no-verify`、push、force、reset、restore、stash、clean 或任意用户/模型 options。
- **阶段一 Prepare**：面板完整展示 existing staged set（始终包含、不可取消）与用户从当前 snapshot 显式选择的 workspace paths。用户第一次确认后，复验 captured HEAD/index/status、conflict 与 selected ids；`git check-attr filter` 任一 selected path 非 unset/unspecified、任何 race/差异均 fail closed。随后只执行 C6 exact safe prefix + `add -A -- <trusted snapshot paths>`。
- Prepare 后必须重新读取真实 HEAD/status/index，展示 exact final index 与 bounded patch；existing staged 必然仍在。若 final index 与“existing staged + selected staging”预期不一致、出现 filter/conflict/race 或 refresh 失败，则不进入 Commit；不得 reset/unstage/rollback，保留真实 Git 状态并要求用户处理/刷新。
- **阶段二 Commit**：用户编辑/确认最终 message，并第二次显式确认 commit **entire displayed final index**。执行前再次复验 HEAD 与 exact index bytes；任何差异 fail closed。模型 draft、默认焦点、重复点击、窗口/项目/线程切换都不能隐式确认。
- commit 无论 success/nonzero/timeout/cancel/ambiguous，都刷新真实 HEAD/status/index 后再呈现结果；只有 HEAD 改变且 committed tree/path set 与 displayed final index 一致才报告成功。不得因不确定结果自动重试。
- 模型 draft 只能由用户点击请求，向现有 Provider 发送 bounded diff 摘要并明确提示；返回 tool call/越界/非法 UTF-8 视为失败。无 provider 时可手写；自动验收只用 MockProvider，模型永不能 stage/commit。

### C7 · branch selector 受 active-state 与 clean-state 双门禁

- 只列 `refs/heads/*` local refs，并同时捕获每项 OID；raw ref 留在 private service，UI 只持 opaque branch id + escaped label。拒绝 detached target、remote guess、create、unknown/stale id 或 captured OID 变化。
- dirty/conflict（含 staged、unstaged、untracked、unmerged、merge/rebase/cherry-pick）一律拒绝。active agent run、pending permission、pending plan review、打开的 commit panel 任一存在也拒绝，不能在活动执行上下文切换 root。
- 用户显式选择后，固定执行 C6 exact safe prefix + `switch --no-guess <validated-local-branch>`；禁止 remote guess/create/detach/force/stash/reset/clean/checkout。无论 exit success/failure/timeout，都刷新真实 HEAD/status/branch 后再显示结果，绝不按期望值伪造 UI。

### C8 · UI、依赖与后置范围

- Diff viewer 默认 unified，可切 side-by-side；文件数、+x/-y、untracked/binary/rename；逐文件折叠与 next/previous hunk。新增行 `success` 8% token 背景、删除行 `danger` 8%、行号 `text-tertiary`、`@@` hunk `code-bg`，无硬编码颜色/字号。
- 语法高亮只复用现有 `vega_markdown`/tree-sitter 四语言；不激活/新增 `similar` 或其他 crate。S6 零新依赖、零 DDL、Cargo.lock 不因依赖变化而改。
- Composer 本 Sprint 只补 A2-16 分支选择器。A2-12 `@引用`、A2-21 `/命令`、A2-14 模型选择器后置；`>8` 行独立 inner-scroll 留 S8。

---

## 卡依赖图

```text
S6 SDD PR
  └─▶ T30 snapshot/diff headless service
          └─▶ T31 Diff UI + stats + hunk navigation
                  └─▶ T32 artifact cards + Open in
                          └─▶ T33 guarded branch selector
                                  └─▶ T34 two-stage commit assistant
                                          └─▶ T35 end-to-end acceptance + report
```

> 工作流严格串行：每卡独立 sibling worktree 与 PR，squash merge 后才开下一卡。

## T30 · Git snapshot/diff headless 服务（A5-01/A5-03）

- **前置/参考**：S5 T29 + S6 SDD；C1-C3/C8；exec-guide §3/§4。
- **范围**：`vega_conversation` private workspace service + safe shared view types；app 必要装配。不得改 UI/DDL/事件 enum，不注册 model tool。
- **产出**：env-scrubbed fixed `/usr/bin/git` runner；concurrent bounded stdout/stderr drain + timeout/cancel kill/reap；canonical root；private raw path + shared opaque `WorkspaceFileId`；generation snapshot；tracked/staged/unstaged/untracked/deleted/rename/conflict/binary/non-UTF8 diff 与统计。
- **验收**：temp repo 覆盖同路径 staged+unstaged、untracked、delete、rename、binary、space/tab/newline/non-UTF8、symlink/special、unborn/detached/non-git、损坏/超限/timeout/cancel/nonzero；恶意 Git env/config/pager/fsmonitor/external diff/textconv 均不生效；pipe flood 不死锁且 bounded；raw path/正文/stderr 不进 shared type/event/Debug/log；旧 generation 不覆盖新 snapshot。
- **命令**：`cargo test -p vega_conversation git_workspace`；`cargo clippy -p vega_conversation --all-targets -- -D warnings`；`cargo tree -p vega_conversation`。
- **commit**：`feat(A5-01): add private git workspace snapshots`（≤3 commits）。
- **禁区/停止**：不执行 mutation，不新增 ConversationEvent/RuntimeEvent；std runner 无法闭合即 `[BLOCKED] S6-T30`。

## T31 · Diff UI + 统计 + hunk 导航（A5-02/A5-03）

- **前置/参考**：T30；ui-spec §2-§5；C3/C8。
- **范围**：`vega_ui` diff panel/model/rows；只消费 opaque shared snapshot，不调用 Git/SQLite。
- **产出**：unified 默认 + side-by-side；统计条；逐文件折叠；next/previous hunk；tracked/untracked/rename/binary/too-large/error 状态；四语言高亮降级。
- **验收**：GPUI 覆盖混合 layer、opaque id、non-UTF8 label、折叠、hunk 首尾、view toggle、键盘与 refresh stable key；addition/deletion 8% token、line/hunk token、硬编码色值零新增；注入 snapshot 的交互 <100ms，无 render-path IO。
- **命令**：`cargo test -p vega_ui diff`；`cargo clippy -p vega_ui --all-targets -- -D warnings`；色值 scan。
- **commit**：`feat(A5-02): render navigable workspace diffs`；必要时 `feat(A5-03): add workspace change statistics`。
- **禁区**：不做 inline comment/editor/LSP，不引 `similar`。

## T32 · 可证明产物卡 + fixed Open in（A5-04/A5-05）

- **前置/参考**：T31；PRD v0.3.3 D5/A5；C1/C4/C5/C8。
- **范围**：private artifact resolver/launcher safe view + `vega_ui` artifact card；无 DB/event/migration。
- **产出**：strict write/edit + current snapshot 才标 agent artifact；bash-only 条目为 workspace change；file/report/image bounded metadata/preview；VS Code/Cursor/Zed/Terminal/default/Finder fixed `/usr/bin/open` handoff。
- **验收**：strict/invalid/stale write-edit projection、bash-created、text/markdown/image、超限/binary、symlink segment/root escape/`.git`/gitdir/special/hardlink、identity race、opaque id expiry；fake launcher 精确断言 allowlist argv，Terminal 恒 project root，未点击/失败均零 spawn。若 decode 前无法证明 image 全上限，断言 metadata-only。
- **命令**：`cargo test -p vega_conversation artifact`；`cargo test -p vega_ui artifact_card`；两个 crate clippy all-targets。
- **commit**：`feat(A5-04): add bounded artifact cards`；必要时 `feat(A5-05): add fixed external app handoff`。
- **禁区/停止**：不自动启动真实 app、不 custom executable、不为 image 加依赖；需新依赖即 `[BLOCKED] S6-T32`。

## T33 · Composer guarded branch selector（A2-16）

- **前置/参考**：T32；C1-C3/C7/C8。
- **范围**：private local-ref/switch coordinator + `vega_ui` branch selector + app active-state wiring；零 DDL/event。
- **产出**：local refs + captured OID；opaque branch id；dirty/conflict 与 run/permission/plan-review/commit-panel active guard；fixed hooks-disabled `switch --no-guess`；所有 exit 后真实 refresh。
- **验收**：clean switch；dirty/staged/untracked/conflict/operation-in-progress；active 四状态；detached/remote/unknown/stale/OID race；success/nonzero/timeout/cancel 后均以真实 HEAD/status 为准；argv 无 create/detach/force/stash/reset/clean/checkout。
- **命令**：`cargo test -p vega_conversation branch`；`cargo test -p vega_ui branch_selector`；两个 crate clippy all-targets。
- **commit**：`feat(A2-16): add guarded branch selection`。
- **禁区/停止**：不自动切换、不 remote guess；active-state 无法可靠获得则 `[BLOCKED] S6-T33`。

## T34 · 两阶段 commit assistant（A5-06）

- **前置/参考**：T33；tech-spec §4.4 trusted handoff；C1-C3/C6/C8。
- **范围**：private trusted stage/commit coordinator、existing provider mockable draft、`vega_ui` commit panel；零 DDL/event/temp file。
- **产出**：bounded draft；Prepare 显示 existing staged + selected paths并第一次确认；filter/conflict/race preflight；C6 exact safe prefix + `add -A -- <trusted snapshot paths>`；刷新 exact final index；第二次确认；bounded UTF-8 stdin + fixed `commit --no-gpg-sign --file=- --cleanup=verbatim`；所有结果 post-refresh。
- **验收**：MockProvider draft/edit/cancel/error/tool-call；existing staged 必含、selected delete/rename/untracked/space/non-UTF8；filter/conflict/HEAD/index/status race；Prepare 后 unexpected index；两次确认/重复/关闭；stdin bound/NUL/invalid UTF-8；success/nonzero/timeout/cancel/ambiguous post-state；hooks/signing disabled；断言零 message file、零 rollback/push/network，模型永不 mutation。
- **命令**：`cargo test -p vega_conversation trusted_git`；`cargo test -p vega_ui commit_panel`；两个 crate clippy all-targets。
- **commit**：`feat(A5-06): add two-stage commit assistance`。
- **禁区/停止**：不 amend/allow-empty/no-verify/reset/unstage；系统 Git 与固定 stdin 契约矛盾即 `[BLOCKED] S6-T34`。

## T35 · S6 端到端验收 + 报告 + README（A5-02）

- **前置/参考**：T30-T34 均 squash merge；phase1-plan S6 DoD、exec-guide §3/§7、ui-spec §6、tech-spec §8。
- **场景**：temp repo + mock agent 生成 tracked edit/rename/untracked text/binary；refresh → diff stats/review/hunk → strict artifact + fake Open in → guarded branch negative/positive → MockProvider draft → Prepare existing staged+selected → exact final index → second confirm → stdin commit → HEAD/tree/status postcondition。
- **门禁**：fmt、workspace clippy all-targets、test all-features、build all-targets、diff-check；runtime/tools headless；共享类型/event enum、UI direct SQLite、production unwrap/expect、色值、六表/migration、key/正文/raw path/temp message file与危险 Git verb scans。
- **报告**：`docs/vega-s6-report.md` + README；列出 SDD/T30-T35 PR/squash commit、原始门禁/精确测试数、S6 DoD、ui-spec §6、红线、偏离。明确真实 API/key/费用未执行；真实外部应用、Light/Dark/CJK/960×600/全键盘/竞品截图/ProMotion/P1-P8 未自动验证；fake/GPUI test 不冒充人工硬件实测。
- **commit**：`feat(A5-02): close Sprint 6 diff review acceptance`。
- **禁区/停止**：只用 temp repo/MockProvider/fake launcher，不碰真实用户 repo、不启动真实 app/LLM；环境失败按 `[BLOCKED] S6-T35`。

---

## S6 完成定义（DoD）

- [ ] PRD v0.3.3 与 phase1-plan S6 已闭合；SDD 先于代码；T30-T35 逐卡 squash merge；master 四门禁全绿。
- [ ] phase1-plan S6 原文链路完整：agent 改代码 → diff 审阅 → Open in 外部应用 → 生成 commit message → 两阶段用户确认提交。
- [ ] snapshot/diff 含 staged/unstaged/untracked、统计、高亮、hunk、折叠、unified/side-by-side；异常路径/content fail closed。
- [ ] raw GitPath 只在 private service；UI 只持 opaque id/escaped label；无新增 ConversationEvent/RuntimeEvent，raw path/正文/stderr 不进 event/Debug/log/DB。
- [ ] 只有 strict successful write/edit + current identity 标 agent artifact；bash-created 为 workspace change；image 无安全 decode 证明则 metadata-only。
- [ ] Open in fixed allowlist、user click、fence/fixed argv；Terminal 恒 project root；custom handoff 后置。
- [ ] branch local refs + captured OID；dirty/conflict/active run/permission/plan/commit panel 都拒绝；`switch --no-guess` 任意 exit 后真实 refresh。
- [ ] Prepare 始终含 existing staged + selected paths；`git add -A` 后展示 exact final index；第二次确认 commit entire index。模型不能 mutation；无 rollback/push/network。
- [ ] commit message bounded UTF-8 stdin，固定 `--no-gpg-sign --file=- --cleanup=verbatim`；不创建 temp message file；全部 mutation hooks disabled。
- [ ] Git env scrub、并发 bounded drain、timeout/cancel kill/reap；`std::process` only、零新依赖/DDL，仍六表，runtime/tools headless，UI 不直连 SQLite。
- [ ] ui-spec §6 分别记录自动化/人工/硬件证据；任一未测不得写 ✅；报告/README 无隐瞒。

## ui-spec §6 Sprint 末检查矩阵

| 检查项 | S6 自动化最低证据 | 必须诚实记录的人工/硬件边界 |
|---|---|---|
| 颜色/字体 token | diff/artifact/branch/commit token assertions + hardcoded scan | 真实窗口字体观感需人工 |
| Light/Dark | 两套 theme component state tests | 真实窗口切换无闪烁需人工 |
| CJK 混排 | CJK/emoji/escaped label 不 panic、不截坏 | fallback/豆腐块需真实窗口 |
| 键盘全流程 | focus tests 覆盖 diff→Open in→Prepare→Commit | 建会话到提交真实链路需人工 |
| 960×600 | layout constraint tests（能力允许时） | 像素级破裂需真实截图 |
| P1-P8 | `xtask bench` 原始值/unsupported | ProMotion/首帧/RSS等留 S8，不虚称 |
| Codex/ZCode 并排 | 无自动化替代 | 必须人工截图，未做即 ⚠️ |

## 已知偏离与后置（原样进入 Sprint 报告）

1. A2-12 `@引用`、A2-21 `/命令`、A2-14 模型选择器后置；S6 只交付 A2-16。
2. Composer `>8` 仍是 caret-follow 8-row viewport，不是独立 wheel/vertical inner-scroll，ui-spec §4.4 部分满足，留 S8。
3. hooks 与 commit signing 固定关闭；依赖 hook/signing 的仓库需离开 Vega 在终端提交，不宣称兼容。
4. Open in v1 只有固定 VS Code/Cursor/Zed/Terminal/default/Finder；custom/configurable handoff、PR assistance、Diff v2/advanced polish 留 Phase 2。
5. image 若无法在 decode 前证明 bytes/dimensions/frame/allocation 上限，Phase 1 metadata-only + Open in，不引新依赖。
6. fake launcher/MockProvider 只证明边界；真实 app、LLM/key/费用与 dogfood 属人类活动。
7. A5 Checkpoint/回退、PR 创建、行内评论均 Phase 2+；真实 UI/硬件/P1-P8 字面复测逐项留报告/S8。

## 未决阻塞检查

- 当前无未决 spec 阻塞；PRD v0.3.3 已按 2026-08-30 人类裁决与 phase1-plan S6 对齐。
- T30 先实测 env scrub/NUL parser/concurrent drain/kill-reap/non-UTF8；T34 先实测 `git commit --no-gpg-sign --file=- --cleanup=verbatim` stdin 与两阶段 index postcondition。若系统行为矛盾，立即上报，不降级 shell/lossy path/temp file/宽权限。

## 变更记录

- v0.1 (2026-08-30) S6 开工 SDD：T30-T34、headless Git boundary、diff/artifact/Open in/commit/branch 与报告边界初稿。
- v0.2 (2026-08-30) 人类批准方案 A：PRD v0.3.3 对齐 phase1-plan；raw path 私有化、Git env/process lifecycle、artifact provenance、fixed Open in、active branch guard、两阶段 selected staging + in-memory stdin commit 定稿；重排 T30-T35。
