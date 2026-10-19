# #308 — 新消息置前与手动拖动统一顺序

版本：v2.6（原冻结契约），2026-10-19。当前修复交接及状态见 [v2.7：GREEN 后审查修复与交付](vega-issue-308-review-repairs.md)。以下原41项契约、历史基线与证据保留；迁移编号以v2.7顺延0029为准，不再使用已被master占用的0028。
Issue：https://github.com/puzige/vega/issues/308
RED 生产行为基线：`ae35310fe7e1ddac2b79d7ec7bc55bcca2d6af2a`（#483）；首轮 CI 的历史基线为 `6c782a630ceb4c83f4f6b1e09bdc645ac8e87092`。
任务分支：`feat/308-new-thread-order`；复用本卡保留的 sidebar-reorder worktree，原 `feat/308-sidebar-reorder` 分支保留。

本轮是原卡用户反馈后的独立验收修订。用户明确取消更新时间/创建时间/手动排序模式切换，改为“有一条新消息就移到顶部，同时允许手动拖动”。本文件取代 [v1.1](vega-issue-308-sidebar-reorder.md) 的时间排序及三种模式契约；旧报告仅作历史证据。先前讨论的“只把未登记会话改按创建时间排序”方案未实施，也不再是本轮方案。其余项目归属、置顶、归档、分页、导航和拖放安全契约沿用。

v2.1 采纳 Vega 的 Rust 存储入口建议，取消 v2.0 的消息 INSERT 触发器。用户随后裁决“排序失败保留消息，toast 或直接吞掉均可”；本轮采用静默降级，排序失败保留原顺序并继续正常消息流程，不新建提示回执或后台补偿链路。M4 的失败预期因此已变更，不能把它当作仅替换实现位置，也不能沿用 v2.0 测试冻结。v2.1 发布时尚无测试或生产补丁。

