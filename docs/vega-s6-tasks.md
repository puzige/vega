# ✦ Vega — S6 任务卡（Sprint 6 · Diff 审阅 & 产物 · W11-12）

**版本** v0.3 · 2026-08-30 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt

**S6 目标**（phase1-plan §2）：git 工作区 diff 视图（高亮、hunk 导航）；产物卡片；Open in…（VS Code/Cursor/Zed/Terminal）；commit 辅助；补齐 Composer 分支选择器。

**Sprint DoD**：agent 改完代码后，用户可审阅包含未跟踪文件的 diff，通过显式点击把围栏内产物交接到 fixed allowlist 外部应用，生成并编辑 commit message，再经过 Prepare 与 Commit 两次用户确认，由不可被模型调用的 trusted Git 路径提交；不引入 shell、新依赖、新事件类型或 DDL。

> 本文档合入即为 S6 的 SDD 开工门禁。T30-T35 严格串行，每卡在前一卡 squash merge 后开工。
>
> **人类裁决（2026-08-30）**：采用 A，以 phase1-plan S6 为准，PRD v0.3.3 已闭合冲突。Phase 1 交付 Diff v1、artifact cards、fixed-allowlist Open in v1、user-confirmed commit assistance v1；Phase 2 保留 Diff v2、custom/configurable handoff、PR assistance 与 advanced polish。D1-D7 不变。
>
> stage/commit、branch switch 与 Open in 都是当前窗口的显式用户动作，不是 `vega_tools` 工具、不注册 provider schema。模型只能生成 bounded commit 草稿，永远不能 Prepare、stage、commit、切分支或启动应用。
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
- bash-created、invalid/stale projection、>1 MiB、无法证明 identity 的条目只能叫 `workspace change`。artifact 来源标签不改变 fence。
- `ArtifactPreviewProjection` 文本/报告最多 1 MiB/10,000 lines/64 KiB line；超限/binary/unsupported/race metadata-only。Phase 1 image **一律 metadata-only + Open in**，不 decode、不引依赖。

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
- launcher 在独立 bounded worker；stdin/stdout/stderr 均 null；timeout 10s，wait/reap `/usr/bin/open`，结果只应用到 current request generation。成功返回后不杀已由 LaunchServices 启动的 app。
- no click、stale/unknown id、fence/identity preflight failure = 0 invocation attempt；app missing、spawn error、nonzero、timeout = exactly 1 attempt且无 retry/fallback/alternate；success = exactly 1 attempt。
- fake launcher 必须按 raw `OsString` 精确断言 space/tab/newline/leading-dash-looking component/non-UTF8 target 的六套 argv；escaped label 永不回流。

### C6 · cross-checked canonical IndexSnapshot + 两阶段 selected staging

- private `IndexSnapshotId` 必须从同一 captured HEAD/request generation 下的 fixed `status --porcelain=v2 -z` 与 fixed `ls-files --stage -z` **交叉构造**；两次 bounded 输入共享 8 MiB/10,000 paths 上限。读取前后 HEAD/generation、raw path membership、tracked status 与 stage entry 任一变化/矛盾都拒绝，不能只信其中一个命令。
- canonical codec bytes-first排序并绑定 captured HEAD、porcelain v2 status classification 与 logical stage entries `(mode, object_oid, raw_path)`。任何 tracked v2 record `XY=.A` 明确代表 intent-to-add，必须 fail closed；`ls-files` 的 stage0/nonzero OID不能推翻该判断。仍拒绝 unmerged stage>0、zero OID、unknown/corrupt record、重复冲突或 overflow。UI 只获 opaque id + bounded safe projection，绝不 hash/read raw `.git/index`。
- 正常 staged empty file 是合法 positive：porcelain `XY=A.` 且 stage0 + repo-format nonzero empty-blob OID（默认 SHA-1 fixture 为 `e69de29...`）；不得把 empty blob误判为 intent-to-add。
- selected `WorkspaceFileId` 展开 literal raw set：rename=old+new，delete=old，add/untracked=new，modify=current；bytes-first dedupe。任何 selected changed `.gitattributes` 直接拒绝。expanded set 经 exact `git check-attr --stdin -z filter`，任一非 `unspecified`/`unset` fail closed。
- **Prepare**：初始 focus Cancel；Esc=Cancel；只有 Cmd+Enter 确认。面板完整展示 existing staged（始终包含、不可取消）+ selected paths。Prepare 前重新执行同一 status+ls-files cross-check codec，复验 HEAD/IndexSnapshot/conflict/filter 后，才用 mutation safe prefix执行 `add -A -- <expanded literal raw paths>`。
- Prepare 后再次以同一 captured HEAD/new generation 的 status+ls-files cross-check codec刷新 exact final `IndexSnapshotId`/bounded cached patch；它必须等于 existing staged + selected staging 的 canonical logical预期，且无 `XY=.A`。任何差异不进入 Commit，不 reset/unstage/rollback，保留真实状态。
- **Commit**：初始 focus Cancel；Esc=Cancel；message editor bare Enter 只换行；只有 Cmd+Enter 第二次确认 commit entire displayed final index。close panel/window/thread/project switch 是 first-wins cancel；重复 callback 不产生第二次 mutation。
- message 是 non-empty/no-NUL/bounded 32 KiB typed UTF-8 `String`。模型仅由用户点击请求，provider summary最多 256 KiB并显式 truncated marker；模型 tool call/超限失败。message/diff-summary用 sentinel 验证不进 Debug/log/event/DB/error。
- commit argv = mutation safe prefix + `commit --no-gpg-sign --file=- --cleanup=verbatim`；message 从内存 stdin 写入，writer/stdout/stderr并发，不建 message file。Commit 前第三次运行同一 status+ls-files cross-check codec并与 displayed final `IndexSnapshotId` exact compare；任何 `XY=.A`/差异均为 zero commit spawn。
- 无论 success/nonzero/timeout/cancel/ambiguous 都刷新真实 HEAD/status/index。成功后 fixed `ls-tree -rz --full-tree HEAD` 解析同一 canonical `(mode, object_oid, raw_path)` codec，与 displayed final index exact compare；HEAD 改变且完全一致才成功，不自动重试。

