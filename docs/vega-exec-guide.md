# ✦ Vega — 执行层开发总纲（Executor's Constitution）

## Vega 本地验证限制（2026-10-16 用户更新）

**仅适用于 Vega 项目，包括它的所有 worktree、主 agent 和子 agent；不限制 Loom 自身或其他项目。**

- **Codex、Vega 及子 agent 本地不运行 Vega 代码，不编译、构建、打包或执行测试。** 定向 `cargo check`、Clippy、test / nextest 也不运行，不因缓存或范围小豁免。
- 本地只做低 CPU、快速结束的差异、格式和静态检查；skill frontmatter 静态校验可执行。自动测试只交 GitHub Actions，打包使用云端发布流程。
- 本规则优先于旧文档、技能、任务规格中的本地运行/编译/定向测试要求。测试用例和回归断言仍须编写；报告区分本地静态检查和待 CI 验证，不把未运行项标为通过。

**2026-09-25 用户更新（Issue #149）**：按 [移除真实 E2E 规格](vega-issue-149-remove-real-e2e.md) 优先将真实外部进程/网络/应用依赖 mock 化并保留业务和安全断言；仅删除 mock 后失去意义的纯真实适配契约测试，保留进程内业务、安全、存储及 UI 测试。该决定取代下文及历史规格的 E2E-first、真实适配覆盖和禁止删除此类测试的要求；真实系统集成由用户手测。

> **2026-09-22 Issue #140 用户更新：** [Git/Shell 测试依赖隔离](vega-issue-140-ci-test-throughput.md#user-directed-revision-isolate-external-execution-2026-09-22) 允许通过内部执行边界替身迁移业务分支测试，保留真实策略与安全断言，另用真实适配层集成测试验证 OS/Git 契约。该范围优先于 §7 的旧 E2E-first 限制；mockall 获批准用于本卡 dev-only 依赖（若需要），证据必须准确标注 mock/real。

> **2026-09-22 Issue #112 supersession:** Read/Edit/Write paths, read-before-mutation, replacement matching and audit/checkpoint path support follow [the file edit parity contract](vega-issue-112-file-edit-parity.md). Its user-authorized absolute/external paths and resolved symlinks replace the earlier project-only/relative-only prohibition for those tools; glob/grep/bash boundaries are unchanged. Existing permission, Git/checkpoint protection and race checks remain.

**版本** v1.1 · 2026-10-16 · 适用对象：Codex、Vega 及承接本仓任务的子 agent
**关联**：[vega-tech-spec-p1.md](vega-tech-spec-p1.md)（实现规格）· [vega-tech-risks.md](vega-tech-risks.md)（难点方案）· [vega-features.md](vega-features.md)（功能点 ID）· [vega-ui-spec.md](vega-ui-spec.md)（UI 准线）

> 本文件是执行模型的**最高行为准则**。每个任务 prompt 都必须附本文件路径。任何与本文件冲突的"看起来更合理"的做法都是错的。

---

## 1. 角色与协作模型

| 角色 | 职责与边界 |
|---|---|
| Codex | 查重建卡、先审查所有需求；Bug 定根因并交付可执行方案，Feature 默认由 Codex 组织实现。主会话负责协调/审查/集成，代码实现委派专职子 agent；负责 PR、CI、合并及用户反馈分析 |
| Vega | 只领取 Codex 已交接、方案完整可访问的 Ready 执行卡；按方案实现和补测试，完成或遇阻交回 Codex。默认不建卡、不改 spec、不扩大范围或开放式排障 |
| 用户 | 产品取舍；合并后的纯手动验收。明确通过前 Issue 开放、状态 In review、任务分支/worktree 保留 |

共用 [vega-kanban-delivery](../.agents/skills/vega-kanban-delivery/SKILL.md)，角色及队列细则见 [Issue 工作流](vega-issue-workflow.md)。以用户明确指定的工作流角色或宿主系统身份识别，Vega 为 `You are Vega`；不从模型、Provider、项目名或路径猜测。只有身份无法确认才询问，有证据时直接路由。架构师职责由 Codex 主会话承担，产品裁决仍由用户决定。

**执行模型三铁律：**
1. **spec 之外零发挥**。任务卡没写的设计决策，不许自己拍——上报，等裁决。
2. **红线清单里的行为，一次都不许出现**（见 §3）。
3. **实现回报、CI 通过、合并及用户验收分别记录**。测试仅在 Actions 执行，未跑写待 CI；Vega 回报后仍需 Codex 审查/集成，完整完成还须用户手测与收尾（见 §2、§7）。

## 2. 工作流（每个任务的标准动作）

