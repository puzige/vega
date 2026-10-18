# Issue #426 — grep 外部读取权限与目录范围

## 授权、基线与范围

用户于本轮明确要求「我们直接接手」，授权当前 Vega 会话直接补齐规格、组织专职子 agent 实现并审查集成，不再等待原有 Ready/Codex 交接前提。Issue [#426](https://github.com/puzige/vega/issues/426) 要求 grep 能搜索明确指定的项目外文件/目录，权限语义与 read 一致。基线为 `75da678a`；复用与本次 fetch 后 `origin/master` 一致的干净分支 `feat/426-external-grep`，原主工作区未提交改动不参与本卡。实现回报后仍交回 Codex 负责 PR、CI 与合并，不冒称已通过云端验证。

本规格仅替代 [#300](vega-builtin-tool-parameter-recovery.md) 中 grep 不支持外部目标的限制，以及执行总纲中 grep 仍限项目内的历史约定。glob 的项目相对 pattern 接口和围栏、read/write/edit、shell、检查点、凭据保护、历史/幂等和权限模式不扩展。无需新增依赖、配置、数据库迁移或 UI 控件。

本地不运行 Vega、编译或测试；仅低 CPU 差异、格式和静态检查。自动回归由 GitHub Actions 执行，真实 Cargo registry/SDK 搜索由用户手测。

## 验收矩阵（先于实现计划）

| 用例 | 操作 | 预期与必须保留的断言 | 自动层级 |
|---|---|---|---|
| A | 搜索项目内相对/绝对文件、子目录及 null/空 path | 与既有结果一致；null/空 path 仅项目根；路径仍项目相对；默认不会遍历项目外 symlink | tools + runtime loop |
| B | 明确指定外部普通文件，Full access 或批准 Once | 返回真实匹配行、1-based 行号及 canonical 绝对路径；同一结果可交 read；不因项目根拒绝 | tools + runtime loop |
| C | 明确指定外部目录 | 仅该目录后代普通文件；不跟随后代 symlink；保留排序、ignore/hidden 过滤、二进制跳过、500 条上限与截断标记 | tools |
| D | 外部文件/目录使用 Confirm、Auto、ReadOnly，包括 Ask/Plan | 沿用 Read 审批；仅批准后执行；Deny、timeout、取消均无匹配正文泄漏，并保留真实拒绝/取消状态 | runtime permission/loop |
| E | Always 与已有 exact Read 规则 | 复用规范化目标的 Read exact rule；目录 exact rule 只复用同一目录调用，不匹配其父目录、兄弟、子路径或任意 glob；不授予写权限 | runtime permission/loop |
| F | 入口 symlink 或带 `..` 的相对外部路径 | 与 read 共用规范化身份；审批显示真实目标；执行不重新解释原别名去追随新目标 | tools + runtime |
| G | 批准等待中更换文件、目录，或将 canonical 目标/祖先换为 symlink | 执行前 fail closed，返回可理解的 target-changed/不支持目标错误；不读取替换目标。文件内容原地更新不等同目标身份变化 | tools + runtime hook |
| H | 目录遍历过程中后代符号链接/目录被替换，或外部兄弟目录含相同文件名 | 实际文件读取以获批目录 descriptor 为锚，逐组件 no-follow；不读取范围外匹配；无效/变化时失败而非重定向 | tools 进程内 fixture |
| I | 凭据根、其 symlink/硬链接别名，或目录下含受保护内容 | 直接受保护目标拒绝；普通目录遍历不返回受保护正文或路径；既有硬链接防护保留；Full access 不绕过 | tools + runtime |
| J | 不存在、dangling symlink、Git 控制路径、FIFO/socket/设备等特殊文件 | 可理解的失败；不阻塞打开 FIFO；不读特殊文件；`.git`/`.GIT` 保护保留 | tools |
| K | 无效 regex、JSON/参数类型 | 执行/审批前或原有固定错误路径拒绝，不回显敏感参数；后续模型仍可纠正，工具只执行一次 | tools + runtime loop |
| L | 模型续轮、工具定义与幂等 | schema 不改字段；描述承认外部读取审批；live 调用保持可执行参数与配对结果；stored 历史不升级成指令；相同 call-id 复用及冲突保护保留 | registry + runtime loop |
| M | glob/写入/shell 回归 | glob 仍拒绝外部/parent pattern，grep 不记录 read-before-write 版本，不增加写入能力，不改变 shell 根目录 | tools + 既有回归 |

以上均为进程内 fixture、MockProvider 和权限 hook；不新增真实外部进程、网络或应用 E2E。仅更新本次明确替代的 grep 外部拒绝断言，其余既有安全和输出断言不放宽。

## 冻结行为

### 路径与展示

- `grep({"pattern":"...","path":null})`、省略 path 和空字符串仍从当前项目根开始；不是 HOME、进程 cwd 或全盘。
- 非空 path 与 read 采用相同的绝对/项目相对路径解析，包括现存入口 symlink 解析为真实目标和 `..`。不展开 shell `~` 或环境变量。不能授权未解析的 dangling symlink。
- 文件或目录必须存在；入口必须为普通文件或目录，不能打开特殊文件取内容。Git 控制组件保持拒绝。
- 项目内结果保留 `relative/path:line:content`；项目外结果为 `canonical/absolute/path:line:content`，行号仍从 1 起。外部目录结果不相对该目录缩短到失去定位信息。
- 外部目录授权范围为该 canonical 目录之下的普通文件，不含父/兄弟、不跟随后代 symlink。不支持 glob 风格扩展授权。

### 权限、冻结与执行

- grep 为只读搜索；外部目标走现有 `RuntimeMutatingTool::Read` 与 `RuntimePermissionTarget`，不新增存储权限枚举。Full access、exact rule、Once、Always、拒绝、timeout、取消和审批审计沿用 ExternalRead 决策顺序。
- Always 保存 canonical 入口的 exact Read rule。批准一个目录不增加 path-prefix 规则，也不允许对另一个入口自动跳过审批；该目录调用本身可搜索获批子树。
- prepare 解析并冻结 canonical 入口和其实际对象身份，执行只消费此 prepared target。文件的 descriptor/metadata 身份、目录的 descriptor/metadata 身份须在批准后、读取前复核；不能仅比较路径字符串或把普通 Readonly 分支当授权。
- 原始入口别名不在执行时重新解析。canonical 目标或其祖先被重定向、更换 inode/类型、删除时失败；可固定使用已打开但仍经核对的 descriptor。相同文件 inode 内原地内容更新按读取语义读取，不为 grep 新增内容版本锁。
- 目录下实际文件的打开必须以获批目录 descriptor 为锚，逐组件 `O_NOFOLLOW`，并保留普通文件、nonblocking、硬链接与凭据检查。只做 pathname starts_with 和 walker.follow_links(false) 不足以代替 descriptor 范围保证。
- 取消检查覆盖审批前后及执行起点；搜索循环使用已有 CancellationToken 路径适时退出，不产生取消后成功结果。输出上限达成后停止读取更多匹配。
- 无审批或拒绝时不得读取文件正文。prepare 可解析、stat/open descriptor 以冻结身份，不执行匹配、二进制探测或结果正文采集。

### 凭据、忽略规则与错误

- 凭据根及受保护别名禁止直接搜索；遍历普通目录时过滤受保护目标，保留已有祖先/别名检查和 descriptor 硬链接拒绝。默认项目搜索不能因新增入口逻辑丢失现有过滤。
- 保留 ignore crate 既有 `.gitignore`/`.ignore`/git exclude/hidden 规则、确定性排序、二进制检测、500 条输出上限及截断标记。grep 不构成 read-before-write 所需的完整文件读取。
- 输入错误、目标变化和不支持目标返回可操作错误，不把拒绝包装为成功空结果，不输出凭据正文。运行失败与审批拒绝/取消状态保持区分；不新增原始参数/内容日志。

## 实现顺序与允许修改范围

1. `crates/vega_tools/src/grep.rs`：引入私有字段的 prepared grep 目标（与现有 PreparedWrite/Edit 方式一致），统一 prepare/execute；既有 `Tools::grep` 可作为即时 prepare+execute 的只读入口。使用共享路径解析与 credential guard；目录锚定读取可在 `text_file.rs` 小范围抽取 helper，不改变 read 行为。
2. `crates/vega_tools/src/lib.rs` 导出必要 prepared 类型；`tools.rs`/`text_file.rs`/`fence.rs`/`credentials.rs` 仅在共享安全 helper 确有需要时修改。移除本次行为已明确失效的局部注释，不新增代码注释；唯一例外为紧邻 unsafe 的 `// SAFETY:`。
3. `crates/vega_runtime/src/agent/tools_exec.rs`：grep 单独 prepare；对 frozen 目标判定内外；外部复用 Read 审批；所有 grep 执行消费 prepared 对象并复核；保留原始模型参数、审计身份、取消、幂等与 terminal result。glob 留在既有围栏分支。
4. 同文件权威 grep 描述与必要运行说明更新，字段仍为 `pattern`/nullable `path`，null 默认项目根。不得修改其他工具接口或添加噪声 UI 文案。
5. 在 grep/text_file 单测和 `agent/tests/loop_tools.rs`、既有权限 fixture/registry 测试补齐矩阵。用进程内权限 hook 在 approve 时替换目标，证明生产 prepare→authorize→execute 接线，不只测辅助函数。
6. README 加规格入口；#300 和 exec-guide 补充本卡 supersession 指向。主 agent 审查差异与静态检查后提交实现并交回同卡；Codex 接回后开 PR/运行 CI，Issue 在用户手测前保持开放。

实现必须保持小而连贯，不新增与本卡无关的抽象或依赖。若既有架构无法安全实现 descriptor 范围或无法保留审批/历史契约，停止实现并报告具体缺口，由主 agent 修订本规格后再继续。

## 验证、手测与回滚

本地：`git diff --check`、仅受影响 Rust 文件的 rustfmt 或 `cargo fmt --all -- --check`（不编译）、静态核对无新增非测试 unwrap/expect、普通注释、依赖或权限绕行。

GitHub Actions：现有 PR check 执行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo nextest run --workspace`；required 汇总 `check (fmt, clippy, test)` 必须对应最新提交。每五分钟查询一次，不 watch/短轮询；未执行结果明确标待 CI。

用户手测：更新至包含本 PR 的云端构建后，在普通审批模式对一个 Cargo registry 文件及其目录分别 grep，确认审批真实目标、匹配行可继续 read；拒绝一次不返回正文；null 搜索仍仅项目；Full access 同目标无需询问。真实依赖路径不写入公开日志/截图。

回滚：通过新 PR revert 本卡 squash/实现提交恢复原 grep 项目围栏；不迁移/删除用户权限或数据。本卡不改写入与 shell 策略。

## 变更记录与证据状态

- 2026-10-18：v1，按用户本轮交付授权冻结范围、用例、目录授权、路径展示及目标冻结；代码与自动回归尚待实现/Actions，真实依赖搜索待用户手测。
