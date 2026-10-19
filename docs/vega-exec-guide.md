# ✦ Vega — 执行层开发总纲（Executor's Constitution）

## Vega 本地验证限制（2026-10-16 用户更新）

**仅适用于 Vega 项目，包括它的所有 worktree、主 agent 和子 agent；不限制 Loom 自身或其他项目。**

- **Codex、Vega 及子 agent 本地不运行 Vega 代码，不编译、构建、打包或执行测试。** 定向 `cargo check`、Clippy、test / nextest 也不运行，不因缓存或范围小豁免。
- 本地只做低 CPU、快速结束的差异、格式和静态检查；skill frontmatter 静态校验可执行。自动测试只交 GitHub Actions，打包使用云端发布流程。
- 本规则优先于旧文档、技能、任务规格中的本地运行/编译/定向测试要求。有行为变化时按 §7.2 先写测试；报告区分本地静态检查和待 CI 验证，不把未运行项标为通过。上一轮编号清理的临时本地执行授权不延续到本轮或全局。

**当前用户裁决**：[Rust 测试开发/交付生命周期](vega-rust-test-delivery-lifecycle.md) 要求 **先写测试 → Actions 有效 RED → 最小实现 → 同一测试 GREEN → 删除 Rust 测试与独占支持 → 最终空集门禁 → 正式 PR**。开发期间可保留测试，最终交付树不包含 Rust 测试；测试源码/提交与 RED/GREEN 证据固定在 PR 中作为可追溯来源。允许先建 draft PR 执行 RED/GREEN；Python 发布/组件准备/delta 测试及 fixture、生产、benchmark 与打包能力不变。禁止事后补测，验收变化必须重开周期。详见 §7.2；[最初清零规格](vega-rust-test-reset-tdd.md)的历史记录保留，旧的永久保留新增测试要求由本次裁决取代。

**历史决策（2026-09-25，Issue #149）**：[移除真实 E2E 规格](vega-issue-149-remove-real-e2e.md) 当时保留进程内业务、安全、存储及 UI 测试；该保留要求被本次 Rust 清零取代。外部依赖隔离、真实系统集成由用户手测的边界仍适用于后续测试，不恢复真实 E2E 自动门禁。

> **历史决策（2026-09-22，Issue #140）：** [Git/Shell 测试依赖隔离](vega-issue-140-ci-test-throughput.md#user-directed-revision-isolate-external-execution-2026-09-22) 记录当时的测试替身迁移与真实适配层覆盖；不构成本次清零的保留要求或后续真实 E2E 门禁。mockall 当时获准用于该卡 dev-only 依赖；新依赖仍须批准，证据必须准确标注 mock/real。

> **2026-09-22 Issue #112 supersession:** Read/Edit/Write paths, read-before-mutation, replacement matching and audit/checkpoint path support follow [the file edit parity contract](vega-issue-112-file-edit-parity.md). Its user-authorized absolute/external paths and resolved symlinks replace the earlier project-only/relative-only prohibition for those tools. Existing permission, Git/checkpoint protection and race checks remain.

> **2026-10-18 Issue #426 supersession:** Explicit grep file/directory targets follow [the external grep contract](vega-issue-426-external-grep.md) and Read permissions, with separate exact directory-search signatures. Default/null/empty searches remain project-root-only; glob patterns and bash boundaries are unchanged. Descriptor-scoped traversal, identity checks and credential protection remain mandatory.

**版本** v1.3 · 适用对象：Codex、Vega 及承接本仓任务的子 agent
**关联**：[vega-tech-spec-p1.md](vega-tech-spec-p1.md)（实现规格）· [vega-tech-risks.md](vega-tech-risks.md)（难点方案）· [vega-features.md](vega-features.md)（功能点 ID）· [vega-ui-spec.md](vega-ui-spec.md)（UI 准线）

> 本文件是执行模型的**最高行为准则**。每个任务 prompt 都必须附本文件路径。任何与本文件冲突的"看起来更合理"的做法都是错的。

---

## 1. 角色与协作模型