v2.2 接回 Vega 的[未完成测试检查点](https://github.com/puzige/vega/issues/308#issuecomment-6253691406)，保留其 29 个新测试定义并迁移到 #476 清理后的基线；不恢复历史侧栏测试集合。本版产品排序/失败契约不变，明确 M6 名称的手测证据、M7 真实回执观察边界、Plan 暂无 assistant start 的场景和重复 start 的既有错误语义。检查点尚未冻结，无 RED/GREEN；Vega 可按本版修改尚未完整的测试，不能据测试数声称验收已齐。

v2.3 处理 02:58 合入的 #482：该提交在 v2.2 基线核对后删除现存测试及独占支持，同时改为“开发保留到同批 GREEN，交付前独立删测”。这次确实删掉应用 `tests.rs/diff.rs`、provider mock 和 dev 接线，v2.2 的“helper 仍保留”只对 `b3c2ffaf` 成立。Codex 已重接到最新基线，保持删除结果，列出下文可恢复的最小支撑；不把已知支撑缺失的编译错误算作 RED。

v2.4 处理 Vega 的[错误观察范围报告](https://github.com/puzige/vega/issues/308#issuecomment-6254375832)：M7 已要求断言 Plan 启动阻塞与重复 start 的既有错误，原只读字段清单却只列 entries/identity/text。明确批准 `ConversationStream::controller_error_message() -> Option<&str>`，实现仅为 `self.controller_error.as_deref()`；不改错误/事件语义，不读 DB、不注入状态，也不新增依赖或通用测试接口。该方法仅供本卡跨 crate 测试观察，带测试 GREEN 固证后的独立删测阶段删除。验收目标与预期不变；现存工作区补丁仍未冻结，无 RED/GREEN。

v2.5 由 Codex 接回 [Vega 的 #479 基线漂移报告](https://github.com/puzige/vega/issues/308#issuecomment-6254640797)，保护全部测试进展后同步 `6c782a63`。完整保留上游复制入口、Final 缓存、代码块原文、Tab 遍历和选区菜单生命周期修复，不恢复该卡已删除的 13 项测试或独占支持。三个只读方法继续从实际 Assistant entries/identity、`copy.original_text()` 与 controller_error 读取；这些字段和原文接口在新基线未改，新增 Final 复制缓存独立维护。M7 正文、终态、健康流/错误/replay 的目标与预期不变，M6 的 Sidebar + TextInput/组件 Root 夹具不经过 ConversationStream 的 Composer Tab 遍历。Vega 已停写，剩余基线集成、两处格式修正与最终审查由 Codex 组织专职子任务完成，直接进入 draft PR / RED。当前 41 个源码测试定义（conversation 24、M6 7、snapshot 3、app 7）仅表示静态清点；发现、执行与通过计数待 Actions。

v2.6 固定 [首轮 CI 与接线审查](https://github.com/puzige/vega/issues/308#issuecomment-6254877233)：[测试提交 f72d590c](https://github.com/puzige/vega/commit/f72d590cfcf2ad8eba654d62c8caadb62838a856) 的 [Actions 39163406130](https://github.com/puzige/vega/actions/runs/39163406130) 实际执行 41、通过 2、失败 39、跳过 0；其中 17 个界面用例受初始 bounds 接线问题阻断，Clippy 另报一个借用切片 lint，不能整体冻结为有效 RED。Codex 批准下表最小观察修正，保留全部测试定义及业务断言。同步新 master `ae35310f` 时保留 #483 的后台完成未读、系统通知和回执路径，不恢复上游测试；仅把完成后的期望快照明确包含该既有未读变化，完整顺序和其他字段仍逐项保持。已有 CI 来源通过保留祖先的合并提交继续可访问，不 rebase/force-push 丢弃。

本轮执行切片明确交给 Vega：先写齐以下测试并交回 Codex；Codex 用测试先行 draft PR 确认有效 RED、冻结后，Vega 才修改生产实现。遵守 [执行总纲](vega-exec-guide.md)、[工作流](vega-issue-workflow.md) 和 [测试开发/交付生命周期](vega-rust-test-delivery-lifecycle.md)。本地不运行 Vega、不编译、打包或执行任何测试。

## 验收矩阵（先于实现计划）

以下均为待测试/CI，不能沿用上一轮通过结果。测试使用 owned 文件数据库与进程内 GPUI 上下文，不连接真实模型、外部进程或用户数据。预期 ID 顺序由场景直接声明，不通过复制生产排序算法计算。

| ID | 前置与操作 | 可观察结果及必须保留的边界 | 测试层级/预期 RED |
|---|---|---|---|
| M1 | 已有任意顺序 A、B、C；依次打开 C、B，重命名 A、修改模型/权限，并刷新 | 会话顺序保持；打开仍正常清除未读、更新已有访问元数据；内容、绑定和置顶语义不变。包含原来没有 rank 的新会话、旧 Updated/Created 偏好 | conversation 公共 API + 排序服务；旧实现会随 updated_at 重排 |
| M2 | 项目内、已固定、最近任务分别有三条会话，初始顺序 A、B、C；通过 `messages::insert` 向 C 首次提交新 user 消息，再向 B 首次提交新 assistant 消息；另通过 `review_plan` 批准 Plan 并插入执行指令 | 顺序依次 C、A、B 和 B、C、A，Plan 批准指令同样将源会话置前；同区其他项及其他区域相对顺序不变。隐藏子会话消息不改顺序/revision；已固定的可见子会话只移动自身，不移动父会话或项目。消息时间相同/早于旧记录也按真实提交先后生效 | 两个 public store 消息 API → conversation snapshot/投影；覆盖父/子可见性、归属、pinned、归档状态及项目列表不变 |
| M3 | 新消息使 C 置前；随后把 C 拖到 B 之后；继续更新同一 assistant 消息正文、响应片段、工具/usage、终态并重放 UI 事件；最后插入另一条新消息 | 拖后位置保持到下一条新消息真正提交，之后才再次置前；不按 delta/完成回执/同消息内片段反复置前。包含重开 DB、归档隐藏邻项和未渲染邻项，完整顺序不丢项 | store + service 真实事务及排序；公共消息 ID 与拖动动作，不 mock 被测决策 |
| M4 | 分别经 `messages::insert`（autocommit 连接及外层事务）与 `review_plan` 写消息；测试重复 ID/非法消息、外层 rollback；在 owned DB 注入仅侧栏 UPDATE 失败、损坏排序 JSON、revision 上限；另测两连接先后提交消息/拖动 | 消息本身失败或外层回滚时不留下消息或置前。仅排序失败时 API 仍报告消息成功，提交后消息可读，Plan 状态与执行指令正常保留，排序/revision/其余元数据不变；不弹发送失败或异步重放失败的置前。排序成功且新消息先提交时旧 revision 拖动拒绝、可刷新重试；拖动先提交则后续新消息置前；故障解除后下一条新消息正常置前 | public store/service + 文件数据库；错误注入只作 fixture，操作仍走公共 API。成功路径验证同笔提交，排序降级路径验证消息保留而无半写元数据 |
| M5 | 旧 schema 27 文件库包含 Updated、Created、Manual 偏好（逐案）、部分手排 rank、已归档/已删除邻项；升级并重开 | 升级一次建立完整、去重、有效 ID 顺序；各区域可见相对顺序按旧版本当时契约保留，已有手排不丢；以后点开不变，新消息/拖动按新规则。旧 JSON 可读，无消息内容/时间或绑定修改 | migration → service 文件库；测试先构造真实旧 schema，不以源码扫描充数 |
| M6 | 挂载真实侧栏，键盘/鼠标操作原排序位置的归档按钮与会话菜单 | 无更新时间/创建时间/手动排序入口；归档按钮有可访问名称、选中态，能显示/隐藏归档项；上移/下移、Esc、普通点击和草稿正常，未加常驻说明 | 自动：GPUI 输入、实际行/背景/焦点与草稿；名称：实现属性审查及用户 VoiceOver 手测，未取得前明确待验。不能把键盘可达性或不存在的 a11y 树当作名称证据 |
| M7 | 有效 active generation 下，后台会话的新消息已提交，进入生产 `MessageStarted` batch；新消息间穿插拖动；另按下节分别投递旧组织快照、旧 agent generation、Plan 批准回执、流式更新及排序失败后的正常消息回执 | 排序成功后实时刷新侧栏，不需用户切换/重启；只刷新已提交顺序，不由 UI 再做一次置前写入。排序失败不改成发送失败，消息流继续且真实正文正确；不抢当前会话/项目/草稿，旧快照不能回滚较新 revision。重复 start 的既有协议错误不被本轮改写 | GPUI/controller 真实回执 → 文件 DB；旧 agent generation 与旧组织快照独立覆盖，断言真实行顺序、消息投影、错误及导航终态 |

本轮测试目标在 RED 前写齐。最新基线已清除全部现存 Rust 测试；不恢复旧 `tests.rs`、改名的旧模式用例或整套支持。原检查点对此前 6 个历史迁移/拖动/模式生命周期用例的修改随上游删除；本卡真正新增的 29 个定义保留，其正文/属性在两次基线适配中未变。需要的生命周期、菜单键盘、分页/归档邻项与持久化边界按 M1–M7 独立验证，不能因旧文件删除而声称已有覆盖。后续补齐属于冻结前测试阶段，实施后不得追加测试追认或修改期望。

## 测试检查点的解除阻塞方案

### 基线与最小接线

- 原检查点 `1bdbc2e1bfb2d701ef66599c74fb33b6e674698e` 基于 `3a97f4d9`，v2.2 检查点 `7b163ad0f91f638ac0f7b770e19b0bf97f75eb2b` 基于 `b3c2ffaf`，均保留为此前评论的来源证据。本轮先同步 #482，再 rebase 到 `6c782a63`；首轮 CI 后以保留该来源的 merge 同步到 `ae35310f`，保留 #476/#482 的删除结果和 #480/#479/#483 实际生产变更。先读 [生命周期](vega-rust-test-delivery-lifecycle.md) 与[合并纠偏历史](vega-rust-reset-reconciliation.md)，不以解决冲突恢复已清除集合。
- conversation 的 `message_order_tests.rs` 直接在 `sidebar_organization/mod.rs` 的 `cfg(test)` 下接线，内部只提取本卡实际需要的 `open/change/new_group/ordered_ids`；failure 子模块保留，legacy 4 案独立接线并重新静态审查真实 schema 27 fixture。
- UI 使用 `organization/archive_controls_tests.rs` 中最小 mounted-sidebar fixture 承载 `tests/archive_controls.rs` 两个新用例；不重建已删除的历史 `organization/tests.rs`。原菜单/上移下移/Esc/分页边界如缺覆盖，Vega 按本矩阵补新的有区分度行为用例，不批量移回历史测试。
- 应用历史 `tests.rs/diff.rs` 已在 #482 删除，rebase 冲突保留删除，不新建旧根模块或恢复其测试。将原全局初始化中本卡实际需要的少量注册移入 `artifact_message_order_tests.rs` 本地 fixture，仅用 owned DB/config 及进程内 GPUI。现有 `SidebarConfigPath` 导入不是可用公共 API（在 `b3c2ffaf` 也不存在），删除该错误导入/设置，不为此新增同名生产全局或读取用户配置。侧栏必要 globals 由 fixture 显式初始化。
- M4 的故障 fixture 先用公共边界预检：侧栏 UPDATE 的 ABORT 应由真实组织 `apply` 返回指定错误标记且事务回滚，坏 JSON 应使 `snapshot` 返回存储/解码错误，revision 上限由 `apply` 返回 Limit。随后才执行保留消息及下一条新消息恢复场景。旧基线本来就没有尝试置前，因此“故障中消息保留”可能先通过、直到故障解除后的新消息预期才 RED；报告真实失败位置，不能把 fixture 预检或恢复失败写成已观察到消息入口中的排序错误捕获。不为探测调用次数新增生产遥测或依赖。
- 延迟外键 COMMIT 故障先确认连接 `foreign_keys=1`、fixture 的延迟约束有效，针对 autocommit/外层事务/Plan 三入口断言具体 SQLite 外键错误（Plan 解包其 Store），不能只用宽泛 `is_err` 让无关错误满足条件。区分“COMMIT 错误保持传播”的保底断言与之后恢复消息置前的 RED，不声称当前基线已吞掉 COMMIT 错误。

### #482 后允许的最小开发支撑

以下由 Codex 明确批准，只供本卡开发期使用；全部单列清单，GREEN 后按生命周期删除无生产消费者的部分。不能因此恢复历史测试、通用 HTTP/provider mock、旧共享测试模块或修改安全校验。

| 位置 | 允许动作 | 边界 |
|---|---|---|
| `vega_conversation/Cargo.toml` | 恢复 workspace `tempfile` 的 dev dependency | 本卡 owned 文件库 fixture；不恢复 runtime test-support、Tokio test-util/多线程 feature |
| `vega_ui/Cargo.toml`、`vega/Cargo.toml` | 恢复现有 `gpui_kit` 的 dev-only `test-support` feature 与 workspace `tempfile` | 不恢复 runtime mock feature 或额外 UI/runtime 依赖，除非本卡实际引用可列证且先报 Codex |
| `Cargo.lock` | 为上述 feature 恢复原锁版本的必要测试支持包/依赖边 | 不升级现存包、不引入旧基线之外的新包；可用低 CPU 的离线 lock/metadata 解析，无编译。对照 `b3c2ffaf` 与新基线逐项记录恢复理由，不整份取回旧锁文件 |
| `window/mod.rs`、`window/session.rs` | 仅恢复 `cfg(test)` 的 owned config path override 字段/初始化和对应读取分支；测试构造窗口时跳过 updater 启动 | 生产条件投影不变，避免接触用户配置或真实更新网络。pricing 路径仍来自 owned DB，其他外部服务保持未配置/阻塞 |
| `artifact_message_order_tests.rs` | 本地最小 globals fixture，移除 `MockProvider`/`agent_provider_override` 设置及无用导入 | M7 手动驱动真实 ingress；Plan 在创建 worker 前由既有前置条件阻塞，不需要 provider mock。不得恢复 app_agent/provider 的测试绕过链路 |
| `conversation_stream/content.rs` | 允许现有 entries 数量、assistant identity/text 投影和 `controller_error_message() -> Option<&str>` 的最小只读查询 | 错误 getter 精确读取 `self.controller_error.as_deref()`，仅两个既有错误场景及健康流无错误断言消费；不改变字段、映射或事件路径。跨 crate 方法与其余测试独占观察在 GREEN 固证后独立删除 |
| `sidebar/mod.rs`、`threads_block.rs`、`organization/{render,menu}.rs` | 为本卡已查询的真实 sidebar-scroll、会话行/菜单/菜单项、通用控件、最近标题和项目分页元素增加与现有 ID 一致的 `debug_selector` | 锁定 GPUI 的 `debug_bounds` 不读 ElementId，只读该独立登记。只观察真实布局，非 test-support 构建中由 GPUI 自身 no-op；不改 ID、布局、焦点、事件、可访问性、排序或菜单，不新建 feature/API，GREEN 后独立删除 |

首轮接线修正只允许把 failure fixture 的单元素 clone 比较等价改为借用切片，不放宽/删除断言或添加 lint 豁免。#483 的 `finish_completion_attention` 在后台完成时持久化源会话 `unread=true`，现有 `threads::update` 不修改时间或组织 revision；因此 M7 完成后整份快照比较应从完成前期望克隆、只将源会话的期望 unread 设为 true，再完整相等比较。不能从实际结果反推期望、只比较 rank 来丢弃 metadata 断言、预设所有会话已读/未读以掩盖变化，或禁用真实终态/通知路径。replay 在完成后取的快照、Plan 无 assistant start 和 duplicate 场景保持原断言。

若发现未列出的编译依赖/支持缺口，给出消费者和最小差异后交 Codex，不能恢复整套旧支持。所有支撑只用于观察真实生产决策，不替代排序、fence、Plan 持久化或正文投影。

### M6 的名称证据

锁定的 `gpui-pre 0.3.4` 中，`TestWindow` 未实现 `PlatformWindow::a11y_init`，activation 回调被默认空实现丢弃；`Window::debug_a11y_tree_json` 不能提供实际 frame 树。本轮不修改依赖、不维护自制 GPUI platform、不把测试自己提供的字符串或源码搜索当作 mounted a11y 证据。

Codex 明确将名称证据分配给实现属性审查及用户手测，产品名称要求仍保留：按钮处于隐藏归档项状态时可识别“显示已归档”，显示后可识别“隐藏已归档”，名称与可用动作相符。Vega 的自动测试覆盖真实鼠标、Tab/Enter/Space、焦点、选中背景、三区归档项、导航/草稿及菜单行为。名称手测不阻止本轮自动行为 RED，但在最终交付报告中必须保持待验，用户确认前不关闭卡片或 Done；不能声称 M6 全部由自动测试验证。

### M7 的回执和正文边界

1. **晚到旧组织快照**：补 UI 层真实回执测试，不能用旧 agent generation 用例替代。允许将 `organization_request` 的现有 UI completion 闭包原样提取为模块内 `finish_organization_request` 与必要的 request token（generation、epoch、route、editor、was_mutation），dispatch 的生产任务和测试都调用这一份处理器。该提取只能移动既有逻辑，不改 pending/refresh、revision/generation/metadata 判断、错误或路由分支；逐项比对原闭包，作为测试前置纯结构调整单独列证，不能提前实现排序或刷新修复。
2. 测试用 owned DB 的真实 `snapshot/apply` 得到旧结果 S0 和更新顺序的 S1，以明确字面 ID 顺序为预期；通过已有异步 executor/oneshot 控制回执投递，在 S1 经同一生产处理器应用并渲染后才投递 S0。分别验证相同 generation/epoch 下较旧 revision 不回滚新顺序、旧 request generation 被拒绝、`begin_task_mutation/finish_task_mutation` 改变 epoch 时旧线程元数据不覆盖当前缓存且触发权威刷新。token/state 可作显式 fixture 前置，但不能在测试复制 fence、直接把新 snapshot 塞进缓存充作“已应用”，或用 drain 后普通拖动充作晚到结果；生产判断只由上述同一 completion 处理器执行。以最终行顺序/元数据/导航和 revision 断言为准，调用次数不是结果。
3. **Plan 暂无 assistant start**：在应用层驱动真实 `finish_owned_plan_review`（含匹配的 pending review），断言 approved 状态、Execute 模式、正式执行指令、实际侧栏顺序及前台草稿。使用现有可观察的运行前置阻塞（例如 `model_catalog_loading`）让后续 agent dispatch 在创建 worker 前返回，并确认没有新 assistant 消息/MessageStarted；预期启动错误可以保留，不能冒充排序失败或正常运行。为独立观察 UI 回执刷新，可在调用前通过真实组织 service 将数据库置为较新顺序但保留侧栏旧投影；Plan 插入自身的置前已由 M2 独立验证。不得只调用 `messages::review_plan` 或直接刷新侧栏替代该回执。
4. **正文与 replay**：正常流只发一次 start，后续发 delta/finish；在拖动和排序失败场景断言实际 ConversationStream 投影的 assistant 正文与预期文本一致，再验证终态、DB 和草稿。允许最小只读投影观察方法（优先已有复制/渲染观察边界；确需新方法时只读取现有 entries/identity/text，以及本版精确批准的 `controller_error.as_deref()`；不查 DB、不注入状态、不复刻事件处理）。支持跨 crate 的必要只读查询应明确记录，禁止建立通用测试接口。错误文本必须来自实际字段，不能用测试自己设置的字符串或 has_active_agent 等替代证据。
5. active generation 的同一 `MessageStarted` 重复到达目前触发 controller error，这是既有协议语义。本轮只要求它不新增消息或置前，若覆盖须明确断言现有错误，不能放在“健康流继续”样例中隐藏错误。健康 replay 使用已结束/旧 generation 的真实 Stale 回执，并确认当前有效流的正文/状态不被污染。不在本轮修改 duplicate-start 的生产语义。

以上属于冻结前观察边界修订，不增加排序产品行为。Vega 完成全部自动用例/接线与上述窄范围纯提取后交回；Codex 审查覆盖、断言、纯提取和实际生产差异，再建 draft PR。若仍有未解决的自动覆盖缺口，继续同卡回报，不靠编译失败或已有测试数量申请冻结。

## 行为契约

1. **一份顺序**：继续按原来的三个会话区域投影：已固定、各项目内、最近任务。新消息置前和手动拖动修改同一份持久顺序；消息/拖动以事务实际提交先后为准。项目列表仍只受原有项目拖动影响。
2. **何谓新消息**：本轮以正式 `messages` 历史中新 `user` 或 `assistant` 消息 ID 的首次成功 INSERT 为一次。assistant 的 streaming 行建立时计一次；以后 delta、同一行中的 commentary/final 片段、重试/完成/失败状态更新不再计。草稿、输入预测、尚在 pending queue 且未投递进消息历史的输入、历史读取、UI 事件重放和工具/usage/诊断记录不计；排队输入实际成为正式消息时计一次。已有消息 ID 的重复 INSERT 必须按原约束失败且不改变顺序。
3. **消息提交**：将该会话移到它所属区域首位；其他项保持相对顺序，不跨区域移动。支持正在后台运行的会话。沿用存储层的可见性条件 `parent_thread_id IS NULL OR pinned = 1`：隐藏子会话（有 parent 且未固定）的消息直接跳过置前及 revision 写入；已固定的可见子会话可在已固定区域移动自身。两者均不改变父会话或项目列表顺序；不能用“有 parent 一律跳过”误伤已固定项。
4. **手动操作**：上下半行、首尾、菜单上移/下移继续按完整区域顺序定位。一次拖动提交后，直到下一条新消息才会再次置前。持续流式输出期间可以拖动；后续相同消息的持久化刷新不得覆盖拖动结果。
5. **访问不是消息**：保留 `visit_thread` 清未读与原有访问元数据，不用取消 updated_at 更新修补侧栏，也不再以这些字段驱动侧栏会话次序。改标题、模型、权限及归档状态更新均不置前。
6. **新会话初始化**：首次正式消息将其置前；尚无消息而已出现在列表的新建会话以创建先后初始化在前面。创建时间仅作没有 rank 的新 ID 的确定性落位，不提供时间排序模式，不随访问时间改变；同毫秒用稳定 ID 打破平局。
7. **旧数据**：升级保留当时各区的可见顺序一次；旧 Updated/Created 仅用于这次初始化，不再持续生效。旧 Manual 使用保存 rank，缺失 ID 按旧版本当时的 fallback 补入，再清除已删除/重复 ID。归档隐藏项仍有位置，可恢复；不读取消息正文推断顺序。
8. **错误与并发**：正常路径在消息提交前同步尝试置前，同一事务提交消息与成功的排序更新；仅排序失败时舍弃该次置前，允许消息继续提交，保留原顺序/revision。消息本身的校验、约束、外层事务及 COMMIT 失败仍按原消息错误语义处理，不吞掉。此降级仅隔离可局部失败的侧栏元数据操作，不承诺数据库损坏/磁盘故障导致整笔事务失效时仍能保存消息。沿用组织 revision 拒绝 stale 拖动、generation/metadata fence 与错误恢复；禁止 UI 乐观改成未提交顺序、自动重放过期拖动或失败置前。不引入异步重试、持久补偿队列或第二份顺序。没有真实顺序变化时可跳过无意义写入，但不能遗漏未登记 ID。

## 代码证据与根因

基线 `ae35310f` 的静态链路如下，未运行本地应用复现；相较 v2.1，#476/#482 清理历史测试及支持，#480 修改输入模型选择器和空上下文用量，#479 修改消息/代码块复制、正文 Final 缓存与 Tab/选区菜单行为，#483 增加后台完成未读/通知及其刷新，没有实现本轮消息 INSERT 置前或即时 MessageStarted/Plan 批准刷新契约：

- `vega_ui::sidebar::ThreadsBlock::open_thread` → `vega_conversation::threads::visit_thread` → `vega_store::threads::visit_thread` 更新 `threads.updated_at` 后刷新。
- `sidebar_organization/ordering.rs::sorted_threads` 对 Updated 及 Manual 的未登记 ID 仍按 updated_at 排序，导致点击移动；已登记 rank 则不随真正的新消息改变。
- UI 的项目会话、已固定、最近任务和拖动锚点共用此排序函数。`organization/menu.rs::organization_menu_items` 与 `render.rs::Control::ToggleSort` 仍提供模式切换。
- 普通 native 及 steering 的消息入口为 `agent/pipeline.rs::prepare_run_with_images_and_reasoning`、`agent/events.rs::persist_runtime_event`；Codex 后端使用 `vega/src/codex_agent.rs::persist_codex_messages`。它们使用 `vega_store::messages::insert` 并由外层事务提交。Plan 批准指令在 `messages/mod.rs::review_plan` 内直接 INSERT。生产 INSERT 实际收敛于同一 store 模块的这两个函数；v2.1 审查时 `recovery.rs/lib.rs` 的其余命中属于旧测试 fixture，已由 #476 清理，不是额外生产入口。因此两处接入 Rust helper 即可覆盖，不需要消息触发器。
- `messages::insert` 当前只执行 INSERT，自身没有事务；不能假定所有调用者都传入外层事务。`review_plan` 已拥有 immediate transaction。新增同步排序时必须覆盖 autocommit 与已有事务两种调用方式。
- `vega_conversation::sidebar_organization/mutations.rs` 通过 Rust `checked_add` 和 `i64::MAX` 限制 revision；store 的 `sidebar_organization::write` 会全量重写 memberships/project_order。新 helper 应下沉在 `vega_store::sidebar_organization`，共享 Rust revision 校验，只 UPDATE 必要排序元数据，不调用整表组织写入，更不能从 store 反向依赖 conversation。
- `messages::update_streaming_content`、`finish_streaming` 与 `messages/responses.rs::record_assistant_response` 是既有消息更新/片段，不是新的正式消息 ID。
- `ConversationEvent::MessageStarted` 在持久化提交后发出，经 `vega/src/window/artifact.rs::apply_agent_batch_ingress` 进入 UI；当前分支没有在该事件统一刷新侧栏顺序。已有 `Sidebar::reload_sessions` → `ThreadsBlock::reload` → `refresh_organization` 可复用，带旧 revision/generation 防护。

## 实现计划与允许范围

### 第一阶段：先行测试，不改生产

1. Vega 写入本轮 M1–M7，Codex 按 v2.6 集成到最新基线并完成冻结前审查。测试直接使用公开消息/组织/存储边界；UI 用真实输入/渲染结果和 controller ingress，不查源码字符串、不仅数 mock 调用、不写镜像排序。M6 fixture 通过现有只读列表 API 获取新建对象和快照，检查本身不访问会话或改访问时间。
2. 可使用新测试文件、最小 `cfg(test)` 接线及上节明确批准的 dev 支撑、completion 纯提取/只读投影观察；除这些支撑外，生产业务逻辑保持本版基线，不提前实现排序、迁移、入口替换或侧栏刷新。只恢复清单内已有锁版本的必要开发依赖；不得恢复整套历史测试支持、加入外部进程/网络 E2E、修改 CI 或升级依赖。旧 schema fixture 要与 schema 27 一致；若远端基线漂移，先向 Codex 报告，不能悄悄改验收。
3. 提交测试补丁并交回同一 Issue，附基线、测试 commit、目标失败预期、静态检查及完整 diff，区分测试/接线、completion 纯提取、只读观察器和业务差异；列证业务行为仍为基线、没有提前实施第二阶段，然后停止写入，当前执行者待领取、下一执行者 Codex。
4. Codex 审查后建立本轮 draft PR 触发现有 Actions。至少新消息不置前/点开重排或排序入口残留的目标行为断言构成有效 RED；无关编译、依赖、runner 失败不算 RED。失败原文、run/job 和 commit 均保留；未取得 RED 时不得进入下一阶段。

### 第二阶段：Codex 确认 RED 并冻结后才允许生产实现

1. **一次性迁移**：新增 `crates/vega_store/migrations/0028_sidebar_message_order.sql`，在 `vega_store/src/lib.rs::MIGRATIONS` 尾部接线；只按“行为契约 7”初始化完整顺序、归一旧偏好，不创建任何消息触发器。若编号被新基线占用，先交 Codex 修订，不覆盖已有迁移。利用现有 `sidebar_organization.preferences.manual_thread_order`，不另建第二套会话顺序、不加依赖或改线程/消息内容字段。
2. **Rust 消息入口**：在 `vega_store/src/sidebar_organization.rs` 新增 `promote_thread(conn, thread_id)`，从 `messages::insert` 的正式 user/assistant INSERT 成功后和 `messages::review_plan` 的批准指令 INSERT 成功后调用；拒绝/放弃/过期 Plan 不新增消息、不触发置前。先核对源可见性，读取现有 preferences，清理已删除/重复 ID，按区域与 created_at/稳定 ID 补齐缺失的可见线程，再将源移到该区域首位；保留隐藏归档项的已有位置与其他项相对次序。不可只改某个 native/UI 入口，也不放到 streaming UPDATE、片段或 token 事件。
3. **事务与静默降级**：`messages::insert` 在 autocommit 连接上拥有 immediate transaction，已有事务时复用调用者事务；Plan 复用其现有事务。helper 先在 Rust 完成读取、JSON 改写及 revision 校验，最后仅执行一条原子的 `UPDATE sidebar_organization` 写 preferences/revision；无后续多语句写入，不调用会整表重写的 `sidebar_organization::write`。这样普通语句级排序失败不留下半写，两个入口可忽略该排序错误并继续消息提交，保持现有成功返回类型。不能忽略消息 INSERT、Plan 状态转换或 COMMIT 的错误，也不能为了排序失败回滚消息。共享 Rust 的 checked revision 递增/上限校验，正常组织 mutation 保留原有 Limit 错误；上限耗尽对消息仅表示此次不置前。保留 preferences 的 view/project_view/扩展键及 collapsed、groups、memberships、project_order；只在新消息时做有界元数据处理，不读正文、不在 delta 上反复处理。不得为提示引入新依赖、全局警告通道或存储事件基础设施。
4. **统一投影**：`crates/vega_conversation/src/sidebar_organization/ordering.rs` 不再根据旧 sort 选择时间排序，只使用保存 rank 和新 ID 的创建次序落位；UI 与锚点继续复用同一生产函数。`types/sidebar_organization.rs` 保留旧枚举值/字段可解码，默认及正常组织写入使用统一顺序，`mutations.rs::SetPreferences` 保留服务当前顺序，`ReorderThread` 继续 revision/事务校验。不让旧 Updated/Created 设置重新开启自动排序。
5. **移除选择入口、保留归档**：在 `vega_ui/src/sidebar/threads_block/organization/{render,menu}.rs`、`organization.rs` 删除可达的三种排序选择及 `ToggleSort`/Filter 的必要分支；原工具栏位置改为直接“显示已归档/隐藏已归档”图标按钮，复用已有 `Control::Archive`，保留可访问名称、键盘和选中态。沿用 token/几何，不增加新的说明段、模式标签或成功提示。不重开退休的分组/时间线 UI。
6. **提交后实时刷新**：在 `vega/src/window/artifact.rs::apply_agent_batch_ingress` 的有效 `MessageStarted` 分支聚合一次 `sidebar.reload_sessions`，覆盖 native、steering 与 Codex 后端的已提交消息；Plan 指令若没有随后的 MessageStarted，沿用/补齐其已提交回执上的侧栏刷新。只发异步读取，不在 UI 写排序或直接读 SQLite，不在 delta/历史重放/重复 event 上再次置前。保留 generation 与组织 revision fence，旧异步结果不能覆盖新拖动。
7. 完成后静态审查 frozen 测试未变、生产文件仅在以上范围、无新增代码注释。Vega 提交实现并交回 Codex，同一 draft PR 验证相同测试 GREEN。实现期间发现新验收缺口应停止并交回 Codex，不能补测追认。

允许范围以本版检查点解除阻塞章节为准；不修改消息时间语义、runtime/模型/权限/定价、其他 UI、CI 或发布流程。迁移是生产实现阶段新增文件，禁止修改已发布旧迁移、访问真实用户 DB，或从旧版报告复制通过结论。当前已有未冻结测试检查点，没有本轮生产实现提交。

## Actions 命令、冻结证据与交接

本地仅快速 `git diff --check`、`cargo fmt --all -- --check` 与静态审查；本轮文档阶段无行为变化，不强加无效测试。以下执行全部交现有 `pull_request` Actions：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `python3 scripts/release_tests.py`
- `python3 scripts/prepare_computer_use_tests.py`
- `python3 -m unittest discover -s scripts/tests -v`
- `cargo nextest list --workspace --message-format json` 清点后，非空执行 `cargo nextest run --workspace`。本轮应有实际先行测试，空集不构成 RED 或 GREEN。
- required check：`check (fmt, clippy, test)`。RED/GREEN 都记录发现/执行/通过/失败/跳过，未知计数标未取得。每 5 分钟查状态，通知/用户即时要求除外。

当前仅授权先行测试与 RED。用户已连续授权本卡“修订方案 → Vega 补齐测试 → Codex 建 draft PR 跑 RED”；Vega 已因基线漂移停写，Codex 接回剩余基线集成、格式和最终审查后直接建立本轮 draft PR，不领取其他卡，也不重复交 Vega 做同一轮集成。RED 证据、测试补丁 commit 和冻结内容待 Codex 确认后在 Issue 发布下一次交接；不能依据本文件提前实施第二阶段。采用“方案 → 测试 → 实现”最多 3 个本轮提交，修订方案和未冻结测试分别整理进原方案/测试提交，原检查点链接保留；不改历史 RED 证据、不通过删除有效断言换绿。

首轮 [Nextest job 117781274190](https://github.com/puzige/vega/actions/runs/39163406130/job/117781274190) 在 14 个 binaries 中执行 41 个测试（2 通过/39 失败/0 跳过），独立 JSON 清点数未从输出取得，Python 为 5+15+14 全部通过。conversation 的 22 个失败属于目标顺序/revision/初始化缺失，另 2 个约束/Plan 保底通过；app 7、M6 7、snapshot 3 尚受观察接线阻断。[Clippy job 117781273934](https://github.com/puzige/vega/actions/runs/39163406130/job/117781273934) 的 `cloned_ref_to_slice_refs` 不是行为 RED，fmt 已通过。v2.6 修正后须重新跑最新 head；不沿用首轮结果冻结，也不把 job 脚本打印的空集分支字符串当成实际 0 测试结果。自首轮起保留原提交及 run，后续仅追加有审查记录的兼容/接线修正提交。

v2.4 已先独立提交文档，使观察范围裁决可访问并保持暂停工作区中的测试进展。Vega 续做的完整补丁已保护为检查点，Codex 在 draft PR / RED 前将 v2.5 文档及非证据性准备提交整理为“方案 → 测试”，保留授权和原检查点链接。该次整理时尚无实际 RED/GREEN，后续取得后不得通过此整理安排丢弃唯一测试证据。

#482 新规则优先于上段旧的三阶段提交安排：开发期保留同一冻结测试，带测试 GREEN 的提交/源码及云端 run/job/准确计数先固定到 draft PR，再以独立提交删除 Rust 测试、可执行 doctest 和独占 fixture/mock/helper/feature/dev 接线。completion 提取中有生产消费者的处理器保留，纯测试观察器/config override/updater 测试开关等无生产消费者的部分删除。Python、真实业务/安全/无障碍/焦点/绘制行为、迁移、benchmark 和通用 CI 保留。不能为提交数把独立删测混进实现，后续由 Codex 整理非证据性准备提交；已取得 RED/GREEN 的唯一源码证据不得 rebase/force-push 丢弃。

删测后最新 head 再跑现有完整门禁，确认 Rust **0 发现/0 执行、`no Rust tests`**，此结果不能替代带测试 GREEN；两阶段证据均可读回后才转正式 PR/合并。当前只到补测试及 RED，未授权提前清理 Rust 测试或进入生产实现。

## 用户手测与回滚

新版本合并后，由用户验证：先后点开不同会话不动；向非首项发送消息/收到新回复后移到同区顶部；拖到下方后让同条回复继续流式输出，应保持拖后位置；下一条新消息才再置前；检查三个会话区域、归档入口、刷新/重启和草稿。使用 VoiceOver 聚焦归档按钮，检查隐藏/显示两状态分别朗读正确动作名称，并能键盘触发且保留焦点；该项当前待验。无需先选择排序模式或每次新建后再拖一次。

回滚须另行 PR 处理：保留消息/手动顺序和全部用户数据，通过 Rust 调用点停用新消息置前；一次性初始化的数据保持可读，无消息触发器需要停用。必要的数据修正使用新补偿迁移，不能编辑已执行迁移或直接改用户数据库。没有改动不可解码的旧枚举，无需用户改配置。

带测试 GREEN 证据和独立删测后的最新 head 完整空集门禁均核实后，由 Codex 将 draft 转正式并按工作流 squash merge；Issue 开放并置 In review，分支/worktree 保留；用户明确通过后才收尾。本轮尚未取得任何 RED/GREEN 或应用实测证据。