1. 用户指定或明确继续的卡优先；否则自动完整读取本角色候选队列，过滤关闭、占用、当前角色/阶段前提未满足、缺所需方案和未交接 PR，再按 Issue `createdAt` 降序、同时间编号降序取最新一张，不按更新或 Project 默认顺序。解释、审阅和维护 skill 不启动其他卡。
2. Codex 取可开始审查的 Backlog、等待 Codex 的 Ready，或 Vega 最新交接明确交回、当前待领取且无活跃执行者的 In progress；这类阻塞卡可由 Codex 接回分析，不能因导致 Vega 阻塞的依赖缺口排除。Vega 仅取明确交给 Vega、方案完整可访问且执行依赖已满足的 Ready。领取前重读，立即置 In progress 并读回；交回卡列不变。Issue 记录阶段、当前/下一执行者、时间、方案版本和工作上下文，冲突时让出，不把读回当原子锁。
3. Codex 先审查需求/spec，先写验收用例再写实现计划。Bug 须交付根因/证据、具体文件/符号和修改动作、边界、测试矩阵、Actions 命令、手测/回滚、基线/分支/交接 commit 及回报约定；提交到 Vega 可访问位置并在 Issue 发布链接后才 Ready，不能只给会话结论或未提交本地文件。Feature 默认仍由 Codex 组织专职子 agent 实现。
4. 修改仓库前 fetch 并更新基线，保护已有改动；每卡用 `~/Workspace/worktrees/` 下独立 worktree 和 `feat/<task-id>-<slug>` 分支，不直接改 master。同卡单一写入者，交接后前任停止写入。Vega 按已发布方案实现/补测试；完成或遇阻回报同卡，仍 In progress，当前执行者待领取、下一执行者 Codex 并停止写入，Codex 可自动接回。
5. **本地仅快速静态 check**。Codex 审查 Vega 或子 agent 的 diff/覆盖后开 PR，不使用自动关闭 Issue 的关键字。测试交 Actions，CI 启动后每 5 分钟查询，不短轮询或 watch；收到完成/失败通知或用户要求即时查看除外。最新 head 的 required check 通过后仅 Codex 主动 squash merge，才置 In review，**停下等用户纯手动验收**。
6. 用户明确手测通过 → Codex 回写 → 安全检查并完成本卡远端分支及本地分支/worktree 清理 → 关闭 Issue 并读回 → 最后置 Done 并读回；清理或关闭受阻则保留 In review、标待收尾，不能先 Done 再补清理。未通过 → Codex 记录反馈、退回 In progress 并先分析，Bug 修订方案后再 Ready 交 Vega。手测前不关闭、Done 或清理；Done 仅表示验收和全部收尾均已完成。
7. 默认一张，到 Codex 交 Ready、Vega 交回或合并后等用户时停止，只有连续授权才取下一张；无合格卡说明阻碍并停止。提交格式为 `feat(A2-09): <一句话>` / `fix(A3-07): <一句话>`（ID 见 vega-features.md）。
8. 实现报告写修改文件/提交、静态检查命令与退出码、待 CI 测试及对应云端原始输出、限制/耗时和规格偏离（必须为无），不把“已派发/提交”当“已通过/完成”。

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
- ❌ 为通过测试而改测试断言（测试不过 = 实现错，除非任务卡注明测试待更新）

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
| 测试 | 每模块 `#[cfg(test)]`；runtime 用 mock provider 回放（tech-spec §8） |
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

