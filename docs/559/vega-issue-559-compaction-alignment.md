# #559 — 压缩过程行对齐与阶段收纳

版本：v2，2026-10-25。Issue：[#559](https://github.com/puzige/vega/issues/559)。

## v2 用户追加：与工具过程共用收纳栏

本节是本轮实现契约，取代下方 v1 的“无新增交互”和禁止修改 `context_control` / `phase` / `run_activity` 的范围限制；v1 对齐与首行图标几何、错误和持久化规则继续保留。

本轮基线：`429c6a819fc5a30f8280afd2f6ed015d4682bf41`。用户要求将 `Compacting context` 和上方工具/命令过程放在同一收纳栏，不再在工具阶段下方常驻独立的压缩过程行。

### 根因与最小方案

- `context_control::apply_context_status` 虽调用 `RunActivityGroup::set_compacting`，但只切换回合 phase；压缩内容仍独立追加为 `StreamEntry::ContextCompaction`。
- 工具与思考经 `ensure_live_run_activity_segment` 进入 `RunActivitySegment`；阶段标题、展开内容和估算都由 `RunActivityGroup` 管理。当前压缩未接入这一过程投影，`process_entry_hidden` 也只隐藏成功/取消，不隐藏已纳入收纳的活跃压缩。
- 保留原始 `ContextCompaction` entry、generation、identity 与状态持久化；为当前回合当前阶段增加真实的压缩过程投影。已有活跃阶段时复用，不因压缩再建第二个收纳栏；正文已经分隔阶段时不跨正文合并；无阶段但有当前回合时通过现有入口建立阶段。
- 压缩作为过程项而非伪造工具调用，不生成工具 call_id、命令输出或假成功记录。实现可采用阶段内有 generation 归属的状态或专用过程 child，避免将 ToolCard 泛化成无关状态容器。
- 仅当活跃压缩已确实投影到所属阶段时，隐藏对应独立行。无所属回合的手动压缩、无法建立所属阶段的状态继续复用原行，不能为了去重无条件隐藏。

### 验收矩阵

| 场景 | v2 可观察结果 |
|---|---|
| 读取文件/执行命令后开始压缩，阶段收起 | 只有同一个过程收纳栏；标题显示真实的 `Compacting context` 与现有压缩图标，使用现有运行反馈；不能仍只显示已结束的 `Read files` |
| 展开该阶段 | 工具/命令与压缩按发生顺序位于同一详情区，复用现有箭头、鼠标与键盘展开/收起；不增加解释段或常驻卡片底色 |
| 只有压缩一个过程项，或正文后新阶段首先压缩 | 紧凑显示真实压缩状态，不出现空 `Thought` 标题、重复行或无内容的展开按钮；不跨正文合并到先前阶段 |
| 压缩成功/主动取消 | 从收纳标题与详情中移除临时压缩项及其全部占位，既有工具保持原状态、详情与展开状态；没有工具的临时阶段也不留空白 |
| 压缩失败 | 不把失败藏入正常过程收纳；仍按现有回合底部安全红字或独立手动压缩错误显示，不重复、不吞另一真实原因；恢复入口不变 |
| 压缩后继续产生工具/思考/正文 | 真实新活动恢复现有标题选择与运行反馈，无永久 `Compacting context`、错误的 tool count、重复或倒序 |
| 迟到事件/多次 generation/切线程/历史恢复 | 保留原有隔离与去重，不把旧状态投影到新回合；成功/取消提示不复活；没有新增持久化格式 |
| 窄窗口、浅深主题、滚动和焦点 | 使用原有字号/图标/间距 token；render、row count、height estimate、隐藏投影与锚点一致；复用已有焦点迁移与键盘控件，不吞必要错误 |

### 允许实现范围与验证

专职子 Agent 仅修改 `crates/vega_ui/src/conversation_stream/{context_control,run_activity,phase,render_rows,message_anchors,content}.rs` 中必要接线，确需其他文件先报告。优先复用现有压缩行渲染和高度计算，保持 generation 与 owner 绑定；同步受影响的穷尽分支、checkpoint/rollback、计数、可见性、布局失效和最终运行详情。不得修改压缩算法、预算、运行时事件、数据库、用户配置、权限、工具行为、主题 token、依赖、CI 或安装应用。

这是 UI 过程投影变化，按最小交付不临时制造 Rust 测试，不恢复已清除测试。本地只执行 diff/fmt 等静态检查，不运行 Vega、编译、Clippy 或测试，不使用 CUA。实现停写并提交精确版本后，由非实现者独立审查上述矩阵及静态调用链；现有 Actions 对最终 head 运行完整 required `check (fmt, clippy, test)`，Rust 结果应标为 `no Rust tests`（0 发现 / 0 执行），不能冒充行为测试通过。原生像素、交互、浅深主题与滚动效果待用户手测。CI 每五分钟查询；通过且独立审查无缺口后 squash merge，Issue 保持开放并置 In review，不自动安装或关闭。

回滚：回退本轮过程投影代码可恢复 v1 独立临时行；保留全部原始状态与历史数据。

---

## v1 已交付基线（历史范围）
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
