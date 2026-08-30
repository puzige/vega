# ✦ Vega — S6 任务卡（Sprint 6 · Diff 审阅 & 产物 · W11-12）

**版本** v0.1 · 2026-08-30 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt

**S6 目标**（phase1-plan §2）：git 工作区 diff 视图（高亮、hunk 导航）；产物卡片；Open in…（VS Code/Cursor/Zed/Terminal）；commit 辅助；补齐 Composer 分支选择器。

**Sprint DoD**：agent 改完代码后，用户可在 Vega 内审阅包含未跟踪文件的工作区 diff，通过显式点击把围栏内产物交接到外部编辑器，生成并编辑 commit message，最终经受信任且不可由模型直接调用的 Git 路径提交；全过程不引入 shell、外部依赖或新 DDL。

> 本文档合入即为 S6 的 SDD 开工门禁。T30-T34 严格串行，每卡在前一卡 squash merge 后开工。
>
> S6 的 Git stage/commit、分支切换与 Open in 都是**用户动作**，不是 `vega_tools` 工具，也不注册 provider schema。模型只能生成 commit message 草稿，永远不能批准、stage、commit、切分支或启动外部应用。
>
> Phase 1 只做代码 viewer + diff 审阅（PRD D5）；不做自研编辑器/LSP、Checkpoint 回退、PR 创建或终端视图。

---

## S6 最小契约闭合

### C1 · Git 工作区服务是 headless 受信任边界

- Git workspace service 落在 headless `vega_conversation` 协调层；跨 UI 的 `GitPath`、状态、diff、artifact、branch、commit request/result 等共享类型只定义在 `vega_conversation::types`。`vega_ui` 不直接 spawn 进程、不直接读 SQLite，`vega_runtime`/`vega_tools` 不新增反向依赖或同名 wire type。
- 全部 Git 调用只使用 `std::process::Command` 启动固定的 `/usr/bin/git`，`current_dir` 固定为 canonical project root；禁止 `sh -c`、`zsh -c`、拼接命令字符串、Git alias、自定义 executable 或从模型输入构造 option。公共实现不得出现 production `unwrap/expect`。
- read-only 命令只允许固定模板的 `status`、`diff`、`rev-parse`、`for-each-ref`、`check-attr`；统一禁 pager/color/prompt、external diff/textconv 与 fsmonitor hook，输出与超时均有固定上限。非零退出、timeout、输出溢出、损坏记录、非 git repo 或 HEAD/索引竞态都返回 typed、脱敏、可重试错误，不把 stderr、绝对路径或文件正文写日志。
- 进程调用必须离开 UI 上屏关键路径，在 bounded blocking worker 中完成；同项目只保留一个有效 refresh generation，旧 generation 结果不得覆盖新快照。工具完成事件触发即时 refresh，面板可见时再用有界低频 polling 捕获外部编辑器改动；折叠/导航等纯 UI 交互不等待 Git IO。
- 每个可变动作携带读取时的 HEAD oid、分支与 exact status/path-state snapshot；动作前重新读取并逐字节比较，任何变化均 fail closed，提示刷新后重试。快照新鲜度不依赖内容 hash，也不把 SHA-256 引入 S6 wire 契约。

### C2 · project-root/path fence 与 bytes-first GitPath

- project root 必须存在、为目录且 canonicalize 成功；所有文件参数先拒绝 absolute、空路径、NUL、`.`/`..` component，再验证 lexical join 位于 root。需要读取或 Open in 的目标还必须 canonicalize 后仍位于 root；不存在目标只允许由 Git status 证明为删除项，不得交给 Open in。
- Git status/diff 使用 NUL-delimited machine-readable 输出保留空格、tab、换行与 rename 的 old/new path；内部身份保留 macOS `OsString`/raw bytes，UI 只显示稳定 escaped label。不得把 `to_string_lossy()` 的展示串反向用于 stage、diff、Open in 或分支动作。
- symlink 作为 Git 条目可显示其 link metadata，但 artifact preview/Open in 不跟随到 root 外；FIFO、socket、device 等特殊文件只显示 metadata，不读取、不 launch。围栏校验失败不产生部分结果。
- 每次固定 argv 的 path list 前都有 `--`；路径来自当前受信任 snapshot 或用户在该 snapshot 中的选择，禁止 provider/raw markdown 直接注入。

