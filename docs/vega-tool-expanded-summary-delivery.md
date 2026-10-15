# #381 展开标题与英文过程界面

2026-10-15；最新展示契约以 `docs/vega-tool-block-codex-parity.md` 的“展开时切换标题”及英文裁决为准，替代旧行内耗时与中文产品标签示例。

## 实现

- ToolCard 使用统一的动态标题投影：收起显示安全命令，不带耗时；展开显示 `Ran command in 3s`、`Running command for 0s`、`Command failed after 3s` 等准确状态。完整命令和原始输出仍在详情，鼠标、键盘、程序自动展开和 hydration 共用投影，不依赖某个点击回调刷新缓存。
- Pending 为 Awaiting command approval，Approved 为 Ready to run command，真正 Running 才显示运行态。历史缺失起点/时长不编造数字；Corrupt 与 InvalidRejected 优先保留明确错误。紧凑英文时间使用 ms/s/m/h，原计时来源不变。
- 工具卡、工具组、run_activity 阶段与 Thinking 的产品状态、动作、aria 和截断提示改英文；保留模型生成标题/正文、命令/输出、用户内容。Thinking slider 已有 Off/Default，不改配置行为。整体 Working for / Worked for 保留原设计与时长格式。
- 工具权限卡英文覆盖按钮、可选拒绝理由占位、危险确认标题/提示；原 danger_reason 和 ACP option.name 保留原文，权限决策语义不变。
- 会话 Skill 激活条及历史来源/状态产品标签同步英文；技能名称和源数据不改，不扩到全局技能管理界面。
- 原箭头实现未改；详情滚动块右下角 Check + Success / Close + Failure、截断/复用标记及安全展示边界保留。

## 文件与回归

实现：tool_card.rs、tool_activity_group.rs、permission_card.rs、conversation_stream 的 run_activity.rs、thinking.rs、skills.rs 与 render_rows.rs 的 SkillActivation 分支。直接相关 tests 迁移产品显示期待，保留原文 fixture 和安全/隐私/错误断言。

先新增展开/收起回归再实现。覆盖成功、非零/-1、无metadata失败、拒绝/取消、待批准/批准未运行、历史运行缺起点；0/1/2/65秒及独立工具5/3秒、终态冻结；英文单位边界；宽窄窗口的收起省略、展开简短标题与原箭头相邻；鼠标/Enter/Space/程序切换使用相同投影。原footer滚动/图标与失败颜色、完整命令和中文输出原样保留、权限和隐私回归继续执行。

## 本地证据与风险

`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 均 exit 0。未执行本地编译、check、Clippy 或测试；新增/迁移测试等待云端 PR Check / CI，不标记已通过。真实浅深主题、窄窗口、展开与键盘操作等待用户手测。

英文迁移涉及多个已有展示期待，需云端全量回归确认遗漏；模型原文与权限业务字段不翻译是刻意边界。无新依赖、数据迁移、工具执行后端或应用重启。未 commit/push。


## PR #389 首轮 CI 反馈与修复

首轮 Clippy 通过；Nextest 为 2680 passed、6 failed、5 skipped。保留首轮失败记录，不将修复后尚未执行的测试标记通过。

1. `folded_stage_hides_running_pending_and_failed_tool_status`：漏迁移中文“失败”期待；改为明确英文 Command failed，原 needs_attention、折叠/不可见边界及错误归属断言保留。
2. `issue296_phase_rejected_tools_and_permissions_survive_final_collapse`：漏迁移中文“拒绝”期待；改为 Command rejected，原权限入口、Final 收起与状态存续断言保留。
3. `issue296_single_activity_mcp_names_are_safe_bounded_and_deduplicated`：MCP 多工具聚合文案仍期待中文；改为英文完整期待，名称去重、数量和参数/身份不泄露断言保留。
4. `issue296_polish_p02_all_disclosures_theme_keyboard_and_disabled`：旧断言要求展开前后箭头 x 坐标不变，与展开时替换标题的最新契约冲突。工具行改为核对箭头尺寸/垂直位置、紧跟新标题、位于行内；增加 hover/focus 不移动、收起恢复原位置、鼠标与 Enter/Space 对应几何一致断言。其他 disclosure 仍要求原始边界完全不变，浅深主题/显隐/禁用覆盖保留。
5. `issue381_mouse_keyboard_and_programmatic_expansion_share_title_projection`：测试在普通 window.update 中调用仅可用于绘制阶段的 keyed state helper，触发 GPUI 阶段保护。改用已有 `issue296_stream_polish::focus_row` 从已挂载控件真实遍历焦点，再验证 Enter/Space；不改生产焦点实现，也不模拟直接改展开状态替代键盘断言。
6. `issue70_t70_6_hydration_matches_live_and_reopen_resets_expansion`：live_text 与 hydrated_text 严格相等已经通过，下一条静态期待误将单卡 Searched content 写成组摘要 Completed: search content。仅修该静态期待；严格恢复相等、边界/图标和重新打开重置展开状态全部保留。

修复前执行 `git fetch origin` 与当前分支 upstream 的 `git merge --ff-only`，返回 Already up to date，未合入其他 master 改动。修复仅涉及对应测试及本记录；本地 fmt、fmt check、diff check 均 exit 0，未编译或执行测试，修复效果待后续 CI。
