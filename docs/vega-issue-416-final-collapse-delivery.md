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
| `crates/vega_ui/src/conversation_stream/model.rs` | 识别选区光标归属；清除隐藏 copy 的选择 |
| `crates/vega_ui/src/conversation_stream/thinking.rs` | 在本模块范围访问思考选择与焦点归属 |
| `crates/vega_ui/src/conversation_stream/render.rs` | 删除为等待阅读保护解除而保留的 paint 补收起路径 |
| `crates/vega_ui/src/conversation_stream/tests/issue296_phases.rs` | 阅读模式、可见焦点隔离、真实消息选择及活动轮重复 Final |
| `crates/vega_ui/src/conversation_stream/tests/issue296_process_timeline.rs` | 即时收起、真实滚轮回看、原始顺序与再次展开 |
| `crates/vega_ui/src/conversation_stream/tests/issue296_codex_stream.rs` | 工具展开/原生选区与阶段焦点不阻挡 Final |
| `crates/vega_ui/src/conversation_stream/tests/issue296_expansion.rs` | 思考原始复制、真实选择清理和再次展开 |
| `crates/vega_ui/src/conversation_stream/tests/issue296_stream_block_spacing.rs` | 在既有产物外显/几何断言上增加产物焦点保留 |
| `crates/vega_ui/src/conversation_stream/tests/thinking.rs` | 移除已删除 reading helper 的测试调用，保留阶段显示与焦点断言 |

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

以下均为测试覆盖已实现或保留的状态；**所有行为测试均 NOT RUN，待云端 PR Check**，不把格式化和代码审查作为行为通过证据。

| ID | 自动回归入口 | 覆盖与状态 |
|---|---|---|
| FC1 | `issue296_phases` / `issue296_process_timeline` | Commentary→工具→Commentary→工具→Final 的唯一正文与原文/顺序断言保留；NOT RUN |
| FC2 | `issue416_final_collapses_once_despite_reading_and_preserves_visible_focus`、`issue416_timeline_final_collapses_reading_without_followup_input`、`issue416_timeline_scrolled_back_final_collapses_before_scroll_resume` | 上滚、真实滚轮、过程选择/焦点、工具与过程已展开；不要求后续输入，不强制尾随；NOT RUN |
| FC3 | `issue416_final_clears_only_hidden_native_message_selection`、`issue296_expansion_p07_markdown_selection_and_original_copy`、产物高度用例 | 真实 Commentary/思考选区清理，Final/其他轮选区与 Composer/产物焦点保留；NOT RUN |
| FC4 | 原有 Final/锚点用例、真实消息选择用例、timeline 阅读模式用例 | 活动轮重复 Final、完成、重绘保持后续主动展开；复制源与身份不变；NOT RUN |
| FC5 | 现有 phases 历史/未知/UTF-8/越界/冲突/失败、timeline 历史/缺口/取消/跨轮用例 | 保留原有保守投影与数据断言；NOT RUN |
| FC6 | 既有权限/拒绝工具、等待确认、产物外显与几何用例 | 保留权限、错误、待处理事项与产物规则，并增加产物焦点隔离；NOT RUN |

## 待云端与用户验证

云端门禁执行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`。以上功能点用例均在 workspace nextest 内；执行结果由主 agent 在 PR 中记录。

原生工具选择的帧清扫与焦点归属、隐藏项缩短后的列表锚点仍待 CI/用户真实界面确认。真实模型会话、浅深主题视觉与手动回看体验未在本地运行；未安装或重启 Vega，未修改用户运行时数据。没有发现需要扩大实现范围的阻挡项。
