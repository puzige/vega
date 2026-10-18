# 移除 Composer 预测快捷键提示

状态：v1.0，Codex 审查方案，等待 Vega 编写先行测试。关联 [Issue #474](https://github.com/puzige/vega/issues/474)。本轮只交接方案，不代表实现、测试或安装已完成。

## 1. 目标与边界

移除 Composer 输入预测出现时工具栏上的“Tab 接受”文字，不以图标、tooltip、其他文字或新增帮助入口替代。

继续显示弱提示色的预测文本；无修饰 Tab / → / Enter 仍将预测接受为可编辑草稿，不自动发送。保留原 placeholder、无预测时的键位、Esc 丢弃、中文 IME、复制/选区、发送、模型与权限控件及预测设置、请求、取消和计费行为。

本规格仅替代 [输入预测规格](vega-issue-469-composer-predictions.md) P12 与 §3 中要求显示“Tab 接受”的展示条款，其余行为和已有有效断言不变。不修改模型回答、用户内容或任何业务数据。

## 2. 验收矩阵（先于实现计划）

自动测试只由 Actions 执行。下面是冻结的验收目标，不是已取得的执行证据。

| 用例 | 前置状态与操作 | 可观察结果 | 测试层级与状态 |
|---|---|---|---|
| H1 有预测时无额外提示 | 真实 ConversationStream，空 Native Composer；经 begin/apply 接入有效预测；浅/深主题分别完成渲染 | 预测保持可用、真实草稿为空且不可发送；Composer 与主要控件已布局；渲染树中没有预测快捷键提示节点 | 新增 GPUI 先行回归，待 Vega 编写、待 CI RED/GREEN |
| H2 无预测与布局稳定 | 同一窗口、相同主题与宽度，比较没有预测、预测可见、丢弃预测后的状态；覆盖正常与最小窗口宽度 | 各状态均没有提示；Composer 外框、添加/权限/模型/发送控件保持原布局，无额外提示行、控件消失或预测引入的位置跳动 | 与 H1 合并为最少的参数化 GPUI 回归，待编写/CI；原生外观待用户手测 |
| H3 接受仍只填草稿 | 预测可见，分别按无修饰 Tab / → / Enter，然后编辑 | 完整预测只接受一次、光标末尾、可继续编辑；没有 ComposerSubmitted 事件 | 保留现有 prediction_bare_accept_keys_fill_editable_draft_once_without_submission，待最新 head CI 复验 |
| H4 原键位与失效边界 | 无预测 Tab、预测可见 Shift+Tab/修饰键、Esc 丢弃；开始中文/emoji marked composition | 原遍历/编辑/发送语义不变；Esc 不复活旧预测；IME 候选确认不接受预测或发送 | 保留现有预测键位、IME 与失效测试，待最新 head CI 复验 |
| H5 原生展示 | 合并并通过云端发布后，正常/窄窗口、浅/深主题查看真实预测，再 Tab 接受并编辑 | 工具栏不再有“Tab 接受”，灰色预测仍可见，模型/权限/发送可操作且不自动发送 | 用户纯手动验收，尚未执行 |

H1 必须有正向前提断言：`begin_composer_prediction` 成功、`apply_composer_prediction` 为 true、`prediction_text()` 等于固定合成建议、`text()` 为空，且 `composer-shell` 和主要控件有实际渲染 bounds；不能通过关闭预测或没有绘制窗口获得空集成功。预测本身的 placeholder/真实草稿隔离沿用现有 TextInput 渲染回归。

新增测试观察运行后的 GPUI 元素树/布局，例如 `VisualTestContext::debug_bounds("composer-prediction-hint")` 为 None，并结合上述正向前提和布局断言；不是扫描 Rust 源码或仅断言符号不存在。若锁定依赖提供的观测能力不足，先交回 Codex，不为测试扩大生产公共 API。

## 3. 根因与代码证据

审查基线：`db111643`，已 fetch 并更新。证据为静态代码审查，未在本地运行 Vega 或复现界面。

- `ConversationStream::apply_composer_prediction`（`crates/vega_ui/src/conversation_stream/composer_predictions.rs`）把有效建议放入 TextInput，并触发重绘。
- `TextElement::prepaint`（`crates/vega_ui/src/text_input/element.rs`）在真实内容为空时，用预测替代 placeholder 排版，使用现有弱提示色；这条预测显示路径不需要删除。
- `ConversationStream::render_composer`（`crates/vega_ui/src/conversation_stream/render.rs`，基线第 335–347 行）另在工具栏的弹性占位中检查 `prediction_text().is_some()`，添加 `composer-prediction-hint` 子节点与固定文字“Tab 接受”。这是本卡要移除的展示来源。
- `TextInput::accept_prediction`、`right` 与 Composer 原动作绑定负责接收/键位；提示节点没有独立点击或接受逻辑。删除该节点不需要改键盘处理、预测所有权或 provider 调用链。
- 旧输入预测规格 P12 / §3 明确要求该提示；本次用户要求覆盖这两处展示要求，必须同步登记，避免按旧契约重新添加。

## 4. 执行顺序与允许文件

### 4.1 先写测试并交回，不改生产

1. 复用任务分支 `feat/474-remove-tab-accept-hint`，worktree 按仓库约定位于 `~/Workspace/worktrees/vega-474-remove-tab-accept-hint`。修改前 fetch 并核对远端基线；保护原工作区及其他卡片的修改。
2. 在 `crates/vega_ui/src/conversation_stream/tests/composer_predictions.rs` 写齐 H1/H2 的最少先行测试；复用 `install_prediction`、`open_controller_stream`、`focus_composer` 与 `VisualTestContext`。参数只覆盖有区分度的主题/窗口与预测状态，不复制全套旧预测测试。
3. H3/H4 与 `crates/vega_ui/src/text_input/tests.rs::prediction_uses_placeholder_layout_without_changing_real_text_or_viewport` 已存在，保留原断言，不为报告追加同义用例。
4. 记录测试提交/补丁及生产未改的基线。在 Issue 交回 Codex，保持 In progress、当前执行者待领取、下一执行者 Codex，并停止写入。
5. Codex 审查测试补丁后建立/复用 base 为 master 的测试先行 draft PR，触发现有 pr-check。有效 RED 应来自 H1 在有效预测已显示时发现提示节点仍有 bounds；H2 或其他保护回归可以原本通过。无关编译、依赖、runner 或网络失败不算有效 RED，不能进入实现。

### 4.2 Codex 确认有效 RED 后才实施

1. Codex 留存 Actions run/job、原始失败与冻结测试目标/补丁，明确交回 Vega。
2. 生产修改仅限 `crates/vega_ui/src/conversation_stream/render.rs::render_composer`：把带条件提示子节点的工具栏占位简化为 `.child(div().min_w_0().flex_1())`。保留该弹性占位与相邻控件，不新增替代文案，不改其他渲染分支或键位。
3. 冻结后不追加或放宽测试，不改输入预测实现/设置/请求/取消/计费，不升级依赖、不改 Cargo/CI/主题 token/用户配置/安装应用，不清理无关代码或注释。
4. Vega 提交实现与本地静态检查结果，在同卡交回 Codex；由 Codex 在同一 PR 检查相同测试 GREEN 与最新 head 完整门禁。

除本规格、旧规格对应展示条款与交付证据外，允许新增测试的文件仅为上述 `conversation_stream/tests/composer_predictions.rs`；唯一生产修改文件为 `conversation_stream/render.rs`。发现遗漏边界、接线/API 问题或需要扩大范围，按执行指南 §6 先交回 Codex；验收变化须独立重开先测后修周期。

## 5. 验证命令与证据

本地只允许快速静态检查：

```sh
git diff --check
cargo fmt --all -- --check
```

编译、Clippy、测试和应用运行不在本地执行。现有 `.github/workflows/pr-check.yml` 在测试阶段与实现阶段分别运行：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/release_tests.py
python3 scripts/prepare_computer_use_tests.py
python3 -m unittest discover -s scripts/tests -v
cargo nextest list --workspace --message-format json
cargo nextest run --workspace
```

本卡基线已有预测测试，按非空集合正常执行；`no Rust tests` 不能证明本卡验收。无需新建 workflow 或本地测试入口。H1/H2 新测试使用行为命名，不把 issue 编号写入代码。保留 H3/H4 和现有 TextInput 渲染测试的准确测试名及同一测试先后结果。

交接/PR 必须逐阶段记录：生产未改的 RED 基线、测试提交/补丁、目标与期望、原始失败及原因、后续实现顺序、相同测试 GREEN、最新 head required check `check (fmt, clippy, test)`。保留可访问链接，squash 后仍可审查；未运行写待 CI，未取得计数不猜测。检查启动后每 5 分钟查询，不 watch 或短轮询。

当前状态：仅完成静态审查与方案；新测试尚未编写，Actions RED/GREEN、PR、实现及用户手测均尚未取得。

## 6. 手测、交付与回滚

用户在合并并完成云端发布后的版本上，等待一次正常 Native 主轮产生灰色预测；检查浅/深主题与窄窗口均无工具栏快捷键提示，分别用 Tab / → / Enter 接受、继续编辑，确认不会自动发送。再确认无预测时的 Tab 遍历、Esc 丢弃和中文 IME 未改变。

Vega 只按本方案交回测试/实现，不自行开 PR、合并或关闭 Issue。Codex 仅在最新 head 云端门禁通过后 squash merge，改 In review 等用户手测；保留 Issue 开放及本卡分支/worktree，不因方案交接写“已完成”。

若需回滚，通过 PR revert 本卡实现提交；本卡无数据库、配置或接口迁移。不关闭全局输入预测开关来替代修复或回滚，也不恢复/删除其他卡片的内容。

## 7. 本轮交接约定

- 方案版本：v1.0；生产基线：`db111643`。
- 分支：`feat/474-remove-tab-accept-hint`；提交身份与可访问链接记录于 Issue #474 的正式交接评论。
- 下一执行者：Vega；第一阶段仅写先行测试并交回 Codex，不能跳过 Actions 有效 RED 开始实现。
- 发现范围或验收缺口时按 `[BLOCKED]` 在同卡回报，保持 In progress 并交回 Codex。
- 本轮 Codex 发布可访问方案、将卡置 Ready 并读回确认后停止写入；不宣称 Vega 已领取或已开始实现。