### C3 · diff/status 覆盖 tracked、staged、untracked 与异常文件

- 工作区快照区分 staged、unstaged、untracked、deleted、renamed、conflicted；默认聚合为从 HEAD 到当前工作树的审阅视图，同时保留每个 hunk 的 layer/source，不能因同一路径同时 staged+unstaged 而静默丢失任一层。unborn HEAD 使用显式 empty-index 路径，不硬编码 SHA-1 empty-tree oid。
- tracked patch 固定启用 rename detection，并显式关闭 external diff/textconv；status/raw/numstat 均走 `-z` bytes parser。rename 保留 old/new identity，空格/控制字符只影响 escaped label，不影响动作参数。
- `git diff` 默认不含 untracked 内容，因此对 status 证明的 untracked regular file 走同一 read fence，合成整文件 addition hunk并计入文件数/+N；读取前后 metadata/identity 变化、过大、symlink/special file、binary 或非 UTF-8 时只呈 bounded metadata placeholder，不猜文本 diff。
- binary 文件显示 binary + size/rename metadata，numstat 的 `-` 不伪造行数；非 UTF-8 路径以 escaped label 展示，非 UTF-8 内容不做文本 patch。删除文件只用 Git patch/preimage，不从工作树读取。
- status/diff/单文件 preview 都使用集中常量的 command timeout、per-command bytes、per-file lines/bytes 与 total snapshot bytes 上限；触顶时显示明确 truncated/too-large 状态，parser 不 `read_to_end` 无界增长。具体常量在 T30 首个实现 commit 固定并由边界测试守护，后续不得由 UI 临时放宽。

### C4 · stage/commit 是显式确认后的 trusted mutation

- stage/commit 入口只由当前窗口用户点击触发；确认面板必须展示 canonical project 的安全显示名、当前分支、exact selected path labels、staged set 与最终 commit message。提交按钮默认不因模型输出自动获得焦点或触发，重复点击幂等；窗口/项目/线程切换视为取消。
- stage 只接收当前 snapshot 中用户勾选的 `GitPath`，固定 argv 且路径在 `--` 后；既有 staged 项必须在确认面板完整列出。执行前复验 HEAD/status/selected paths；index 中出现未展示项、conflict、路径身份变化或 `git check-attr filter` 显示 selected path 配有外部 clean filter时 fail closed。失败后不运行 reset/restore/stash 伪造回滚，保留 Git 的真实状态并要求刷新。
- commit 只提交确认面板展示且复验一致的 index；禁止 `--amend`、`--allow-empty`、`--no-verify`、签名、push、force、reset、stash 或任意额外 Git option。Phase 1 固定关闭 commit signing，并把 hooks path 置为无 hook 的系统空路径，避免受信任桥间接执行仓库脚本；此兼容性限制进入 S6 报告。
- commit message 先是可编辑草稿。模型生成只能由用户点击请求，使用现有 Provider/MockProvider 管线并明确提示会把 bounded diff 摘要发给已配置 provider；生成结果不得携带 tool call，也不能直接触发任何 Git 动作。无 provider 时允许用户手写，不以真实 API/key 作为自动验收前提。
- `git commit --file <message-file>` 的文件只能位于 canonical `/private/tmp` 下每次独占创建的 Vega-owned 随机 0700 专用目录；message file 必须 create-new、regular、0600、no symlink，并在 spawn 前重验 canonical containment 与 dev/inode identity。完成、失败、取消、timeout 后仅对 exact identity 做 no-follow 清理；身份变化或清理失败时不递归追随，返回脱敏错误并留给后续安全 GC。禁止 broad `/private/tmp` 清理或把 message 放项目树/数据目录。
- commit 成功后重新读取 HEAD/status，只有 HEAD 改变且提交内容与确认 set 一致才报告成功；失败不得把可能已成功的 commit 重放。S6 不 push、不创建 PR、不产生网络请求。

### C5 · 分支选择器 fail closed

