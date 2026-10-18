# Issue #449：全局滚动边缘渐隐交付记录

实现对应 [规格 v1.2](vega-issue-449-scroll-edge-fade.md)，关联 [#449](https://github.com/puzige/vega/issues/449)。代码与自动用例已写入；本地仅静态检查，所有行为、绘制断言与完整回归待 GitHub Actions，原生视觉及输入法待用户手测。

## Freeze

- 初始基线：`75da678a69fa809159f922615e1137bb48b528ba`；集成基线：master `abd5dc67`（包含历史来源编号与测试清理及侧栏排序）。
- 分支：`feat/449-scroll-edge-fade`；先行规格原提交 `c88428c2`，最终 rebase 后为 `6ee563ce`。
- 初始开工前 `git fetch origin`、`git rebase origin/master` 均退出 0，基线无变化。集成时主 agent 再次 fetch 后发起 rebase，执行者保留 `cf00946e` 的清理结果并解决本卡冲突。
- 主审最后同步 `abd5dc67`，rebase 退出0且无冲突；复核侧栏排序与本卡接线均保留，`git diff --check origin/master...HEAD`、`cargo fmt --all -- --check` 均退出0，原始 stdout/stderr 为空。
- v1.1 的吸顶标题退让及 v1.2 的 master 清理适配条款均由主 agent 在对应修改前写入并暂存；本实现提交纳入澄清。本文件与生产代码同一实现提交，不预填尚不存在的实现 SHA。
- 首次时钟采样：2026-10-18 08:43:39 UTC / 16:43:39 Asia/Shanghai。最终静态检查时间及采样区间见下方检查记录；采样前的初始仓规/API 阅读未计入该区间，不把采样区间冒充完整会话耗时。

## 实现

共享 `scroll_edge_fade.rs` 直接透传原 Element 的 layout、id、a11y、prepaint 和 paint，不新建滚动容器或命中区域。既有句柄经 `ScrollbarHandle` 只读读取；原本隐式滚动的 Div 在同一 Element 持久状态中保存句柄，不在 render 中重置 offset。

新增 Layout token：compact 16、reading 24、epsilon 1。横向使用 compact，阅读纵向使用 reading。原生视口按实际 overflow 可滚轴屏蔽隐藏/可见但不可滚的另一轴；变高 List 显式声明内置纵向轴。各边按真实隐藏距离计算强度，fade 长度最多占该轴四分之一；无溢出和零尺寸不绘制。四条边的裁剪区域不重叠。整视口大小的圆角渐变 quad 配合局部 content mask，stop 为 F/有效轴长；显式保留原 border inset、圆角及 scrollbar gutter。16px 窄条不会把 12px 表面半径错误钳成 8px。 工具详情与提交清单从实际父 Style 继承圆角及 border（包括 rem），避免只读到内层无圆角的 Style。待发送 rail 的父半径16、底部 overlap8 使用完整父高度的圆角 quad；底渐变起止坐标为 8/父高、(8+F)/父高，裁剪仍限于真实子视口，保留落在父底部圆弧内的透明角。

主会话只新增顶部渐隐。原 48px Composer foreground、动态高度、底部安全余量及锚点/跟随逻辑保留。活动/阶段标题的现有 pinned Cell 只读暴露；透明行包装器在该行当前帧 prepaint 完成后的最终 paint 中收集实际可见标题，主 List prepaint 前清空集合，List paint 后判断是否关闭顶部渐隐。测量、overscan、隐藏/零高项和被丢弃的测量重试不会拿上帧 pinned 状态冒充当前可见标题。

TextInput 使用生产软换行视觉行、当前首行、可见行及真实行高；单行使用 shaped advance 求右侧隐藏距离，不新增水平滚动或改变 caret/IME 几何。光标、活动选区、marked text 所在边退让。文字绘制限定于其既有 viewport；原文、复制和输入处理不变。默认输入表面跟随 elevated；已有 base 表面的编辑器、提交消息及 MCP 多行字段明确使用 base。

TerminalSnapshot 增加 `max_scrollback`。worker 在既有 generation 更新的锁内用 vt100 的 O(1) clamp 保存 offset、探测最大实际历史并恢复，仅缓存只读投影；UI snapshot/paint 不探测 parser、不额外发送命令或增加 generation。终端按真实行高及最大历史计算上下隐藏距离，保护活动光标行，状态工具条在 canvas 外。

## 全局接入清单

下列 39 个现有 native/virtual 滚动调用路径均已接入一次；表中的“已接入”是源代码状态，不是运行验收通过。各路径保留原 id、尺寸约束、句柄/列表状态、固定控件及既有事件路径。

| 已接入入口 | 句柄/方向 | 表面与尺寸 |
|---|---|---|
| `command_palette.rs::render` 可滚动结果行 | 原 ScrollHandle，纵向 | elevated / 16 |
| `automation_editor.rs::render` 任务详情 | 持久 Element 句柄，纵向 | base / 16 |
| `file_preview.rs::render` 文件正文 | 持久 Element 句柄，双轴 | base / 纵24横16 |
| `user_question_panel.rs::render` 问题与选项 | 原 ScrollHandle，纵向；保留 gutter | elevated / 16 |
| `tool_activity_group.rs::render` 工具分组内容 | 原 ScrollHandle，纵向；保留 gutter | base / 16 |
| `image_preview.rs::render` 应用选择菜单 | 持久 Element 句柄，纵向；保留自身 border/radius | elevated / 16 |
| `tool_card.rs::render_detail` 工具正文 | 原 ScrollHandle，纵向；保留 gutter | code / 16 |
| `branch_selector.rs::render` 分支行 | 原 UniformListScrollHandle，纵向 | elevated / 16 |
| `sidebar/mod.rs::render` 主任务列表 | 原 ScrollHandle，纵向 | sidebar / 16 |
| `sidebar/threads_block.rs::render_action_menu` 任务菜单 | 原 ScrollHandle，纵向 | elevated / 16 |
| `settings/provider_management.rs::render_provider_management` 模型配置页 | 持久 Element 句柄，纵向 | base / 24 |
| 同入口：提供方列表 | 原 list_scroll，纵向 | base / 16 |
| 同入口：提供方详情 | 原 detail_scroll，纵向 | base / 24 |
| `settings/render_impl.rs::render` 其余设置页正文 | 原 section_scrolls；提供方页外层关闭渐隐，由各内部视口承担 | base / 24 |
| `settings/updater.rs::render_updater` 发布说明 | 原 ScrollHandle，纵向 | base / 24 |
| `settings/skills.rs::render_body_preview` Skill 正文 | 持久 Element 句柄，纵向 | code / 16 |
| `settings/mcp.rs::mcp_confirmation` 授权事实详情 | 持久 Element 句柄，纵向 | base / 16 |
| `settings/mcp.rs::mcp_oauth_preparation` OAuth 预备详情 | 持久 Element 句柄，纵向 | base / 16 |
| `diff_view/render.rs::render` 首次快照/旧快照两条 Diff 分支 | 原 UniformListScrollHandle，两个互斥入口各一次 | base / 24 |
| `commit_panel/panel.rs::render` 提交清单 | 原 UniformListScrollHandle，纵向 | base / 24 |
| `conversation_stream/render_rows.rs::markdown_blocks` 代码块 | 持久 Element 句柄，横向；读取原圆角 | code / 16 |
| 同文件 `render_table_selectable` 可选择表格 | 持久 Element 句柄，横向 | base / 16 |
| 同文件 `render_table` 普通表格 | 持久 Element 句柄，横向 | base / 16 |
| `conversation_stream/run_activity.rs::render_with_details` 活动详情 | 原 ScrollHandle，纵向；保留 gutter | base / 16 |
| 同文件 `render_segment_with_top` 阶段详情 | 原 ScrollHandle，纵向；保留 gutter | base / 16 |
| `conversation_stream/subagents.rs::render` 子 Agent 结果 | 持久 Element 句柄，纵向 | base / 16 |
| `conversation_stream/render.rs::render_picker_list_layer` 模型行 | 原 ScrollHandle，纵向；标题外置 | elevated / 16 |
| 同文件 `ConversationStream::render` 主/只读/子 Agent 消息 | 原 ListState，仅顶部；实际吸顶时退让 | base / 24；原底部48保留 |
| `conversation_stream/thinking_body.rs::render_body` 思考内容 | 原 ScrollHandle，纵向；保留 gutter | base / 16 |
| 同文件 `BodyElement::prepaint` 横向代码/正文 | 原 horizontal_scrolls map，横向 | code 或实际 base / 16 |
| `conversation_stream/privacy.rs::render_privacy_control` 检查结果 | 持久 Element 句柄，纵向 | base / 16 |
| `conversation_stream/pending_messages.rs::render_pending_queue` 待发送行 | 原 ScrollHandle，纵向 | elevated / 16 |
| `sidebar/threads_block/organization/render.rs::render_pinned_pi` 置顶任务 | 持久 Element 句柄，纵向 | sidebar / 16 |
| 同目录 `menu.rs::render_organization_menu` 组织菜单 | 原 menu_scroll，纵向 | elevated / 16 |
| `vega/src/window/workspace.rs::render_workspace_pane` 工作区标签 | 原 tab_scroll，横向 | base / 16 |
| 同入口：artifact 文件预览 | 按 artifact 保留的 UniformListScrollHandle；关页/换路由清理 | base / 24 |
| 同文件 `render_workspace` 创建菜单 | 持久 Element 句柄，纵向 | elevated / 16 |
| `vega/src/window/subagent_panel.rs::render_subagent_panel` 子 Agent 列表 | 原 ScrollHandle，纵向 | base / 24 |

另有两个自绘入口：`text_input/element.rs::TextElement`（16）及 `terminal.rs::paint_terminal`（16），均使用共享度量/绘制。`file_selector.rs` 为模型/有界路径，`menu_list.rs` 为行 chrome，实际滚动由上述调用者承担；无滚动的图片画布、truncate/overflow_hidden、测试夹具及 benchmark 不增添滚动或装饰。

默认、运行、完成、失败、禁用状态保持各生产页面原有分支；装饰无状态文案。必要错误、权限、保存冲突、重启事项及主操作仍在原来的固定区域。滚轮、嵌套边界冒泡、拖选/复制、键盘焦点与菜单关闭逻辑未新增拦截。新组件无计时/循环/入场动画，Reduce Motion 无额外动画分支。

## 自动验收覆盖（全部 NOT RUN locally）

新增 18 个 `scroll_edge_` 用例，保留本卡全部新增断言，不加 ignore/retry，不启动 PTY/shell/真实网络。相对集成基线不删除或放宽仍保留的测试断言。

| 矩阵 | 新增证据 |
|---|---|
| A1 | `strength_is_spatial_clamped_and_continuous`；`exact_fit_epsilon_and_partial_strength_use_current_layout`：空、恰好放下、1px 容差、9px 隐藏时半强度 |
| A2 | `quads_partition_corners_and_protect_gutters`；`hidden_non_scroll_axis_never_gets_fades`；`rounded_gradient_uses_full_quad_and_excludes_border_and_gutters`；`nested_parent_rounding_and_overlapping_rail_keep_real_corner_geometry`：不重叠区域、真实 stop/content mask、border inset、16px 条上的实际12px圆角 |
| A3 | `virtual_handles_preserve_real_geometry_and_offset`：120行、40px overdraw，最初尾部未实例化；首/中/真实尾端及缩短列表，原句柄不移动 |
| A4 | `ordinary_bounds_programmatic_scroll_resize_and_theme`：真实 resize、四方向主题渐变、内容增长/缩短与真实端点 |
| A5 | `fade_allows_underlying_click_and_focus`；`implicit_handle_survives_rerender_and_wheel_reaches_viewport`：渐隐处命中/焦点、真实滚轮及重绘保位 |
| A6 | `production_conversation_only_adds_top_fade_and_preserves_composer`；`top_fade_yields_to_real_pinned_headers_and_recovers_on_reused_list`：真实会话浅深主题、单一原48px过渡、running/final 吸顶连续帧及恢复 |
| A7 | 在同一 `production_conversation_only_adds_top_fade_and_preserves_composer` 中扩展：浅深主题各在贴底/回看下将真实 Composer TextInput 短→8行→短；实际前景高度与 `foreground_height` 一致，单一48px quad 及位置保留，末条实际余量等于前景高且条数不增；回看 identity/offset 不变，`resume_tail` 后末条完整进入前景上方 |
| A8 | `input_uses_visual_rows_and_protects_cursor_selection_and_marked_text`；`single_line_hidden_extent_uses_shaped_advance_and_protects_active_edge`：软换行、实际行首/尾、选区/marked text 与 shaped extent、原选区不移动 |
| A9 | `terminal_range_is_actual_bounded_and_snapshot_is_read_only`；`terminal_production_paint_tracks_real_history_and_protects_cursor`：实际历史/容量区别、offset/generation/命令数、resize/reset/alternate screen 及真实 TerminalView canvas |
| A10 | `file_preview_keeps_header_outside_fade_and_clears_after_shrink`；`settings_provider_list_uses_its_real_viewport_and_surface`：真实文件/设置根视口与表面；以上逐入口清单 |

表中的函数名均带 `scroll_edge_` 前缀。会话新增场景位于 `conversation_stream/tests/scroll_edges.rs`，只迁入本卡新增的前景/吸顶用例及必要最小 fixture；本卡其它新用例保留在各所属模块。master 已删除的历史测试及辅助方法不恢复，不再列作门禁；Actions 执行最新 master 保留的完整测试集及本卡新增语义回归。

## Results

初始实现静态收口为 2026-10-18 09:22:33 UTC / 17:22:33 Asia/Shanghai；已采样工作区间 08:43:39–09:22:33 UTC，共38分54秒，不含首次采样前的仓规/API 阅读。rebase 适配的静态检查首次采样为 09:31:08 UTC / 17:31:08 Asia/Shanghai；最终采样为 09:35:50 UTC / 17:35:50 Asia/Shanghai，静态审查采样区间4分42秒；采样前的冲突处理不计入该区间。未运行任何本地 Vega、编译、构建、打包、Clippy、test/nextest 或 benchmark。

| 静态命令 | exit code | 原始输出 |
|---|---:|---|
| 首次迁移后 `cargo fmt --all -- --check` | 1 | 三处纯排版差异；随后 `cargo fmt --all` 退出0修正 |
| 最终 `cargo fmt --all -- --check` | 0 | 空 stdout/stderr |
| `git diff --check` | 0 | 空 stdout/stderr |
| `git diff --cached --check` | 0 | 空 stdout/stderr |

静态入口对照另核实：生产装饰器39处，保留原 native/virtual 入口数量；新增代码注释0条。rebase 对照确认 master 删除的113个源码/测试文件恢复0个；本卡18个新测试的原断言全部保留，17项及1个进程内 helper 在语义改名、fixture 迁移与格式归一化后函数体一致；另1项仅扩展 A7 的真实 Composer 几何/滚动断言，断言未削弱；相对集成基线未删除现有测试断言。自动合并的生产接线继续使用当前符号，已删除的历史模块 import 无残留。上述对照为源码审查，不替代类型、行为或像素验收。

云端现有 PR Check（尚未启动；由主 agent 接续）：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

如需在云端定位新用例，可使用 `cargo nextest run --workspace -E 'test(scroll_edge_)'`；这不替代完整 required check。此交付时无云端运行结果，不填造日志或 PASS。

## Residuals

- 规格偏离：无；吸顶处理按实施前 v1.1 澄清，历史清理适配按实施前 v1.2 澄清。
- CI NOT RUN：类型检查、Clippy、全部自动行为/绘制用例尚待 Actions。
- 手测 NOT RUN：原生浅深主题像素、系统始终显示滚动条、真实 IME/菜单/复制/嵌套滚动、终端 PTY 系统集成及用户完整 A1–A10 走查待合并后进行。
- 未改依赖、配置/持久化或用户运行数据；不需要数据迁移。回滚为回退本卡集成提交。
- 尚未 push、开 PR、合并、安装应用、关闭 Issue 或清理分支/worktree；后续由主 agent 按 Vega 流程完成。
