# Rust 测试仅在开发阶段保留

## 用户裁决与范围

用户在 #476 合并后追加规则：生产实现过程中可以保留测试，但最终交付代码不包含测试；测试及其验证过程留在 commit 和 PR 中，作为以后找回原始测试的可信来源。用户进一步确认：

- **仅适用于 Rust**；Python 发布、组件准备、delta 测试及其 fixture 继续保留。
- **本次一并删除现存 Rust 测试及独占支持**，不是只改规则；初始为 115 个，期间最新 master 的 #480 新增 8 个，最终集成范围合计 **123** 个。
- 允许先用 **draft PR** 触发现有云端 RED/GREEN；删测并验证最终 head 后，再转为正式 PR。不增加本地执行许可或新 CI 触发设施。

本轮初始基线为合并后的 `1f70e72d4074c96846119398e52697aaab894d54`，提交前远端新增 #480，最终集成基线更新为 `b3c2ffafb9ae39e32654c6692b2f35da3955d76a`。保留其模型选择器几何稳定、空会话隐藏用量及键盘路径修复，删除新增 8 个测试及独占观察接线。该规则取代此前“新增有效 Rust 测试应永久保留”的交付要求，但不放宽先测后修、测试质量、安全边界或禁止事后补测的约束。旧的清零与合并纠偏报告保留为历史记录，不改写原结论。

## 固定交付顺序