- Composer 分支选择器显示当前 branch 或 detached 状态，只列 `refs/heads/*` 中已存在的本地分支；raw ref 与 escaped label 分离，拒绝空值、NUL、控制字符、leading `-` 或未出现在最新枚举中的输入。
- 切换只能由用户显式选择并确认，固定 argv 使用 `git switch <existing-branch>`；执行前复验当前 branch/HEAD 与 exact clean status。任何 staged、unstaged、untracked、unmerged/conflict、进行中的 merge/rebase/cherry-pick 或快照变化都 fail closed。
- 禁止 `--force`、`-C/-c` 新建分支、checkout、stash、reset、clean 或自动解决冲突；失败只刷新并展示脱敏错误，不修改用户状态。S6 不实现新建/删除/重命名/remote branch。

### C6 · Diff 与产物 UI 只使用 token，preview 有界

- Diff viewer 默认 unified，可切 side-by-side；文件列表显示文件数、+x/-y、untracked/binary/rename，逐文件折叠且能跳上/下 hunk。新增行使用 `success` 8% token 背景、删除行使用 `danger` 8% token 背景、行号用 `text-tertiary`、`@@` hunk 头用 `code-bg`；禁止新增硬编码颜色/字号。
- 文本语法高亮仅复用现有 `vega_markdown`/tree-sitter 四语言能力；未知语言和 pending/invalid 内容降级等宽纯文本。不得引入 `similar` 或任何新 crate，diff 事实以 Git patch 为准。
- 产物卡至少覆盖 file/report/image 三类安全投影：相对路径、类型、bounded size/line/dimension metadata、preview 状态与 Open in 动作；Finder 作为 A5-05 默认交接项。文本/图片只在集中 per-kind bytes/pixels 上限内预览，超限/binary/unsupported/读取竞态显示 metadata placeholder；正文不写日志或 SQLite，S6 不建 artifacts 表。
- Open in 只由用户点击发起，目标必须通过 C2 围栏并是允许类型；应用为固定 allowlist：VS Code、Cursor、Zed、Terminal、default app、Finder。Phase 1 统一使用 `/usr/bin/open` 的 fixed argv/app name，不搜索 PATH 中的 `code`/`cursor`/`zed`，不允许模型、自定义 app name、shell 或任意 arguments。
- Open in 失败、应用缺失与目标消失均为内联错误，不降级执行其他程序。自动测试用 injectable recorder/fake launcher 验证 argv 与零 spawn 分支；真实外部编辑器启动必须在报告标为人工未验证或附人类实测证据。

### C7 · 零依赖、零 DDL、范围后置

- S6 只使用 `std::process` 与 workspace 已存在依赖；不激活/新增 `similar`，Cargo.lock 不应因依赖解析变化而改动。若实现证明必须新增依赖，按停止条件上报。
- 不新增或修改 migration，不建第七表；diff/artifact/branch snapshot 为运行时派生状态。必要的用户偏好仅放现有配置文件的 additive 可选字段，不能写 key、文件正文或绝对临时路径。
- Composer 本 Sprint 只补 A2-16 分支选择器。A2-12 `@引用`、A2-21 `/命令`、A2-14 模型选择器继续后置并在报告注明；Composer `>8` 行仍是 caret-follow 的 8-row painted viewport、尚无独立 wheel/vertical inner-scroll，归 S8 打磨。

---

## 卡依赖图

```text
S6 SDD PR
  └─▶ T30 git workspace status/diff headless service
          └─▶ T31 Diff viewer + stats + hunk navigation
                  └─▶ T32 artifact cards + Open in
                          └─▶ T33 commit helper + branch selector
                                  └─▶ T34 end-to-end acceptance + report
```

> 工作流严格串行：每卡独立 sibling worktree 与 PR，squash merge 后才开下一卡。

## T30 · Git workspace status/diff headless 服务（A5-01/A5-03）

