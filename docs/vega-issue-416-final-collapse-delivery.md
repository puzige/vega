# Issue #416 · Final 过程收起交付记录

关联：[Issue #416](https://github.com/puzige/vega/issues/416) · [行为规格](vega-issue-416-final-collapse.md)。实现分支：`feat/416-final-collapse`。

## 实现

首次可信 Final 继续通过现有 `final_confirmed` / `collapse_pending` 路径确认，每轮在下一次生产 render 中统一收起一次。移除上滚、选字、过程焦点和此前展开的阻挡条件，以及不再使用的阅读保护状态和 paint 阶段补收起入口。后续手动展开、搜索和锚点定位保持有效；重复 Final、完成事件和重绘不重新收起。

仅清理本次隐藏的 Commentary 与思考选区；持有窗口选区或局部选区的隐藏 copy 才触发窗口选区清除。隐藏过程的焦点转移到同轮可见入口，Composer、Final、其他轮与产物焦点保持。工具详情的原生选择沿用 GPUI 的未绘制参与者清扫；回归包含收起与重新打开均无残留选择。收起不调用尾部跟随或强制滚动。

元信息验证、正文与工具原始内容、存储、provider、失败/取消和跨轮身份规则未修改。新增产品可见文案：无。规格偏离：无。

## 修改路径

| 路径 | 修改 |
|---|---|
| `crates/vega_ui/src/conversation_stream/phase.rs` | 无阅读阻挡的 Final settle；按本轮过程 copy 清理选择 |
| `crates/vega_ui/src/conversation_stream/run_activity.rs` | 一次收起及隐藏焦点迁移；删除孤立的阅读保护状态/函数 |
| `crates/vega_ui/src/artifact_card.rs` | 按细化规格仅增加 crate 内实际七个产物按钮的焦点查询，隔离外显产物焦点 |
| `crates/vega_ui/src/conversation_stream/model.rs` | 识别选区光标归属；清除隐藏 copy 的选择 |
| `crates/vega_ui/src/conversation_stream/thinking.rs` | 在本模块范围访问思考选择与焦点归属 |
| `crates/vega_ui/src/conversation_stream/render.rs` | 删除为等待阅读保护解除而保留的 paint 补收起路径 |
| `crates/vega_ui/src/conversation_stream/tests/issue296_phases.rs` | 阅读模式、可见焦点隔离、真实消息选择及活动轮重复 Final |
| `crates/vega_ui/src/conversation_stream/tests/issue296_process_timeline.rs` | 即时收起、真实滚轮回看、原始顺序、真实工具子焦点迁移与再次展开 |
| `crates/vega_ui/src/conversation_stream/tests/issue296_codex_stream.rs` | 工具展开/原生选区与阶段焦点不阻挡 Final |
| `crates/vega_ui/src/conversation_stream/tests/issue296_expansion.rs` | 思考原始复制、真实选择清理和再次展开 |
| `crates/vega_ui/src/conversation_stream/tests/issue296_stream_block_spacing.rs` | 在既有产物外显/几何断言上增加首个及另一实际按钮的焦点保留 |
| `crates/vega_ui/src/conversation_stream/tests/thinking.rs` | 移除已删除 reading helper 的测试调用，保留阶段显示与焦点断言 |
| `crates/vega_ui/src/conversation_stream/tests/issue147_markdown_selection.rs` | 首轮 CI 后修订漏掉的旧阅读保护断言，保留真实选择、原文、Final 外显与 Composer 焦点覆盖 |
| `crates/vega_ui/src/conversation_stream/tests/mod.rs` | 用真实 Enter 展开/收起行为验证可见过程入口收到键盘焦点，避免测试 update 调用渲染阶段 API |

主 agent 拥有的规格与设计指南修订单独保留，专用实现 executor 未改写这些文件。

## 本地实际验证

验证时间：2026-10-16 03:06 UTC / 11:06 Asia/Shanghai。严格遵守 Vega 本地限制；未运行编译、构建、打包、Clippy 或任何 test/nextest。

| 命令或检查 | 结果与原始输出 |
|---|---|
| 首次 `cargo fmt --all -- --check` | exit 1；输出仅为本次修改的 rustfmt 差异，随后执行格式化 |
| `cargo fmt --all` | exit 0；stdout/stderr 为空 |
| 最终 `cargo fmt --all -- --check` | exit 0；stdout/stderr 为空 |
| `git diff --check` | exit 0；stdout/stderr 为空 |
| Python 静态检查：新增注释、孤立阅读保护状态和调用 | exit 0；`PASS: no added comments; no obsolete reading-protection state/callers` |

## 回归矩阵

以下均为测试覆盖已实现或保留的状态。前三轮云端结果与修订记录见下文；**本次真实前置 fixture 修订版待第四轮云端 PR Check**，不把格式化和代码审查作为行为通过证据。

| ID | 自动回归入口 | 覆盖与状态 |
|---|---|---|
| FC1 | `issue296_phases` / `issue296_process_timeline` | Commentary→工具→Commentary→工具→Final 的唯一正文与原文/顺序断言保留；待复验 |
| FC2 | `issue416_final_collapses_once_despite_reading_and_preserves_visible_focus`、`issue416_timeline_final_collapses_reading_without_followup_input`、`issue416_timeline_scrolled_back_final_collapses_before_scroll_resume` | 上滚、真实滚轮、过程选择/焦点、工具与过程已展开；不要求后续输入，不强制尾随；待复验 |
| FC3 | `issue416_final_clears_only_hidden_native_message_selection`、`issue296_expansion_p07_markdown_selection_and_original_copy`、产物高度用例 | 真实 Commentary/思考选区清理，Final/其他轮选区与 Composer/产物焦点保留；待复验 |
| FC4 | 原有 Final/锚点用例、真实消息选择用例、timeline 阅读模式用例 | 活动轮重复 Final、完成、重绘保持后续主动展开；复制源与身份不变；待复验 |
| FC5 | 现有 phases 历史/未知/UTF-8/越界/冲突/失败、timeline 历史/缺口/取消/跨轮用例 | 保留原有保守投影与数据断言；待复验 |
| FC6 | 既有权限/拒绝工具、等待确认、产物外显与几何用例 | 保留权限、错误、待处理事项与产物规则，并增加产物焦点隔离；待复验 |

## 首轮云端失败与修订

[PR #417 首轮 Actions](https://github.com/puzige/vega/actions/runs/38844032402)：云端 fmt、Clippy 通过；Nextest 失败。原始摘要为 `Summary [ 158.132s] 2736 tests run: 2730 passed, 6 failed, 5 skipped`，进程退出码 100。原始日志 `ci-first-nextest.log` 的 SHA-256 为 `2755b503390fdce6914a6ac4c60c9a27bf579a6046a8dae72ef341f0f516132e`，保留首次失败，不以重跑覆盖。

| 首轮失败用例 | 根因 | 修订 |
|---|---|---|
| `issue296_timeline_t06_real_selection_clear_and_composer_focus_release_final` | 漏改旧阅读保护预期，在 Final 后仍断言过程可见、原生选择保留 | 改为立即收起/清除隐藏选择，保留真实拖选、过程原字节、Final 外显及 Composer 焦点断言；用例更名为 `issue416_real_process_selection_clears_on_final_and_preserves_composer_focus` |
| `issue416_stage_anchor_reveals_inline_and_final_moves_hidden_focus`、`issue296_expansion_p07_markdown_selection_and_original_copy`、`issue416_final_clears_only_hidden_native_message_selection`、`issue416_final_collapses_once_despite_reading_and_preserves_visible_focus` | 测试的 update 回调调用 `disclosure::focus`，经 `use_keyed_state` 进入 `current_view`，违反 GPUI 必须处于 layout/prepaint/paint 的契约而 panic | 改用真实 GPUI Enter：可见 run disclosure 必须收到键盘并切换展开，再恢复原状态；继续验证隐藏焦点迁移，不删除焦点断言，不改生产行为 |
| `issue296_timeline_t02_final_folds_process_and_reopens_in_source_order` | 极小 fixture 整体可放进视口，原生列表此时允许回到 tail-follow，不能用该场景证明非底部滚动保留 | 此用例保留默认收起、原文字节与顺序、再次展开断言；不强制尾随仍由已有长历史回看/真实滚轮用例验证，其有意义的非尾随断言保持 |

本轮只修改上述测试及共用测试辅助函数，生产代码不变。2026-10-16 03:24 UTC / 11:24 Asia/Shanghai 完成静态复查：`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 退出码均为 0；无新增代码注释，测试代码未再直接调用上述渲染阶段查找 API。未在本地运行 Vega 代码、测试、编译或 Clippy。**第二轮云端结果待 CI，不能视为已通过。**

## 第二轮云端失败与生产焦点修订

[第二轮 Actions](https://github.com/puzige/vega/actions/runs/38845467566)：Nextest 失败，原始摘要 `Summary [ 137.087s] 2736 tests run: 2731 passed, 5 failed, 5 skipped`。原始日志 `ci-second-nextest.log` 的 SHA-256 为 `fc9222cd0445d0dcbe0646e37e7c02fbd3b6c1cc240e71a80053871d645bc589`。前一轮的旧阅读保护和小视口断言已不再失败；五项失败全部落在共用 Enter 焦点断言第一次切换时的 `left: false / right: true`，真实入口未收到键盘，不能通过移除断言解决。

受影响的五项为 `issue416_real_process_selection_clears_on_final_and_preserves_composer_focus`、`issue416_stage_anchor_reveals_inline_and_final_moves_hidden_focus`、`issue296_expansion_p07_markdown_selection_and_original_copy`、`issue416_final_clears_only_hidden_native_message_selection`、`issue416_final_collapses_once_despite_reading_and_preserves_visible_focus`。

根因：`Window::use_keyed_state` 经 `with_global_id` 使用当前 `element_id_stack` 生成身份。流顶层 settle 与真实列表行渲染时的元素层级不同，即使字符串相同也会创建不同句柄；原先移交到的是未绑定可见入口的句柄。工具及工具组的字符串查找同样不能可靠识别真实子焦点。

按主 agent 先补入的焦点细化规格修订生产实现：`RunActivityGroup` 构造稳定 `header_focus`，由 render 和 settle 共用；删除 settle 的工具/工具组字符串枚举，改用实际阶段父容器的 `contains_focused` 及既有阶段/思考句柄。父容器同时包含外显产物，因此仅在 `ArtifactCard` 增加 crate 内查询，检查既有七个按钮句柄，精确排除这些焦点；不添加产物状态或改动其渲染。

全部 Enter、选区、原文字节与跨轮断言保留。现有真实工具子入口持焦用例增加 Final 后的 Enter 展开/恢复关闭验证，覆盖阶段父链；产物焦点用例分别保留首个按钮和键盘导航到的另一个真实按钮，原有外显与高度断言均保留。

2026-10-16 03:42 UTC / 11:42 Asia/Shanghai，本次本地 `cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 均 exit 0、无输出。静态审计 exit 0，输出 `PASS: render/settle share stable header focus; no keyed-state lookup in settle; actual artifact handles excluded; Enter helper unchanged; no added comments`。未运行 Vega 代码、测试、编译或 Clippy。**第三轮云端结果待 CI，不能视为已通过。**

## 第三轮云端失败与真实前置修订

[第三轮 Actions](https://github.com/puzige/vega/actions/runs/38846900829)：Nextest 原始摘要 `Summary [ 169.512s] 2736 tests run: 2735 passed, 1 failed, 5 skipped`。原始日志 `ci-third-nextest.log` 的 SHA-256 为 `88331cd954656a1c968278527eb7f51d656859adb93bcf3394fa9a178c344687`。唯一失败为 `issue416_final_collapses_once_despite_reading_and_preserves_visible_focus`，仍在 Enter 辅助断言第一次切换时报告 `left: false / right: true`；原日志没有 mode，保留该诊断限制。

真实消息拖选、真实阶段/工具焦点、展开思考源和两个产物按钮的其他回归已通过。只读检查发现该循环有不真实的前置：`focus` 给未显示的 run details 句柄持焦；`thinking-focus` 使用仅有 Summary 的未展开源句柄；三个选择模式只写 `selected_text` 缓存，没有原生 snapshot。GPUI 包含查询在句柄相同时直接返回 true，不能据此添加重复的 `is_focused` 防御分支。现有证据不能锁定具体 mode，也不能据该失败认定新的生产缺口。

按先补入的第四轮规格仅修改测试与证据，生产代码保持不变：循环局部复用已有带 `TextSelectionLayer` 的 Harness，八个 mode 全保留；阶段焦点改用真实可见 `segment_header_focus(0)`；思考模式注入 raw `ThinkingDelta` 并实际展开源。三个选区模式复用现有布局观测和真实 mouse down/move/up，断言原生选中文本、活动选区及 Final 前实际焦点，未再直接写缓存或局部选择范围。原有收起、原字节、选区清理、Composer/Final 焦点隔离与 Enter 展开/恢复关闭断言保持，并为循环及共用 Enter 断言增加 mode/场景标签。

2026-10-16 04:02 UTC / 12:02 Asia/Shanghai，`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 均 exit 0、无输出。静态审计 exit 0，输出 `PASS: production unchanged; all eight modes retained; native selection input and visible-focus preconditions present; contextual Enter assertions retained; no added comments`。未在本地运行 Vega 代码、测试、编译或 Clippy。**第四轮云端结果待 CI，不能视为已通过；若真实前置仍失败，须按明确 mode 继续分析生产调用顺序。**

## 待云端与用户验证

云端门禁执行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`。以上功能点用例均在 workspace nextest 内；执行结果由主 agent 在 PR 中记录。

原生工具选择的帧清扫与焦点归属、隐藏项缩短后的列表锚点仍待 CI/用户真实界面确认。真实模型会话、浅深主题视觉与手动回看体验未在本地运行；未安装或重启 Vega，未修改用户运行时数据。没有发现需要扩大实现范围的阻挡项。
