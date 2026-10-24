# #545 滚动派生缓存实施规格 v2

阶段：接口已静态核对，可实施；尚无产品、CI 或性能通过结论。
基线：`b7cb6be20aa6e6c72bcfad45d9649804c6d9ce45`。
占用：`scroll-cache-takeover-20261024T1741Z`。
旧规格及矩阵：[v1 固定源码](https://github.com/puzige/vega/blob/2fc14fd1ae805d832ca1e1830472f2d7eb9d01ce/docs/545/vega-issue-545-scroll-cache.md)。本版替代旧的未知接口及强制 TEST-FIRST 流程，不减少用户可观察验收。

## 1. 验收

| ID | 操作 | 必须保持的结果 |
|---|---|---|
| A1 | 非空生产 render，列表及锚点 rail | notice 投影与 stage insets 各计算一次，消费者共享本次已刷新结果；空会话无旧 rail |
| A2 | 长历史预热，纯滚动/无关刷新实际触发父 render | 未变化且仍驻留的正文不重新扫描估高或构造预览；冷暖产物一致；不声称整体 O(1) |
| A3 | 流式追加/修订/结束，同时上翻历史 | 变化项失效，历史驻留项复用；预览及时，阅读锚点/偏移及原底部跟随保持，不抢焦点 |
| A4 | 支持的增删/身份替换/分页与顺序位移，直到清空 | identity 不依赖索引；移除项退休，新 incarnation 不复用旧正文；顺序与定位正确，未变驻留项保留 |
| A5 | 有效正文宽度改变/恢复，展开/收起 | 正文估高匹配实际估算约束；显示层组合即时更新；选字、完整复制、展开、导航不变 |
| A6 | notice/continuation 改变；空/非空 thread 切换 | 当帧 notice/insets/位置一致，不串 thread；不因展示变化淘汰未变化的正文预览 |
| A7 | 超预算历史、超长回复、表格/代码、测量修正 | 空间有界，miss 正确重算，实测优先；估高不冒充真实尾部到达 |

四类用户手测组合：长历史暖滚动、单条长回复、表格/代码、生成时上翻。生产耗时/帧间隔尚未采样，不预报收益数字，不用 BenchStreamView 代替生产体验。

A4 澄清：缓存必须支持现有分页/身份替换入口；本卡不新建任意重叠页去重或消息编辑功能。相同 key 的不同正文 owner 必须使缓存失效，不假定上游永不复用身份。

## 2. 已核对根因及实际接口

- `conversation_stream/render.rs::render` 已在同步活动 model、settle_final_presentation 后调用 refresh_run_notices 和 stage_top_insets；message_anchor_geometry 再各计算一次。
- message_anchor_projections 每次构造预览；estimate_entry_height/estimate_markdown_height 扫描正文。`phase.rs::estimated_run_height` 也估算展开过程里的助手正文，必须走同一缓存。
- StreamEntryIdentity::key 编码 thread/message/segment 或本地身份，不代表内容版本。
- MessageCopy::id 提供正文 owner incarnation；content_generation 对用户源可用，但不能替代助手 Markdown model 的最终物化版本。
- StreamModel::sync 已返回 changed，但没有 O(1) 聚合版本。应在此 owner 内每次 changed 时增加版本，覆盖追加、reset、块版本变化、pending 替换/移除/finish。
- measured_entry_heights 现由显式 invalidation 管理，不在本卡重做版本化测量系统。

## 3. 最小实现契约

1. rail/geometry 接收 render 已计算的 stage insets，消费已刷新的 self.run_notices；保持刷新顺序与列表快照一致，无需通用快照框架。
2. 缓存仅保留用户文本估高、助手 raw Markdown 估高、用户/助手已脱敏预览。failure 行、owned/unowned 调整、copy footer、notice、stage inset、process visibility、run 展开/cap、非文本 card 估高继续即时计算，不缓存整体 geometry。
3. source token：用户为 `(stable key, variant, copy.id, copy.content_generation())`；助手为 `(stable key, variant, copy.id, model.content_revision())`。查命中只比较便宜元数据，不 hash/比较/复制全文，不遍历 block version 向量。
4. estimate 另匹配当前有效约束 `clamp(workspace_width - 2 * CONVERSATION_PADDING, 1, CONVERSATION_MAX_WIDTH)`，区别未知/default 与已确认宽度；不得粗略舍入丢掉换行差异。现有排版指标是编译期常量，不引入无消费者的全局 metrics 服务；未来运行时指标加入 key。
5. preview 与宽度/notice/footer 无关，不随这些事件失效。只保留脱敏后有限预览，不保留第二份全文、行数组、entity 或渲染树。
6. 每 stream 最多 2048 个文本 entry record、2 MiB 可变 payload（实际保留 key 与 preview 的 allocation）、单 record 4 KiB。每 key 只保留当前 source/current width，旧 revision/宽度替换，不累积。超限不 admission，结果仍返回正确。
7. 确定性 admission 防止全历史 sweep 导致 LRU 每帧全部抖动：最多 1792 个阅读位置附近条目和 256 个尾部条目，去重；集合外 miss 不驱逐已选集合，历史未满时全部 eligible 条目可驻留。索引仅决定 admission，不决定 identity。无界历史允许非驻留 miss，不声称扫描完全消除。
8. set_entry_identity/list_remove 退休旧 key；history window replacement 清空；每 render 轻量元数据准备可清理其他直接 splice 路径的过期 key。不同 thread 隔离。invalidate_item 不增长正文版本或清正文缓存，因为该方法也服务纯显示状态变化。
9. 保留原测量优先级、256 测量扫描/512 保留窗口、width/read-mode reset、anchor restoration，以及 MessageAnchorLayout::capture 通过真实 bounds/reading bottom 得出的 at_tail。绝不改用估高判断尾部。

## 4. 修改范围及顺序

- model.rs：StreamModel owner revision/getter；不改 sync 返回值语义或解析行为。
- core.rs：缓存 owner/初始化及 identity/removal retirement。
- message_anchors.rs（允许私有 sibling cache module）：有界存储、source key、admission、预览/正文估高 helper、rail/geometry 输入。
- render.rs：prepare 及传入当帧 insets。
- phase.rs：estimated_run_height 的助手估高复用 helper，保留现有 display composition。集成上游 #517 后，copy_source_for_entry 的非空检查使用现有 copy.has_text()，避免每次 geometry fallback 为判断非空而 clone 全文；保持新 Codex copy/footer eligibility，不改变复制来源或结果。
- content.rs：history window replacement 清理。

先实现最小生产修改；开发测试按风险覆盖同长正文版本变化、finish、同 key 新 incarnation、宽度变化与 preview 复用、有界/超限/admission/retirement、冷暖输出及正文组合。不得新增依赖、schema、runtime/store 改动、通用 CI 或新的产品说明文字。代码不新增注释/unwrap/expect。

## 5. 验证与交付

本地只允许快速差异/格式/静态检查，不运行 Vega、构建、测试、profile、CUA 或安装/重启。现有 Actions macOS Clippy/Nextest 执行自动验证，用户在合并后手测真实交互。可在开发分支加必要私有测试/计数 seam，不扩大 public API；若用 GPUI harness，须先确认发生生产 render，再解释零次扫描，计数须结合正确产物/位置，不能以未触发路径冒充缓存命中。

实现停写后给非实现者固定内容独立审查。使用开发 Rust 测试时固定可恢复源码/完整 SHA/实际 CI 结果，验证后独立提交清理及审查；最终 head 无 Rust 测试/独占支持并由完整 required CI 确认 0 发现/0 执行。空集写 no Rust tests，不称行为通过。失败不删断言、不 ignore、不盲目重试；先查原日志修责任层。CI 查询间隔 5 分钟。

最新 head 审查和 required check 通过后 squash merge；Issue 保持开放，Project In review，等用户手测。本版无运行时观测或性能承诺。
