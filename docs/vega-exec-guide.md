# ✦ Vega — 执行层开发总纲（Executor's Constitution）

## Vega 本地验证限制（2026-10-16 用户更新）

**仅适用于 Vega 项目，包括它的所有 worktree、主 agent 和子 agent；不限制 Loom 自身或其他项目。**

- **主 Agent 与子 Agent 本地不运行 Vega 代码，不编译、构建、打包或执行测试。** 定向 `cargo check`、Clippy、test / nextest 也不运行，不因缓存或范围小豁免。
- 本地只做低 CPU、快速结束的差异、格式和静态检查；skill frontmatter 静态校验可执行。自动测试只交 GitHub Actions，打包使用云端发布流程。
- 本规则优先于旧文档、技能、任务规格中的本地运行/编译/定向测试要求。有行为变化时按 §7.2 选择必要验证；报告区分本地静态检查和待 CI 验证，不把未运行项标为通过。上一轮编号清理的临时本地执行授权不延续到本轮或全局。

**当前用户裁决**：遵守 [验证与快速交付政策](vega-rust-test-delivery-lifecycle.md)，以最小改动快速交付，按风险选择验证，可直接修复或先实现后补必要测试。不强制 TDD、前置测试独立审查、有效 RED、测试冻结、GREEN 后删测或配套证据链，不为流程造测试或删测提交。历史无 Rust 测试最终树只是现状；不批量恢复历史集合，保留现存 Python 测试及 fixture、生产/benchmark/打包与通用 CI。详见 §7.2；历史清零规格和证据不再构成测试先行门禁。

**历史决策（2026-09-25，Issue #149）**：[移除真实 E2E 规格](149/vega-issue-149-remove-real-e2e.md) 当时保留进程内业务、安全、存储及 UI 测试；该保留要求被本次 Rust 清零取代。外部依赖隔离、真实系统集成由用户手测的边界仍适用于后续测试，不恢复真实 E2E 自动门禁。

> **历史决策（2026-09-22，Issue #140）：** [Git/Shell 测试依赖隔离](140/vega-issue-140-ci-test-throughput.md#user-directed-revision-isolate-external-execution-2026-09-22) 记录当时的测试替身迁移与真实适配层覆盖；不构成本次清零的保留要求或后续真实 E2E 门禁。mockall 当时获准用于该卡 dev-only 依赖；新依赖仍须批准，证据必须准确标注 mock/real。

> **2026-09-22 Issue #112 supersession:** Read/Edit/Write paths, read-before-mutation, replacement matching and audit/checkpoint path support follow [the file edit parity contract](112/vega-issue-112-file-edit-parity.md). Its user-authorized absolute/external paths and resolved symlinks replace the earlier project-only/relative-only prohibition for those tools. Existing permission, Git/checkpoint protection and race checks remain.

> **2026-10-18 Issue #426 supersession:** Explicit grep file/directory targets follow [the external grep contract](426/vega-issue-426-external-grep.md) and Read permissions, with separate exact directory-search signatures. Default/null/empty searches remain project-root-only; glob patterns and bash boundaries are unchanged. Descriptor-scoped traversal, identity checks and credential protection remain mandatory.

**版本** v1.4 · 适用对象：承接本仓任务的主 Agent 与子 Agent
**关联**：[vega-tech-spec-p1.md](vega-tech-spec-p1.md)（实现规格）· [vega-tech-risks.md](vega-tech-risks.md)（难点方案）· [vega-features.md](vega-features.md)（功能点 ID）· [vega-ui-spec.md](vega-ui-spec.md)（UI 准线）

> 本文件是执行模型的**最高行为准则**。每个任务 prompt 都必须附本文件路径。任何与本文件冲突的"看起来更合理"的做法都是错的。

---

## 文档路径

Ticket 文档统一放在 `docs/<ticket>/`，同编号规格、交接与交付记录放在一起，保留文件名；通用规范留在 `docs/` 根层。不推断 R/A 编号与 Ticket 的映射。迁移时更新本地引用并按原目标重算相对链接，不改固定历史远程证据。入口见 [文档导航](README.md)。

## 1. Agent 职责与协作

