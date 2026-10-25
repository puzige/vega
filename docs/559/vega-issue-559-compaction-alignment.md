# #559 — 压缩过程行对齐

版本：v1，2026-10-25。Issue：[#559](https://github.com/puzige/vega/issues/559)。
基线：`420a0eb9a06594173b658f7e3a4437e3d2b5432d`。

## 目标与证据

修复 `Compacting context` 与相邻工具过程行的垂直布局不一致，不改变压缩、错误或持久化语义。

- Issue 附件为 1104×382 图片，HTML 显示尺寸 552×191。离线读取附件，不使用 CUA。
- 附件中两行图标的非背景像素横向范围均为 x=178..205，文字起点约 x=221..222。没有支持横向整体平移的证据，不添加局部 x/y 偏移。
- `context_control::apply_context_status` 将压缩状态投影为 `StreamEntry::ContextCompaction`；`render::Render` 经 `render_stream_entry` 调用 `render_entry_internal`。已有 `stage_top_insets` 负责外部过程间距。
- `render_entry_internal` 的非终态压缩行使用 `BODY` / `BODY_LINE_HEIGHT`、28px 最小高度和贴顶布局，图标单独放进正文行高容器。
- 相邻工具行和聚合阶段标题实际使用 `CONVERSATION` / `CONVERSATION_LINE_HEIGHT`、`CONVERSATION_TOOL_HEIGHT` 与居中布局。旧压缩布局未同步，导致行高、字体和内容中心使用不同基准。
- `message_anchors::estimate_entry_height` 同样使用旧压缩正文 token，修改显示时必须同步活跃行估算。

本次以实际工具过程行 token 为对齐基准，覆盖 [旧压缩状态布局](../vega-compaction-status-layout.md) 中仅针对活跃行的 13px / 28px 约定；保留 [临时提示与错误展示](../vega-compaction-error-presentation.md) 的生命周期、错误、安全与归属契约。不修改共享 token 数值。

## 验收矩阵（实现前）

| 前置与操作 | 可观察结果 | 验证路线与状态 |
|---|---|---|
| 同一运行先显示工具阶段，再开始压缩 | 图标槽宽、图文间隔、字号、行高与工具行一致；单行内容共用垂直中心，无常驻底色或新文案 | 静态核对当前 renderer/token；云端编译；像素外观待用户手测 |
| 单独手动压缩或压缩为首个过程项 | 相同紧凑行布局；既有外部过程间距保持 | 静态调用链与独立审查；手测待用户 |
| 可用宽度不足，状态文字换成多行 | 文字自然换行，不裁切；图标仍对齐第一行中心，不移动到整段中心；行高估算包含相同的行高与内距 | 静态几何核算与独立审查；手测待用户 |
| 压缩成功或取消，随后出现正文/工具 | 临时行零高度，不复活提示，不增加空白，不改排序或滚动策略 | 核对现有早返回、隐藏投影与 identity 路径；手测待用户 |
| 独立压缩失败或有所属回合的失败 | 保持现有红色安全错误文字、自然换行、归属和去重；不得把错误藏入正常过程行 | 核对失败早返回及估算分支未改变；手测待用户 |
| 浅/深主题、恢复历史 | 保留现有语义色及恢复规则；活跃行几何一致；无新增交互/焦点目标 | 静态审查；主题及历史外观待用户手测 |

## 几何核算

值均为当前共享 token 的逻辑像素，不新增字面几何数值：

| 项目 | 公式 | 结果 |
|---|---|---|
| 旧压缩文字行高 | `13 × 1.55` | 20.15 |
| 过程文字行高 | `13.5 × (23 / 13.5)` | 23 |
| 单行过程高度 | `CONVERSATION_TOOL_HEIGHT` | 25 |
| 上下内距各一份 | `(25 - 23) / 2` | 1 |
| 图标首行容器高度 | 过程文字行高 | 23 |
| 图标槽顶端 | `1 + (23 - 16) / 2` | 4.5 |
| 图标中心与文字行盒中心 | `4.5 + 16 / 2 = 1 + 23 / 2` | 12.5 |
| n 行活跃状态高度 | `max(25, n × 23 + 2 × 1)` | 单行 25，两行 48 |
| 文字可用宽度 | `max(1, text_width - ICON_SIZE - CONVERSATION_INLINE_GAP)` | 扣除现有 16+6 槽位 |

## 最小实现计划

1. `crates/vega_ui/src/conversation_stream/render_rows.rs`：只调整非终态压缩行的最小行高、字体/行高及垂直内距，使图标容器等于首行行盒并明确水平居中。保留 `items_start` 和自然换行，避免多行时图标随整段垂直居中。
2. `crates/vega_ui/src/conversation_stream/message_anchors.rs`：只同步非失败活跃压缩行的字体列宽近似、行高和上下内距；成功/取消零高度与失败现有估算不变。
3. 不改 `context_control`、`phase`、`run_activity`、工具行、共享 token、压缩算法、预算、数据库、用户配置、权限或 CI。不新增代码注释、依赖、测试支持或产品文案。

## 按风险验证与交付

本次是局部展示/估算修复，无新增业务决策；不为该差异新增临时 Rust 测试，不恢复历史测试。本地只做 `git diff --check`、格式与静态核对，不运行 Vega、编译、Clippy 或测试。代码由专职实现子 Agent 修改，停止写入后独立只读审查。

最新任务 head 必须通过现有 GitHub Actions：fmt、全目标 Clippy、保留的三组 Python 检查、Nextest 清点/运行及 required `check (fmt, clippy, test)`。最终 Rust 空集须明确为 `no Rust tests`（0 发现 / 0 执行）。CI 每 5 分钟查询，不用 watch。

不使用 CUA，不把静态检查或 CI 当作像素验收；合并后 Issue 保持开放并置 In review，待用户手测。原生应用安装不在本次交付内。

当前状态：spec 已明确，代码实现、独立审查与云端验证待执行。