- **前置/参考**：S5 T29 + S6 SDD；phase1-plan S6；C1-C3/C7；exec-guide §3/§4。
- **范围**：`vega_conversation::types` 与新的 headless workspace/git service；app 只做必要装配。不得改 UI/DDL，不注册 model tool。
- **产出**：canonical root + bytes-first `GitPath`；fixed `/usr/bin/git` runner；bounded/cancellable generation refresh；HEAD/branch/status snapshot；staged/unstaged/untracked/deleted/rename/conflict/binary/non-UTF8 models；tracked patch + synthesized untracked additions；文件数与 +/- 统计。
- **验收**：临时 Git repo 覆盖普通修改、同文件 staged+unstaged、untracked、删除、rename、binary、space/tab/newline path、macOS non-UTF8 path/content、symlink/special file、unborn HEAD、detached HEAD、non-git root、损坏/超限/timeout/nonzero；断言 untracked 纳入视图，bytes identity 不经 lossy label 回流，external diff/textconv/pager/prompt/fsmonitor 不执行，旧 refresh generation 不覆盖新快照。
- **命令**：`cargo test -p vega_conversation git_workspace`；`cargo clippy -p vega_conversation --all-targets -- -D warnings`；`cargo tree -p vega_conversation`。
- **commit**：`feat(A5-01): add fenced git workspace snapshots`（≤3 commits）。
- **禁区/停止**：不得执行 stage/commit/switch，不用 shell，不读 root 外内容；若 std runner 无法实现超时/有界输出与 bytes path 验收则 `[BLOCKED] S6-T30`。

## T31 · Diff viewer + 统计 + hunk 导航（A5-02/A5-03）

- **前置/参考**：T30；ui-spec §2-§5；C3/C6。
- **范围**：`vega_ui` 的 diff panel/model/rows；只消费 T30 shared snapshot，不直接调用 Git/SQLite。
- **产出**：unified 默认 + side-by-side toggle；文件统计条；逐文件折叠；next/previous hunk 与键盘焦点；tracked/untracked/rename/binary/too-large/error 状态；现有四语言只读高亮降级。
- **验收**：GPUI tests 覆盖空态、混合 staged/unstaged/untracked、binary/rename/space/non-UTF8 label、折叠状态、首尾 hunk wrap/no-op、unified↔side-by-side、键盘全可达与 snapshot refresh 后稳定 key；断言 addition/deletion 8% token background、line number/hunk token 且色值 grep 零新增。交互响应以注入快照测量 <100ms，不用同步 Git IO 冒充 UI 测试。
- **命令**：`cargo test -p vega_ui diff`；`cargo clippy -p vega_ui --all-targets -- -D warnings`；`rg -n '#[0-9a-fA-F]{6}|rgba?\(' crates/vega_ui/src`（既有白名单逐条解释）。
- **commit**：`feat(A5-02): render navigable workspace diffs`；必要时 `feat(A5-03): add workspace change statistics`。
- **禁区**：不做 inline comment/A5-10、自研编辑器/LSP，不引 `similar`，不在 paint/render 路径执行 IO。

## T32 · 产物卡片 + Open in…（A5-04/A5-05）

- **前置/参考**：T31；PRD D5/A5；ui-spec §4.2/§4.6；C2/C6/C7。
- **范围**：headless bounded artifact reader/launcher shared contract + `vega_ui` artifact card；无 DB/migration。
- **产出**：file/report/image 卡片与 bounded preview；default app/Finder/VS Code/Cursor/Zed/Terminal 交接菜单；固定 `/usr/bin/open` argv、路径围栏、用户点击 gate、inline failure；tool result 与 Git snapshot 可产出 artifact projection但不持久化正文。
- **验收**：temp root 覆盖文本/markdown/图片 metadata、超限/binary/unsupported、deleted、root 内 symlink、外跳 symlink、special file、读取竞态、空格/leading-dash/non-UTF8 path；fake launcher 精确断言每个 allowlist argv、cwd、无 shell，未点击/围栏失败/app 失败均零后续 spawn。GPUI 测 preview 展开/折叠、键盘菜单与内联错误。
- **命令**：`cargo test -p vega_conversation artifact`；`cargo test -p vega_ui artifact_card`；两个 crate clippy all-targets。
- **commit**：`feat(A5-04): add bounded artifact previews`；必要时 `feat(A5-05): add fenced external app handoff`。
- **禁区/停止**：不自动启动真实应用，不搜索 PATH/自定义 executable；若 GPUI 图片能力需白名单外依赖则 metadata placeholder 并按 `[BLOCKED] S6-T32` 上报后等待裁决，不自行加 crate。

## T33 · Commit 辅助 + Composer 分支选择器（A5-06/A2-16）