### C7 · branch target materialization preflight

- 只列 `refs/heads/*` local refs并捕获 target OID及其bytes short branch name；raw ref/name留 private，UI只持 opaque id/escaped label。拒绝 empty/NUL/control/leading-`-` name、detached/remote guess/create/unknown/stale id/OID change；non-UTF8/ref label只展示 escaped串。
- dirty/conflict（staged/unstaged/untracked/unmerged）或 merge/rebase/cherry-pick/revert/bisect/sequencer/`git am` operation marker一律拒绝。active agent run、pending permission、pending plan review、open commit panel 任一存在也拒绝。
- current OID→captured target OID 固定执行 read safe prefix + `diff --name-status -z --diff-filter=ACMRT -M --no-ext-diff --no-textconv <current_oid> <target_oid>`，bounded bytes parser取得 materialized target path set（rename只取target/new path）；target deleted path不 materialize。若 changed set 含任意 `.gitattributes` 直接拒绝。
- materialized target paths通过 stdin交 fixed `git check-attr --source=<target_oid> --stdin -z filter`；任一值非 `unspecified`/`unset`拒绝。`--source` capability self-test失败也拒绝。preflight后再次复验 target OID、clean/status/operation/active guards。
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
- **产出/验收**：C2/C6/C8；porcelain-v2 + stage-entry cross-checked IndexSnapshot/post-tree codec；`git add -N intent.txt` 的 `XY=.A` 必须 fail closed且 stage/commit spawn均为0，即使 `ls-files` 显示stage0+nonzero OID；正常 staged empty file `XY=A.` + nonzero empty-blob OID必须通过。另覆盖rename old+new/delete old、literal `:(glob)**`/`:!safe`、non-UTF8、changed `.gitattributes`/filter reject；Prepare前/后/Commit前都cross-check；两次Cancel focus/Esc/Cmd+Enter/bare Enter、duplicate/close/switch、empty/NUL/32KiB+1/Unicode boundary、full-duplex stdin/drain、PGID descendants/maintenance no-detach、message/summary redaction与零rollback/push/network/model mutation。
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
- **停止**：stdin/index/tree契约矛盾即 `[BLOCKED] S6-T34`，不得降级 raw index hash/temp file。

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
- [ ] IndexSnapshot由同一 HEAD/generation 的 porcelain v2 + `ls-files --stage -z` 交叉构造；`XY=.A` intent-to-add在Prepare前/后/Commit前均拒绝且零stage/commit spawn，不能靠nonzero OID放行；`XY=A.` staged empty file正例通过；仍拒绝stage>0/zero OID/corrupt/overflow。rename/delete mapping、post `ls-tree` compare与两次确认/first-wins完整。
- [ ] commit 32 KiB UTF-8 stdin、provider summary 256 KiB、hooks/signing disabled；模型不能 mutation；无 temp/rollback/push/network；payload redacted。
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

1. filter driver repository：selected staging或target materialization命中 filter一律拒绝；same-user 在preflight后改 attributes/config是已知 TOCTOU residual，不宣称原子隔离。
2. 所有 Git child收拢 inherited PGID descendants；主动 `setsid` 逃逸是 residual。
3. hooks 与 signing固定关闭；依赖它们的repo需在终端提交。
4. Phase 1 image metadata-only；Open in仅六个fixed targets。custom/configurable handoff、PR assistance、Diff v2留 Phase 2。
5. Composer @引用、/命令、模型选择器与 >8独立inner-scroll后置。
6. T35 report无法包含自身尚未产生的 PR/squash hash；只列 branch commit + pending，Phase 1最终报告补 hash。
7. fake launcher/MockProvider不等于真实 app/LLM/key/费用；真实 UI/CJK/960×600/竞品截图/ProMotion/P1-P8逐项留人类/S8。

## 未决阻塞检查

- 当前无未决 spec/API blocker；review确认本机支持 `check-attr --source`、stdin commit与所需GPUI API。
- T30/T33/T34 若实际系统行为与本 frozen contract矛盾，立即 `[BLOCKED]`，不得降级 shell/lossy path/temp file/宽权限。

## 变更记录

- v0.1 (2026-08-30) S6 初始 SDD：T30-T34 与基础安全边界。
- v0.2 (2026-08-30) 人类批准 A：PRD v0.3.3、raw path private、artifact provenance、two-stage stdin commit，重排 T30-T35。
- v0.3 (2026-08-30) 最终 executable hardening：bounded projections、literal pathspec、PGID/maintenance、exact caps/Open argv、target filter preflight、porcelain-v2 + stage-entry cross-checked logical IndexSnapshot（显式拒绝 `XY=.A` intent-to-add、允许 `XY=A.` staged empty file）、controller ownership、copyable gates与报告 evidence timing定稿。
