# AGENTS.md — Vega 仓库协作准则

## Vega 本地验证限制（2026-10-16 用户更新）

**仅适用于 Vega 项目，包括它的所有 worktree、主 agent 和子 agent；不限制 Loom 自身或其他项目。**

- **Codex、Vega 及子 agent 本地不运行 Vega 代码，不编译、构建、打包或执行测试。** 包括定向 `cargo check`、Clippy、test / nextest，不因缓存、范围小或预计耗时短而豁免。
- 本地只做低 CPU、能快速完成的差异、格式和静态检查；skill frontmatter 静态校验可执行。自动测试只由 GitHub Actions 运行，打包使用云端发布流程。
- 本规则优先于旧文档、技能、任务规格中要求本地运行、编译或定向测试的条款。有行为变化时按下文 TEST-FIRST 编写测试；报告区分本地静态检查和待 CI 验证，不把未运行项标为通过。此前仅针对上一轮编号清理的本地执行授权不延续到本轮或全局。

Cross-agent instructions for Vega — a native AI agent desktop (Rust + GPUI).

## Rust 测试清零与 TEST-FIRST（当前规则）

以 [Rust 测试开发/交付生命周期](docs/vega-rust-test-delivery-lifecycle.md) 和 [执行指南 §7.2](docs/vega-exec-guide.md#72-test-first-与有效测试当前规则) 为准，适用于主 agent、子 agent、Feature 与 Bug：**Rust 测试在开发阶段保留到 GREEN，最终交付树不保留测试**。Python 测试继续保留；[最初清零规格](docs/vega-rust-test-reset-tdd.md)中的旧保留要求以本次裁决为准。

- **一次性清零**：从 `cf00946e` 删除整个 workspace（含 `xtask`）的现存 Rust 测试、可执行 doctest 及其独占支持/接线；不改生产行为、benchmark 或打包契约。保留 Python 发布、组件准备、delta 测试及 fixture；保留 Cargo/Nextest、fmt、Clippy 能力。不批量改写历史规格或报告，也不为清零人为造新测试/RED。
- **守住合并基线**：旧分支先同步最新基线，不从旧 ref 整目录或整仓恢复 `master` 已清除的测试集合。逐项审查本轮测试及必要支持的来源，开发 GREEN 后也须删测才能正式交付。历史测试改名/改写不自动算新回归，确需新回归独立先测后修并留证；纠偏历史见 [合并纠偏规格](docs/vega-rust-reset-reconciliation.md)。
- **验收 → 测试 → 有效 RED → 最小实现 → 同一测试 GREEN → 删除测试 → 最终门禁 → 正式 PR**：生产实现尚未修改时先写齐本轮行为与失败边界测试，记录基线与测试补丁/提交；由 Codex 审查并建立/复用 draft PR 触发现有 Actions，确认目标行为缺失导致失败后才进入实现。最小缺失 API 可作接口缺失 RED，必须说明行为断言尚未执行；无关编译、依赖、网络或 runner 失败不算 RED。
- **GREEN 后删测、PR 留证**：开发和实现期间保留 Rust 测试及必要接线；同一冻结测试云端 GREEN 且证据固定后，用独立提交删除 Rust 测试、可执行 doctest 及独占 fixture/mock/helper/feature/dev dependency。保留生产、benchmark 和通用 CI 能力，不通过删除失败或未执行测试逃避 GREEN。删测后的最新 head 完整门禁通过、确认 0 发现/0 执行后，才转正式 PR/申请合并；空集不是测试通过。
- **冻结测试目标与预期，禁止实现后补测**：不按实现输出反推断言、不为报告/覆盖率追加用例、不放宽断言、不 ignore、不自动重试换绿。新需求、遗漏边界或新 Bug 必须停止本轮，记录独立验收变化并重新开始先测后修；不能用补测追认已完成实现。测试自身错误/接线修正须先留证审查且不改变验收含义，否则重新开轮。
- **有效测试**：验证稳定/公开边界的可观察结果、必要副作用和业务/安全不变量；mock 只隔离外部依赖，不替代被测决策。不以源码字符串/符号存在、仅 mock 调用/机械转发、镜像算法、重复同义断言或内部字段覆盖率充数；调用次数只有本身属于契约且结合结果/终态才有效。新测试用行为命名，不把 issue/task 编号写入非文档代码。
- **范围与证据**：纯文档/格式变更无行为变化时不强加测试；纯重构的验证测试也只在开发阶段保留到 GREEN，最终同样删除。交接/PR 固定可访问的测试源码/补丁、完整提交 SHA、RED 原始失败、实现顺序、带测试 GREEN、独立删测提交和最终 head 空集门禁；squash/删分支后仍能从 PR 找回原测试，不依赖本机临时文件或会过期的 artifact。绿色 CI 或提交顺序不自动证明测试先行，不用删测后的空集替代带测试 GREEN。

**历史决策（2026-09-25）**：[移除真实 E2E 规格](docs/vega-issue-149-remove-real-e2e.md) 当时要求保留进程内测试。本次 Rust 清零取代其保留旧测试要求；外部依赖隔离、真实系统集成由用户手测的边界仍适用于后续测试，不恢复真实 E2E 自动门禁。

## 最高原则：SDD（Spec-Driven Development）

**历史决策（2026-09-22）**：[测试依赖隔离规格](docs/vega-issue-140-ci-test-throughput.md#user-directed-revision-isolate-external-execution-2026-09-22) 记录 Git/Shell 测试迁移及当时的真实适配层覆盖要求；不构成本次清零的保留要求或后续真实 E2E 门禁。不得把 mock 证据标作真实进程验收。

**Spec 先行，代码不允许先于 spec。** 所有实现工作必须对应 [`docs/`](docs/) 中的具体规格章节。设计文档以本仓库 `docs/` 为准（主索引见 [README](README.md#状态)）。

- [`docs/vega-exec-guide.md`](docs/vega-exec-guide.md) 是**执行宪法**：红线清单、依赖白名单、遇阻上报协议、验收协议。任何 agent 开工前必读。
- 视觉/UI 工作必须先读 [`docs/vega-design-guidelines.md`](docs/vega-design-guidelines.md)。它统一设计语言与 token 使用；产品行为、安全契约和任务级冻结规格仍按各自更具体的 spec 执行。
- 发现 spec 缺陷 → 提 issue 或修改 spec 文档并注明变更记录，**禁止代码先行**。

## 通用 UI/UX 规则（2026-10-13 用户裁决）

适用于界面设计、实现与评审，包括主 agent 和子 agent。**优先通过信息层级、控件、交互和真实状态表达功能；辅助文案必须服务用户当下的判断或操作。** 本规则约束产品额外添加的说明，不改写用户内容、模型回答或业务数据，也不降低安全、数据真实性和无障碍要求。

- **新增文案前先判断用途**：它帮助用户识别对象、选择动作、理解结果，还是避免具体错误？若只是解释实现、复述需求、证明功能已完成，放进开发文档或交付报告，不放进产品界面。
- **先设计行为，再补必要解释**：可编辑性用输入框/只读值表达，选择用选中态表达，运行用真实进度或运行态表达，完成用实际结果表达。不要用一段说明代替缺失的状态、操作入口或错误恢复路径；禁用控件的必要原因应在相关位置可获取。
- **使用用户任务语言**：内部字段名、数据结构、配置来源、记录数量和后端执行阶段，不直接拼成副标题或状态条。只展示影响当前决策的内容；排障细节进入详情或日志，面向用户的失败提示说明问题及下一步。
- **默认界面保留主要内容与动作**：不机械地给每个页面、卡片、列表项添加副标题、说明段或提示条。同一信息在合适层级表达一次；文件尺寸、格式、来源等元数据按任务重要性安排，次要信息按需查看，不能因“有数据可展示”就占据主要视觉位置。
- **帮助按需、就近出现**：不影响当前操作的背景、规则细节和快捷键放在详情、展开区或帮助入口；必要的字段格式、输入约束和首次使用指引可就近简短呈现。不要把删掉的说明批量换成常驻 info 图标或 tooltip，制造另一种噪声。
- **反馈有明确生命周期**：结果已清楚可见时，不再叠加成功说明。异步等待或结果不明显时，可提供局部、适时的反馈；普通成功反馈结束后消退，错误和待处理事项在解决前保持可见或可再次访问。文案必须符合可验证事实，“已提交/已交给外部应用”不能冒充“已完成/已打开”。
- **保留必要提示**：真实错误、权限请求、阻塞条件、保存冲突、待重启或待用户处理事项，以及影响决策的费用、隐私、不可逆操作风险，必须在相关时机清楚呈现；不能为了简洁藏进折叠区或悬停帮助。提示说明影响与可采取的动作，条件解除后移除；普通操作不泛化为警告或确认弹窗。
- **简洁不能降低可发现性与可访问性**：保留清楚的操作名称、字段标签和必要单位；陌生或含糊的动作使用文字或图标加文字。图标按钮提供可访问名称，帮助支持键盘访问，关键信息不只依赖颜色、图标或 hover。

常见取舍：

| 场景 | 优先表达 |
|---|---|
| 图片预览里的尺寸 | 普通阅读时放次级信息或详情；核对分辨率本身是任务时可直接展示 |
| 调用外部应用打开文件 | 以实际打开行为反馈；确需回执时简短、短暂且只陈述已知状态，失败时提供恢复入口 |
| 字段或时段不可修改 | 显示真实值和只读状态，必要原因就近可查；不在每行重复内部 metadata/锁定说明 |
| 多个列表项共享同一规则 | 在所属分组表达一次；只对例外项增加有意义的提示 |

**交付前检查**：逐项审视新增的可见文字，能否删除、由控件/状态表达或延后到需要时再显示？同时核对适用的默认、运行、完成、失败、禁用与键盘操作状态，确保主要动作可发现、结果可信、错误可恢复、必要提示仍可见。删除冗余文案必须与交互检查一起完成；验证方式遵守目标仓库的本地检查与 CI 限制。

## 工作流

1. **不在 `master` 上直接实现**——每卡用 `~/Workspace/worktrees/` 下独立 worktree 和 `feat/<task-id>-<slug>` 分支。**任何仓库修改前先 fetch 并更新基线**；通常为 `git fetch` 后 `git rebase origin/master`，有脏改动或分叉先保护核实，不 reset 或覆盖。
2. **共用入口与身份**：取卡、实现、交接和收尾使用 [vega-kanban-delivery](.agents/skills/vega-kanban-delivery/SKILL.md)，细节以 [Issue 与角色工作流](docs/vega-issue-workflow.md) 为准。角色优先采用用户明确指定的工作流角色，否则采用宿主系统身份；Vega 为 `You are Vega`，不从模型名、项目名或路径猜身份，有证据不重复询问。
3. **自动单卡**：用户指定或明确继续的卡优先；否则完整读取本角色候选队列，先排除关闭、占用、当前角色/阶段前提未满足、缺所需方案和未交接 PR，再按 Issue `createdAt` 降序、同时间编号降序取最新一张。Codex 取可审查的 Backlog、等待 Codex 的 Ready，或 Vega 最新交接明确交回、当前待领取且无活跃执行者的 In progress；这类阻塞卡可接回分析，不能因导致 Vega 阻塞的依赖缺口排除。Vega 仅取方案完整可访问、执行依赖已满足且明确交给 Vega 的 Ready。领取即 `In progress`，Issue 写阶段及当前/下一执行者；交回的 In progress 保留原列。解释、审阅或维护 skill 不启动其他卡。默认一张，仅连续授权才取下一张。
4. **Codex 先接卡**：Codex 查重建卡并先审查所有需求；Bug 定位根因，先冻结验收与测试方案、再写具体到文件/符号/修改动作的实现计划，提交并发布可访问交接链接后置 Ready 等 Vega。新功能默认由 Codex 组织实现，**代码实现委托专职 subagent**；Codex 主会话负责协调、审查、集成、PR/CI/合并及用户反馈分析。卡外发现单独记录，不扩大实现范围。
5. **Vega 执行**：领取已交接 Ready 后立即 `In progress`，先写齐测试并交 Actions 确认有效 RED，再按方案实现和验证同一测试 GREEN，禁止实现后补测；不建卡、不改 spec、不开放式排障。完成或遇阻都在同卡交回 Codex，保持 `In progress`，写当前执行者待领取、下一执行者 Codex 并停止写入。Codex 交到 Ready、Vega 交回或合并后等待用户，均为默认单卡停止点；同卡始终只有一个实现写入者。
6. **合并与验收**：本地仅静态检查，测试只交 GitHub Actions；Codex 审查并确认最新 head 的 required check 通过后主动 squash merge master，才改 `In review` 并停下等用户纯手动验收。用户明确通过后 Codex 回写，按 [分支与 worktree 清理规则](docs/vega-issue-workflow.md#分支与-worktree-清理规则)完成本卡剩余安全清理，再关闭 Issue 并读回，最后置 Done 并读回；清理或关闭受阻时保持 `In review`、标待收尾，不先 Done 再补清理。未通过由 Codex 先分析，Bug 修订方案后重新交 Vega。**手测前不得关闭或 Done；最新 head 的 required check 通过并合并后，允许自动删除已合并远端任务分支，证据、完整 SHA 和补丁须可恢复。本地分支/worktree 仅在确认无运行 agent、无未提交或未合入改动后安全清理；允许删分支不等于授权删除用户数据。Done 仅表示验收和全部收尾均已完成。**
7. **遇阻与证据**：按 exec-guide §6 用 `[BLOCKED]` 上报，Vega 的方案缺口交回 Codex，禁止自创方案绕过。后续先行测试按任务覆盖进程内业务/安全/存储/UI；真实模型、服务和 UI 系统集成由用户在合并后的 master 上手测，不以 Agent 自产 E2E/截图替代；证据分级见 [exec-guide §7](docs/vega-exec-guide.md#7-验收协议每个任务卡通用)。

## 问题与需求收集

- 由 Codex 按 [Issue 与看板工作流](docs/vega-issue-workflow.md) 查重并收集明确要跟踪的问题和需求，Vega 默认不建卡。**登记不等于承诺立即修复**：暂未开始时给用户链接和真实 Backlog 状态，不把它假装成进行中。
- GitHub Issue 是执行事项与证据的单一入口，GitHub Project 只管理流转状态；Notion PRD 管产品目标，本仓 `docs/` 管实现规格。Issue 本身不替代上述 spec，也不授权卡外代码实现。
- 无法访问 Project 时，不得声称已入看板；如已获授权且可创建 Issue，先保留 Issue 并明确说明待关联。仓库公开，截图、日志和复现信息必须脱敏。

## 提交与 PR

- 提交格式：`feat(scope): <一句话>` / `fix(scope): <一句话>`（功能点 ID 见 [features](docs/vega-features.md)）
- 小步提交，一个任务卡 ≤3 个 commit
- PR 必须附：本轮冻结测试目标、实现未改的 RED 基线/测试源码或补丁与原始原因、实现顺序、带相同测试的 GREEN 提交与云端输出、独立删测提交及最终 head 空集完整门禁（未运行标待 CI 验证），以及 spec 偏离说明（必须为无）。测试源码与提交链接须可从 PR 找回；正式 PR 最终树无 Rust 测试。文档/格式等无行为变化项说明不适用，不伪造 RED；不附本地全量测试。
- 合并方式：由 Codex squash merge。**用户手测通过后才删除本卡远端功能分支及本地分支/worktree**；合并时保留，取代旧的合并后立即删分支规则。
- **合并由 Codex 主动完成**：确认待合并最新 head 的 required check 绿后合入 master，完成后才把卡片改为 `In review` 等待用户手测；PR 不使用自动关闭 Issue 的关键字。

## 验证门禁与云端 CI（2026-09-25 用户裁决，相关规格）

**门禁全部在云端。本地 commit/push 不做任何强制检查、不排队、不锁 target。** 当前 PR gate 见 [相关规格](docs/vega-issue-188-parallel-ci-checks.md)（在 [相关规格](docs/vega-issue-183-nextest-ci.md) 测试运行器基础上并行 Clippy 与 nextest）；云端门禁拆分的历史背景见 [相关规格](docs/vega-issue-123-pr-check-pipeline.md)。

- **Master 只允许 PR merge，不允许直接 push。** PR 必须通过云端 `check`（fmt / clippy / test）才能合并；分支保护由 admin 在 GitHub Settings → Branches 开启并勾选 required check。
- `.github/workflows/pr-check.yml`：仅在 `pull_request`（base `master`）运行两个无相互依赖的 `macos-latest` job：Clippy job 顺序运行 `cargo fmt --all -- --check` 和 `cargo clippy --workspace --all-targets -- -D warnings`，Nextest job 保留三组 Python 检查（`python3 scripts/release_tests.py`、`python3 scripts/prepare_computer_use_tests.py`、`python3 -m unittest discover -s scripts/tests -v`），先用 `cargo nextest list --workspace --message-format json` 清点 Rust 测试；仅 `test-count` 为 0 时运行 `cargo nextest run --workspace --no-tests=pass`，非空仍运行 `cargo nextest run --workspace`。日志与 job summary 明确区分 `no Rust tests`（0 发现/0 执行）与非空执行通过；非空发现数不冒充执行数，执行/跳过数看 Nextest 原始输出。两者使用 Rust 1.98.0；Clippy job 安装 `rustfmt` / `clippy`。轻量汇总 job 的 required check 名固定为 `check (fmt, clippy, test)`，依赖两个执行 job，以 `always()` 运行；仅两者结果均为 `success` 才通过，失败、取消或跳过均使汇总失败。通过 `taiki-e/install-action@v2` 安装固定 `nextest@0.9.146`，使用默认测试并发。CI 覆盖单元和集成测试；nextest 不支持 doctests，按用户只使用 nextest 的要求不另跑 `cargo test --doc`。任何失败都使 required check 失败。保留 `contents: read`、执行 job 的 60 分钟超时和同 ref 并发取消；使用 `Swatinem/rust-cache@v2` 在两个执行 job 中恢复 master CICD 的 `vega-master-build` 缓存，但 PR 不写缓存；不使用归档、分片、额外独立 build 步骤或手动 dispatch（list/run 本身会在云端构建测试目标）。
- `.github/workflows/cicd.yml`（GitHub Actions 名称 `master`）：push 到 master（PR merge 后）直接调用 `release.yml` 共用发布流程，固定事件 SHA 自动分配 patch 版本、打包并发布 Release；与 PR check 使用相同 `shared-key: vega-master-build`，仅自动 master 发布写缓存。rust-cache 的实际缓存还依赖 runner 平台、工具链和 Cargo 输入，因此只有兼容的缓存条目会命中。
- 2026-09-25 用户裁决：每次合入 master 自动 patch 发布；保留人工 stable tag 与手动重跑，共用发布串行队列 `queue: max`，不取消正在运行的发布。先 draft，资产完整才 publish；同 SHA 幂等、正式资产不可覆盖，拒绝逆序发布。见 [发版指南](docs/vega-release.md)。
- 历史 [相关规格](docs/vega-issue-123-pr-check-pipeline.md) 与 [相关规格](docs/vega-issue-140-ci-test-throughput.md) 的 PR gate 拓扑已由 [相关规格](docs/vega-issue-175-single-pr-check.md) 取代；旧的运行数据与测试证据保留为历史记录。
- 本地不再安装 git hooks：`.githooks/`、`scripts/verify.py`、`cargo-lock.sh` / `cargo-coordinate.py` / `cargo-share-target.sh` 已删除；现存 `scripts/tests/` 是保留的 Python 脚本回归，不属于 Rust 清零。调度器的移除不代表允许本地编译；本地验证遵守顶部的快速 check 限制。
- `.config/nextest.toml` 用于云端 PR gate：`retries=0`、`fail-fast=false`、600 秒挂起保护；不覆盖默认并发。
- **本地仅做快速静态 check，功能点测试与全量门禁只交云端 `pr-check`**（2026-10-16 用户更新，取代本地定向测试要求）。有行为变化时必须先写有效测试/矩阵并确认 RED，再实现；同一测试 GREEN 后固定源码/提交与云端证据，再删除 Rust 测试及独占支持并验证最终空集 head，才转正式 PR。不得先删失败/未执行测试、加 ignore、放宽断言或重试到绿。空集放行不吞编译、发现或执行失败，不替代带测试 GREEN 或证明 TDD 时序。

## PR 检查轮询节奏（2026-10-15 用户裁决）

- **PR 的云端检查启动后，每 5 分钟查询一次状态**；适用于主 agent、子 agent 和所有 worktree，不使用更短间隔的轮询或 watch。
- 等待期间继续独立的审查、文档等工作；没有新结果时不反复查询或播报相同状态。
- 收到 CI 完成/失败通知，或用户明确要求立即查看时，可以立即检查；发现失败后读取日志并修复，新一轮检查启动后恢复 5 分钟间隔。
- 此规则只调整检查频率，不降低门禁：仍须确认待合并的最新提交通过 required checks，才能合并。

## 应用安装入口（2026-09-25 用户更新约定）

- 用户明确批准 `/Applications/Vega.app`、`~/Applications/Vega.app`，兼容 `~/Documents/Vega/Vega.app`；bundle ID 保持 `ai.vega`。只更新实际运行的 canonical app；不得迁移或修改其他副本。独立更新签名与路径规则见 [规格](docs/vega-issue-181-independent-update-signing.md)。
- 新版本原位替换当前运行的受支持 app；不要创建带版本号的安装名、指向 worktree 的符号链接或从 `dist` 启动日常应用。启动使用明确路径，不按名称或 bundle ID 选择副本。权限不足时提示人工安装，不提权、不移除安全属性。
- `dist/Vega.app` 仅是打包产物。备份优先保存为 zip，附原二进制哈希和构建 commit；不要长期散落可被系统索引的 `.app` 备份。既有验收证据不得擅自删除。
- 安装须独占，先检查正在运行的任务；未经确认空闲不得强退。候选构建签名/哈希验证完成后再替换固定位置，并核对安装后二进制身份。
- 不主动执行 `lsregister -f`、刷新 Dock/Launchpad 或重建系统应用数据库。macOS 仍可能自动发现 Documents 中的应用；目录约定不是禁止系统索引的机制，不承诺 Launchpad 永不显示。
- 如用户明确要求清理旧入口，只对确认 bundle ID 为 `ai.vega` 的旧构建/备份路径逐个注销注册；先留存清单，不删除备份文件，不重置整个 Launch Services 数据库或 Launchpad 布局。
- “代码已合并”和“应用已安装”分别报告，不能把旧安装当作最新 master。纯文档约定不触发重编译或应用安装；用户明确要求迁移既有 app 时核对迁移前后哈希。

## 原生 UI 验收：合成输入事件无效（工具限制）

以下原生验收/GPUI 实测段落保留为历史参考；其中测试名、路径、数量和命令不证明清零后的测试仍存在，也不授权本地执行。后续测试须按当前 TEST-FIRST 规则重建，自动执行仅交 Actions，原生手测交用户。

**不要用合成键盘/鼠标事件验证本应用的焦点行为——它们驱动不了 GPUI 的焦点链。** 这是工具限制，不是产品缺陷；写成规则以免每轮重复踩坑。

2026-09-13 实测，四种方法全部失败：

| 方法 | 结果 |
|---|---|
| Quartz `CGEventCreateKeyboardEvent` 发 Tab | 焦点不移动（连续 10 次停在同处） |
| AppleScript `key code 48` | 同上 |
| Quartz unicode 键盘事件打字 | 字符不进入输入框 |
| macOS AX 查询 `AXFocusedUIElement` | 恒返回 `AXWindow`；窗口 AX 子元素仅 4 个 |

根因：GPUI 应用不把内部焦点暴露给 macOS 辅助功能树，且 `focus_visible` 只在键盘导航路径下渲染。

**推论与正确做法：**

- **焦点归属不能靠截图判断**。composer 等控件用 `focus_visible`（仅键盘导航时可见），鼠标点击后的截图看不出焦点在哪。要验证焦点，读**代码链路**或用 GPUI `TestAppContext`（见下）。
- **焦点行为的权威验证方式是生产测试**。GPUI 测试可直接注入并断言真实焦点状态：`window.update(cx, |_, w, cx| handle.focus(w, cx))` 后用 `handle.contains_focused(w, cx)` 断言，`window.focus_next(cx)` 可验证键盘导航路径。范例：`crates/vega/src/window/workspace.rs` 的 `_inactive_close_follows_parent_focus_and_group_hover`。
- **合成鼠标事件仍可用于**：悬停（tooltip 会出现）、点击按钮触发无焦点依赖的动作。但依赖焦点状态的交互（tab 键盘激活、`focus_visible` 样式）不可用此法验证。
- **原生像素验收仍有效**，用于颜色/几何/布局：截图 + 像素测量是可靠证据（如暗色下 tab pill 填充 `(39,39,39)` 与 `0.03×255+0.97×32=38.61` 吻合）。不可靠的只有**依赖焦点的**那部分。
- 验收报告须区分「行为已验证（生产测试）」与「像素外观已验证（截图）」；不要把前者当作后者。

同类记录：`docs/vega-r47-panel-structure-alignment-delivery.md:80`。

## 原生验收的操作方法：六个必踩的坑（2026-09-14/15 实测）

 验收耗时远超预期，全部时间花在**定位窗口和坐标**上，与产品无关。以下每条都真实发生过一次，照做可省一整轮。

### 坑 1 · `pgrep -f '<path>'` 会匹配到自己

`pgrep -f '/Applications/Vega.app/Contents/MacOS/vega'` 恒返回命中——模式字符串出现在执行 pgrep 的 **shell wrapper 自己的命令行**里。

**改用**：`pgrep -x vega`（精确进程名），或 `ps aux | grep '[V]ega'`。

### 坑 2 · ZCode 客户端窗口标题就是 "Vega Desktop"

ZCode 自己的窗口标题是 `Vega Desktop`，界面上也有 `Full access` 等相同字样。**只看截图无法分辨**，点击会落到 ZCode 上。

**改用**：`CGWindowListCopyWindowInfo` 按 owner name + bundle id 定位目标窗口。

### 坑 3 · `screencapture -l <winid>` 的阴影边距会变

窗口 ID 截图会把投影拍进去，边距**随窗口阴影状态在 34～56 逻辑像素间变化**。用它做像素→坐标换算必然错位（这是本轮坐标反复失准的真正原因）。

**改用**：`screencapture -R x,y,w,h`（x/y/w/h 取自 `CGWindowListCopyWindowInfo` 的 bounds），输出严格 2× 且无边距，`px = 2 × window-rel`。

### 坑 4 · shell 往返会让终端抢回焦点

`open -a Vega` 与随后的点击若分属两次 Bash 调用，中间的终端窗口会重新成为前台。

**改用**：把「激活 → 等成为前台 → 点击 → 截图」写进**同一个进程**。`scripts/native-drive.swift` 是为此写的驱动（`NSRunningApplication` + `CGWindowListCopyWindowInfo` + `CGEventPost` + `screencapture -R`）。

注意：`NSRunningApplication.activate()` 在本机被系统拒绝（返回后仍非前台），**必须用 `open -a` 激活**，然后轮询 `NSWorkspace.shared.frontmostApplication?.bundleIdentifier`。

### 坑 5 · 用窗口内相对坐标，不要用屏幕绝对坐标

窗口位置在验收过程中会变（本轮实测 origin 从 (318,53) 变到 (346,52)）。写死屏幕坐标会在下一次窗口移动时静默失效。

**改用**：每次都从 `CGWindowListCopyWindowInfo` 现读 origin，再加窗口内相对偏移。

### 坑 6 · 点不开某个 UI 时，先查数据前提，再调坐标（实测）

 验收时我反复打不开滑块卡片，**花了十几轮在调整点击坐标**，全是白费。真实原因是数据前提不满足：`~/.config/vega/reasoning.toml` 只给 `glm-5.3-flash` 声明了档位，而当时选中的模型是 `deepseek-v4-flash` —— 按规则，**无档位的模型不渲染滑块卡片**。

**规则**：某个 UI 元素反复点不开时，**先确认它的渲染前提**（该模型的配置、该路由的状态、该 feature 开关），再去调坐标。前提不满足时，坐标怎么调都不会有反应。

配置位置速查：reasoning 档位在 `~/.config/vega/reasoning.toml`（**不在** `~/Library/Application Support/ai.vega/` 的数据库里，也不在任何 `.toml` 仓库文件里）。

## GPUI 圆角：`overflow_hidden` 不裁圆角，且半径会被钳制（实测）

两条都必须记住，各自踩过一次坑。

### 1 · `overflow_hidden` 只裁矩形，`rounded` 不影响子元素

`ContentMask`（`gpui-pre-0.3.4/src/window.rs:2081`）是 `struct { bounds: Bounds<P> }`——**没有圆角字段**。`overflow_mask`（`style.rs:638`）据此构造遮罩，所以：

**给父元素加 `.rounded(N).overflow_hidden()` 不会把子元素裁成圆角。** `rounded` 只作用于元素**自己的背景**。

需要圆角的子元素必须**自己带 `rounded`**。 的滑块填充左端方角就是这个原因——它一直指望父级轨道裁剪，而那条路在 GPUI 里不存在。

### 2 · 圆角半径按 quad 尺寸钳制

`clamp_radii_for_quad_size`（`geometry.rs:2467`）：

```rust
let max = cmp::min(size.width, size.height) / 2.;
// 每个角实际半径 = min(设定值, max)
```

**窄元素上设大半径会被静默压小。**  首轮我写"12px 宽的填充设半径 12，钳到 6 就是半圆"——错。真半圆需要半径 = 高度的一半（12），钳到 6 只能得到 6px 圆角，于是填充戳出轨道 6px。

**正确做法**：先把元素宽度撑到 `>= 2 × 目标半径`（本例撑到高度 24），再设半径。多出的宽度要确保被后绘的元素盖住。

## 规格里出现数值断言时，必须当场验算（经验）

 首轮规格里我写了"半径钳到 6.0 视觉上就是左半圆"，**没有验算**。实际是填充左界 x=6 而圆点左界 x=12，露出 6px。这个未验算的断言直接导致首轮修复无效，浪费一整轮实现 + 打包 + 验收。

**规则**：规格中凡是出现半径、坐标、宽度、差值等**数值断言**，必须在写规格时就逐行算一遍，并把算术表附在规格里。不要用"视觉上""大概"这类词替代计算。

## 改「形状/对称性」类缺陷时，先列全所有产生该形状的代码路径（经验）

 修滑块填充左端时，我没问"另一端呢"。实际上产生这个形状的有 **flat + gradient 两个分支 × 左右两端 = 4 个角**，我只改了其中一个，于是最强档渐变右端仍是方角（用户第二次复验才发现）。

**规则**：这类缺陷开工前先枚举所有代码路径与所有端/角，逐一核对，并在验收判据里**为每一个都写一条**。

## GPUI 绘制顺序：弹出层必须 `deferred`（实测）

**在带边框的卡片里挂浮层，卡片边框会画在浮层之上。** 这是 GPUI 的绘制顺序决定的，不是 z-index 问题，`occlude()` 解决不了。

`gpui-pre-0.3.4/src/style.rs:688` 的 `Style::paint` 顺序：

```rust
window.paint_quad(background);   // 1. 背景
continuation(window, cx);        // 2. 子元素（浮层在这里画）
if self.is_border_visible() {
    window.paint_quad(border);   // 3. 边框 ← 在子元素之后
}
```

**`occlude()` 只影响鼠标命中测试**，不影响绘制顺序——`div.rs:1208` 的 `Div::occlude` 只做 `interactivity().occlude_mouse()`，即设 `HitboxBehavior::BlockMouse`。/ 给每个浮层都加了 `occlude()` 并据此认为层级已处理，**这个推断是错的**（ §2 记录了错误链条）。

**正确做法**：浮层包一层 `gpui_kit::deferred(...).with_priority(2)`。语义是*"delay the painting of its child until after all of its ancestors, **while keeping its layout as part of the current element tree**"*（`elements/deferred.rs:11`）——布局不变、只把绘制挪到祖先之后，由 `window.rs:3343` 的 `paint_deferred_draws()` 在 `root_element.paint()` 之后执行。

注意两点：

- **`deferred` 不改变坐标**。`defer_draw` 会记录 `absolute_offset`（`window.rs:4105`）。 曾记录"deferred 导致卡片落在 x 1232.5"——那是它把 deferred 包在 `max_w + mx_auto` 的**外层盒子**里所致（ 已删该结构），不是 deferred 本身。
- **仓库里已有正确先例**：`branch_selector.rs:997`、`render_file_dropdown`（`render.rs:702`）、侧栏菜单。新增浮层时照抄这个模式。

**验收方法**：浮层类改动不能只看"内容对不对"。必须做一次**像素行扫描**：取浮层中部一行，确认其中没有其他层画上来的元素（ 用 `vfind`/`vscan` 定位到 `rgb(232,232,232)` = `border_subtle` 的连续 run）。

## GPUI hover：`.hover()` 不带 `.id()` 就永不重绘（实测）

**给一个匿名 div 加 `.hover(...)`，它永远不会变色。** 必须同时给它 `.id(...)`。

原因：GPUI 只在元素拥有 **element state** 时才注册 hover 翻转的重绘监听（`gpui-pre-0.3.4/src/elements/div.rs:2783`），而 element state 只在 `Element::id()` 返回 `Some` 时存在（`div.rs:1848`，id 来自 `interactivity.element_id`，即 `.id(...)`）。绘制分支（`div.rs:3391-3405`）在有 hitbox 时读 `hitbox.is_hovered(window)`，无 hitbox 时退化到 element state——匿名 div 没有 element state。

 的临时探针实测（同一份代码，只差一个 `.id()`）：

```
ANONYMOUS: base        renders=1 bg=BLUE
ANONYMOUS: hover-child renders=1 bg=BLUE   ← 永不变化
ANONYMOUS: hover-block renders=1 bg=BLUE   ← 永不变化
STATEFUL:  base        renders=1 bg=BLUE
STATEFUL:  hover-child renders=2 bg=RED    ← 生效
STATEFUL:  left        renders=3 bg=BLUE   ← 退出还原
```

**这条最阴的地方是它静默**：不报错、不 panic，只是永远不亮。若把某个原本常驻的底色改成 hover 态（ 就是），漏掉 `.id()` 会得到一个**比改动前更糟**的结果。

**加 `.id()` 不等于加交互。** id 只注册一个 `HitboxBehavior::Normal` 的 hitbox，不注册任何监听器；  约束的是**点击行为**，`_the_highlight_container_is_inert` 在加了 id 之后仍通过。

**顺带**：hover 命中父块时，指针落在带自己 hitbox 的**子元素**上也一样有效——`hit_test` 收集指针下所有 hitbox，`HitboxBehavior::Normal` 不遮挡后面的（`window.rs:1095`）。所以"整块 hover"不必给每行单独加 hover。

## 测试平台能观测绘制：用 `painted_quads()`，不是 `capture_screenshot`（实测）

旧注释说"测试平台没有 headless renderer，所以绘制结果不可观测"——**只对了一半**，别再用它当借口跳过绘制断言。

- `VisualTestContext::capture_screenshot` → `window.render_to_image()`：**确实**需要 `HeadlessRenderer`，未配置时 `bail!`（`gpui-pre-0.3.4/src/platform/test/window.rs:441`）。
- **`Window::painted_quads()`**（`window.rs:2618`）：直接读 `rendered_frame.scene.quads`，**不需要渲染器**。每个 `Quad` 带 `bounds` / `corner_radii` / `background`。

```rust
let quads = visual
    .update_window(window, |_, window, _| window.painted_quads())
    .expect("painted quads");
```

两个坑：

- **`painted_quads` 的 bounds 是缩放像素**（本机 2×），`debug_bounds` 是逻辑像素。要按 `window.scale_factor()` 换算后才能互相比较。
- **`Background` 的 `tag`/`solid` 字段是 `pub(crate)`**，外部无法 `match Background::Solid(..)`。用公开的 `Background::as_solid()` 取 `Hsla`，再比色。

 因此把"hover 才亮"从"看截图差不多"变成了可证伪的生产测试（`_a3..a6`）。

## 验证 hover 类改动不能用点击（实测）

`scripts/native-drive.swift` 只做点击。**hover 态不能靠它验证**——点标题行会进二级列表，点别处又测不到 hover。需要一个 **move-only** 驱动：激活 → `CGEvent(mouseMoved)` 移动（不要 down/up）→ 截图。

另外：hover 是**翻转**触发的，所以移动要分两步（先到块外的起点，再到目标点），一步直接落在目标上可能不触发。 的驱动先移到窗口左下角再移到目标。

## GPUI 点击外部关闭：`on_mouse_down_out` 会与 mouse-up 触发器互相打架（实测）

`on_mouse_down_out` 是"点击外部关闭"的正确原语。三条实测结论：

- **不需要 `.id()`**（与 `.hover()` 相反，见上一节）；
- **在 `gpui_kit::deferred` 包裹下照常工作**，不影响的绘制顺序修复；
- 跑在 **capture 阶段**，且只在指针位于该元素 bounds **之外**时触发。

### 坑：触发器的开关在 mouse-up，会被 out 处理器吃掉

若触发器的开关跑在 mouse-**up**（本仓库两个 composer 弹窗都是），而 out 处理器跑在 mouse-**down**，点触发器会变成「down 先关 → up 再开」，**看起来完全没反应**：

```
1. click chip        -> open=true
2. click chip again  -> open=true   ← 错，关不掉
```

**修法**：触发器改用 **`capture_any_mouse_down(|_, _, cx| cx.stop_propagation())`**，在 capture 阶段抢先声明手势。

注意 **bubble 阶段的 `on_mouse_down(stop_propagation)` 挡不住**——它比 capture 晚。这是最容易踩错的一步。

### 坑：别把 out 处理器挂到「包含触发器的外层」

那样能修好触发器，但会**破坏弹窗内部点击**：弹窗是 `absolute` 定位，落在 wrapper 的布局 bounds **之外**，内部点击会被当成「外部」。实测点弹窗内部会把弹窗关掉。**用 capture 方案，不要用外层方案。**

### 弹窗内部点击要防穿透

弹窗自己仍要保留一个 bubble 阶段的 `.on_mouse_down(stop_propagation())`，否则点弹窗内部会穿透到**祖先**的外部点击处理器（如 composer 卡片自己的）。它与 capture 阶段的 out 处理器**阶段不同、互不冲突**，两个都要有。

## 对齐参考实现时，先确认 CSS 变体作用域（经验）

参考实现的 CSS 里有大量 `[data-vega-window-type=browser]` / `=electron` 变体块。**同一组 token 在不同变体下值不同**，取错会得出完全反向的结论。

曾发生过：从 `[data-vega-window-type=browser]` 块取了 `--menu-item-height: 36` 等值，写下"行高该 32→36、内边距差一倍"；而 Codex **桌面截图**实测行高 28.5、行内边距只差 2px——**真正的缺陷是卡片宽度差 30%**。若照第一版做，会把行高改大（反向优化）而没修宽度。

**方法**：

1. 取 token 时**连 selector 一起记录**，确认它属于哪个变体；
2. **用截图交叉验证**：先找一个已知量校准截图的缩放比例（用 Vega 自身的 32px 行高：实测 64px 行距 ÷ 32 = 2×），再按比例读其它量；
3. 两者矛盾时，**以产品截图为准**，并把这个矛盾写进报告。

## 架构红线（速记，详见 exec-guide）

- `vega_runtime` 禁止依赖 GPUI/任何 UI crate（headless 可测）
- 跨 crate 共享类型只放 `vega_conversation::types`
- API key 只存配置根下独立的 owner-only 明文凭据文件，不写 config.toml/项目文件/日志；不访问旧 Keychain
- 非测试代码禁止 `unwrap()`/`expect()`
- schema 只增不删，走 `migrations/` 递增文件
- **代码中禁止任何注释**（`///`/`//!`/行内 `//`/`/* */`）：注释说明必要 = 命名或结构没写好，去改命名/拆函数/提类型。仅 `unsafe` 块的 `// SAFETY:` 例外（2026-09-23 用户裁决，详见 exec-guide §3/§4）

## 行为技能

- [vega-kanban-delivery](.agents/skills/vega-kanban-delivery/SKILL.md)：Codex/Vega 共用的角色识别与最新单卡交付；Codex 先审查并交接 Bug、Vega 按方案执行、Actions 验证、Codex 合并、用户手测后收尾。维护 skill 本身不触发其他真实队列。

- [karpathy-guidelines](.agents/skills/karpathy-guidelines/SKILL.md)：写/审/重构代码时的
  行为准则（最小改动、表面化假设、可验证成功判据）。**冲突时以本文件与 exec-guide 为准**，
  适配边界见该 SKILL.md 的「Vega 适配」一节。canonical 在 `.agents/skills/`，
  `.claude/skills/` 为软链；新增 agent 入口时按同样方式链接。
