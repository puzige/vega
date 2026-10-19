# #486 崩溃修复：先行测试阶段

2026-10-19。依据[崩溃修复方案 v1](vega-issue-486-list-state-borrow-crash.md)冻结 B01–B05。用户最新要求由 Codex 直接实现，专职子 agent 执行本阶段测试，主会话审查、Git/PR/CI 与集成。本阶段**仅完成测试及必要开发接线；生产借用冲突尚未修复，云端 RED/GREEN 尚未取得**。

## 基线与冻结内容

- 生产基线：`b32ad22e530d27925e375df00b6bd401bf9646a3`。
- 编写前分支 HEAD：`edeed51746fadd029e179d8e271fbb063ac8b836`。fetch 后与远端任务分支相同，ff-only 更新无新提交，工作树初始干净；HEAD 相对 master 仅既有文档差异。
- 本轮新写 **5 个 GPUI 测试定义**，不是发现数、执行数或通过数；未恢复旧 42 项集合。测试提交待主审审查后固定，不预填未产生的 SHA。

| 测试源码/fixture | SHA-256 |
|---|---|
| `crates/vega_ui/src/conversation_stream/link_preview_tests.rs`（558 行） | `f369318c1875c0cf358e6b561a2daacc39828f2befb9a421564fab5a63fd19b1` |
| `crates/vega_conversation/src/link_preview/test_support.rs`（183 行） | `a83eba45519905a4a2e948500734d94d9d6142b93c8810eb0d8c31a6a3a0e331` |

主审冻结后保留上述目标、输入、时间和断言。若接线本身错误，先固定原始失败证据并审查具体修正；不把无关编译或 runner 错误称为本轮有效 RED。

## 行为映射

| 目标 | 新测试定义 | 真实边界与断言 |
|---|---|---|
| B01 | `conversation_first_and_repeated_frames_register_links_without_preview_requests` | `ConversationStream::new` + `apply_history_page` 物化真实 UserText 裸 URL、AssistantText Markdown 链接；首帧及重复帧完成，真实 ListState 有正视口且两个链接的角色、标签、目标、文本/图标命中几何完整；无 hover、请求、DNS 或外部打开。 |
| B02 | `virtualized_wrapped_and_table_links_register_when_revealed_and_reentered` | 同一真实列表的 81 项历史；链接项初始不可见，使用该列表 `scroll_to` / `scroll_to_end` 滚入、滚出、再次进入；跨行链接多段几何及表格链接实际登记，未 hover 时始终无请求或打开。 |
| B03 | `list_wheel_and_programmatic_scroll_close_loaded_and_pending_previews_without_reopening` | 真实列表中 wheel/programmatic × pending/loaded 四路径；399ms 无请求、400ms 显卡，Enter 保真打开完整目标；滚动期间鼠标坐标不变、实际 ListOffset 改变，本帧关卡；pending 端口取消，释放迟到响应并再推进 1000ms 后不重弹、不重请求或重复打开。 |
| B04 | `unchanged_list_scroll_preserves_preview_during_redraw_and_neighbor_stream_materialization` | 真实列表已有卡片后刷新，通过生产 MessageStarted/TextDelta 物化相邻回复；检查实际物化文本、前后 ListOffset 相同、同一链接身份与卡片保留、标题正确，继续计时无重载或打开。 |
| B05 | `file_reading_keeps_delay_thumbnail_selection_keyboard_open_and_scroll_close` | 生产 FilePreview 内原有 ScrollHandle；首帧零请求、399/400ms 边界，经过真实预览服务读取 HTML/PNG 并显示正确比例缩略图；真实拖选复制投影、链接焦点 Enter 保真打开、实际阅读区滚轮关闭，静止指针不重弹或重请求。 |