| 角色 | 职责与边界 |
|---|---|
| Codex | 查重建卡、先审查所有需求；Bug 定根因并交付可执行方案，Feature 默认由 Codex 组织实现。主会话负责协调/审查/集成，代码实现委派专职子 agent；负责 PR、CI、合并及用户反馈分析 |
| Vega | 只领取 Codex 已交接、方案完整可访问的 Ready 执行卡；按方案先写测试并确认 Actions 有效 RED，再实现及验证同一测试 GREEN，禁止实现后补测；完成或遇阻交回 Codex。默认不建卡、不改 spec、不扩大范围或开放式排障 |
| 用户 | 产品取舍；合并后的纯手动验收。明确通过前 Issue 开放、状态 In review；分支/worktree 按安全清理规则处理，不以保留分支代替证据留存 |

共用 [vega-kanban-delivery](../.agents/skills/vega-kanban-delivery/SKILL.md)，角色及队列细则见 [Issue 工作流](vega-issue-workflow.md)。以用户明确指定的工作流角色或宿主系统身份识别，Vega 为 `You are Vega`；不从模型、Provider、项目名或路径猜测。只有身份无法确认才询问，有证据时直接路由。架构师职责由 Codex 主会话承担，产品裁决仍由用户决定。

**执行模型三铁律：**
1. **spec 之外零发挥**。任务卡没写的设计决策，不许自己拍——上报，等裁决。
2. **红线清单里的行为，一次都不许出现**（见 §3）。
3. **实现回报、CI 通过、合并及用户验收分别记录**。测试仅在 Actions 执行，未跑写待 CI；Vega 回报后仍需 Codex 审查/集成，完整完成还须用户手测与收尾（见 §2、§7）。

## 2. 工作流（每个任务的标准动作）