**PR gate 拓扑（2026-09-24 用户裁决，[Issue #175](vega-issue-175-single-pr-check.md)；缓存和 workflow 命名由 [Issue #179](vega-issue-179-shared-pr-cicd-cache.md) 更新，测试运行器按 2026-09-25 [Issue #183](vega-issue-183-nextest-ci.md) 更新，并行结构按 [Issue #188](vega-issue-188-parallel-ci-checks.md) 更新）**：门禁全部在云端。`.github/workflows/pr-check.yml` 仅响应 base 为 `master` 的 PR，并以两个无相互依赖的 `macos-latest` job 并行执行：Clippy job 依次运行 `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings`；Nextest job 运行 `cargo nextest run --workspace`。required check `check (fmt, clippy, test)` 是依赖两者的轻量汇总 job，以 `always()` 运行，仅两个结果均为 `success` 才通过，失败、取消和跳过均使汇总失败。通过 `taiki-e/install-action@v2` 安装固定 `nextest@0.9.146`，使用默认测试并发。CI 覆盖单元和集成测试；nextest 不支持 doctests，按用户只使用 nextest 的要求不另跑 `cargo test --doc`。失败直接阻断合并，不自动重试。该 workflow 保留 Rust 1.98.0、`contents: read`、执行 job 的 60 分钟超时和并发取消；两个执行 job 使用 `Swatinem/rust-cache@v2` 恢复 master CICD 写入的 `vega-master-build` 缓存，PR 的 `save-if` 条件为 false，因此 PR 不保存缓存；不使用归档、分片、额外 build 或手动 dispatch。`.github/workflows/cicd.yml` 的 GitHub Actions 名称为 `master`，仅在 push 到 master 时打包并上传 artifact；它和 PR check 使用相同 `shared-key: vega-master-build` 及 `save-if: ${{ github.ref == 'refs/heads/master' }}`，master 是唯一写入方。rust-cache 还会按 runner 平台、工具链及 Cargo 输入派生实际缓存键。旧的 [Issue #123](vega-issue-123-pr-check-pipeline.md) / [Issue #140](vega-issue-140-ci-test-throughput.md) 流水线描述只记录历史决策；云端 nextest 使用 `.config/nextest.toml` 默认 profile：`retries=0`、`fail-fast=false`、600 秒挂起保护，不覆盖默认并发。
- **门禁执行**：云端 `check` 是合并前唯一强制门禁，失败由 Codex 读取 PR 原始日志分析；Master 只允许 PR merge，不允许直接 push。本地**仅快速静态 check**，不运行 Vega、编译或测试；本卡功能点及 workspace 测试只交 GitHub Actions。本地 commit/push 不做强制检查、不排队、不锁 target；`.githooks/`、`scripts/verify.py` 与 cargo-lock 调度器已删除。
- **验收节奏（2026-10-16 用户更新）**：Vega/子 agent 实现回报 → Codex 审查并开 PR → 最新 head 的 required check 通过 → Codex 主动 squash merge → In review → **停下等用户纯手动验收**。CI 每 5 分钟查询，通知/用户即时请求除外，不短轮询/watch。合并时 Issue 开放、不 Done、不删除本卡远端分支和本地分支/worktree；用户明确通过才回写，安全检查并完成清理，再关闭 Issue 并读回，最后 Done 并读回。清理或关闭受阻时保持 In review、标待收尾，不先 Done 再补清理。用户不通过由 Codex 退回 In progress 先分析，再给 Vega 具体 Bug 方案。真实模型/服务/UI 集成由用户在 master 上手测，Agent 不以自产 E2E/截图替代。
- **任务级**：任务卡附验收矩阵与命令；运行程序、测试或 benchmark 的命令交 Actions，手工走查由用户完成，本地仅快速静态检查。
- **架构级**：`cargo tree` 检查无红线依赖关系；新增公共类型在 `vega_conversation::types`
- **报告**：贴本地静态检查与对应提交的云端命令原始输出，未运行项标待 CI；未收到用户结论标待手测，不许用“通过了”替代证据或假装完整完成。收尾失败保留上下文并说明待办。
- **进程内自动回归（2026-09-25 用户裁决，取代 E2E-first）**：自动测试覆盖业务、安全策略、解析、存储及 GPUI 测试上下文行为。移除真实外部进程/网络/应用 E2E，不新增同类自动门禁；系统集成由用户手测。禁止为了测试扩大 production public API；保留范围内的断言不得放宽以变绿。
- **证据留存**：每 Sprint 报告必须记录命令、UTC/本地时间、branch、测试时 tree/content hash、结果与 accepted residual；raw 日志可暂存 `/tmp`，仓库文档不得写入真实 key、raw workspace path/OID或伪造尚不存在的 commit/PR。

### 7.1 历史 E2E 证据分级与仓库模板

以下分级用于解释既有历史报告，不要求继续保留真实 E2E 自动测试。当前自动门禁范围以 #149 的 2026-09-25 修订为准。

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

---

*本文件随 spec 演进更新，变更记录：v0.1 (2026-08-29) 初版；v0.2 (2026-08-29) 验收门禁执行方式改为本地 git hooks（人类决策，防 CI 费用）；v0.3 (2026-08-29) UI 白名单 gpui/gpui_platform 来源改为 zed 官方仓库 git rev 锁定（crates.io 停滞且无 gpui_platform，人类批准）；v0.4 (2026-08-29) 基础白名单新增 toml（config.toml 解析，人类批准）；v0.5 (2026-08-29) mdstream 白名单条件激活（`待 spike 确认` → T14 spike 确认引入，锁定 =0.3.0）；v0.6 (2026-08-31) 人类冻结 E2E-first 验收与仓库证据留存规则，限制 test-only 笛卡尔扩张；v0.7 (2026-09-22) 门禁全部上云（Issue #123）：删除本地 hooks/verify.py/cargo-lock 调度器，PR 走云端 fmt/clippy/test，push master 打包，发布仍由 tag 触发；v0.8 (2026-09-23) 交付节奏改为「本地只跑本卡功能点测试 → 开 PR → 云端 check 绿 → Agent 主动合并 master → 卡改 In review → 用户手测」：用户手测通过才回写/关 Issue/Done/清理，未通过退回 In progress；真实验收由用户在 master 手动完成；v0.9 (2026-09-23) 用户裁决代码中禁止任何注释（`///`/`//!`/行内 `//`/`/* */`）：注释说明必要即命名或结构没写好，改命名/拆函数/提类型；仅 `unsafe` 块的 `// SAFETY:` 一行例外；v1.0 (2026-09-24) Issue #179：PR gate 恢复 master 的 `vega-master-build` Cargo cache 但不保存；`.github/workflows/cicd.yml` 的 workflow 名为 `master`。v1.1 (2026-10-16) Codex 先审查并交接具体 Bug 方案、Vega 按方案执行后交回；按角色自动取最新合格单卡，本地仅静态检查，测试只交 Actions，用户手测通过后才清理远端分支及本地 worktree。
