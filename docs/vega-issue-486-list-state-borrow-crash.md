# #486 · 真实消息列表预绘制借用崩溃诊断与下一轮方案

2026-10-19。用户手测未通过，#486 从 In review 退回 In progress。最初按用户“只做诊断”记录静态根因和独立验收变化；用户随后授权立即按 Bug 流程修复。当前执行边界及交接见下一节，不启动 #309。

需求 v1 的链接外观、完整预览及交互范围不变。本页修订技术方案 v2 的滚动状态采样时机，并为遗漏的真实 ListState 场景重新建立先测后修周期；不回填原 42 项测试来追认原交付。

## 2026-10-19 执行授权更新

用户先授权快速修复，发布可执行方案后曾置 Ready 等待 Vega；随后明确指示**“你直接实现 不用等 vega”**，覆盖本卡默认执行路由。本页仍为**崩溃修复方案 v1**，B01–B05 的目标、数据与预期不变。Codex 直接组织专职子 agent 执行，主会话负责审查、Git/PR/CI 与集成，不再等待 Vega。第一阶段只写齐测试与必要开发接线；真实列表场景的云端有效 BorrowError RED 经主会话核实前，不移动生产 `.stamp()`。

该专职子 agent 在上一止点只做接口核对，未写测试或生产代码；现按用户最新授权恢复测试阶段。Project 已接回 In progress，当前执行者 Codex（专职子 agent 独占代码），下一执行者 Codex（测试审查、云端 RED 和后续集成）。先前 Ready/Vega 交接及旧测试证据作为历史保留，当前状态以本段、Issue 最新交接和[崩溃修复交付记录](vega-issue-486-crash-repair-delivery.md)为准。

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

以下为冻结的五项行为目标，实际分阶段执行结果见[交付记录](vega-issue-486-crash-repair-delivery.md)，目标数不冒充发现、执行或通过数。Rust 测试用行为命名，不写任务编号。

| 目标 | 前置与动作 | 必须观察到的结果 |
|---|---|---|
| B01 首帧真实消息列表 | 用 `ConversationStream::new` 与 `apply_history_page(HistoryPage)` 注入合成 AssistantText/UserText，含显式 Markdown 链接和裸 URL；真实 Render，根层最后挂 MarkdownLinkPreviewLayer；不移动鼠标到链接、不等待 hover | 首帧和重复帧完成，不 panic；链接仍真实呈现/可命中，不能略过登记蒙混通过；0 预览请求、0 外部打开 |
| B02 虚拟化可见性 | 同一真实 ConversationStream 加足够历史项，使含链接项先不可见，再用生产 ListState 在渲染外滚入/滚出；覆盖跨行或表格内链接 | 首次可见、滚出再进入均不 panic；没有 hover/focus 就不请求；不得以普通 ScrollHandle 容器替代 list |
| B03 列表滚动关闭 | 真实列表链接 dwell 达400ms，受控外部元信息完成显示卡；分别滚轮和程序改变真实 ListState，指针保持原位 | 本帧关闭并取消订阅；迟到结果/静止指针不重弹；Open/目标保真仍有效 |
| B04 相同滚动状态重绘 | 真实列表显示卡后重复刷新/物化相邻流式内容，保持目标和实际滚动签名不变 | 不误关/误重载；快照既不能永久为空，也不能沿用上一帧失效值；仍无借用 panic |
| B05 普通阅读面回归 | 生产 FilePreview/ScrollHandle 阅读宿主显示同类链接，按需打开卡再滚动 | 首帧正常且0请求；400ms才请求；滚动关闭，图片/选择/键盘接线不被采样调整破坏 |

B01/B02 必须通过真实 ConversationStream、生产 `apply_history_page`/消息物化和 GPUI list，保留同一 ListState 的外层 `track_list` 与内部 `list`，不能复制一个不持有借用的假列表，不能直接单独调用链接 helper。GPUI 使用测试平台；mock 仅隔离网络/时钟/外部打开，不能替换滚动读取、消息渲染或协调器决策。

每项验证稳定可观察的帧、卡片、网络副作用或生命周期。无 hover 时0请求是产品契约，须与完成真实列表渲染结合，不能只验证 mock 次数。必要的 cfg/dev fixture 或只读观察须最小化且共用真实 role/label/状态；不得为测试添加另一套生产判定。B01 不能使用 should_panic/catch_unwind 把旧崩溃标成通过。