1. 用户指定或明确继续的卡优先；否则自动完整读取本角色候选队列，过滤关闭、占用、当前角色/阶段前提未满足、缺所需方案和未交接 PR，再按 Issue `createdAt` 降序、同时间编号降序取最新一张，不按更新或 Project 默认顺序。解释、审阅和维护 skill 不启动其他卡。
2. Codex 取可开始审查的 Backlog、等待 Codex 的 Ready，或 Vega 最新交接明确交回、当前待领取且无活跃执行者的 In progress；这类阻塞卡可由 Codex 接回分析，不能因导致 Vega 阻塞的依赖缺口排除。Vega 仅取明确交给 Vega、方案完整可访问且执行依赖已满足的 Ready。领取前重读，立即置 In progress 并读回；交回卡列不变。Issue 记录阶段、当前/下一执行者、时间、方案版本和工作上下文，冲突时让出，不把读回当原子锁。
3. Codex 先审查需求/spec，先写验收用例再写实现计划。Bug 须交付根因/证据、具体文件/符号和修改动作、边界、测试矩阵、Actions 命令、手测/回滚、基线/分支/交接 commit 及回报约定；提交到 Vega 可访问位置并在 Issue 发布链接后才 Ready，不能只给会话结论或未提交本地文件。Feature 默认仍由 Codex 组织专职子 agent 实现。
4. 修改仓库前 fetch 并更新基线，保护已有改动；每卡用 `~/Workspace/worktrees/` 下独立 worktree 和 `feat/<task-id>-<slug>` 分支，不直接改 master。同卡单一写入者，交接后前任停止写入。Vega 按已发布方案和 §7.2 完成测试/有效 RED 后才实现，确认同一测试 GREEN，不补测追认；完成或遇阻回报同卡，仍 In progress，当前执行者待领取、下一执行者 Codex 并停止写入，Codex 可自动接回。
5. **本地仅快速静态 check**。Codex 审查测试补丁后先建/复用测试先行 draft PR，触发 Actions 确认 RED；实现回报后审查 diff/覆盖，在同一 draft PR 验证带测试 GREEN，固定源码/提交和云端证据后独立删测，验证最终空集 head 再转正式 PR，不使用自动关闭 Issue 的关键字。CI 启动后每 5 分钟查询，不短轮询或 watch；收到完成/失败通知或用户要求即时查看除外。最新 head 的 required check 通过后仅 Codex 主动 squash merge，才置 In review，**停下等用户纯手动验收**。
6. 用户明确手测通过 → Codex 回写 → 按 [分支与 worktree 清理规则](vega-issue-workflow.md#分支与-worktree-清理规则)完成本卡剩余安全清理 → 关闭 Issue 并读回 → 最后置 Done 并读回；清理或关闭受阻则保留 In review、标待收尾，不能先 Done 再补清理。未通过 → Codex 记录反馈、退回 In progress 并先分析，Bug 修订方案后再 Ready 交 Vega。手测前不关闭或 Done；最新 head 的 required check 通过并合并后，允许自动删除已合并远端任务分支，前提是证据、完整 SHA 和补丁可恢复。本地分支/worktree 仅确认无运行 agent、无未提交或未合入改动后安全清理；允许删分支不授权删除用户数据。Done 仅表示验收和全部收尾均已完成。
7. 默认一张，到 Codex 交 Ready、Vega 交回或合并后等用户时停止，只有连续授权才取下一张；无合格卡说明阻碍并停止。提交格式为 `feat(A2-09): <一句话>` / `fix(A3-07): <一句话>`（ID 见 vega-features.md）。
8. 实现报告写修改文件/提交、静态检查命令与退出码、§7.2 的 RED 基线/冻结测试源码及提交/实现顺序、带测试 GREEN 云端原始证据、独立删测提交和最终 head 空集门禁、限制/耗时和规格偏离（必须为无）。未运行标待 CI，无行为变化说明不适用；不把“已派发/提交”当“已通过/完成”。

## 3. 红线清单（违反 = 任务失败重来）

### 架构红线
- ❌ `vega_runtime` 依赖任何 UI/GPUI crate（headless 原则，tech-spec §1）
- ❌ 跨 crate 共享类型不经过 `vega_conversation::types`（禁各自定义同名结构体）
- ❌ UI 直接读写 SQLite（必须经 vega_conversation 事件流）
- ❌ 新建数据表/改 DDL 不改 `migrations/` 递增文件（schema 只增不删）

### 安全红线
- ❌ API key 写入 config.toml/项目文件/日志/代码（R10 人类裁决：只存配置根下独立 owner-only 明文凭据文件，不访问旧 Keychain）
- ❌ 删/改用户工作区里 Vega 未创建的文件（工具实现必须路径围栏，risks #4）
- ❌ 权限门禁被任何"便捷路径"绕过（tech-spec §4.3 决策顺序不可改）
- ❌ 危险命令硬拦截清单被注释/削弱

### 代码红线
- ❌ `unwrap()` / `expect()` 出现在非测试代码（用 VegaError，tech-spec §7）
- ❌ 代码中出现任何注释（`///`、`//!`、行内 `//`、`/* */`）。**需要注释才能读懂 = 命名或结构没写清楚**，改命名、拆函数、提类型，而不是写注释。唯一例外：`unsafe` 块紧邻的 `// SAFETY:` 一行（Rust 审计要求陈述不变量，2026-09-23 用户裁决）。本裁决取代此前任何任务卡/spec 中「新增或保留 why-comment」「同步更新 doc comment/文件头注释」的要求；遇到这类旧条款按本红线执行，不再新增注释。
- ❌ 颜色/字号硬编码（必须 ui-spec token；验收 grep 会查）
- ❌ 在 `select!` 分支里调用非取消安全 API（`read_exact`/`write_all`/`read_to_string`；risks #3）
- ❌ 引入新依赖不在任务卡允许清单内（每加一个 crate 需架构师批准）
- ❌ 为通过测试而放宽断言、改期望、ignore 或重试换绿。测试自身错误/接线修正须先留证审查且不改变验收含义；需求、边界或 Bug 变化按 §7.2 停止本轮并重新先测后修，不以“测试待更新”绕过。

### 性能红线
- ❌ 每 token delta 直接触发全量渲染/全量 reparse（risks #1 #5 管线必须走）
- ❌ 上屏关键路径上的同步 IO（落库/计价必须异步攒批，risks #5）

## 4. 编码约定

| 项 | 约定 |
|---|---|
| Rust edition | 2024，stable 工具链（rust-toolchain.toml 锁定） |
| 格式化 | `cargo fmt --all`（CI 强制） |
| Lint | 云端 PR Check 执行 `clippy --all-targets -- -D warnings`，零警告 |
| 异步 | tokio；取消一律 `CancellationToken`（禁 abort） |
| 错误 | 统一 `VegaError`（tech-spec §7）；`thiserror` 定义，跨线程 `Send + Sync` |
| 日志 | `tracing`；禁 `println!`；敏感信息（key/文件内容）禁入日志 |
| 注释 | **禁止任何注释**（`///`、`//!`、行内 `//`、`/* */`）。需要注释才能读懂，说明命名/结构没写清楚——改命名、拆函数、提类型。唯一例外：`unsafe` 块紧邻的 `// SAFETY:` 一行（2026-09-23 用户裁决） |
| 测试 | 按 §7.2 写任务相关的有效先行测试，不要求每模块造测试；mock 只隔离外部依赖，不替代业务/安全判断；新测试用行为命名，issue/task 编号只留文档 |
| 提交 | 小步提交，一个任务卡 ≤3 个 commit |

## 5. 依赖白名单（S1-S8，新增需批准）

```
基础: tokio, serde, serde_json, thiserror, tracing, tracing-subscriber, toml(=1.1.4), anyhow(仅 xtask)
UI: gpui, gpui_platform (git=https://github.com/zed-industries/zed, rev 锁定, gpui_platform features=[font-kit]; 禁第三方发行版、禁混用来源、不开 runtime_shaders)
数据: rusqlite (bundled, WAL), ulid
网络: reqwest (rustls), eventsource-stream, tokio-util, futures
工具: ignore, regex, similar(diff), tree-sitter, pulldown-cmark, mdstream (=0.3.0，T14 spike 确认引入；上游 0.4 已换模型，vendoring 预案，tech-spec §5.0)
安全: libc（R10 已批准凭据文件 IO；R11 已批准终端生命周期，以及 vega_tools 只读预览 O_NOFOLLOW/O_NONBLOCK descriptor 检查）, cap-std(备选)
终端: portable-pty(=0.9.0), vt100(=0.16.2)（R11 架构师批准，真实 PTY 与有界 ANSI screen）
测试: insta, tempfile
```

## 6. 遇阻上报协议（执行模型必须遵守）

遇到以下情况**停下来上报**，禁止自创方案：
1. spec 描述与官方文档/实际 API 矛盾
2. 任务卡之间出现依赖冲突或覆盖范围空隙
3. 验收命令因环境问题（非代码问题）无法通过
4. 发现 spec 有设计缺陷（说明理由 + 建议，等裁决）
5. 需要引入白名单外依赖

上报格式：`[BLOCKED] 任务ID | 问题一句话 | 已排除的假设 | 建议（如有）`

Vega 在原 Issue 上报，保持 In progress，写当前执行者待领取、下一执行者 Codex 并停止写入；由 Codex 分析和修订方案，再按完整交接流程交回，不让 Vega 自行更改 spec 或绕过限制。

## 7. 验收协议（每个任务卡通用）

**PR gate 拓扑（2026-09-24 用户裁决，[Issue #175](vega-issue-175-single-pr-check.md)；缓存和 workflow 命名由 [Issue #179](vega-issue-179-shared-pr-cicd-cache.md) 更新，测试运行器按 2026-09-25 [Issue #183](vega-issue-183-nextest-ci.md) 更新，并行结构按 [Issue #188](vega-issue-188-parallel-ci-checks.md) 更新）**：门禁全部在云端。`.github/workflows/pr-check.yml` 仅响应 base 为 `master` 的 PR，并以两个无相互依赖的 `macos-latest` job 并行执行：Clippy job 依次运行 `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings`；Nextest job 保留三组 Python 检查（`python3 scripts/release_tests.py`、`python3 scripts/prepare_computer_use_tests.py`、`python3 -m unittest discover -s scripts/tests -v`），先用 `cargo nextest list --workspace --message-format json` 清点 Rust 测试；仅 `test-count` 为 0 时运行 `cargo nextest run --workspace --no-tests=pass`，非空仍运行 `cargo nextest run --workspace`。日志与 job summary 明确区分 `no Rust tests`（0 发现/0 执行）与非空执行通过；发现数包含 ignored/skipped，不冒充执行数，执行/跳过数看原始 Nextest 输出。required check `check (fmt, clippy, test)` 是依赖两者的轻量汇总 job，以 `always()` 运行，仅两个结果均为 `success` 才通过，失败、取消和跳过均使汇总失败。通过 `taiki-e/install-action@v2` 安装固定 `nextest@0.9.146`，使用默认测试并发。CI 覆盖单元和集成测试；nextest 不支持 doctests，按用户只使用 nextest 的要求不另跑 `cargo test --doc`。失败直接阻断合并，不自动重试。该 workflow 保留 Rust 1.98.0、`contents: read`、执行 job 的 60 分钟超时和并发取消；两个执行 job 使用 `Swatinem/rust-cache@v2` 恢复 master CICD 写入的 `vega-master-build` 缓存，PR 的 `save-if` 条件为 false，因此 PR 不保存缓存；不使用归档、分片、额外独立 build 步骤或手动 dispatch（list/run 本身会在云端构建测试目标）。`.github/workflows/cicd.yml` 的 GitHub Actions 名称为 `master`，仅在 push 到 master 时打包并上传 artifact；它和 PR check 使用相同 `shared-key: vega-master-build` 及 `save-if: ${{ github.ref == 'refs/heads/master' }}`，master 是唯一写入方。rust-cache 还会按 runner 平台、工具链及 Cargo 输入派生实际缓存键。旧的 [Issue #123](vega-issue-123-pr-check-pipeline.md) / [Issue #140](vega-issue-140-ci-test-throughput.md) 流水线描述只记录历史决策；云端 nextest 使用 `.config/nextest.toml` 默认 profile：`retries=0`、`fail-fast=false`、600 秒挂起保护，不覆盖默认并发。
- **门禁执行**：云端 `check` 是合并前唯一强制门禁，失败由 Codex 读取 PR 原始日志分析；Master 只允许 PR merge，不允许直接 push。本地**仅快速静态 check**，不运行 Vega、编译或测试；本卡功能点及 workspace 测试只交 GitHub Actions。本地 commit/push 不做强制检查、不排队、不锁 target；`.githooks/`、`scripts/verify.py` 与 cargo-lock 调度器已删除。
- **验收节奏（当前规则）**：先冻结验收并写齐测试 → Codex 审查并建/复用测试先行 draft PR → Actions 确认有效 RED → 最小实现 → Vega/子 agent 回报 → Codex 审查并在同 PR 验证同一测试 GREEN → PR 固定原始测试源码/提交及 RED/GREEN 证据 → 独立删除 Rust 测试与独占支持 → 最新 head 的空集 required check 通过 → 转正式 PR → Codex 主动 squash merge → In review → **停下等用户纯手动验收**。CI 每 5 分钟查询，通知/用户即时请求除外，不短轮询/watch。合并时 Issue 开放、不 Done；允许自动删除已合并远端任务分支，证据、完整 SHA 和原始 patch 须可恢复。本地分支/worktree 仅确认无运行 agent、无未提交或未合入改动后按 [安全清理规则](vega-issue-workflow.md#分支与-worktree-清理规则)处理，不授权删除用户数据；用户明确通过才回写并完成剩余安全清理，再关闭 Issue 并读回，最后 Done 并读回。清理或关闭受阻时保持 In review、标待收尾，不先 Done 再补清理。用户不通过由 Codex 退回 In progress 先分析，再给 Vega 具体 Bug 方案。真实模型/服务/UI 集成由用户在 master 上手测，Agent 不以自产 E2E/截图替代。
- **任务级**：任务卡附验收矩阵与命令；运行程序、测试或 benchmark 的命令交 Actions，手工走查由用户完成，本地仅快速静态检查。
- **架构级**：`cargo tree` 检查无红线依赖关系；新增公共类型在 `vega_conversation::types`
- **报告**：贴本地静态检查、§7.2 测试先行证据与对应提交的云端命令原始输出，未运行项标待 CI、无计数明确未取得；空集写“no Rust tests”，不称 Rust 测试通过。未收到用户结论标待手测，不许用“通过了”替代证据或假装完整完成。收尾失败保留上下文并说明待办。
- **进程内自动回归**：本次现存 Rust 测试清零后，后续先行测试按任务覆盖业务、安全策略、解析、存储及 GPUI 测试上下文行为。不新增真实外部进程/网络/应用 E2E 自动门禁；系统集成由用户手测。禁止为了测试扩大 production public API；新写的有效断言不得放宽以变绿。
- **证据留存**：每 Sprint 报告必须记录命令、UTC/本地时间、branch、测试时 tree/content hash、结果与 accepted residual；raw 日志可暂存 `/tmp`，仓库文档不得写入真实 key、raw workspace path/OID或伪造尚不存在的 commit/PR。

### 7.1 历史 E2E 证据分级与仓库模板

以下分级与示例仅用于解释既有历史报告，不要求保留其中 Rust 测试、恢复真实 E2E 或在本地执行。当前清零范围、后续测试和门禁以 [本轮规格](vega-rust-test-reset-tdd.md) 与 §7.2 为准。

| 等级 | 允许的 seam | 可证明的范围 |
|---|---|---|
| `E2E-REAL` | owned `TempDir`、repo-local config、`MockProvider` | 真实 filesystem/Git/production service 或 controller 链；不代表真实 provider/network/key |
| `INTEGRATION-DELEGATING` | observer/recorder须逐次委托真实 executable并复核repo终态 | exact argv/stdin/process lifecycle 与真实命令效果 |
| `FAULT-INJECTION` | bounded scripted executable/provider/clock | 该故障后的 typed failure、zero/one attempt与恢复责任；不得冒充真实 happy path |
| `UNIT/PROPERTY` | 纯函数、bytes/table/state-machine | 闭合 grammar/codec/bounds/redaction/并发不变量；不得冒充production wiring |

`E2E-REAL` 禁止 override Git read/mutation executable、伪造 authority/prepared/result或由 probe 决定成功分支；`MockProvider` 只能替换 provider/network 边界。probe只能观察，且断言必须同时包含真实repo/UI/controller终态。每个不同production故障阶段至多保留一个代表 seam；不得为覆盖率扩大public API。

仓库证据文档使用以下最小结构（大日志只留fresh `/private/tmp`，仓库只记录bounded footer与hash）：

```markdown
## Freeze
- verified_at_utc / verified_at_local
- git_head / tracked_diff_sha256 / task_contract
- os_arch / rustc / cargo / git

## Results
| requirement | evidence class | exact command | result | duration | bounded footer/hash |

## Residuals
- ACCEPTED / LIMIT / SKIP / NOT RUN（不得把skip、mock或未执行项写成PASS）
```

日志与报告禁止包含absolute workspace path、raw Git path/OID、diff/provider正文、Authorization/Keychain值。失败后重跑必须保留首次失败及原因，不能只留下最终pass。

### 7.2 TEST-FIRST 与有效测试（当前规则）

本节适用于主 agent、子 agent、Feature 与 Bug；当前完整裁决见 [Rust 测试开发/交付生命周期](vega-rust-test-delivery-lifecycle.md)。测试只在开发阶段保留到同一冻结测试 GREEN，最终交付树不包含 Rust 测试；Python 测试继续保留。不为纯维护删测造新测试或伪造 RED，不能删除失败/未执行测试逃避验证。纯文档/格式变更不要求无用测试；纯重构所需验证测试同样在 GREEN 后删除，不永久恢复历史集合。

1. **验收先行**：从 spec 明确最少的可观察行为、失败/恢复边界、非目标及基线。验收矩阵不是实际测试的替代品。
2. **先写齐测试**：在新增或修改本轮生产实现前写齐验收所需测试，记录生产未改的基线、测试提交/补丁和 Actions 命令。使用行为名称，不把 issue/task 编号写入非文档代码。
3. **确认有效 RED 才实现**：由 Codex 审查测试补丁并建立/复用测试先行 draft PR（base `master`），让现有 `pull_request` Actions 在该基线上验证；不添加本地执行许可或新调度基础设施。Vega 提交测试阶段交接后停止写入，由 Codex 发起/检查 RED，再明确交回冻结目标才继续实现。保留目标行为缺失导致的原始失败。无关编译错误、缺依赖、网络/runner 故障不算 RED。API 尚不存在时只接受最小目标接口缺失，明确标为接口缺失 RED、行为断言尚未执行，不冒充行为失败。RED 未确认或云端不可用就停在此阶段，不以本地执行绕过。
4. **冻结后最小实现，再 GREEN**：冻结测试目标、数据、断言与预期后才修改生产实现；验证相同测试与最新 head 完整 PR gate。实现阶段及实现完成后均不追加证明实现的用例，不按实现输出反推断言，不为覆盖率/报告补测，不放宽/改期望、不 ignore、不自动重试换绿。
5. **GREEN 后删除 Rust 测试**：先在 draft PR 固定测试源码/补丁、完整提交 SHA、有效 RED 与同一测试 GREEN 的 run/job 和准确计数，再用独立提交删除 workspace（含 `xtask`）的 Rust 测试、可执行 doctest 及其独占 fixture/mock/helper/feature/dev dependency。保留生产、benchmark、Python 和通用 CI 能力；不得先删失败、忽略或未执行测试再用空集冒充 GREEN。
6. **最终空集门禁后正式 PR**：审查删测前后生产差异与独占支持残留，对最新 head 运行现有完整 PR gate。Rust 必须 0 发现/0 执行并标 `no Rust tests`，同时 fmt、全目标 Clippy、Python 与发现/构建成功；只有带测试 GREEN 证据和最终门禁均可核实，才转正式 PR/申请合并。
7. **验收变化重开一轮**：GREEN 后只允许删测/独占接线清理，不夹带新生产行为。同步基线或解决冲突改变被验证实现时，从证据恢复同一冻结测试重新云端 GREEN 后再删测；验收含义变化则重开周期。新需求、遗漏边界或新 Bug 先停止当前轮，记录独立验收变化、现有实现基线与新失败目标，再走测试 → Actions 有效 RED → 修复 → GREEN → 删测/最终门禁；不能倒填测试追认已完成实现。测试自身错误或接线修正须先记录证据并审查，保持验收含义；改变验收就重开，不暗改断言过关。

**合并基线**：旧分支先同步最新 master，不从旧 ref 整目录/整仓恢复已删除的历史集合。新增 Rust 测试及必要支持只用于本轮开发验证，GREEN 后必须删除；改名或改写旧测试不自动算新回归，确需新回归独立先测后修。合并前审查原测试来源、RED/GREEN、独立删测结果和最新 head 空集门禁，不只看总数或 CI 绿色。历史原因见 [合并纠偏规格](vega-rust-reset-reconciliation.md)。

**有效测试标准**：每个测试指出具体可失败行为、业务不变量或安全边界，验证公开/稳定边界的结果及必要副作用，优先少量有区分度的正常、失败、恢复样例。禁止仅检索源码字符串/符号存在、只验证 mock 调用/机械转发、镜像生产算法、重复堆叠同义断言、为内部字段/API 单独造覆盖率测试。mock 可隔离外部进程、网络和应用，不可 mock 掉被测业务/安全决策；调用次数仅在“不得重复执行/不得请求”等本身属于契约且结合结果/终态时有效。

**可审查证据（交接与 PR 均必需）**：

- RED：生产未改的基线/状态、测试提交或补丁/内容标识、冻结测试目标与预期、准确命令、Actions run/job 与原始失败原因；说明是行为断言失败还是最小接口缺失。
- 实现：后续生产实现提交/补丁与先后顺序、冻结测试未被追改的审查结果；若重开周期，逐轮记录验收变化与各自基线，不混用证据。
- GREEN：带相同冻结测试的提交、命令与云端原始结果；分别记录发现/执行/通过/失败/忽略/跳过，未取得写“未取得”，未运行写“待 CI”，不得猜测。
- 删测与交付：独立删除提交、测试/独占支持清单和生产差异审查，及最终 head 完整门禁。空集明确 0 发现/0 执行、`no Rust tests`，不是 Rust 测试通过，也不能替代上一步 GREEN。
- PR 固定可访问的原始测试源码/补丁、完整 SHA 与 RED/GREEN run/job；squash/删分支后仍能找回，不依赖本机临时文件或过期 artifact，不以 force-push 丢弃唯一来源。提交和源码是内容来源证据，不单独证明先行时序或有效 RED/GREEN。最终绿色空集 CI 也不证明 TDD。

---

*本文件随 spec 演进更新，变更记录：v0.1 (2026-08-29) 初版；v0.2 (2026-08-29) 验收门禁执行方式改为本地 git hooks（人类决策，防 CI 费用）；v0.3 (2026-08-29) UI 白名单 gpui/gpui_platform 来源改为 zed 官方仓库 git rev 锁定（crates.io 停滞且无 gpui_platform，人类批准）；v0.4 (2026-08-29) 基础白名单新增 toml（config.toml 解析，人类批准）；v0.5 (2026-08-29) mdstream 白名单条件激活（`待 spike 确认` → T14 spike 确认引入，锁定 =0.3.0）；v0.6 (2026-08-31) 人类冻结 E2E-first 验收与仓库证据留存规则，限制 test-only 笛卡尔扩张；v0.7 (2026-09-22) 门禁全部上云（Issue #123）：删除本地 hooks/verify.py/cargo-lock 调度器，PR 走云端 fmt/clippy/test，push master 打包，发布仍由 tag 触发；v0.8 (2026-09-23) 交付节奏改为「本地只跑本卡功能点测试 → 开 PR → 云端 check 绿 → Agent 主动合并 master → 卡改 In review → 用户手测」：用户手测通过才回写/关 Issue/Done/清理，未通过退回 In progress；真实验收由用户在 master 手动完成；v0.9 (2026-09-23) 用户裁决代码中禁止任何注释（`///`/`//!`/行内 `//`/`/* */`）：注释说明必要即命名或结构没写好，改命名/拆函数/提类型；仅 `unsafe` 块的 `// SAFETY:` 一行例外；v1.0 (2026-09-24) Issue #179：PR gate 恢复 master 的 `vega-master-build` Cargo cache 但不保存；`.github/workflows/cicd.yml` 的 workflow 名为 `master`。v1.1 (2026-10-16) Codex 先审查并交接具体 Bug 方案、Vega 按方案执行后交回；按角色自动取最新合格单卡，本地仅静态检查，测试只交 Actions，用户手测通过后才清理远端分支及本地 worktree。v1.2 本轮 Rust 测试一次性清零，保留 Python 测试；后续强制测试先行与云端有效 RED，禁止实现后补测，空集 CI 明示且保留原有门禁。v1.3 用户要求 Rust 测试仅在开发阶段保留：draft PR 验证 RED/GREEN，固定原始源码/提交与证据，独立删测并验证最终空集 head 后转正式 PR；Python 测试不变。

当前规则更新：CI 通过并合并后允许自动删除已合并远端任务分支，证据/完整 SHA/patch 可恢复；本地资源仅安全核实后清理，不授权删除用户数据；Issue 关闭/Done 仍等用户手测通过。此裁决取代上述历史版本中手测前保留分支的要求，历史变更记录原文保留。