宿主只装配真实 ConversationStream 或 FilePreview、TextSelectionLayer，并把 `MarkdownLinkPreviewLayer` 放为最后 sibling。没有测试版 list、ScrollHandle 替代列表、协调器决策或滚动值。ConversationStream 的生产 Render 同时持有内部 `list(self.list.clone())` 与外层 `track_list(self.list.clone())`；测试只在渲染之外克隆和操作这个 ListState。

## 外部边界与临时接线

- 新 conversation fixture 只实现真实服务的 resolver/transport/clock ports：固定公开 DNS 响应、有限脚本 HTML/PNG 字节及可释放响应闸门、受控时间；记录真实请求/取消 token。测试仍使用现有私有 `with_ports` 创建的 LinkPreviewService，未复制 URL 安全、解析、缓存、取消或结果判定。
- conversation Cargo 增加临时 `test-support` feature；`link_preview.rs` 仅增加 feature-gated fixture 子模块。正常 dependencies 表不变，不增加 UI image 边。
- UI Cargo 仅增加两个 dev 边：pinned GPUI `test-support`、conversation `test-support`。`conversation_stream/mod.rs` 仅增加 cfg(test) 测试模块。
- UI `link_preview.rs` 仅加 cfg(test) 服务/opener 注入及实际已登记链接 key/target/role/label/description/几何/focus 的只读观察。观察克隆生产 VisibleLink 中同样由焦点代理消费的值；不合成角色、滚动或可见终态。真实卡片、标题和图片补三处 `.test_support()` 观察标记，保留原 ID 与无障碍语义。
- Cargo.lock **914 → 924**：仅恢复获批的 `bit-set 0.8.0`、`bit-vec 0.8.0`、`convert_case 0.11.0`、`proptest 1.11.0`、`proptest-macro 0.5.0`、`quick-error 1.2.3`、`rand_xorshift 0.4.0`、`rusty-fork 0.3.1`、`unarray 0.1.4`、`wait-timeout 0.2.1` 历史同版本条目。保留 914 包的身份/来源/checksum 完全相同；现有包仅新增 GPUI → backtrace/proptest 两条测试 feature 语义边，其余仅同名多版本标签展开。完整离线 locked metadata 已确认图有效，无版本升级。

同一冻结测试 GREEN 且 PR 先固定源码/RED/GREEN 后，才可另行授权删除两份新文件、两处模块接线、feature、两个 dev 边、cfg(test) 注入/观察、三个标记及十个测试独占锁条目。保留全部生产 ports/工厂、正常依赖、角色/标签/ID/焦点、列表渲染、选择、预览与滚动能力。清理后再验证最终空集 gate，不能拿空集替代本轮行为 GREEN。

## 生产未修复证明

三处混合 Rust 文件按精确新增 cfg/测试观察投影移除后，与编写前 HEAD 逐字节一致；两个清单的正常 dependencies 表完全相同。`render.rs`、`inline_code.rs`、`content.rs`、VegaWindow 根 render、FilePreview、真实 IO ports、metadata 未改。关键保护哈希：

| 文件 | SHA-256 |
|---|---|
| `crates/vega_ui/src/conversation_stream/render.rs` | `2e5b1dd887dc3ccbd85a6c5b443be7839ab9e2c28857e032e2258cb9211f5d79` |
| `crates/vega_ui/src/conversation_stream/inline_code.rs` | `0599cb9e18f4cabcbca3bcc8e97f26fa37aa84b3f43514999a6cdb650bbbf723` |
| `crates/vega_ui/src/file_preview.rs` | `5098371e5896c6c016fe7d1a7035db941e1ecd9d68f675d80276ce9b9090a8c2` |

危险的 `scroll: scopes.iter().map(|scope| scope.scroll.stamp()).collect()` 仍在子项 `prepaint_link_targets` 内（新增开发接线后第 718 行），根层没有加入采样。预期有效 RED：真实 GPUI List::prepaint 持有同一 RefCell 可变借用时，该行经 `ListState::logical_scroll_top()` 触发 BorrowError。B01–B04 不 catch panic、不 should_panic、不跳过登记；必须由云端实际执行确认原始 panic，本地静态调用链不是已取得的 RED。

