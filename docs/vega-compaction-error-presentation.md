# 压缩临时提示与回合错误展示

2026-10-15，用户截图修订，Issue #397。关联 #381、#372、#375。本规格覆盖旧的压缩终态常驻和 Interrupted 一律警告约定。

## 展示契约

- 压缩进行中显示英文 `Compacting context`，使用现有过程图标与中性色。与上方运行标题分隔线及后续过程行使用现有 conversation process spacing token，不紧贴分隔线；不额外增加卡片、解释段。
- 成功后移除临时提示且释放全部布局空间。取消后也移除临时行。成功/取消记录仍持久化用于真实状态、预算和代际隔离；重开历史不复活提示，不能删除数据来隐藏UI。
- 失败使用所属回合底部的单一错误行：红色具体且安全的原因，可换行，不使用边框、圆角卡片或背景。独立手动压缩失败无回合归属时仍在其位置保留可见错误。错误提示不因为折叠思考/工具而消失。同回合不同真实原因在同一底部区域各显示一条安全文字，相同原因去重；不能因存在任意运行失败就吞掉另一种压缩失败。已被后续成功操作解决的旧压缩失败不继续常驻，保持operation/generation归属与隔离。
- 已有类型化具体失败优先于后续笼统 Failed/Interrupted、摘要与停止事件。无真实原因的历史失败使用准确的通用失败提示，不猜测原因或读取原始敏感日志。
- 用户主动取消或主动新消息替换当前工作，不显示“已中断”错误；无已知具体原因的 Interrupted 也不制造报错行，保留整体中性 `Stopped after ...`、终止计时和已产生内容，不把未知原因标成用户取消或成功。错误与取消竞态中已确认具体错误仍保留。
- 恢复/权限/凭据安全操作入口保持可发现和键盘可用，权限卡本身不改成普通错误行。重连是临时状态而非终态错误，不改变重试策略。
- 延续工具过程英文裁决，压缩过程与本轮调整的错误展示产品文案英文；用户/模型内容、工具原文不改写。已有安全错误类型优先，不把未知raw provider payload直接展示。

## 验收矩阵（实现前）

| 情况 | 预期 | 验证 |
|---|---|---|
| 压缩开始 | 和运行头/后续工具有一致间距，正常过程样式 | GPUI 几何、浅深主题 |
| 成功/取消 | 临时行、图标与占位移除，内容顺序与滚动锚点保持 | 事件、布局、历史恢复 |
| 失败 | 回合底部单条红色具体错误，无卡片，长错误换行 | 投影与GPUI |
| 错误后中断/摘要 | 具体错误不被笼统中断覆盖，不重复 | 多事件顺序回归 |
| 手动停止/消息替换 | 无错误提示，计时终止，内容保留 | 事件及历史回归 |
| 未知原因中断 | 保留整体中性Stopped状态，不伪造具体错误/取消意图；已有真实失败始终优先 | 原因分类回归 |
| 重开 | 成功压缩不重现，失败原因保留 | hydration |
| 安全/恢复 | 不泄露raw字段、保留必要恢复按钮和键盘操作 | 既有安全及权限回归 |

## 计划与边界

主 agent 负责规格、审查、集成与CI；专用UI subagent实现 context_control/render_rows/run_notice 及相应过程可见性、测量和测试。必要终态原因传播需先报告后授权；不改压缩算法、预算策略、timeout、全局权限或用户运行数据库。

本地只允许fmt/diff等快速静态检查，禁止编译/build/test/打包。测试交云端 PR Check。完整绿后合并并 In review，用户手测前不关闭Issue或清理工作区。

## 参考边界

Codex公开core的 compact.rs 把 Interrupted/TurnAborted 与真正错误分开：非取消错误才发送 Error 事件。参考 https://github.com/openai/codex/blob/main/codex-rs/core/src/compact.rs 。这不证明桌面端具体样式；Vega视觉以用户截图及上述明确裁决为准，不解包私有桌面资产。

## 错误与取消传播修订

只读诊断发现真实压缩错误会被同时发生的 cancellation token 覆盖：自动压缩 loop_.rs 仅看token、手动compaction.rs使用 error Cancelled 或token；pipeline合流把 Cancelled 当 Error。最小修复只依据已经返回的类型化错误判定取消：Cancelled走Interrupted，其他具体失败保留Failed/Error，即使token稍后取消。错误合流不得让Cancelled掩盖另一支实际错误。保留完整usage/persistence/diagnostic与duration，不增加重试或放宽预算。

UI在先收到Interrupted后收到同回合真实失败时允许Failed升级，并保持已冻结耗时；正常Completed不被无关迟到事件污染。恢复历史必须沿用持久化的原因，不伪造取消意图。

后端executor独占 vega_runtime/src/agent/loop_.rs、vega_conversation/src/agent/compaction.rs、entry.rs 及必要pipeline取消投递与对应测试；UI executor不改后端。新增失败+token取消竞态、typed取消、pipeline错误与取消分支回归，全部交CI。
