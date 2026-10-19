# #486 · 真实消息列表预绘制借用崩溃诊断与下一轮方案

2026-10-19。用户手测未通过，#486 从 In review 退回 In progress。本轮遵守用户“只做诊断”边界：记录静态根因、独立验收变化与修复计划，没有修改 Rust 生产/测试、没有执行测试或启动修复；不启动 #309，不将本卡置 Ready 触发领取。

需求 v1 的链接外观、完整预览及交互范围不变。本页修订技术方案 v2 的滚动状态采样时机，并为遗漏的真实 ListState 场景重新建立先测后修周期；不回填原 42 项测试来追认原交付。

## 证据与基线

- 用户报告：当前安装 v0.1.174，打开含可见链接的会话即 panic/SIGABRT，无须 hover；两次崩溃堆栈一致。本轮未独立读取原始 crash 文件或在本地运行应用，堆栈/安装事实以用户报告为据。
- 发布元数据确认：[v0.1.174](https://github.com/puzige/vega/releases/tag/v0.1.174) 于 2026-10-19 07:06:21 UTC 发布，目标提交为 `b32ad22e530d27925e375df00b6bd401bf9646a3`，包含原合并 `57393139fa8aae9bab74254b1a93bb966c4f2c91`。两提交间的本卡根层、render、inline_code、link_preview 源码未变；不是仅旧源码的推测。
- 最新生产诊断基线为上述 `b32ad22e`。保留本卡原分支 `feat/486-markdown-link-preview` 与 worktree；fetch 后通过正常 merge 同步，未改写原 RED/GREEN 历史。后续执行前仍须 fetch 并审查基线漂移。
- 原 [42 项 GREEN](https://github.com/puzige/vega/pull/491#issuecomment-6261003738) 的断言确实执行通过，但 [Host 源码](https://github.com/puzige/vega/blob/8f53229bc0b7fb349657aabe0e01f2c4b595fbdd/crates/vega_ui/src/conversation_stream/link_preview_tests.rs#L148) 统一使用 `ScrollHandle`/普通滚动 div。它覆盖了正文渲染函数，未覆盖生产消息流的 GPUI 虚拟列表借用边界，不能证明该路径安全。

## 静态调用链与根因

1. `ConversationStream::render` 在 [render.rs](https://github.com/puzige/vega/blob/b32ad22e530d27925e375df00b6bd401bf9646a3/crates/vega_ui/src/conversation_stream/render.rs#L1495) 用 `list(self.list.clone(), ...)` 绘制真实消息项；外层阅读 scope 通过同文件约 L1668 的 `track_list(self.list.clone())` 持有同一个 ListState。
2. pinned `gpui-pre 0.3.4` 的 `src/elements/list.rs::List::prepaint`（约 L1535）取得 `self.state.0.borrow_mut()`，并在该 guard 存活期间调用 `ListStateInner::prepaint_items`；后者约 L1290 调用子项的 `item.element.prepaint_at(...)`。
3. 子项 [InlineCodeText::prepaint](https://github.com/puzige/vega/blob/b32ad22e530d27925e375df00b6bd401bf9646a3/crates/vega_ui/src/conversation_stream/inline_code.rs#L103) 无条件进入 `prepaint_link_targets`。遇到可见链接时，[link_preview.rs L705](https://github.com/puzige/vega/blob/b32ad22e530d27925e375df00b6bd401bf9646a3/crates/vega_ui/src/link_preview.rs#L705) 为登记对象计算 `scope.scroll.stamp()`。
4. `ScrollSource::List` 随即调用 `ListState::logical_scroll_top()`；pinned 实现约 L561 对同一 RefCell 执行 `borrow()`。父列表的可变借用尚未释放，因而直接触发 BorrowError/panic。

这是同步 prepaint 阶段的重入借用冲突，不依赖网页请求、网络结果、缓存或 400ms dwell。删除 hover 功能、延迟网络请求无法修复打开会话即崩溃的问题。准确阶段为 **prepaint**，泛称“绘制时”不能把它误定位到后面的 paint。

## 新一轮验收，先于实现

以下为待编写/执行的五项行为目标，均 **NOT RUN**；不是已有测试数量、发现数或通过数。Rust 测试用行为命名，不写任务编号。

| 目标 | 前置与动作 | 必须观察到的结果 |
|---|---|---|
| B01 首帧真实消息列表 | 用 `ConversationStream::new` 与 `apply_history_page(HistoryPage)` 注入合成 AssistantText/UserText，含显式 Markdown 链接和裸 URL；真实 Render，根层最后挂 MarkdownLinkPreviewLayer；不移动鼠标到链接、不等待 hover | 首帧和重复帧完成，不 panic；链接仍真实呈现/可命中，不能略过登记蒙混通过；0 预览请求、0 外部打开 |
| B02 虚拟化可见性 | 同一真实 ConversationStream 加足够历史项，使含链接项先不可见，再用生产 ListState 在渲染外滚入/滚出；覆盖跨行或表格内链接 | 首次可见、滚出再进入均不 panic；没有 hover/focus 就不请求；不得以普通 ScrollHandle 容器替代 list |
| B03 列表滚动关闭 | 真实列表链接 dwell 达400ms，受控外部元信息完成显示卡；分别滚轮和程序改变真实 ListState，指针保持原位 | 本帧关闭并取消订阅；迟到结果/静止指针不重弹；Open/目标保真仍有效 |
| B04 相同滚动状态重绘 | 真实列表显示卡后重复刷新/物化相邻流式内容，保持目标和实际滚动签名不变 | 不误关/误重载；快照既不能永久为空，也不能沿用上一帧失效值；仍无借用 panic |
| B05 普通阅读面回归 | 生产 FilePreview/ScrollHandle 阅读宿主显示同类链接，按需打开卡再滚动 | 首帧正常且0请求；400ms才请求；滚动关闭，图片/选择/键盘接线不被采样调整破坏 |

B01/B02 必须通过真实 ConversationStream、生产 `apply_history_page`/消息物化和 GPUI list，保留同一 ListState 的外层 `track_list` 与内部 `list`，不能复制一个不持有借用的假列表，不能直接单独调用链接 helper。GPUI 使用测试平台；mock 仅隔离网络/时钟/外部打开，不能替换滚动读取、消息渲染或协调器决策。

每项验证稳定可观察的帧、卡片、网络副作用或生命周期。无 hover 时0请求是产品契约，须与完成真实列表渲染结合，不能只验证 mock 次数。必要的 cfg/dev fixture 或只读观察须最小化且共用真实 role/label/状态；不得为测试添加另一套生产判定。B01 不能使用 should_panic/catch_unwind 把旧崩溃标成通过。

## 通过有效 RED 后的最小实现计划

允许生产修改仅为 `crates/vega_ui/src/link_preview.rs` 的采样时机；若真实根层顺序检查发现冲突，交回 Codex 修订，不自行扩大范围。

1. `prepaint_link_targets` 只登记可见几何、目标、焦点和 scope 的可克隆句柄。删除此子项路径的即时 `.stamp()`；此时不读取任何 ListState 滚动位置。
2. 在窗口根 `MarkdownLinkPreviewLayer::prepaint` 的现有 `entity.update` 内，**将 staged 换入 visible 和比较旧/新 scroll 之前**，从 staged 链接保存的 scope 句柄生成该帧拥有的 ScrollStamp 值。该层在 [VegaWindow 根](https://github.com/puzige/vega/blob/b32ad22e530d27925e375df00b6bd401bf9646a3/crates/vega/src/window/render.rs#L497) 是最后的 sibling，消息列表的子项 prepaint 已返回，ListState 的可变借用已释放。
3. root 采样使用列表完成布局/自动定位后的状态，当前帧即完成比较。不能只在列表布局前采样而错过本帧内部定位，也不能延后一帧掩盖关闭时序。历史 visible 保留上一帧拥有的值；新值解析完成后再走既有 scroll/liveness、抑制重入、取消和焦点恢复逻辑。
4. 对 List、ScrollHandle、None 统一保留原 ScrollStamp 含义；子项登记不取状态，根层解析后发布。保留普通文本流更新与滚动的区别，不修改图标、预览服务、网络策略、400/150ms、点击/复制或键盘行为。
5. 不改 GPUI、不升级依赖、不改 CI/发布、不加全局开关。禁止用 try_borrow 失败回退 None、吞 panic、跳过链接登记或永久取消滚动检测来“修复”。

此计划本轮未实施。正式实现须由单一专职执行者完成；Codex 负责审查、集成和 CI。后续按 Bug 流程交给 Vega 前，需明确进入执行并发布带版本的可访问交接，不因本诊断文档自行开工。

## 新周期的时序、命令与交接

1. 复核最新 master，生产尚未修改时一次写齐 B01–B05 与必要外部边界 fixture；记录测试先行提交/完整SHA/内容hash。
2. Codex 审查并建立新的 draft PR。**仅云端**使用现有 workflow：`cargo nextest list --workspace --message-format json` 后正常 `cargo nextest run --workspace`，同时保留 fmt、全目标Clippy、Python三组和 required check。
3. 有效 RED 必须实际执行真实 ListState 场景并因上述目标借用冲突失败；本轮不能以人为缺失API、无关编译/依赖/runner故障替代。测试接线错误先留证修正，不碰生产。
4. 固定全部新测试目标/源码/hash及实际RED后，才实施上节生产改动；相同断言云端 GREEN，记录实际发现/执行/通过/失败/跳过，而非预填5通过。
5. 在新 PR 固定原始测试/RED/GREEN，再独立删除本轮 Rust 测试及专用支持；最终 head 全门禁成功且明确no Rust tests（0发现/0执行）后才正式PR/squash合并，再In review等用户打开真实会话手测。

本地只允许快速差异/格式/静态检查，不运行Vega、不编译、Clippy、测试或打包。CI每5分钟查询，通知或用户即时要求除外。不要恢复原已删除42项全套测试，不重试旧head换绿，不新增真实网络/浏览器E2E门禁。

## 当前交接与止点

- 当前阶段：诊断与方案已记录；Project In progress，当前执行者 Codex，下一执行者 Codex（安排新周期），不是 Vega 已领取。
- 本轮：只改本页和索引/交付记录文档；生产和测试相对最新 master 零差异；未建立新 draft PR，未取得本轮 RED/GREEN，未修复或发布。#309 由用户说明暂停，未操作其代码或启动状态。
- 用户手测：先重新打开原会话验证首帧不崩，再验证滚动/缩放/切会话/预览；保留两次既有崩溃证据。若修复回归，整体撤销该修复提交或按新证据重开，不能只隐藏卡片而保留危险读取。