## 专职子 agent 第一阶段：先行测试与允许的临时接线

下列接口已静态核对，可以直接据此编写测试；不因等待新的接口许可而扩大生产实现。若仍遇到实际 API 冲突，按执行总纲在同卡交回 Codex，不自行替代真实列表。

- 在 `crates/vega_ui/src/conversation_stream/mod.rs` 注册新的 cfg(test) 子模块，测试放 `conversation_stream/link_preview_tests.rs`。用 `ConversationStream::new` 创建实体，经 `content.rs::apply_history_page` 注入 `HistoryPage`，克隆该实体真实 `list`，调用生产 Render。测试宿主只装配同一实体和最后的 `MarkdownLinkPreviewLayer`，不复制消息物化或链接判定。B05 调用 `crates/vega_ui/src/file_preview.rs` 的真实 FilePreview。
- `crates/vega_conversation/src/link_preview.rs` 的现有 `with_ports` 工厂及 `link_preview/ports.rs` 的 resolver/transport/clock 保持原语义；允许新增仅 `cfg(test)` / `test-support` 生效的重导出与工厂包装。新的有界脚本 fixture 只实现这三个外部边界，不复制地址策略、缓存、取消或服务结果判定。`crates/vega_conversation/Cargo.toml` 的临时 feature 与 `crates/vega_ui/Cargo.toml` 的开发依赖接线限此用途。
- UI 开发依赖允许启用 pinned `gpui_kit` 的 `test-support` 以及上述 conversation 测试 feature。在 `crates/vega_ui/src/link_preview.rs` 仅新增 cfg(test) 的服务/opener 注入、实际已登记链接几何/焦点/role/label 的只读观察；可给实际卡片控件补最小稳定选择器。观察必须来自真实登记/控件，不伪造 visible、scroll 或卡片终态。生产采样和协调器决策保持未修复状态直到有效 RED。
- 依赖不升级。测试 feature 如需恢复此前同版本的锁条目，仅允许 `bit-set 0.8.0`、`bit-vec 0.8.0`、`convert_case 0.11.0`、`proptest 1.11.0`、`proptest-macro 0.5.0`、`quick-error 1.2.3`、`rand_xorshift 0.4.0`、`rusty-fork 0.3.1`、`unarray 0.1.4`、`wait-timeout 0.2.1`；现存包身份、版本和 checksum 不变。其它依赖缺口交回 Codex。不得恢复原 42 项测试集合或其全部支持。
- 本阶段只交测试补丁、必要接线和证据记录，不修复生产。一次写齐 B01–B05 后记录每份源码 SHA-256、生产投影未改证据、快速静态检查及未运行项，向主会话交回并停止写入。主会话审查、提交完整 SHA，并在同一卡建立新的 draft PR 运行 RED；原已合并 PR #491 不复用为新修复 PR。

## 通过有效 RED 后的最小实现计划

允许生产修改仅为 `crates/vega_ui/src/link_preview.rs` 的采样时机；若真实根层顺序检查发现冲突，交回 Codex 修订，不自行扩大范围。