| 执行者/职责 | 职责与边界 |
|---|---|
| 主 Agent | 在授权内查重建卡、分析需求/根因、修订 spec、组织实现；主会话负责协调、架构审查、集成、PR/CI/合并及反馈分析/收尾，代码实现必须委派专职子 Agent |
| 实现子 Agent | 按明确范围与验收最小实现/直接修复，按风险补必要测试并核实实际验证结果；回报证据，缺口交主 Agent 分析，不自行扩大范围或改验收 |
| 独立审查 Agent | 非该实现写入者，默认只读；审查验收、必要验证、实现契约与差异、证据与最新 head 门禁，不以实现者自审替代 |
| 用户 | 产品取舍、授权与合并后的纯手动验收；明确通过前 Issue 开放、状态 In review；资源按安全清理规则处理，不以保留分支代替证据留存 |

共用 [vega-kanban-delivery](../.agents/skills/vega-kanban-delivery/SKILL.md)，队列细则见 [Issue 工作流](vega-issue-workflow.md)。**当前用户裁决统一使用 Agent，不按厂商、宿主或模型划分职责、权限或队列**；主会话/子 Agent 是协作职责，不是厂商身份。主 Agent 可接手同卡全流程，不因缺旧交接或等待另一角色停工；旧分工仅作历史。产品裁决和超授权操作仍由用户决定，白名单外依赖仍须批准。