- **前置/参考**：T32；tech-spec §4.4 的 S6 trusted handoff；C1-C5/C7。
- **范围**：headless trusted Git action coordinator、existing provider 的 mockable commit-draft path、`vega_ui` commit panel/branch selector、app wiring；零 DDL。
- **产出**：用户触发的 bounded commit draft；可编辑确认面板；filter/hook/signing/竞态 fail-closed preflight；专用 `/private/tmp` message file + safe cleanup；selected paths stage + exact index commit + postcondition；current/local branch list与 clean-only `git switch`。
- **验收**：temp repos + MockProvider 覆盖 draft/edit/cancel、provider error/恶意 tool call、existing staged set、selected untracked/rename/space/non-UTF8 path、external clean filter、hook/signing config、conflict、HEAD/status race、commit success/nonzero/timeout/ambiguous post-state、temp create/identity/cleanup failure；断言模型输出永不触发 mutation，commit 仅含确认 set，message 不进 repo/log/DB，零 push/network。分支覆盖 clean switch、dirty/untracked/staged/conflict/detached/race/unknown/leading-dash branch 均按 C5。
- **命令**：`cargo test -p vega_conversation trusted_git`；`cargo test -p vega_ui commit_panel`；`cargo test -p vega_ui branch_selector`；两个 crate clippy all-targets。
- **commit**：`feat(A5-06): add user-confirmed commit handoff`；必要时 `feat(A2-16): add fail-closed branch selection`。
- **禁区/停止**：不允许 model/provider 直接调用、不 push/PR/amend/force/stash/reset/clean；若系统 Git 行为与 C4 固定契约矛盾则 `[BLOCKED] S6-T33`。

## T34 · S6 端到端验收 + 报告 + README（A5-02）

