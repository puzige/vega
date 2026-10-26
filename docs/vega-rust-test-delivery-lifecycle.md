# 验证与快速交付政策

## 当前用户裁决与范围

**取消强制 TDD，但 Rust 测试仍不得进入 master。** 2026-10-23 用户明确纠正 [PR #569](https://github.com/puzige/vega/pull/569) 将取消 TDD 扩大成允许保留测试的表述；开发验证顺序和最终交付树是两个独立约束。适用于主 Agent、子 Agent、Feature 与 Bug，优先于历史规格、技能和交接。文件名为兼容既有引用保留。

- 明确 spec 验收与风险后，可直接实现/修复，或先实现后补必要测试；按影响范围、安全边界、回归风险及已有覆盖选择验证。纯文档/格式只做必要静态检查，不为流程强造测试。
- 不强制测试先行、独立测试前置审查、有效 RED、测试冻结或 RED/GREEN 时序证据链。必要 Rust 测试可在开发分支/draft PR 中辅助验证并留痕；是否写测试按风险决定，不能据此决定将测试留在 master。
- **合并前清理**：删除本轮 Rust 单元/集成/GPUI/属性测试、可执行 doctest 及仅供测试使用的模块、fixture/mock/helper、feature/dev dependency/接线；不以 `#[cfg(test)]`、ignore、改名或挪目录规避。不从旧 ref 批量恢复历史测试，不把可执行测试复制到最终树的其他位置。不误删生产逻辑/安全断言、benchmark、现存 Python 发布/CI 测试及 fixture、打包或通用 CI 能力。
- **开发留痕而非带入 master**：在 PR/交付记录固定可访问的原始测试源码/patch 链接、完整提交 SHA、内容标识与实际云端命令/run/job/结果，清理或 squash 后仍可恢复；不只依赖本机临时文件、会过期的 artifact 或未核实可达性的 SHA。测试内容留在开发提交历史/PR 附件，不进入最终 master 文件树。
- **最终无 Rust 测试**：静态清点最终 head 无 Rust 测试及独占支持，并由现有完整 required CI 确认 Rust 0 发现/0 执行，成功后才合并。开发阶段的实际功能验证与删测 head 的空集门禁分别记录，空集不能冒充测试验证通过；清理不消解未解决的产品、测试或 CI 失败。
- 测试需要时验证稳定边界的行为、业务/安全不变量，mock 不替代被测决策。不按实现输出反推预期、放宽断言、删掉失败/未执行测试后伪造通过、ignore 或盲目重试换绿。测试/观测错误修责任层，验收变化先明确并更新 spec。

## 交付与结果记录

- 既有 spec 和非实现者独立审查要求不变，审查范围与结束条件按 [AGENTS 独立审查的范围与结束条件](../AGENTS.md#独立审查的范围与结束条件) 执行；实现默认由主 Agent 直接完成，按 [按需委派政策](../AGENTS.md#按需委派) 判断；不额外设置 TDD 测试审查站。同卡无验收缺口连续推进，真实产品/权限/技术阻碍按 [执行指南 §6](vega-exec-guide.md#6-遇阻上报协议所有-agent-必须遵守) 上报。
- **本地不运行 Vega 代码、不编译/构建/打包/测试**，包括 cargo check、Clippy、定向 test/nextest；只做低 CPU、快速静态检查。自动测试仅由现有 Actions 运行，打包仅云端发布；合并前待合并最新 head 必须通过 required check `check (fmt, clippy, test)`，不更改 CI 或绕过分支保护。
- 交接/PR 记录实际改动、精确版本、已执行静态检查、实际云端 run/job/结果、未决项与风险；使用开发测试时附可恢复的测试来源和最终清理清点。未运行写待 CI，未知写未取得；不要求证明先红后绿或测试冻结。计数区分发现/执行/通过/失败/忽略/跳过；空集 `no Rust tests`（0 发现/0 执行）不是 Rust 测试通过，真实编译/发现/执行失败仍阻断。
- CI 查询仍每 5 分钟一次，完成/失败通知或用户即时请求除外。合并后置 In review、Issue 保持开放，不以 CI/合并冒充用户手测通过；用户明确通过后回写、安全收尾、关闭 Issue，最后 Done。手测失败退回 In progress，按反馈直接修复并重新验证，不重开强制 TDD 周期。
- 分支和 worktree 清理遵守 [安全清理规则](vega-issue-workflow.md#分支与-worktree-清理规则)，保留实际变更、开发测试和门禁的可恢复证据，不删除用户数据。无测试任务不制造旧 TDD 生命周期材料；有开发 Rust 测试时，留痕与最终清理不可省略。

## 历史记录边界

以下保留当时 Rust 测试清理的范围、验收和证据原文，仅描述当次操作；历史计数不替代当前 head 的验证。后续按上文执行“开发可测、合并必清”，不恢复更早规格或合并纠偏记录中的强制 TDD 流程。

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