开工包、READY/增量 followup、固定协作者、失败诊断与中断恢复按 [skill「分析、委派与连续推进」](../.agents/skills/vega-kanban-delivery/SKILL.md#分析委派与连续推进) 执行。实现审查须绑定已停写的精确内容版本，可与该版本的现有 Actions 并行；按 §7.2 选择必要验证，不额外设置测试前置审查站。复用结论仍须审增量、受影响不变量，并独立确认完整最终 head 与 required check。原始证据可按需复查，不能替代本轮必要验证与最新 head 门禁；等待遵守既有取消、用户输入及 5 分钟 CI 查询节奏。

**Agent 执行三铁律：**
1. **spec 先行，范围不扩张**。缺口由主 Agent 在授权内先分析、补齐或修订 spec；不能代码先行、绕过冻结验收或自行决定需用户裁决的产品取舍。真实阻碍按 §6 上报。
2. **红线清单里的行为，一次都不许出现**（见 §3）。
3. **实现回报、独立审查、CI 通过、合并及用户验收分别记录**。测试仅在 Actions 执行，未跑写待 CI；子 Agent 回报后主 Agent 组织独立审查/集成并继续同卡，完整完成还须用户手测与收尾（见 §2、§7）。

## 2. 工作流（每个任务的标准动作）

1. 用户指定或明确继续的卡优先；否则自动完整分页读取候选队列，过滤关闭、占用、未交接 PR 及下一阶段真实前提未满足的卡，再按 Issue `createdAt` 降序、同时间编号降序取最新一张，不按更新或 Project 默认顺序。解释、审阅和维护 skill 不启动其他卡。
2. 候选为可开始分析的未占用 Backlog/Ready，或最新记录已释放写入权、当前待领取且无活跃执行者的 In progress；可接回分析方案/依赖缺口，不因实现依赖不足一概排除，但前提未满足不得直接实现。缺旧交接先分析恢复，不抢其他进行中卡。领取前重读，立即置 In progress 并读回；接回卡列不变。Issue 用阶段和当前/下一执行者 Agent / 待领取 / 用户记录状态，附时间、方案版本及会话/worktree 上下文；沿用五列，Ready 仅为实际待执行，不是强制交接站。冲突让出，读回不是原子锁。
3. 主 Agent 先审查需求/spec，明确验收与风险再确定最小实现计划。Bug 给出根因/证据、具体文件/符号/动作、边界、按风险选择的验证、必要云端命令、手测/回滚、基线/分支/方案版本和回报约定；跨会话交接提供可访问提交/链接，不能只留会话结论或本地路径。Feature 与 Bug 的代码实现均委派专职子 Agent，独立审查由非该实现写入者的 Agent 承担；能力不可用时报告限制，不绕过。
4. 修改前 fetch 并更新基线，保护已有改动；每卡用 `~/Workspace/worktrees/` 下独立 worktree 和 `feat/<task-id>-<slug>` 分支，不直接改 master。同一实现范围只有一个写入者，主会话不并行改受委派范围，交接后前任停止写入，独立审查默认只读。并行仅限授权内：不同任务须使用各自独立 worktree；同卡明确委派且写入范围互不重叠的子任务可共享本卡 worktree，Git/index、提交与集成仅由主 Agent 统一协调；不得并发修改共享状态或接口，也不得冲突写入文件。按 §7.2 最小实现、按风险验证，可直接修复或先实现后补必要测试；子 Agent 回报后主 Agent 继续组织审查和后续阶段，不在人为角色交接处停止。
5. **本地仅快速静态 check**。按 §7.2 风险选择必要验证，自动测试与完整门禁只交现有 Actions，不要求测试先行 draft PR 或 RED/GREEN/删测周期。PR 记录实际修改、精确版本、已执行静态检查、云端结果和未决项，不用自动关闭 Issue 的关键字。CI 启动后每 5 分钟查询，不短轮询/watch；完成/失败通知或用户即时请求除外。实现独立审查无缺口且最新 head 的 required check 通过后，主 Agent 主动 squash merge，才置 In review，**停下等用户纯手动验收**。
6. 用户明确手测通过 → 主 Agent 回写 → 按 [分支与 worktree 清理规则](vega-issue-workflow.md#分支与-worktree-清理规则)完成本卡剩余安全清理 → 关闭 Issue 并读回 → 最后 Done 并读回；清理/关闭受阻保留 In review、标待收尾。未通过 → 记录反馈、退回 In progress，由主 Agent 分析并修订 spec/验收，组织最小修复与必要验证。手测前不关闭或 Done；最新 head 门禁通过并合并后允许自动删除已合并远端任务分支，先固定并读回可恢复的证据、完整 SHA/源码/原始 patch。本地分支/worktree 仅确认属于本卡、无运行 Agent/构建、无未提交（含未跟踪）或未合入改动后安全清理，squash 核对等价内容；用户数据/未知文件/占用或不明改动保留报告，不 reset/强删/覆盖。允许删分支不授权删除用户数据；Done 仅表示验收及全部收尾完成。
7. 默认一张，同卡无验收缺口时从分析连续推进到合并，不在方案完成/Ready/子 Agent 回报处停工；仅真实产品裁决/权限/技术阻碍按 §6 上报。合并后等用户；只有连续授权才取下一张，无合格卡说明原因并停止。提交格式为 `feat(A2-09): <一句话>` / `fix(A3-07): <一句话>`（ID 见 vega-features.md）。
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
- ❌ 为通过测试而放宽断言、改期望、ignore 或重试换绿。测试自身错误/接线修正须说明依据，不改变验收含义迁就实现；需求、边界或 Bug 变化先更新 spec，再按 §7.2 最小修复与必要验证，不以“测试待更新”绕过真实失败。

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
| 测试 | 按 §7.2 按风险选择任务相关的必要验证，可先实现后补测或直接修复，不要求每模块造测试；mock 只隔离外部依赖，不替代业务/安全判断；新测试用行为命名，issue/task 编号只留文档 |
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

## 6. 遇阻上报协议（所有 Agent 必须遵守）

主 Agent 可在授权范围内分析缺口并先修订 spec；以下问题经核实仍需产品裁决、权限或无法自行解决的技术条件时，**停下受阻步骤并上报**，不自创方案绕过：
1. spec 与官方文档/实际 API 矛盾，无法在既定验收/授权内消解
2. 任务依赖冲突、范围空隙或写入占用，无法在本卡授权内安全解决
3. Actions/验收环境不可用，不能取得有效 RED/GREEN 或最终门禁证据
4. spec 缺陷涉及未授权的产品取舍、验收变化或安全边界，需要用户裁决
5. 需要引入白名单外依赖或进行其他尚未批准的操作

上报格式：`[BLOCKED] 任务ID | 问题一句话 | 已排除的假设 | 建议（如有）`

在原 Issue 记录证据、真实执行者、阻碍及恢复条件，保持 In progress；只有已释放写入权时才标待领取。子 Agent 将缺口交主 Agent 分析，主 Agent 能在授权内解决则修订方案、按 §7.2 重开必要周期并继续同卡；不能解决才向用户上报，不等待旧厂商角色。缺旧交接本身不是阻碍；冻结验收、授权、安全及云端门禁不能绕过。

## 7. 验收协议（每个任务卡通用）

**PR gate 拓扑（2026-09-24 用户裁决，[Issue #175](175/vega-issue-175-single-pr-check.md)；缓存和 workflow 命名由 [Issue #179](179/vega-issue-179-shared-pr-cicd-cache.md) 更新，测试运行器按 2026-09-25 [Issue #183](183/vega-issue-183-nextest-ci.md) 更新，并行结构按 [Issue #188](188/vega-issue-188-parallel-ci-checks.md) 更新）**：门禁全部在云端。`.github/workflows/pr-check.yml` 仅响应 base 为 `master` 的 PR，并以两个无相互依赖的 `macos-latest` job 并行执行：Clippy job 依次运行 `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings`；Nextest job 保留三组 Python 检查（`python3 scripts/release_tests.py`、`python3 scripts/prepare_computer_use_tests.py`、`python3 -m unittest discover -s scripts/tests -v`），先用 `cargo nextest list --workspace --message-format json` 清点 Rust 测试；仅 `test-count` 为 0 时运行 `cargo nextest run --workspace --no-tests=pass`，非空仍运行 `cargo nextest run --workspace`。日志与 job summary 明确区分 `no Rust tests`（0 发现/0 执行）与非空执行通过；发现数包含 ignored/skipped，不冒充执行数，执行/跳过数看原始 Nextest 输出。required check `check (fmt, clippy, test)` 是依赖两者的轻量汇总 job，以 `always()` 运行，仅两个结果均为 `success` 才通过，失败、取消和跳过均使汇总失败。通过 `taiki-e/install-action@v2` 安装固定 `nextest@0.9.146`，使用默认测试并发。CI 覆盖单元和集成测试；nextest 不支持 doctests，按用户只使用 nextest 的要求不另跑 `cargo test --doc`。失败直接阻断合并，不自动重试。该 workflow 保留 Rust 1.98.0、`contents: read`、执行 job 的 60 分钟超时和并发取消；两个执行 job 使用 `Swatinem/rust-cache@v2` 恢复 master CICD 写入的 `vega-master-build` 缓存，PR 的 `save-if` 条件为 false，因此 PR 不保存缓存；不使用归档、分片、额外独立 build 步骤或手动 dispatch（list/run 本身会在云端构建测试目标）。`.github/workflows/cicd.yml` 的 GitHub Actions 名称为 `master`，仅在 push 到 master 时打包并上传 artifact；它和 PR check 使用相同 `shared-key: vega-master-build` 及 `save-if: ${{ github.ref == 'refs/heads/master' }}`，master 是唯一写入方。rust-cache 还会按 runner 平台、工具链及 Cargo 输入派生实际缓存键。旧的 [Issue #123](123/vega-issue-123-pr-check-pipeline.md) / [Issue #140](140/vega-issue-140-ci-test-throughput.md) 流水线描述只记录历史决策；云端 nextest 使用 `.config/nextest.toml` 默认 profile：`retries=0`、`fail-fast=false`、600 秒挂起保护，不覆盖默认并发。
- **门禁执行**：云端 `check` 是合并前唯一强制执行门禁，失败由主 Agent 读取 PR 原始日志分析；独立审查仍是合并前提。Master 只允许 PR merge，不允许直接 push。本地**仅快速静态 check**，不运行 Vega、编译或测试；本卡功能点及 workspace 测试只交 GitHub Actions。本地 commit/push 不做强制检查、不排队、不锁 target；`.githooks/`、`scripts/verify.py` 与 cargo-lock 调度器已删除。
- **验收节奏（当前规则）**：明确验收与风险 → 专职子 Agent 最小实现/直接修复 → 必要验证与实现独立审查 → 最新 head required check 通过 → 主 Agent 主动 squash merge → In review → **停下等用户纯手动验收**。主 Agent 连续协调，不在职责交接处人为停止。CI 每 5 分钟查询，通知/用户即时请求除外，不短轮询/watch。合并时 Issue 开放、不 Done；允许自动删除已合并远端任务分支，证据、完整 SHA 和原始 patch 须可恢复。本地资源按 [安全清理规则](vega-issue-workflow.md#分支与-worktree-清理规则)另行检查，不授权删除用户数据；用户明确通过后才回写、完成剩余安全清理、关闭 Issue 并读回，最后 Done 并读回。清理/关闭受阻保持 In review、标待收尾。手测不通过退回 In progress，主 Agent 分析并组织最小修复与必要验证。真实模型/服务/UI 集成由用户在 master 上手测，不以 Agent 自产 E2E/截图替代。
- **任务级**：任务卡附验收矩阵与命令；运行程序、测试或 benchmark 的命令交 Actions，手工走查由用户完成，本地仅快速静态检查。
- **架构级**：`cargo tree` 检查无红线依赖关系；新增公共类型在 `vega_conversation::types`
- **报告**：贴本地静态检查、§7.2 按风险选择的实际验证及对应提交的云端命令原始输出，未运行项标待 CI、无计数明确未取得；空集写“no Rust tests”，不称 Rust 测试通过。未收到用户结论标待手测，不许用“通过了”替代证据或假装完整完成。收尾失败保留上下文并说明待办。
- **进程内自动回归**：不恢复历史 Rust 测试集合，必要新测试按风险覆盖业务、安全策略、解析、存储及 GPUI 测试上下文行为。不新增真实外部进程/网络/应用 E2E 自动门禁；系统集成由用户手测。禁止为了测试扩大 production public API；新写的有效断言不得放宽以变绿。
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

### 7.2 快速交付与按风险验证（当前规则）

本轮用户裁决移除强制 TDD，以最小改动快速交付，适用于主 Agent、子 Agent、Feature 与 Bug；完整政策见 [验证与交付政策](vega-rust-test-delivery-lifecycle.md)。本节取代历史测试先行、有效 RED 后才实现、独立测试前置审查、冻结后禁止补测、GREEN 后删测及配套证据链的强制流程。下方旧版本变更记录仅保留历史，不恢复这些要求。

1. **明确验收，最小实现**：从 spec 明确可观察行为、失败/恢复边界与非目标，选择最小改动。可直接修复，也可先实现后补必要测试；不为流程造临时测试、等待 RED 或安排删测提交。既有代码实现委派和实现独立审查要求不变。
2. **按风险选择验证**：根据影响范围、安全/数据风险、回归概率和已有覆盖选择必要静态检查、云端回归与用户手测。纯文档/格式不强加测试；新增边界、遗漏需求或新 Bug 可在明确验收后直接继续处理，不要求重开 TDD 周期。验收变化先更新 spec，不按实现输出反推断言。
3. **本地仅静态，云端 required CI 不变**：主 Agent 与子 Agent 本地不运行 Vega 代码、编译、构建、打包或测试（包括 cargo check、Clippy、test/nextest）。自动测试只由现有 Actions 运行，打包用云端发布流程；合并前确认待合并最新 head 的 required check `check (fmt, clippy, test)` 通过，不以静态检查代替 CI。云端不可用时报告待 CI，不本地绕过，也不阻止授权内的实现工作。
4. **修复真实失败，不伪造通过**：先读原始失败证据，区分产品缺陷、测试/观测接线错误、验收变化和外部环境失败，修责任层。不得放宽断言、删除失败测试、ignore 或盲目重试换绿；修正错误测试须说明依据，不改变验收含义以迁就实现。
5. **不恢复历史集合，不强制删测**：先同步最新 master，不从旧 ref 整目录/整仓恢复已清除的历史 Rust 测试及独占支持；历史无 Rust 测试最终树只是现状，不是每轮新增后删除测试的要求。必要新测试可按风险增加，不要求 GREEN 后删除。保留现存 Python 测试与 fixture、生产/benchmark/打包和通用 CI 能力，不恢复真实外部 E2E 自动门禁。
6. **诚实交付，用户手测独立**：记录精确版本、实际修改、已执行静态检查及其结果、云端 run/job 与未决风险，无需提供测试先行时序或 RED/GREEN/删测交接链。未运行写“待 CI”，缺失计数写“未取得”；有计数时区分发现/执行/通过/失败/忽略/跳过。空集标 `no Rust tests`（0 发现/0 执行），不是 Rust 测试通过。CI 与合并后仍为 In review，用户明确手测通过才关闭 Issue / Done；真实集成不以 mock 证据冒充。

**有效测试标准（需要测试时）**：验证公开/稳定边界的可观察结果、必要副作用和业务/安全不变量，优先少量有区分度的正常、失败、恢复样例。不以源码字符串/符号存在、仅 mock 调用/机械转发、镜像生产算法、重复同义断言或内部字段覆盖率充数。mock 只隔离外部依赖，不替代被测决策；调用次数仅在本身属于契约且结合结果/终态时有效。测试用行为命名，不把 issue/task 编号写入非文档代码。

---

*本文件随 spec 演进更新，变更记录：v0.1 (2026-08-29) 初版；v0.2 (2026-08-29) 验收门禁执行方式改为本地 git hooks（人类决策，防 CI 费用）；v0.3 (2026-08-29) UI 白名单 gpui/gpui_platform 来源改为 zed 官方仓库 git rev 锁定（crates.io 停滞且无 gpui_platform，人类批准）；v0.4 (2026-08-29) 基础白名单新增 toml（config.toml 解析，人类批准）；v0.5 (2026-08-29) mdstream 白名单条件激活（`待 spike 确认` → T14 spike 确认引入，锁定 =0.3.0）；v0.6 (2026-08-31) 人类冻结 E2E-first 验收与仓库证据留存规则，限制 test-only 笛卡尔扩张；v0.7 (2026-09-22) 门禁全部上云（Issue #123）：删除本地 hooks/verify.py/cargo-lock 调度器，PR 走云端 fmt/clippy/test，push master 打包，发布仍由 tag 触发；v0.8 (2026-09-23) 交付节奏改为「本地只跑本卡功能点测试 → 开 PR → 云端 check 绿 → Agent 主动合并 master → 卡改 In review → 用户手测」：用户手测通过才回写/关 Issue/Done/清理，未通过退回 In progress；真实验收由用户在 master 手动完成；v0.9 (2026-09-23) 用户裁决代码中禁止任何注释（`///`/`//!`/行内 `//`/`/* */`）：注释说明必要即命名或结构没写好，改命名/拆函数/提类型；仅 `unsafe` 块的 `// SAFETY:` 一行例外；v1.0 (2026-09-24) Issue #179：PR gate 恢复 master 的 `vega-master-build` Cargo cache 但不保存；`.github/workflows/cicd.yml` 的 workflow 名为 `master`。v1.1 (2026-10-16) Codex 先审查并交接具体 Bug 方案、Vega 按方案执行后交回；按角色自动取最新合格单卡，本地仅静态检查，测试只交 Actions，用户手测通过后才清理远端分支及本地 worktree。v1.2 本轮 Rust 测试一次性清零，保留 Python 测试；后续强制测试先行与云端有效 RED，禁止实现后补测，空集 CI 明示且保留原有门禁。v1.3 用户要求 Rust 测试仅在开发阶段保留：draft PR 验证 RED/GREEN，固定原始源码/提交与证据，独立删测并验证最终空集 head 后转正式 PR；Python 测试不变。

当前规则更新（v1.4，用户裁决）：统一使用 Agent，主 Agent 接手分析/spec/组织实现/PR/CI/合并/收尾，代码实现仍委派专职子 Agent 并经独立审查；同卡无验收缺口连续推进，不按厂商/宿主身份路由或等旧角色交接。CI 通过并合并后允许自动删除已合并远端任务分支，证据/完整 SHA/patch 可恢复；本地资源仅安全核实后清理，不授权删除用户数据；Issue 关闭/Done 仍等用户手测通过。此裁决取代上述历史版本的角色分工、人工交接停止点及手测前保留分支要求，历史变更记录原文保留。
