# Issue #408 实现交付记录

契约见 [工具执行与工具列表对齐](vega-tool-block-codex-parity.md) 的「#408 修订：活动短语句首大写」。本记录在代码实现前建立，运行状态只记录实际证据。

## 根因与调用链

`#408` 报告的界面位置是会话运行活动的阶段标题与工具组聚合行，二者都把 `ToolActivityCategory::action_phrase()` 直接拼进句子：

- 阶段标题：`crates/vega_ui/src/conversation_stream/run_activity.rs` 的 `format_stage_tool_categories`，首项用 `action.to_owned()`。
- 工具组聚合：`crates/vega_ui/src/tool_activity_group.rs` 的 `aggregate_summary`，把 `action_phrase()` 用 `", "` 连接后加 `Completed:` / `In progress:` / `Rejected calls:` / `Cancelled calls:` / `Failed calls:` 前缀。
- 单卡活动标签镜像：`run_activity.rs` 的 `RunActivityGroup::activity_presentation`（`#[cfg(test)]`，`test_activity_label` / `test_activity_presentation` 走它）拼接同一 `In progress: {actions}` 文案。

回归由 `#389`（commit `cd79141a`，2026-10-15）引入：该卡把 `action_phrase()` 改成小写英文短语，并删除此前「首位短语大写」的处理，之后没有任何展示层再做句首大写，于是 `run commands, read files` 与 `Completed: run commands` 以句首小写出现在产品界面。参考实现同一位置使用 `.leading` 句首形式（`Read files`），非首位使用小写形式（`read files`），两套并存；Vega 只有一套小写短语且丢失了句首大写。

单卡摘要路径（`generic_verb`、`build_bash_summary`）本来就输出 `Read files` / `Ran echo hello`，不受影响，本卡不改。

## 验收矩阵与实现计划

| ID | 风险与前置状态 | 操作 | 可观察结果 | 证据层级 | 状态 |
|---|---|---|---|---|---|
| C1 | 阶段只含一个工具类别 | 观察阶段标题 | 标题为 `Run commands`、`Read files` 等句首大写形式，词数与非首位形式一致 | 生产 `format_stage_tool_categories` 单元断言 | 已实现 / 待 CI |
| C2 | 阶段含多个工具类别 | 观察阶段标题 | 首项句首大写，后续项保持小写：`Read files, run commands` | 既有 GPUI 阶段标题回归（`issue296_expansion`、`issue296_codex_stream`、`issue403_conversation_underlay`） | 已实现 / 待 CI |
| C3 | 工具组聚合的每种终态 | 观察聚合行 | `Completed: Run commands`、`In progress: Run commands, read files`、`Rejected calls:` / `Cancelled calls:` / `Failed calls:` 后首项同样句首大写，后续项小写 | 既有聚合回归（`issue70_tool_activity`、`issue296_expansion`） | 已实现 / 待 CI |
| C4 | 单卡活动标签（`#[cfg(test)]` 镜像） | 触发并行运行态 | 标签 `In progress: Run commands, read files`、`In progress: Read files`、`In progress: Call MCP · lookup, call MCP · query and more (3 total)`，首位大写、后续小写 | 既有 `issue296_single_activity` 回归 | 已实现 / 待 CI |
| C5 | 首位短语超长触发 96 字符省略 | 观察阶段标题 | 仍以句首大写开头、仍以 `, …` 结尾、长度不超过 96 | `run_activity.rs` 的 `category_title_tests` | 已实现 / 待 CI |
| C6 | 大小写以外的字符被改动 | 逐类别检查 | 只大写第一个字符，其余字符逐字保留，字符数不变；空串保持空串 | `tool_card.rs` 的 `issue408_sentence_initial_capitalizes_only_the_first_character` | 已实现 / 待 CI |
| C7 | 隐私、图标、计数与既有权重 | 观察同批断言 | `Not run after takeover` 分支、图标选择、类别去重、计数后缀、脱敏断言全部不变 | 既有 `issue70_tool_activity`、`issue296_single_activity`、`tool_activity_group` 断言 | 已实现 / 待 CI |

实现顺序：先在 `tool_card.rs` 增加只大写首字符的共享 helper 与单元覆盖，再让阶段标题与聚合摘要的首项走它，最后同步 `#[cfg(test)]` 镜像并核对既有断言的新期望。非目标：不改 `action_phrase()` 的短语本身、不改单卡摘要、不改命令标题/详情/权限卡/模型与用户内容/命令文本/工具输出、不把原形改为过去式、不新增依赖或产品文案。

## 实现

- `crates/vega_ui/src/tool_card.rs`：新增 `pub(crate) fn sentence_initial(phrase: &str) -> String`，取第一个字符做 `to_uppercase()`，其余按原样拼接；空串返回空串。`action_phrase()` 的 11 个小写短语保持原值。
- `crates/vega_ui/src/conversation_stream/run_activity.rs`：`format_stage_tool_categories` 首项改为 `sentence_initial(action)`，省略分支改为 `sentence_initial(action).chars().take(95)`；`#[cfg(test)]` 的 `activity_presentation` 首项按同一规则处理，其余分支（Thinking / 摘要标题 / 等待确认 / 准备调用 / 终态 `label()`）不变。
- `crates/vega_ui/src/tool_activity_group.rs`：`aggregate_summary` 按索引对首项应用 `sentence_initial`，后续项保持 `action_phrase()` 原文；`Not run after takeover` 分支不变。

未偏离规格；没有新增依赖、配置、产品文案或代码注释。

## 生产回归（未在本地执行）

新增覆盖：

