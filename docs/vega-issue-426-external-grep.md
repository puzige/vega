# Issue #426 — grep 外部读取权限与目录范围

## 授权与范围

本轮用户明确选择「完整实现并提交 PR」，并在发现另一会话并发写入后选择「本会话继续」。本会话使用独占分支 `feat/426-external-grep-pr`、独立 worktree，从 fetch 后最新 `origin/master`（`75da678a`）建立；保留原 `feat/426-external-grep` 的全部文件和提交，不覆盖其并发工作。主会话协调、冻结规格、审查和提交 PR；代码委派专职子 agent。

Issue [#426](https://github.com/puzige/vega/issues/426) 要求 grep 能搜索明确指定的项目外文件/目录，审批与 read 一致。本规格仅替代 [#300](vega-builtin-tool-parameter-recovery.md) 和执行总纲中 grep 限项目内的约定。glob pattern/围栏、read/write/edit、shell、检查点和权限模式不扩展。无需新增依赖、权限存储枚举、数据库迁移、UI 控件或视觉改造；现有权限卡须完整接线。

本地不运行 Vega、编译/check/clippy/build/package 或测试。仅低 CPU 差异、格式和静态检查；自动回归由 GitHub Actions 执行，真实 Cargo registry/SDK 搜索由用户手测。

## 验收矩阵（先于实现计划）

| 用例 | 操作 | 可验证结果 | 层级 |
|---|---|---|---|
| A | 项目内相对/绝对文件、子目录，null/省略/空 path | 既有结果与项目相对展示保持；默认只搜项目，不跟随后代外部 symlink | tools/runtime |
| B | 外部普通文件，Full access 或批准 Once | 真实匹配、1-based 行号、canonical 绝对路径，可交 read 独立授权 | tools/runtime |
| C | 外部目录，含兄弟同名文件、二进制和忽略文件 | 仅指定子树普通文件，排序/hidden/ignore/binary/500 条及截断保留，不读取兄弟目录 | tools |
| D | Confirm/Auto/ReadOnly，包括 Ask/Plan | 外部目标需 Read 审批；Once/Always/Deny/timeout/cancel/FullAccess 状态真实，拒绝不漏正文 | runtime/conversation |
| E | exact rule、文件变目录、旧裸目录规则 | 文件复用 exact Read；目录只匹配 `grep-dir:v1:<canonical-path>`，旧裸文件/目录规则不升级为递归许可；不匹配父/兄弟/子入口，不授予写权限 | runtime/store |
| F | 入口 symlink、含 `..` 路径 | 共用 read 规范化身份；审批真实目标；执行不重新解释原始 alias；alias 改指向不重定向执行 | tools/runtime |
| G | approve hook 替换文件/目录/祖先、删除或变 symlink | prepare→approve→execute 拒绝变化目标，不返回替换对象正文；同 inode 原地更新内容允许 | tools/runtime |
| H | 遍历时后代替换为 symlink | 目录枚举、ignore 和正文全部锚定获批 fd；逐组件 no-follow，不能只换最终 opener；失败或留在获批树，不读外部 canary | tools |
| I | 凭据根、祖先、后代、symlink/硬链接别名 | 直接保护目标拒绝，FullAccess 不绕过；默认/原项目内搜索可返回安全部分，受保护路径及正文不出现在结果 | tools/conversation |
| J | 不存在、dangling link、`.git/.GIT`、FIFO/socket/设备 | 可理解错误，不阻塞，不读特殊文件；Git 控制保护保留 | tools |
| K | 无效 regex/JSON/类型 | 不泄露敏感 pattern/原文，拒绝后模型可纠正，不重复执行 | tools/runtime |
| L | 模型续轮、call-id 重用/冲突、历史恢复 | schema 仍 pattern/nullable path；live 原始参数与 canonical 审计分离，结果配对、幂等和篡改校验有效 | runtime/conversation |
| M | 现有权限卡批准、拒绝、超时、恢复 | grep 仍显示为 grep，授权类别为 Read；call-id/path/签名严格匹配，Once 可完成，坏 tool/path/signature 被拒绝；不展示 pattern 正文 | conversation/UI |
| N | glob、写入、shell 回归 | glob 外部/parent pattern 仍拒绝；grep 不记 ReadState，不满足 read-before-write；shell 根不变 | tools/既有回归 |
| O | 外部 ignore 配置与祖先/全局/git 指针 | 仅隐式读取树内安全 fd 打开的 `.gitignore`/`.ignore`；不加载树外父/global/excludes/gitdir 配置、不跟随 `.git` 指针；项目内规则不变 | tools |
| P | 项目根包含凭据目录，或等于/位于凭据根 | None/空/`.`/绝对项目根仍返回安全普通匹配；受保护根内默认搜索不能绕过；其他 read/write/edit guard 不变 | tools |

仅更新本卡明确替代的外部 grep 拒绝断言，不放宽权限、凭据、输出、历史或其他工具断言。全部自动回归为进程内 fixture、MockProvider、真实临时 Store 和权限 hook；不新增真实外部进程/网络/应用 E2E。

## 冻结契约

### 路径、显示与范围

- null/省略/空 path 仍从项目根开始，非空与 read 共用路径解析；支持显式相对外部 `..`、绝对路径和现存入口 symlink，不展开 shell `~`/环境变量。不存在、dangling link、特殊文件和 Git 控制组件拒绝。
- 项目内结果 `relative/path:line:content`；外部结果 `canonical/absolute/path:line:content`，不能相对外部目录缩短。不可表示的外部 UTF-8 路径明确拒绝，不用 lossy 字符串制造错误 locator。
- 目录授权仅该 canonical 目录的后代普通文件，不含父/兄弟，不跟随后代 symlink，不产生 prefix/glob 规则。

### 授权与对象身份

- 外部 grep 使用现有 `RuntimeMutatingTool::Read`、权限模式、决策、审计和取消流程。文件 `exact_pattern` 为 canonical 文件路径；目录为 `grep-dir:v1:<canonical-directory>`。两者 `display_target` 都只显示真实 canonical 路径，不显示内部签名。
- 目录不继承旧裸 Read 规则，防止过去文件或失败目录 Read 的 Always 扩成递归授权。目录签名只授权同一入口的 grep，不授权 child read、其他入口或 mutation。跨调用 Always 是同类 canonical 路径授权，不保存历史 inode；每次调用仍重新 prepare 并核对其对象身份。
- prepare 冻结 canonical 入口、文件/目录类型、dev/ino 和祖先链身份，可安全 open descriptor，但不得 probe binary、读取正文/ignore 或收集匹配。批准后、实际读取前复核；删除、更换 inode/类型/祖先或 symlink 重定向 fail closed。同 inode 原地内容更新不做内容锁。
- 原始 alias 在执行时不重新解析；实际读取消费 prepared 对象。目录枚举、进入、树内 ignore 与正文打开都锚定获批目录 fd，逐组件 no-follow；仅 walker.follow_links(false) 或 pathname starts_with 不足。普通文件、nonblocking 和 descriptor 硬链接检查继续有效。
- 取消覆盖审批前后、worker 起点和搜索循环；不在取消后返回成功，达到判定截断的第 501 条后停止读取更多匹配。

### 凭据与搜索原语

- 直接凭据根/后代/别名拒绝；显式外部目录与凭据根祖先/后代重叠拒绝整次搜索。默认与既有项目内目录搜索按候选过滤，不因包含受保护目录而整体失败；受保护目录不得进入，项目根本身在保护根内仍拒绝。保留 path guard、保护目录对象身份和 fd 硬链接防护，FullAccess 不绕过。其他工具 guard 不改。
- 项目内保持既有 ignore walker 的父/global/git exclude/hidden 语义，glob 的共享 walker 不改。外部目录仅加载获批树内、经 credential/hardlink/no-follow 检查的 `.gitignore`/`.ignore`；不加载树外父/global/gitdir/excludes 配置。文件名显式指定沿用原文件搜索行为。
- 保留确定性排序、二进制跳过、500 条上限及原截断标记。该上限是结果数量，不声称总时间/字节/内存有界。grep 不记 ReadState，不授予写能力。
- 错误可理解，状态区分 Failed/Rejected/Cancelled；无效 regex 不回显敏感 pattern。不得新增原始参数/内容日志。

### 审计、持久化与权限卡

- 外部 grep 审计中的 `path` 固定为 canonical 绝对路径，`pattern` 沿用既有参数语义；live 模型调用保留原始合法参数，不能经过 canonical_read_input 丢失 pattern。执行身份与 live/audit 分开；call-id 重用及目标篡改仍校验。
- conversation 仅对合法 grep 增加 Read 授权映射，继续严格校验 call-id、Read 类型、absolute path、目录签名和 display 对应关系；glob 不得接受 User/Rule/FullAccess 新来源。目录签名不能仅检查前缀。
- `ToolCardInputProjection` 为合法 absolute grep path 提供权限目标；现有 `ToolCard::permission_identity` 返回其 Read 授权类别，不全局把工具名改为 read。UI 的 tool/target 严格比较保持，不放宽通用匹配。
- 真实临时 Store 覆盖 Once/Always/Rule/拒绝/取消/FullAccess、恢复、伪造 target、目录签名及既有 readonly 历史；权限卡测试覆盖真实队列挂载与 resolve，而非仅投影辅助函数。

## 实现切片与共享接口

同一文件只有一个实现写入者；不改依赖或卡外代码。main 只写 docs/README 并审查集成。

1. **Tools 子 agent（独占 `crates/vega_tools`）**：先补矩阵 fixture，再实现 prepared grep 与安全遍历。约定接口：`Tools::prepare_grep(&self, pattern: &str, path: Option<&str>) -> Result<PreparedGrep, ToolError>`；`Tools::execute_grep(&self, prepared: PreparedGrep, cancel: &CancellationToken) -> Result<ToolOutput, ToolError>`；`PreparedGrep::path(&self) -> &Path`、`is_directory(&self) -> bool`。`Tools::grep` 保留为即时 prepare+execute 包装，lib 导出 PreparedGrep。共享 helper 只在确有需要时小范围改 text_file/fence/credentials，不改变 read。必要安全实现可拆 grep 子模块。
2. **Runtime 子 agent（独占 `crates/vega_runtime`）**：grep 单独 prepared variant、canonical audit/live raw 分离、按冻结签名复用 ExternalRead 授权、只执行 prepared 对象、取消结果；修改权威描述，schema 不变；生产 loop/approval hook 证明接线，覆盖规则/替换/拒绝/历史/幂等。不写 tools/conversation/UI。
3. **Conversation/UI 子 agent（独占 `crates/vega_conversation`、`crates/vega_ui`）**：events::target_matches_state、pipeline::approval_source_matches/恢复、types::tool_calls 权限投影和 UI permission_identity 接线；必要 helper 可放 conversation types 共享，严格限定 grep。补真实 Store 与 GPUI 进程内回归，不改视觉/控件/噪声文案。
4. 主 agent 审查所有接口与矩阵、静态检查，提交 PR。任何实现/规格缺口先报告并修订规格，不自创权限绕行。所有新代码禁止普通注释及非测试 unwrap/expect；仅紧邻 unsafe 的 SAFETY 单行例外。

## 验证与交付

本地仅 `git diff --check`、rustfmt/`cargo fmt --all -- --check`（不编译）及静态检查。GitHub Actions 使用既有 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`；required `check (fmt, clippy, test)` 对应最新 head。每五分钟查 CI，不 watch/短轮询，未执行明确标待 CI。提交 PR 后不自动关闭 Issue，不把批准/提交/CI/手测混称完成；是否合并遵守本轮「提交 PR」交付范围。

用户手测：更新至包含 PR 的云端构建，普通权限模式搜索一个 Cargo registry 文件及其目录，确认真实目标、可继续 read 的结果路径；拒绝不返回正文，null 仍只项目，FullAccess 无询问，目录 Always 不授权 child read。真实本机路径不写公开日志或截图。

回滚通过新 PR revert 本卡实现，不改用户权限/数据，不清理原并发分支或未知 worktree。

## 变更记录 / 证据

- 2026-10-18 v4：在原 v1 审查基础上保留独立安全审查确认的目录精确签名、树内安全 ignore、默认凭据兼容性，补会话/权限卡完整接线、跨 agent 接口；因并发写入改用本会话独占分支，按用户明确选择继续实现并提交 PR。代码与自动回归尚待实现/Actions，真实搜索待用户手测。