当前协作规则统一使用 Agent，不按厂商、宿主或模型分工；主 Agent 可分析/spec/组织实现/PR/CI/合并/收尾，代码实现须委派专职子 Agent，独立审查由非该实现写入者的 Agent 承担。同一实现范围保持单一写入者，交接后前任停止写入；无验收缺口时同卡连续推进，不等待旧角色交接。真实产品裁决/权限/技术阻碍按 [执行指南 §6](vega-exec-guide.md#6-遇阻上报协议所有-agent-必须遵守) 上报，不绕过授权或门禁；历史分工不再适用，下文“本次”清理与验证记录仍保留原文。

**验收 → 生产未改先写齐测试 → 独立审查、draft PR 的现有 Actions 有效 RED → 冻结 → 最小实现 → 同一测试 GREEN → PR 固定原测试源码/完整 SHA/patch 证据 → 独立删测提交 → 最新 head 空集完整门禁 → 正式 PR → 最新 required check 绿后 squash merge → In review 等用户手测。**

1. 在生产实现尚未修改时写齐本轮必要的有效测试，提交并保存生产基线、测试目标、数据/断言和内容标识。测试命名、质量与外部依赖隔离继续遵守执行指南 §7.2。
2. 主 Agent 组织独立审查测试补丁后建立/复用 draft PR（base `master`），仅通过现有 Actions 确认目标行为缺失导致的有效 RED。无关编译、依赖、网络或 runner 失败不算 RED；最小接口缺失须单独标明行为断言尚未执行。没有有效 RED 不进入实现。
3. 冻结目标和预期后写最小实现，开发期间保留测试及必要 fixture/mock/测试接线，验证**同一测试** GREEN。实现阶段及完成后不补测、不按实现输出反推断言、不为报告/覆盖率追加用例、不改预期、不 ignore、不自动重试换绿；新需求、遗漏边界或新 Bug 先停止本轮、记录独立验收变化与基线，再开完整周期。测试自身错误/接线修正须先留证独立审查且不改变验收含义，否则重开。
4. GREEN 经独立审查核实后，先在 PR 留存带测试提交的完整 SHA、可访问原始测试源码/patch 和云端 run/job 链接及准确计数，再用独立清理提交删除 Rust 测试及其独占支持。不得先删失败、未执行或被忽略的测试再拿空集门禁冒充 GREEN。
5. 清理范围包括 workspace 与 `xtask` 的 Rust 单元、集成、GPUI、属性测试、可执行 doctest，及无生产/benchmark 消费者的 fixture、mock、helper、测试开关、模块和 dev dependency/feature。生产自检、业务安全检查/assertion、真实控件 ID、无障碍、焦点/事件/滚动/绘制行为、业务数据、迁移、benchmark 和发布能力不得误删。
6. 独立审查删测前后生产差异，静态清点最终 Rust 测试/独占支持无残留；对删测后的最新 head 运行完整现有 PR gate。GREEN 后只允许测试/独占接线清理，不夹带新生产行为；同步基线或冲突解决改变被验证实现时，先从证据恢复同一冻结测试并重新云端 GREEN，再删测。验收变化须重开测试先行周期，不能沿用旧 GREEN。最终 Rust 应为 **0 发现/0 执行、`no Rust tests`**，不是 Rust 测试通过。Python、fmt、全目标 Clippy 和空集发现/构建仍须成功，任何失败都阻断正式交付。
7. 只有带测试 GREEN 的证据与最终空集 head 门禁均可核实，才把 draft 转正式 PR/申请合并。纯文档/格式维护不强造测试；纯重构需要用来验证契约的测试也只在开发阶段保留到 GREEN，最终同样删除，不从旧分支永久恢复测试集合。
8. 本地仅低 CPU、快速的格式/差异/静态检查，不运行 Vega、不编译/构建/打包/测试；云端 CI 每 5 分钟查询，不用短轮询/watch，完成/失败通知或用户即时请求除外。主 Agent 确认独立审查无缺口及待合并最新 head 的 required check `check (fmt, clippy, test)` 通过后主动 squash merge master，再置 In review 并读回，Issue 保持开放且不 Done，停等用户手测；PR 不用自动关闭 Issue 的关键字。用户明确通过后回写、完成剩余安全清理、关闭 Issue 并读回，最后 Done 并读回；清理/关闭受阻保留 In review、标待收尾。手测失败由主 Agent 分析并组织新一轮先测后修，不等旧角色。

## 可信来源与生命周期

PR 是测试的检索入口，至少记录以下内容，不只写“CI 绿了”：

- 生产未改的基线、测试先行提交的完整 SHA 与可访问源码/补丁；冻结测试目标、内容标识和预期。
- 有效 RED 的 run/job、命令、原始失败原因，以及后续实现顺序。
- 带相同测试的 GREEN 提交、run/job 和发现/执行/通过/失败/忽略/跳过数量；接口缺失、环境失败或缺失的原始证据如实标注。
- 独立删测提交、删测清单、生产差异审查，以及最终 head 的空集清点与完整门禁。

可以保留开发提交，无须把测试源码留在最终树中。squash/清理分支前，在 PR 固定可访问的提交源码或附原始补丁，保证不依赖本机 `/tmp`、已删除分支或会过期的 Actions artifact；验证链接能读回。不得 rebase/force-push 丢弃唯一测试来源，或靠重排历史、补测来追认 TDD。提交/源码能证明测试内容来自哪里，不能单独证明有效 RED/GREEN 或测试先行。

合并后的现行资源规则以 [分支与 worktree 清理规则](vega-issue-workflow.md#分支与-worktree-清理规则)为准：最新 head 的 required check 通过并合并后，允许自动删除已合并远端任务分支，无须等用户手测；证据、完整 SHA 和原始 patch 必须仍可恢复。本地分支/worktree 仅确认无运行 agent、无未提交或未合入改动后安全清理，不授权删除用户数据。Issue 仍等用户明确手测通过后才关闭/Done；下文“本次”范围和验证记录保留为历史原文。

## 本次现存测试清理

### 删除验收

1. 逐项识别并移除最终基线的 **123** 个 Rust 测试定义：vega 10、conversation 33、runtime 21、store 1、tools 29、UI 29（原 21 + #480 新增 8）；其余成员及 `xtask` 为 0。按语法/条件编译边界清理混合文件，不按文件名或源码字符串整段误删。
2. 删除上述集合独占支持和测试依赖；只移除无生产/benchmark 消费者的内容，不升级依赖、不恢复历史测试、不回滚输入预测或外部 grep 功能。
3. 保持 Python 脚本与 fixture、workflows、Nextest 配置、迁移、资源、benchmark、发布/打包契约不变。通用测试 runner 和 CI 空集处理能力保留，开发 draft 仍能验证新测试。
4. 记录删除数量、模块/include 引用、生产差异与受保护文件哈希；本地只做低 CPU 差异/格式/静态检查，不编译、不运行 Vega/Clippy/测试/打包。完整检查仅交 Actions。
5. 本次为用户授权的**无生产行为变化的清理**，不新增测试或伪造 RED，不把已有功能追认为 TDD。本次仅提交新的 PR；不自行合并或清理用户未验收的任务资源。

### 原始测试与已有 GREEN

- 来源：[PR #476](https://github.com/puzige/vega/pull/476)，带测试 head `e33c1318ec5291d209111ab5efffed6b0df9c1ad`，合并结果 `1f70e72d4074c96846119398e52697aaab894d54`。
- [带测试 GREEN：Actions 39152010457](https://github.com/puzige/vega/actions/runs/39152010457)：115 个发现/执行，115 通过、0 跳过，14 个 binaries；Python 5+15+14=34 个执行通过；fmt、全目标 Clippy 与 required check 成功。
- #476 的功能原测试缺少有效先行 RED 的事实保持不变。本次用上述链接定位原始测试与既有执行证据，**不声称原功能满足测试先行**。首轮接线失败也不算有效 RED，详见[合并纠偏记录](vega-rust-reset-reconciliation.md)。

以下固定到已合并提交 `1f70e72d` 的原始源码，测试所在文件合计 115 个定义；该提交与带测试 GREEN 的 `e33c1318` 全树一致，不依赖临时分支或 artifact：

| 原始测试源码 | 定义数 |
|---|---:|
| [应用权限边界](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega/src/app_agent.rs) | 1 |
| [应用预测](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega/src/tests/composer_predictions.rs) | 9 |
| [会话外部 grep](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_conversation/src/agent/tests/external_grep.rs) | 16 |
| [预测控制器](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_conversation/src/composer_predictions.rs) | 13 |
| [外部 grep 投影](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_conversation/src/types/tool_calls/external_grep_tests.rs) | 4 |
| [运行时外部 grep](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_runtime/src/agent/tests/external_grep.rs) | 21 |
| [预测配置](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_store/src/config.rs) | 1 |
| [grep 显式路径](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_tools/src/grep.rs) | 1 |
| [外部 grep 工具](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_tools/src/grep/tests.rs) | 28 |
| [预测界面](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_ui/src/conversation_stream/tests/composer_predictions.rs) | 12 |
| [外部 grep 界面](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_ui/src/conversation_stream/tests/issue426_external_grep.rs) | 6 |
| [预测设置界面](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_ui/src/settings/tests.rs) | 2 |
| [预测输入](https://github.com/puzige/vega/blob/1f70e72d4074c96846119398e52697aaab894d54/crates/vega_ui/src/text_input/tests.rs) | 1 |

### 期间新增的模型选择器测试

- 来源：[PR #480](https://github.com/puzige/vega/pull/480)，8 个新测试；原测试提交 `9db5eb30f68b9b342441d73edb803472900bab48`，有效 RED [Actions 39151902385](https://github.com/puzige/vega/actions/runs/39151902385)，后续实现 `9637111c85f216b365e8b1990d192d3c59537cc8`，最新基线集成及 GREEN `fc24491ab16b9f111a072f5dacabd2c2db13d8c4`。
- [带测试 GREEN：Actions 39154822274](https://github.com/puzige/vega/actions/runs/39154822274)：123 个发现/执行，123 通过、0 跳过。该 head 与合并结果 `b3c2ffaf` 全树一致；这批新增 8 个的先行证据与原 115 个的证据缺口分别记录，不混为同一 TDD 结论。
- 固定原始[模型选择器测试源码](https://github.com/puzige/vega/blob/b3c2ffafb9ae39e32654c6692b2f35da3955d76a/crates/vega_ui/src/conversation_stream/tests/composer_picker.rs)，SHA-256 `604c9b91ece02aba76a9d58a84faf417d80e44d761267469eec6af97ed18c6b5`。生产三文件只清理测试专用 debug selector，不撤销真实布局、内容条件、控件 ID/无障碍/焦点/事件行为。原 [#480 记录](467/vega-issue-467-composer-picker-stability.md)保持不变。

## 本轮验证记录

### 本地静态结果

- 最终集成独立清点 **123 → 0**（初始 115 + #480 新增 8）；保留 368 个 Rust 文件，测试条件属性为 **0**，无可执行 doctest（原有 `text` 围栏不是测试）。旧版语法库仍有 60 个文件存在基线语法覆盖限制，不把静态清点称为类型/借用/运行验证。
- 初始实现审查的 82 个受保护路径逐字节一致；主会话最终对最新 `b3c2ffaf` 的 63 个核心受保护路径复核未变，涵盖 Python、workflows、Nextest 配置、迁移、资源和 benchmark。关键预测/grep 生产文件字节一致；混合文件生产差异按条件编译投影及明确列出的测试专用查询/常量分支简化审查。
- #480 的 `render.rs` 18 个、`thinking_slider.rs` 2 个 debug selector 被清除，除这 20 个测试观察接线外，生产 token 与上游一致；`composer_actions.rs` 与上游逐字节一致。真实 ID、无障碍、布局测量/宽度和内容条件、键盘/焦点/事件行为保留。
- 锁文件由 924 个包减为 914 个，仅移除 GPUI 测试支持独占的 10 个包；保留包的版本、来源和 checksum 不变，无包新增/升级。解析依赖身份后无新增边，只有 8 条现有包的测试支持边移除；消除同名多版本后的锁文件标签缩写不算新增依赖。
- `cargo fmt --all -- --check`、`cargo metadata --no-deps --offline --locked --format-version 1`、`git diff --check` 均退出码 0。3 个 workflow YAML、2 个 skill frontmatter 和新增本地链接目标静态检查有效；14 个原测试源码链接（原 13 + 新增 1）与 GitHub 已合并提交文件树的 blob 身份一致。
- 本地未编译、运行 Vega、Clippy、Rust/Python 测试、benchmark 或打包。

### 最终 head 云端结果

待本轮 draft PR 触发现有 Actions，核实 fmt、全目标 Clippy、Python 与 Rust **0 发现/0 执行**、`no Rust tests` 及 required check；通过后才转正式 PR。尚未取得的结果不标通过，不复用 #476 带测试 GREEN 的发现/执行计数。