## 本地结果与交回

| 快速静态检查 | 结果 |
|---|---|
| `cargo fmt --all` / `cargo fmt --all -- --check` | exit 0 |
| `git diff --check` | exit 0 |
| `cargo metadata --offline --locked --format-version 1` | exit 0；仅解析依赖，未编译 |
| 源码定义数、hash、无新增注释/任务编号、无 ignore/catch/should_panic | 静态通过；5 定义 |
| 生产投影、保护字节、依赖身份与语义边 | 静态通过 |

未本地运行 Vega、编译、构建、Clippy、测试、benchmark 或打包。未 commit/push、开 PR 或操作 CI；主会话自己的修复方案/交付文档修改保持原样。

**交回主审，停止写入。** 主审审查并固定测试提交/源码后建立新的 draft PR，沿用现有 fmt、Clippy、Python、Nextest 与 required check。全部发现/执行/通过/失败/跳过计数及有效 RED 仍为**待 CI**；只有实际 BorrowError 经核实并明确授权后，才能调整生产采样时机。修复后的布局、选择及异步 UI 断言也须由相同测试实际 GREEN，不能把本轮静态核对当作类型或行为验证。

## B04 前置布置修正

首次带修复尝试为 `cfb47565b20005302a9f370cdf1f2e1f2ef01551` 的 [Actions 39223251294](https://github.com/puzige/vega/actions/runs/39223251294)，[Nextest 117963298328](https://github.com/puzige/vega/actions/runs/39223251294/job/117963298328) 实际 5 执行、4 通过、1 失败、0 跳过；B01/B02/B03/B05 通过，没有 BorrowError。B04 在原滚动签名相等断言失败：实际从 `(0, 0px)` 变为 `(0, 18.5px)`。fmt、全目标 Clippy、Python 34 项通过。[原始失败先行留证](https://github.com/puzige/vega/pull/493#issuecomment-6263165467)，不是完整 GREEN。

主审与执行方只读核对 pinned List::layout_items：短内容抵达底部时会重新启用 tail follow，邻接内容增长后产生真实滚动；输入栏测得高度也会更新列表底部 padding。原 B04 首消息仅重复 10 段，没有稳固满足“实际滚动状态不变”的既有前提。根据排版 token 估算，原首消息约 424px，改为 40 段后约 1579px，足以超过 720px 宿主视口；这些数值是静态估算，不是运行测量。邻接回复仍由 ConversationStream::render 的前置 model.sync 真实物化，现有物化文本断言保留。

[具体修正经主审事先公开批准](https://github.com/puzige/vega/pull/493#issuecomment-6263202623)后，仅将 B04 的 `"\n\nStable reading paragraph.".repeat(10)` 改为 `.repeat(40)`。原目标、操作、计时、所有断言及其余四项测试不变，未写入滚动值或协调器终态，没有放宽实际滚动检测。全部生产代码、接线、外部 fixture 和 Cargo 文件逐字节不变。

上表旧冻结 SHA-256 保留为原始来源；该一处批准修正后的 UI 测试 SHA-256 为 `de18c58d1563b6eb797aa68286d2e2e4a75bdc5cbb66c916fd54048853624965`。fixture 仍为 `a83eba45519905a4a2e948500734d94d9d6142b93c8810eb0d8c31a6a3a0e331`。

修正前已 fetch/ff-only 更新，HEAD/远端任务分支均为上述 cfb47565，master 仍为 b32ad22e，执行前源码与 HEAD 一致；主会话的交付文档修改保留。最终 fmt、diff、精确单字符串替换及冻结 fixture/生产字节核对通过；未本地编译、执行测试或运行 Vega，执行方未 commit/push。新的 head 云端结果仍待主审取得；本次仅修正测试前置布置，不声称已完整 GREEN，不授权提前删测。