- **前置/参考**：T30-T33 均已 squash merge；phase1-plan S6 DoD、exec-guide §3/§7、ui-spec §6、tech-spec §8。
- **场景**：在 temp repo 用 mock agent 写出 tracked edit + rename + untracked text/binary/space path；ConversationEvent 完成后刷新 workspace → 统计与 diff 审阅 → 键盘 hunk 导航/折叠 → 产物 bounded preview → fake Open in → MockProvider 生成并人工态编辑草稿 → 明示确认 selected paths → 本地 Git commit → HEAD/内容/status postcondition；另覆盖 dirty branch switch fail closed 与 clean branch switch。
- **门禁**：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --all-features`、`cargo build --workspace --all-targets`、`git diff --check`；runtime/tools cargo tree headless；shared type、UI direct SQLite、production unwrap/expect、硬编码色值、六表/migration add-only、key/正文/绝对 temp path与危险 Git verb scans。
- **报告**：`docs/vega-s6-report.md` + README 状态行；列出 SDD/T30-T34 PR 与 squash commit、原始门禁/精确测试数、S6 DoD、ui-spec §6、红线、偏离/后置。必须明确真实 API/key/费用未执行，真实 VS Code/Cursor/Zed/Terminal/Finder 启动、真实窗口 Light/Dark/CJK/960×600/键盘走查、Codex/ZCode 并排截图与 ProMotion 120Hz/P1-P8 未自动验证；不得用 fake launcher/GPUI unit test 冒充人工或硬件实测。
- **commit**：`feat(A5-02): close Sprint 6 diff review acceptance`。
- **禁区/停止**：e2e 只用 temp repo/MockProvider/fake launcher，不碰真实用户 repo、不真实提交/启动 app/调用 LLM；环境问题按 `[BLOCKED] S6-T34` 停止。

---

## S6 完成定义（DoD）

- [ ] SDD PR 先于代码；T30-T34 逐卡 squash merge；master 四门禁全绿。
- [ ] phase1-plan S6 原文链路完整：agent 改完代码 → diff 视图审阅 → Open in 外部编辑器交接 → 生成 commit message 并经用户确认提交。
- [ ] git 工作区 diff 含 staged/unstaged/untracked、统计、高亮、hunk 导航、逐文件折叠；unified 默认且可切 side-by-side。
- [ ] binary/rename/space/control/non-UTF8/large/failure 状态可见且 fail closed；raw path identity 不经 lossy UI 串回流。
- [ ] artifact file/report/image 卡片 bounded preview；Open in 只由用户点击、path fenced、fixed argv，失败不降级 shell/其他 app。
- [ ] commit draft 不能触发 mutation；stage/commit 必须展示 exact set 并显式确认，复验竞态，专用 `/private/tmp` message file 安全清理；无 push/network。
- [ ] 分支选择器显示当前/local branches；dirty/conflict/race 全部拒绝，不 force/stash/reset/clean。
- [ ] `std::process` only、零新依赖、零 DDL；仍恰好六表；runtime/tools headless；UI 不直连 SQLite；共享类型唯一在 `vega_conversation::types`。
- [ ] ui-spec §6 每项分别记录自动化、人工与硬件证据；颜色/字体 token、Light/Dark、CJK、键盘、960×600、P1-P8、竞品并排截图任一未测不得写 ✅。
- [ ] S6 报告/README 已更新；真实 key/费用/编辑器/UI/硬件未自动验证与所有偏离/后置无隐瞒。

## ui-spec §6 Sprint 末检查矩阵

| 检查项 | S6 自动化最低证据 | 必须诚实记录的人工/硬件边界 |
|---|---|---|
| 颜色/字体 token | diff/artifact/commit/branch component token assertions + hardcoded scan | 真实窗口字体观感仍需人工 |
| Light/Dark | 两套 theme 下组件渲染/状态 snapshot | 真实窗口切换无闪烁需人工 |
| CJK 混排 | CJK/emoji/escaped non-UTF8 label 不 panic、不截坏 | 字体 fallback/豆腐块需真实窗口 |
| 键盘全流程 | focus/key action tests 覆盖看 diff→Open in→commit | 建会话到提交的真实窗口全链路需人工 |
| 960×600 | layout constraint tests/最小尺寸 snapshot（能力允许时） | 像素级破裂需真实窗口截图 |
| P1-P8 | 跑 `xtask bench` 并记录原始值/unsupported | ProMotion 120Hz、首帧、RSS等未实测项留 S8，不虚称 |
| Codex/ZCode 并排 | 无自动化替代证据 | 必须人工截图比较，未做即 ⚠️ |

## 已知偏离与后置（原样进入 Sprint 报告）

1. A2-12 `@引用`、A2-21 `/命令`、A2-14 模型选择器后置；S6 只交付 A2-16 分支选择器。
2. Composer 1-8 行 sizing 已有；`>8` 仍是 caret-follow 的 8-row painted viewport，不是独立 wheel/vertical inner-scroll，ui-spec §4.4 只部分满足，留 S8。
3. Commit bridge 为避免间接执行仓库代码，Phase 1 固定关闭 hooks 与 commit signing；依赖 hook/signing 的仓库需离开 Vega 后在用户终端提交，报告不得宣称兼容。
4. Open in 自动验收只用 fake launcher；真实 VS Code/Cursor/Zed/Terminal/default app/Finder 启动是人类活动。
5. 模型 commit draft 测试只用 MockProvider；真实 LLM、API key、费用与 dogfood 属人类活动。
6. A5-07/A5-08 Checkpoint 自动打点/回退、A5-09 PR 创建、A5-10 行内评论均属 Phase 2+，S6 不实现。
7. 真实窗口 Light/Dark/CJK/960×600/全键盘、Codex/ZCode 并排截图和 ProMotion/P1-P8 字面复测不由 headless/GPUI 单测替代；逐项交 S6 报告和 S8。

## 未决阻塞检查

- 当前无未决 spec 阻塞；T30 开工先以本机 `/usr/bin/git` 实测 NUL-delimited status/diff、timeout/kill 与非 UTF-8 path，T33 开工先实测 hooks/signing/filter fail-closed argv 和 `/private/tmp` identity cleanup。
- 任一实测若与本契约或系统 Git API 矛盾，只能按停止条件上报，不能退回 shell、PATH executable、lossy path 或宽权限降级。

## 变更记录

- v0.1 (2026-08-30) S6 开工 SDD：T30-T34、headless trusted Git boundary、bytes-first path fence、tracked/staged/untracked diff、bounded artifact/Open in、user-confirmed stage/commit、clean-only branch switch、ui-spec §6 与报告边界定稿。