1. `prepaint_link_targets` 只登记可见几何、目标、焦点和 scope 的可克隆句柄。删除此子项路径的即时 `.stamp()`；此时不读取任何 ListState 滚动位置。
2. 在窗口根 `MarkdownLinkPreviewLayer::prepaint` 的现有 `entity.update` 内，**将 staged 换入 visible 和比较旧/新 scroll 之前**，从 staged 链接保存的 scope 句柄生成该帧拥有的 ScrollStamp 值。该层在 [VegaWindow 根](https://github.com/puzige/vega/blob/b32ad22e530d27925e375df00b6bd401bf9646a3/crates/vega/src/window/render.rs#L497) 是最后的 sibling，消息列表的子项 prepaint 已返回，ListState 的可变借用已释放。
3. root 采样使用列表完成布局/自动定位后的状态，当前帧即完成比较。不能只在列表布局前采样而错过本帧内部定位，也不能延后一帧掩盖关闭时序。历史 visible 保留上一帧拥有的值；新值解析完成后再走既有 scroll/liveness、抑制重入、取消和焦点恢复逻辑。
4. 对 List、ScrollHandle、None 统一保留原 ScrollStamp 含义；子项登记不取状态，根层解析后发布。保留普通文本流更新与滚动的区别，不修改图标、预览服务、网络策略、400/150ms、点击/复制或键盘行为。
5. 不改 GPUI、不升级依赖、不改 CI/发布、不加全局开关。禁止用 try_borrow 失败回退 None、吞 panic、跳过链接登记或永久取消滚动检测来“修复”。

本计划已在真实 BorrowError RED 固定后由同一专职子 agent 实施，静态差异仅为子项延后登记快照、根层统一采样；详见[实现阶段](vega-issue-486-crash-implementation-stage.md)。主会话已审查，待同一冻结测试云端 GREEN，不能把静态差异核对当作行为验证。第一阶段的测试授权未用于提前生产修复。

## 新周期的时序、命令与交接

1. 主会话接回前 fetch，并核对 Issue 最新交接、分支与工作目录，确认没有其他活跃写入者后置 In progress 并读回。复核最新 master，专职子 agent 在生产尚未修改时一次写齐 B01–B05 与必要外部边界 fixture；主会话记录测试先行提交/完整 SHA/内容 hash。基线漂移不得覆盖用户改动或旧证据。
2. 子 agent 先交回测试，主会话审查并建立新的 draft PR。**仅云端**沿用 `.github/workflows/pr-check.yml`：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、Python 三组及 `cargo nextest list --workspace --message-format json` 后的 `cargo nextest run --workspace`；required check 仍为 `check (fmt, clippy, test)`，不改 workflow。
3. 有效 RED 必须实际执行真实 ListState 场景并因上述目标借用冲突失败；本轮不能以人为缺失API、无关编译/依赖/runner故障替代。测试接线错误先留证修正，不碰生产。
4. 固定全部新测试目标/源码/hash及实际RED后，才实施上节生产改动；相同断言云端 GREEN，记录实际发现/执行/通过/失败/跳过，而非预填5通过。
5. 在新 PR 固定原始测试/RED/GREEN，再独立删除本轮 Rust 测试及专用支持；最终 head 全门禁成功且明确no Rust tests（0发现/0执行）后才正式PR/squash合并，再In review等用户打开真实会话手测。

本地只允许快速差异/格式/静态检查，不运行Vega、不编译、Clippy、测试或打包。CI每5分钟查询，通知或用户即时要求除外。不要恢复原已删除42项全套测试，不重试旧head换绿，不新增真实网络/浏览器E2E门禁。

## 可访问交接与停止条件

- 生产基线：`b32ad22e530d27925e375df00b6bd401bf9646a3`；前一轮仅诊断提交：`181cabf18e7b10d7cbb19e5f2467493eadbc6ab3`。本方案交接提交的完整 SHA 与不可变链接发布在 [Issue #486](https://github.com/puzige/vega/issues/486) 最新有效交接。
- 任务分支：`feat/486-markdown-link-preview`。优先复用 `~/Workspace/worktrees/vega-486-markdown-link-preview`，核对没有活跃执行者后单一写入；不在主 checkout 或 master 修改。不同宿主没有该目录时，从同一已推送分支建立独立 worktree，不能用旧 ref 恢复整套测试。
- 用户最新授权后由 Codex 直接执行，Project In progress，不再等待 Ready/Vega。子 agent 遇到方案/API/依赖缺口先交主会话审查，保持单一代码写入者，不自行扩大行为范围；主会话负责修订与同卡回报。不要新建卡或转做 #309。
- 接回基线 `edeed51746fadd029e179d8e271fbb063ac8b836` 只有文档变化，生产和测试相对最新 master 零差异；本轮 5 个新测试提交于 `4b60f0b28eb1c7734d51dc1003d9141d45515872`，新 Draft PR #493 已取得真实列表有效 RED，随后才授权生产调整。后续阶段证据写入交付记录；原 42 项通过不能代替本轮证据。
- 修复合并并经云端发布后，用户先重新打开原崩溃会话和含显式/裸 URL 的新会话，验证无 hover 首帧、重复进入和滚入链接均不崩；再检查 400ms 预览、滚轮/程序滚动立即关闭、静止指针不重弹、Open/键盘及普通文件阅读回归，保留既有崩溃证据。未经手测通过不关闭 Issue、不 Done、不清理本卡资源。
- 若修复引入回归，按本轮新 PR 的完整集成提交整体 revert 并走 PR/云端发布；不要只隐藏卡片。回滚到含缺陷基线会恢复本次已知崩溃，须保留 Issue 并由 Codex 按新证据重开周期。
