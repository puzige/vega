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