| 测试 | 可证伪的生产风险 |
|---|---|
| `tool_card::tests::issue408_sentence_initial_capitalizes_only_the_first_character` | 只大写首字符；逐类别验证输出等于「首字符大写 + 其余原样」、字符数不变、首字符为大写字母；`call MCP` / `use Skill` / `Codex ACP` 等含大写词或空串的边界不被破坏 |
| `run_activity::category_title_tests::stage_tool_category_title_elides_many_kinds_without_counts` | 多类别标题首项句首大写；96 字符省略分支以句首大写开头且仍以 `, …` 结尾 |

既有断言按新契约更新期望值（只改首项大小写，其余条件保留）：

以 `git diff origin/master -- <file>` 新增行计数：

- `conversation_stream/tests/issue296_expansion.rs`：29 处阶段标题期望与 2 处聚合期望（`Completed: Run commands`、`In progress: Run commands`），其中 8 处位于 #421 新增的阶段标题用例内。
- `conversation_stream/tests/issue296_codex_stream.rs`：2 处阶段标题期望。
- `conversation_stream/tests/issue296_single_activity.rs`：3 处 `In progress:` 标签期望与 1 处 MCP 标签期望。
- `conversation_stream/tests/issue70_tool_activity.rs`：4 处聚合期望。
- `conversation_stream/tests/issue403_conversation_underlay.rs`：1 处运行中阶段标题期望。
- `conversation_stream/tests/thinking.rs`：本卡无需改动；该文件在 #421 后已改为摘要标题期望，冲突按 master 语义解决。

隐私断言（`sensitive-value`、`private-term`、`01ARZ`、`query: string`、摘要哈希）、图标断言、类别去重与计数后缀断言均未改动。用户禁止本地编译 / 测试，因此没有本地红 / 绿运行记录；上述测试由云端 `pr-check` 的 Nextest job 执行。

## 基线漂移与集成

首轮 PR 期间 `origin/master` 前进两个提交：#421 `8c69ef64`（新摘要暂时取代工具阶段标题）与 #424 `692e09e5`（统一工具契约）。本卡分支已 rebase 到 `692e09e5`，冲突与结果：

- `tests/thinking.rs` 与 `tests/issue296_expansion.rs` 各 1 处旧断言在 #421 后已改为摘要标题（`Writing the fix`、`Reviewing results`、`Updated summary`），按 master 语义保留摘要期望；本卡在该位置不再需要大小写修改。
- `issue296_expansion.rs` 中 #421 新增的阶段标题期望（`run commands`、`read files, run commands` 等 8 处）按本卡规则改为句首大写。
- `tool_card.rs` 与 `tests/permissions_cards.rs` 的 #424 改动自动合并，未与本卡 helper 或断言冲突。
- 冲突解决后重新扫描：`crates/vega_ui/src` 内已无小写句首的类别短语断言，`sentence_initial` 三处调用点与 helper 单元断言完整保留。

## 本地快速检查证据

检查时间：2026-10-16 10:39 UTC / 18:39 Asia/Shanghai。分支 `fix/408-activity-label-sentence-case`，基线已 rebase 到 `origin/master` `692e09e5`（`feat(A3-07): unify tool contracts and preserve execution context (#424)`）。本卡变更的 8 个 `crates/**` 代码文件按排序后的相对路径、NUL、文件内容、NUL 逐个拼接的 SHA-256：`7909b8f1787c2dfb05a5f7711773620fe9a10a6a75348fac132a027a1cd041de`。

| 命令 / 检查 | 退出码 | 原始输出 / 事实 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | stdout / stderr 为空 |
| `git diff --check` | 0 | stdout / stderr 为空 |
| 本卡新增行的注释静态扫描 | 0 | `Added code comments: 0` |
| `rg -n '"[a-z][^"]*(run commands\|read files\|find files\|search content\|write files\|edit files\|call MCP\|use tools\|use Skill\|use computer or browser)' crates -g '*.rs'` | 1 | 无匹配：不存在以小写开头的类别短语字面量断言 |
| `rg -n '^<<<<<<<\|^=======$\|^>>>>>>>' crates docs` | 1 | 无匹配：冲突标记已全部清除 |
| `rg -n 'sentence_initial' crates/vega_ui/src` | 0 | helper 定义 1 处、调用 3 处（`format_stage_tool_categories` 2 处、`activity_presentation` 1 处）、聚合摘要 1 处、单元断言 1 处 |
| 本地编译 / Clippy / test / nextest / 打包 | 不适用 | **未运行**，遵守 Vega 本地验证限制 |

## 云端检查

### rebase 后最终 head（当前有效）

`24e46ef3` 的 `pr-check` [run 38888865028](https://github.com/puzige/vega/actions/runs/38888865028) 全部通过：

| Job | 结果 | 耗时 |
|---|---|---|
| Clippy（`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`） | pass | 2m27s |
| Nextest（`cargo nextest run --workspace`） | pass | 9m55s |
| required check `check (fmt, clippy, test)` | pass | 3s |

Nextest 原始汇总行：

```text
Summary [ 170.495s] 2778 tests run: 2778 passed, 5 skipped
```

### 首轮（已被 rebase 取代，保留历史）

首轮 head `ba76ee4c`（基线 `e2568518`）的 `pr-check` [run 38865605273](https://github.com/puzige/vega/actions/runs/38865605273) 通过：Clippy pass 5m18s，Nextest pass 9m54s（`2741 tests run: 2741 passed, 5 skipped`），required check pass 4s。该 head 在 #421/#424 合入后已不可直接合并，通过记录不适用于当前 head。

## 待用户手测

合并后阶段标题与工具组聚合行应以句首大写开头、后续短语保持小写，单类别阶段标题同样句首大写；长标题省略行为、图标、计数与隐私显示与改动前一致。
