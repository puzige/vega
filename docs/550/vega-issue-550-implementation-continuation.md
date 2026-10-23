# #550 折叠箭头实现续接

## 当前阶段与保存版本

2026-10-23 续接；已保存原始 28 个开发测试及支持为 `6b675d2608dd9653f21acf25109192264cd67075`，源码 SHA-256 `db70f73a94dbee5adc32c9c067b5f0d8524290424603c3b9f4a5211f79a65ea8`，[源码](https://github.com/puzige/vega/blob/6b675d2608dd9653f21acf25109192264cd67075/crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs)、[patch](https://github.com/puzige/vega/commit/6b675d2608dd9653f21acf25109192264cd67075.patch) 已读回 blob 一致。源码定义数不等于发现/执行数，未取得本卡云端结果。

已正常 merge master `9f9fa092a3ca391a65f08419c9206f32979119e6`，本卡 merge head `e24c6b253da99bcf7cdac32f56144f8597f81c18`。当前仓库政策取消强制 TDD，旧规格的等待 RED/冻结后才实施条款为历史；D01–D11 和 #416 全部产品边界不变。Rust 测试仍不得进入 master，失败不能靠删测/放宽断言消解。

## 实现范围

1. 仅两处 disclosure helper 的箭头：单个 ChevronRight，收起 0、展开 π/2，120ms 默认 ease_out_cubic，稳定 owner/property key，正常连续重定向，初挂/重挂终态。
2. 消费 #549 单一 writer 的 `Motion::DISCLOSURE_TURN_MS` 和 `interaction::decorative_value`，不自建曲线/计时/activation 引擎。真实 glyph 可见性由实际 GPUI 样式及 content mask 决定，不能用 expanded 单一布尔值冒充。
3. 字形固定 12×12 或 16×16、原 slot 不变；header/body/aria-expanded/focus/actions 即时，只有绘制方向插值。原 group_hover、expanded、keyboard-focus 显隐、禁用、无内容、Tab 顺序保留；不为了尾帧把已隐藏箭头继续显示。
4. 隐藏、clipped、失活、Reduce Motion、禁用时终态且停止装饰帧；无 inactive draw 即快速恢复不重播。业务流式/permission/terminal/输入活动不暂停，不改 shimmer。
5. #416 首次可信 Final 收起一次及真实选择/回焦/产物/其他轮/权限入口完整保留；不借此改历史/消息缓存/队列/导航算法。

## 已核 API 与 harness 修复

- KitIcon::rotate 必须在类型擦除前；gpui-component 0.6.1 的 into_svg 为私有，Svg::with_transformation 是公开且只改绘制，不影响布局。若需 paint-time 可见性/旋转，选最小透明 Element 接线，不修改 registry、不凭空调用 AnyElement.rotate 或私有 opacity API。
- 实际构造/绘制使用的角度必须与开发 paint recorder 相同；常量、仅目标、另算采样或重写镜像决策不能当绘制证据。证据仍是配置参数/真实委托 paint，不是 GPU transform 独立读回或用户观感。
- 本包宏生成依赖 `gpui::run_test`，Cargo 名称是 gpui_kit；测试模块目前未显式提供 `use gpui_kit as gpui`，需要按锁定宏源码修正，必要时消歧 builtin test。此类导入/格式/API/fixture 修正不改变行为断言，不提高递归上限。
- 允许最小 helper/caller API 接线，若改变 `&Window` 为 `&mut Window`，五类生产 header 及开发 fixture 全部一起核对。不改权限、Final、失效/测量算法以迁就测试。
- 2026-10-23T03:34Z 静态复核发现测试 `pending_permission_remains_visible_and_usable_after_trusted_final_collapses_process` 使用不存在的 `PermissionDecision::AllowOnce`。真实 `types/permission.rs` 返回变体是 `Once`，`permission_card.rs` 的一次允许动作也返回 `Once`。这是保存包的测试 API 错误，后续仅将匹配变体修为 `Once`；仍须验证真实队列许可、可见入口和原动作结果，不改产品返回类型或减弱断言。

## 暂停后恢复检查点

2026-10-23T03:28Z 已确认旧子执行不存活，保存测试源码仍为原 SHA-256；本卡没有产品源码改动。父已 fetch 并正常 merge 新基线 `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`，任务 HEAD `c8a9e1039ffe337af5fff51ff9b7b1565ba741ea`。新基线的许可来源/展示资格与阶段恢复实现保留，不从旧 ref 复原。本阶段继续等待共享接口停写后串行接入；宏导入、上述枚举及必要格式错误归测试接线，尚无编译或行为执行结果。

下一阶段由父集成已停写共享提交后，复用第二层 writer 实现；停写固定版本后独立审查与现有 Actions。只允许本地静态/格式/离线元数据检查，无本地执行 Vega、编译、check、Clippy、test/nextest、bench/profile 或 CUA。最终开发验证、证据固定、独立删测后，对最新无 Rust 测试 head 跑完整 required CI，再合并等待用户手测。

## 2026-10-23 中断保存检查点

旧实现执行 `01M5KQETE3R8NRARENX5G0XZMK/01M5KQETE3NRMWMH4MAMDZNSQ9` 已 interrupted，非 READY。实际保存 `disclosure.rs`、`disclosure_paint_probe.rs`、`disclosure_motion_tests.rs` 三文件 202+/158-；该 tracked diff SHA-256 `eac4c44f84d4d0fef984439cb57a4e4a25492dfa597cce2ca0d1bab0d26df8c0`。只有 paint-time SVG/样式与 mask gate、真实 recorder 及两个 API 接线修正；五类 caller、Final、permission/list/shimmer 算法未改。尚无云端验证。

父 fetch 后正常 merge master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`，本卡 HEAD `1560e8aeaff15e41e31e5f40532a94bc13165787`；保留 #344 core/message_anchors 新行为和全部 28 项原断言。共享源已以 `4c3c75b9977748c0a053739244873b7d4284a757` 接入，不重复实现。下一阶段只收敛此保存补丁的透明委托、生命周期及真实 API/fixture 复核，固定最终内容并 STOP，供非写入者审查和现有 Actions；不重做大范围调查。

## 2026-10-23 窄范围最终格式续接

- 本轮读回 HEAD 与上述三文件 diff SHA-256 均匹配中断检查点；三个源码 SHA-256 也与父指定值一致，格式化后仍逐字节不变。保存实现使用 paint-time 实际 SVG 样式与 content mask 判定可见性，复用共享 120ms sampler；绘制与 recorder 使用同一角度。宏所需 `gpui` re-export 与 `PermissionDecision::Once` 是已保存的 API 接线修复，不改变行为断言。
- 五类生产 caller、Final、list/permission/shimmer 算法未改；28 个测试定义及原断言保留。定义数不是 CI 发现/执行数；未取得 CI 结果，不宣称 GREEN。
- 之前只有单文件格式检查通过；父实际 `cargo fmt --all -- --check` 退出 1，失败位于测试独占支持，不能记作此前全量格式已通过。本轮获准执行 `cargo fmt --all`，实际退出 0；只格式化 `core.rs` 四处测试记录、`phase.rs` 一处测试记录、`disclosure_layout_probe.rs` 的 `Item` literal，以及 `lib.rs` 的 cfg(test) 模块排序，无新增算法或生产行为改动。
- 最终 `cargo fmt --all -- --check`、`git diff --check` 与实际 tracked diff SHA-256 随停写回报记录。未执行 Vega、本地编译/构建/check/Clippy/测试/nextest/bench/CUA；未更改 Git/index、依赖、CI、共享 motion 或 feature。后续由父提交、非写入者独立审查并使用现有 Actions 验证；开发验证留痕后，合并前仍须清理 Rust 测试及独占支持，对最终无 Rust 测试 head 跑完整 required CI。

## 2026-10-23 原始云端编译失败后的窄修复

- 原始首次云端 [Actions 39919579054](https://github.com/puzige/vega/actions/runs/39919579054) 对应实现 `fcf67c8b9038b298acff361bdfdc0b3aedce853e`：Nextest job `120301289770`、Clippy job `120301290193` 和 required 汇总 `120303414725` 均失败。原始日志 SHA-256 已核对为 `f28f302860fae839cb3f2d4cbfc5e7b8876a89ed7100fda505d97447208620ff`。失败发生在编译阶段、Nextest 发现/执行之前；没有取得发现/执行计数或行为结果，不是测试行为失败、GREEN 或 `no Rust tests`。
- 父已 fetch 并 merge master `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae`；本轮修复基于 HEAD `7cf573b4659e957d5e13f7df9294a882a0d375a6`，保留 #575 context usage。此次单一 writer 不执行 Git/index 修改、提交、fetch、push、PR 或 CI 操作。
- 分类与实际修复仅限以下五处，不改变验收：
  1. `disclosure.rs` 的 `self.inner.id()` 被解析成消费 receiver 且需要参数的 `InteractiveElement::id`，产生 E0061/E0308。改为 `Element::id(&self.inner)`，继续委托真实 SVG identity，不伪造 ID、不返回固定 `None`。
  2. 同文件 `Transformation::rotate` 需要 `Into<Radians>`，`f32` 不满足，产生 E0277。仅将同一采样值包装为 `gpui_kit::Radians(radians)`；实际 paint 和 recorder 仍使用同一个原始 float。
  3. `icons.rs` 的 test-only `configured_chevron_paint_control` 存在相同 E0277；实际 `KitIcon::rotate` 同样包装 typed `Radians`，observer 仍记录原始 float。
  4. `disclosure_motion_tests.rs` 的 fixture 通过私有 `artifact` 模块访问服务，产生 E0603。改用既有公开 re-export `vega_conversation::ArtifactService::in_memory_ui_fixture`，不扩大 API 可见性。
  5. 同测试模块的显式 `gpui` re-export 被原始 Clippy `-D warnings` 证实冗余；仅删除该导入。此诊断取代上文保留/补充显式宏导入的旧建议，父模块 prelude 的现有上下文保留，不增加 alias、allow 或递归上限。
- 前两处是生产实现的 API/类型编译错误，后三处是开发测试支持/接线错误；不是验收变更，也没有证据将其归为外部环境失败。仅修改 `disclosure.rs`、`icons.rs`、既有 `disclosure_motion_tests.rs` 并追加本文；D01–D11、28 个测试定义及全部断言/期望值保留，无删除、ignore 或放宽。原始保存源码 `6b675d2608dd9653f21acf25109192264cd67075` 的 Git blob 已读回，SHA-256 仍为 `db70f73a94dbee5adc32c9c067b5f0d8524290424603c3b9f4a5211f79a65ea8`。28 只是源码定义数，不是发现、执行或通过数。
- 本地 `cargo fmt --all -- --check` 与 `git diff --check` 均退出 0；精确源码 delta 静态核对退出 0，确认只有上述五项替换，断言/期望值正文未改。未运行 Vega、编译、构建、check、Clippy、测试、nextest、bench 或 CUA；不改 Final、permission、list、shimmer、共享 motion、依赖或 workflow。
- 修复后的 Actions 与新提交 head 的非写入者独立审查均待父安排；旧独立审查已中断，不能视作有结论。继承 opacity 与没有 inactive draw 时的证明边界继续留作审查限制，不在本次编译修复中扩建架构，也不把既有 recorder 证据称作独立 GPU/观感验证。停写回报附最终真实 Git diff SHA-256；后续开发验证留痕、独立删测和最终无 Rust 测试 head 的完整 required CI 仍由父负责。

## 2026-10-23 首次实际行为结果与有界修复计划

`20534df54aff4de6581b04e887fb18ac56148bb4` 的 [Actions 39922954546](https://github.com/puzige/vega/actions/runs/39922954546)：Nextest `120312339386` 实际 `28 tests run: 1 passed, 27 failed, 0 skipped`；Clippy `120312339053` failure（唯一诊断 manual_noop_waker）；required `120313817592` failure。failed-log SHA-256 `40a9dcfd50e0d7431eb389c7ea3bc0626b7c9d58b1aee4025f76c16d4fa79068`。这次编译 API 已修复且行为确实执行，不能沿用首次“未执行”。唯一通过项为真实配置角度 paint control；不证明五类完整动效通过。

父已按原始 27 个根失败归类：16 项止于 `tool():62` 的 apply_finished；10 项 StreamSurface already being updated；1 项 ConversationStream leaked handles。第一类的 proposed→Success 在 reused=true 时本来合法，不能误改生产状态机；真实 `success_truncation_valid` 要求 reused=true 时 truncated=None，而 fixture 提供 Some(false)，导致严格 projection 判 corrupt。下一最小修复仅纠正 fixture metadata，保留 apply_finished 与所有业务断言。第二类 typed WindowHandle::update 会借用 root，fixture 内 focus/事件可同步触发该 root observer；核对实际回调后，将不需要 root 的 window 操作移到已有非 typed update_window，不改生产观察或吞 reborrow。第三类 StreamFixture 字段按声明顺序先销毁 cx，再销毁保留 stream handle；将真实 entity handle 先 drop，再 shutdown App，保留泄漏检测，不泄漏/forget/禁用 detector。Clippy 改用标准 Waker::noop，保留真实 queue/action/future 轮询断言。

父已正常合入最新 master `988e1e37271c666f05ff8eae46d662324e91afd6`（#535 continuation），当前 HEAD `1068c46aa105faf34fd644711b7a70c22a315f28`；不回退它对同 run/Final/阶段投影的变更。上述修复仅现有开发测试接线与文档，不改 Final/权限/list/shimmer/共享源/依赖或 workflow，28 项断言保留。

### 独立审查与仍未闭合的 D05 证据

非写入者对精确 `20534df54aff4de6581b04e887fb18ac56148bb4` / base `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae` 已返回有界静态审查：没有新增已证实产品缺陷；P2 验证问题是 `rapid_reactivation_without_an_inactive_draw_does_not_replay_missed_motion` 的 blocker activate/run_until_parked 会触发真实 inactive draw，当前断言不能证明“没有 inactive draw”。后续须保留原角度/续帧断言并增加真实 root render/paint 的阳性对照和无中间 draw 检查；锁定 public API 尚未取得可恢复抑制绘制的路线，需明确作为未决，不能假装已覆盖或删回归。继承 opacity 只读到 local style，静态扫描未找到当前 disclosure 位于 zero-opacity ancestor 的实际消费路径，属于证据限制而非已证实产品缺陷；不因此扩大框架 API。

D05 缺口与实际 27 项失败均解决前，不清理测试或合并；已知框架限制不掩盖其他失败，反之亦然。

## 2026-10-23 实际行为失败后的窄范围 harness 修复

- 本次停写基线为 `1068c46aa105faf34fd644711b7a70c22a315f28`，保留父先前未提交的上述行为结果/审查记录，仅追加本节。原始 `/tmp/vega-550-20534-failed.log` 已读回相关根失败且实际 SHA-256 为 `40a9dcfd50e0d7431eb389c7ea3bc0626b7c9d58b1aee4025f76c16d4fa79068`；原 run `39922954546` 仍是实际执行 28、通过 1、失败 27、跳过 0，Nextest `120312339386` / Clippy `120312339053` / required `120313817592` failure，不把本轮静态修复称作行为通过。
- 16 项首个失败属于 fixture metadata 错误：只将 `tool()` 的 `truncated: Some(false)` 改为 `None`，符合真实 `success_truncation_valid` 对 `reused=true` 的约束。保留合法 proposed→Success、原输入（无 timeout 使用解析默认值）、`apply_finished` 严格断言及所有行为期望；未增加 approval 或改生产状态机。
- 10 项 root reborrow 属于 harness 接线错误。锁定 GPUI `window.rs:6950–6955` 的 typed update 确实通过 `view.update` 借用 root；`focus` 会 refresh，`dispatch_key_event` 在 dirty 时先 `draw`（`window.rs:5606–5609`），故在贷款尚未归还时可重入 root render。仅把现有测试中 55 处不使用 root 参数的 window 操作/查询改为已有非 typed `HeadlessAppContext::update_window(window.into(), |_, window, cx| ...)`，闭包正文、选择/焦点/输入/权限动作和断言不变；需要真实 view 的更新保留 typed API。现有 draw helper 原本已非 typed，本轮未把它算作新修复。不改生产 notify/observer/业务回调，不吞 panic。
- 1 项泄漏属于 fixture 生命周期：`StreamFixture` 字段改为 stream、window、cx 声明顺序，真实外部 stream handle 先释放，随后 HeadlessAppContext shutdown；不 forget、不关闭泄漏检测、不改原 entity release 断言。唯一 Clippy 诊断改用标准 `Waker::noop()`，移除自制 Wake 实现，仍在真实可见 `PermissionEnter` 后 poll 原 PermissionQueue future 并严格检查 `PermissionDecision::Once`。
- D05/P2 仍未闭合。本轮在现有 fixture 的 `ProductionHeader::render` 入口加入实际 root render 计数，先用显式 refresh/draw 严格检查计数增长的阳性对照，再在原快速失活/恢复测试中检查失活后以及恢复后、显式 draw 前计数均完全不变；原中间角度、终态和停止续帧断言全部保留。计数不是另造 sampler 或目标值镜像，也不是独立 GPU 读回。锁定 TestPlatform 的 active-status 调度经 foreground executor（`platform/test/platform.rs:284–303`），原 `activate/run_until_parked` 仍可能触发自动 inactive root render；新增严格前提预期会明确暴露该 framework/harness blocker，未取得无中间 render 的可恢复 public API 路径。因此不能宣称 D05 已覆盖或本轮可全绿，下一步须由父审查该控制并解决真实 no-inactive-draw harness 路线，不扩生产框架/私有 API/registry/依赖来消解它。继承 opacity 证据限制保留。
- 最终本地只执行快速格式/静态检查：单文件 rustfmt、`cargo fmt --all -- --check`、`git diff --check` 均退出 0。精确授权替换再交 rustfmt 的静态比对退出 0，实际源码字节完全匹配：五项 fixture/lint 替换、55 处 root-independent API wrapper 与最小 render-counter 接线/新增严格前提；全部 28 定义/名称、原断言/期望及 D01–D11 保留，无 ignore/allow。最初自写静态归一化脚本未处理 rustfmt 的闭包括号变化而退出 1；改为直接以 rustfmt stdout 比对完整预期源码后退出 0，属于静态观察脚本修正，不是修改产品/断言来过检。
- 仅修改现有 `disclosure_motion_tests.rs` 和本续接文档；不运行 Vega、本地编译/构建/check/Clippy/test/nextest/bench/CUA，不调用 Git/index 修改或 fetch/push/PR/CI。新提交、Actions 和独立审查由父负责且均待执行；本轮修复没有运行结果。父须按新 head 的实际失败证据分类处理，不能重复旧 cloud run 冒充本轮验证；D05 和原失败真正解决前不清理测试或合并。

## 20/28 后的责任层收敛与尚未授权的冲突

head `76fd200952154dc5be8c6cda1f6cd513badff106` 的 [Actions 39927968952](https://github.com/puzige/vega/actions/runs/39927968952)：Nextest `120328923841` 实际 `28 tests run: 20 passed, 8 failed, 0 skipped`；Clippy `120328924012` success，required `120331512074` failure；failed-log SHA-256 `16dc786ac7f373cd2d128d91032de81a9d715e24c158cf1edb794afdd9b423f8`。此前 16 项 metadata、10 项 root reborrow、1 项泄漏不再作为本轮全部失败的归因；独立静态增量审查确认这轮修复责任层正确。父另 fetch/正常 merge master `a439483cf2c377fe59f1dfb9b487bfb2200d5c64`（#577 / coordinated storage），当前任务 HEAD `1f706e4b7053be5da6625196d8d661bfa8896163`；保留该存储协调，不借 SQLite writer 序列化冒充 ordinary launch owner 锁。

父读取全部八个根断言并复核实际源码；只读诊断执行虽异常结束，但具体源码结论已由父定点确认，不能称完整审查通过：

| 失败/缺口 | 已核责任层 | 当前处理边界 |
|---|---|---|
| independent owner `ProductionHeader already updated` | fixture 在 typed second_window.update 内再 second.read，同 root 重借用 | 改为非 typed query 或 App read，保留两个 owner、角度和真值断言 |
| long Thinking 原 card 文本没追加 | fixture.begin 已发 TextDelta Commentary，真实 content 会 finish_live_thinking 并清 active_thinking；后续 ThinkingDelta 创建新 card，旧 card 不该继续增长 | 真实纯 Thinking 活跃前置，维持同一个 card 的交错输入/旋转；最后再按真实顺序送 Commentary→Final。保留每步完整文本、草稿和几何断言，不按旧 card 输出改期望 |
| shimmer `progress_finished` 未结束 | fixture 用 Completed group；set_thinking 因 terminal 被拒，finish_thinking 没有 current card | 用 Running group 真实挂载同 card、登记 current_thinking 后执行原 finish；不得手动设 finished 伪造通过 |
| no inactive draw 12 vs 11 | 新真实 counter 已证自动 inactive root render，公开 API 限制 | 严格前提/原角度续帧断言保留；BLOCKED，不再重复相同云端重跑 |
| Final 子 `thinking.expanded` 仍 true | #416 契约要求外层过程关闭、隐藏选区/焦点处理；当前 settle_final 只改 outer expanded/body_in_details/ancestor_visible，内部展开位不清 | 属于验收假设与既有产品范围冲突；本轮不删除该断言或改 Final 算法。需先明确 spec/可观察隐藏与重开语义后再处理 |
| disabled Tab skip | GPUI track_focus 使用句柄自身 tab-stop=true，row.tab_stop(false) 不改显式 handle；既有 master 同样存在 | 可能是既存无内容焦点缺陷，与本卡“焦点顺序不改”范围冲突；本轮不改生产 focus 或降低 skip 断言 |
| fold invalidates 3 vs 1 | group observe 会失效 RunActivity、segment 和 owned process-copy 多条；不能从总数直接推断 decoration 重复 | 需要真实 owner/index/事件来源和操作边界；本轮保留 one-fold 严格断言，不直接改成 3 或 <=3 |
| drag selection empty | fixture 用 glyph bounds 猜正文坐标，未证明实际可选 body 已完成准备/绘制 | 需真实正文 bounds/准备阳性对照，不能直接设置 selection 缓存替代真实拖选 |

另有独审 P2 的合法 failed-tool 路径假阳性：Failed 的 exit_code/duration_ms/truncated 必须全 None；旧 fixture 给 Some 值进入 Corrupt fallback，而 activity_state 仍是 Failed。只授权纠正该 fixture metadata 并补真实 failure 输出仍在 card/render 的观察，保留原状态/正文/选择断言，不改变生产严格 projection。

本轮下一 writer 仅修上述三项确定 fixture 错误及合法 failed-tool 数据证据；余下四类验收/观察冲突与 D05 限制保持明确未决，不扩大产品范围或静默放宽原断言。失败未解前不清理测试/不合并。

## 2026-10-23 恢复核对与执行环境阻塞

2026-10-23T10:04:07Z 核对：本地 HEAD `5da24db0ba4ce9a9f369dffa2807cbc7fc3128c2` 已包含当前 fetch 得到的 master `80eaba474df5a23c3a7849e11b39a0864b7e0ad6`（#542 产物聚合）；PR #579 远端仍为 `76fd200952154dc5be8c6cda1f6cd513badff106`。测试文件在本地 HEAD 与该远端版本的 Git blob 均为 `73474821b5493d6ff8f7b9f9b8fae5a744e6634a`，本次恢复没有新增源码修复。

原修复执行已确认为 interrupted / recovered_abandoned，无新源码交付。重新安排专职 writer 时，执行工具返回 `capacity_exceeded`；对旧执行请求取消后，查询仍返回 running / release_state unknown，不能声称已释放容量或新 writer 已接手。完整目录查询另返回 receipt too large，现有接口没有可传入的筛选参数；不反复调用同一个失败查询。此为执行环境/委派阻塞，不是产品或测试的新结果，也不绕过仓库的专职实现与独立审查要求。

本次重读原始日志及严格计数边界：#550 在重新激活之前的断言已得到 12 vs 11，不能将该失败解释为只统计了恢复后的绘制。#552 的 14 vs 12 检查在恢复后，不能仅凭该总数细分两次绘制的时点；两卡均未取得符合原验收的无 inactive draw 证据。原断言不变，不修改 framework / registry / CI，不盲重跑。

下一步仍是已列明的三项 fixture 修复与合法 failed-tool 输出证据；恢复条件为专职 writer 能获实际执行名额并确认单一写入权。停写后固定精确内容，非写入者增量审查，再交现有 Actions 验证新 SHA。其余验收/观察冲突与 D05 均保持未决；当前无新 CI、无测试清理、无合并。

## 2026-10-23 有限测试修复接手

- 本轮重新核对任务 HEAD `5da24db0ba4ce9a9f369dffa2807cbc7fc3128c2`、测试 blob `73474821b5493d6ff8f7b9f9b8fae5a744e6634a` 和上轮 31 行未提交检查点；接手时完整 binary diff SHA-256 仍为 `6ba98cc001b34663acbf9514ebe387c4cf2f6b443c81fddb6711a874575e5777`。原检查点完整保留，没有后来源码改动。fetch 后远端 master 仍为 `80eaba474df5a23c3a7849e11b39a0864b7e0ad6`，安全更新返回 Already up to date；#542、#535、#344、#577 集成保持不变，不触碰主工作区脏文件。
- 本会话执行目录为空，公开交接无后续占用记录；已成功派发唯一代码 writer，本轮恢复了实际执行名额，不将旧异常执行当作新工作。父负责 Git/index、文档、提交、推送和 PR/Actions；writer 只修改指定测试，必要时在 `tool_card.rs` 增加极小 `cfg(test)` 只读观察，不扩大生产 public API。停写后由非写入者审查固定版本。
- 验收仅为四项已定位的测试责任层修复：非 typed root 查询避免重借用；纯活跃 Thinking 前置并在全部增量后依序 Commentary → Final；仅 shimmer 用例建立 Running group 并登记同一 card，由真实 finish 完成；Failed metadata 三项改为 None，并证明原失败输出保留在真实 card/detail/render 路径。
- `ToolCard` 的 `output_rows`、`detail` 私有且无现有输出 getter。优先真实 header 展开/渲染与已有观察；如需 test-only 接点，只观察真实数据，不另造 projection/selection 缓存或改业务决定。28 项原测试和断言全部保留，其他 begin/fixture 语义不变；本轮不修 no-inactive-draw、Final 子 expanded、disabled Tab、measurement 3 vs 1 或真实拖选空的问题。
- 原始失败日志哈希再次一致，远端 Actions `39927968952` 的 head/job 结果也已读回；这些仅验证旧 `76fd200952154dc5be8c6cda1f6cd513badff106`，不验证当前集成或本轮修复。新结果待停写、提交后交原 PR #579 的现有 Actions，按 5 分钟间隔查询；不改 workflow、不盲重跑换绿。
- 本轮不清理测试、不合并 #550、#552 或相关 PR，保持 Issue OPEN / In progress。禁止本地运行 Vega、编译/build/package、check/Clippy/test/nextest/bench/profile、CUA/OS 自动化；仅快速静态和格式检查。完成实施、独审和新 head 云端结果分别记录，不能以全部 28 项全绿为由扩大授权。

### 限定修复停写产物

- writer 已 READY 并释放写入权，父读回确认：相对 `5da24db0ba4ce9a9f369dffa2807cbc7fc3128c2` 仅两源码共 133 行新增、11 行删除，源码 binary diff SHA-256 `48306ba9399384a3595db3e1f7c59f4a15929352e9ee1e19b7de5b5edb1f38bb`。测试源码 SHA-256 `be049269cbde74cababc2fc79631626da8fff6aa9a694fb096832c4a19c09936`；`tool_card.rs` 为 `2d820e0f3255c95118f8af7c0f4c7cb2dc010fcf63fd9006de3ded0b619e8e07`。
- independent-owner 末尾改 App update 查询，不再在 typed root update 内重借；long Thinking 提取原阶段展开 helper，其余 begin 流不变，本用例保持同一 active card 接收所有增量，再发送 Commentary/Final，额外严格检查活跃身份与未结束状态；shimmer 仅本例新建 Running group，挂载并登记同一卡片，终态仍由原 finish_thinking 完成。
- Failed metadata 的三项改为 None；经真实工具 header paint、focus/Enter 展开、draw、真实 header snapshot 验证可见/展开/焦点，随后精确断言失败正文。唯一新增接点为 `#[cfg(test)] pub(crate) test_detail_output()`，直接返回既有 `detail().output`，不新增投影决定；Corrupt 清空 input/output 后无法通过该正文断言。证据为真实展开/渲染与生产 detail 数据观察，不宣称独立像素文本回读。普通构建的 ToolCard 业务代码逐字节不变。
- writer 报告单文件 rustfmt、全仓 fmt check、diff check 均退出 0；父独立执行 `cargo fmt --all -- --check`、`git diff --check` 与静态范围检查均退出 0，确认 28 个测试名称保留、仅四个授权测试体变化、其余 24 个逐字节不变，以及 ToolCard 唯一差异为上述 test-only accessor。原断言未放宽或删除；writer 静态清点为原 204 个断言全部保留、新增 14 个，不作为执行计数。
- 首次 writer 格式检查仅两处换行，修正后通过；其静态检查脚本首次误把 helper 计入相邻测试，修正函数边界后通过，未改变测试断言。未运行本地编译、程序或测试。固定提交后的独立审查与现有 Actions 尚待取得；五项范围外失败仍未决，不据此清理测试或合并。


## 2026-10-24 artifact 兼容修复接手与实施契约

- 本轮用户只授权修复 `visible_artifact_preview_and_real_control_focus_survive_first_final_collapse` 的 #542 API/fixture/观察兼容，独立审查后取得新 head 的现有 Actions 结果。不合并 #550/#552 或相关 PR，不修改 #552，不清理 Rust 测试，不扩修五类已知未决。当前规则不重新要求 TDD。
- 接手重核原本地/远端/PR head 为 `ed14a21a1c6db5b09a8a4631df5d7e3eeb49c252`，index/worktree 干净，公开交接已释放前轮 writer/reviewer。本会话只读契约核对者没有源码写入权。fetch 后正常 merge master `73977c89eba718da4b2573227c44beb8bcff6911`，本轮基线 `725d63281f891189e81ed0b66665130001e196e6`；上游仅 #307 权限面板，四项旧修复以及 #542/#535/#344/#577 保留，主仓库文件未动。
- 上轮 [Actions 39993796156](https://github.com/puzige/vega/actions/runs/39993796156) 对应 `ed14a21a1c6db5b09a8a4631df5d7e3eeb49c252`：Nextest `120551969424` 与 Clippy `120551969746` 均止于六处 E0599，required `120553551657` failure；Format/Python step success。Rust 发现未完成、行为未开始，执行/通过/失败/忽略/跳过计数未取得；不是 no Rust tests。原始日志仍在且 SHA-256 读回为 `01f9e2e2276e85fb4bc568e57bab1390460deeb76c410b9f604d5f6fb4b83ae5`。公开完整前轮结论见 [Issue](https://github.com/puzige/vega/issues/550#issuecomment-6370815322) / [PR](https://github.com/puzige/vega/pull/579#issuecomment-6370814655)。

### 原验收与当前合法前置

[#542](../542/vega-issue-542-artifact-card-row.md) 明确运行中不向对话流插入产物，产物改为结束回合后的独立聚合；workspace 仍使用 `ArtifactCard::render_row` 和原 `row_count()`。因此旧同段 `RunActivityChild::Artifact` 挂载与 `insert_after_tool/contains_artifact` 结构已不存在，不能恢复。先结束再补当前 Final 也不作为合法事件顺序。

本轮采用同一实时会话、同一路由已完成前轮的合法产物：真实 `register_artifact_call` / `apply_artifact_card` 登记 → `MessageFinished` → 唯一匹配 `StreamEntry::ArtifactGroup` → 已打开同窗 workspace 预览 → 当前轮首次可信 Final。不是历史 hydration 回溯，不宣称继续验证已删除的同阶段内嵌结构。旧结构断言改为精确 card Entity、call owner/message_id、唯一匹配 ArtifactGroup 与聚合 `row_count()==1`；当前轮过程仍须在首次 Final 收起。

| 必保目标 | 修复后的严格观察 |
|---|---|
| 合法产物归属与可见性 | 精确实体/call/回合归属、完成前不入流、完成后唯一聚合；Final 后身份/归属不变 |
| 完整预览 | 当前生产 `render_row` 四行均实际 paint、可见、在 viewport 内，原逐行几何尺寸前后相等；精确比较两行预览正文 |
| 真实控件焦点 | 真实 Preview 控件已有 FocusHandle，paint 后实际设焦，并通过 Enter/ArtifactActivate 产生字段匹配的 `ArtifactPreviewRequested` 阳性证据；Final 后继续保持同一焦点/控件可用 |
| 首次 Final 隔离 | 当前轮真实 Final 路径收起当前过程，以上可见产物的预览/几何/焦点均保持 |

最小 test-only fixture 在同一窗口并列挂载真实 ConversationStream 与 workspace 使用的相同 `uniform_list` / `ArtifactCard::render_row`，默认不启用预览，不改变其余用例前置。已有 snapshot 只提供几何/可见性与 AX，不能读预览 child 正文；允许在 `artifact_card.rs` 增加唯一 `#[cfg(test)] pub(crate) test_preview_lines() -> &[String]`，直接返回既有 `preview_lines`，不另算/复制业务投影，不扩大普通构建 API。该证据是实际行渲染加真实渲染数据观察，不是独立像素文本回读。

### 唯一 writer 与验证边界

- 唯一专职 writer 只改本 artifact 测试、必要的同文件 test-only surface/fixture，以及上述五行以内只读方法；不改其他 27 个测试体、四项旧修复、生产行为/算法、依赖、registry、framework 或通用 CI。writer 不操作 Git/index/commit/push；主 Agent 负责本记录、版本、提交和云端。
- writer READY 必须附精确源码/diff 内容标识、实际静态检查及停写声明；随后主 Agent 核对 28 个名称、其余 27 个测试体与范围外断言保全，固定提交，交非写入者独立审查完整 SHA，继续原 Draft PR #579。
- 本地仅快速静态/格式检查，禁止运行 Vega、编译/build/package、cargo check/Clippy/test/nextest/bench/profile、CUA/OS 自动化。自动测试只交原 Actions，不改 workflow；启动后每五分钟查询，完成/失败通知或用户即时请求除外。新结果分类后仅可继续修本轮授权内 fixture/API/观察错误，范围外失败只记录。

### artifact 兼容修复停写产物

- 唯一 writer 已 READY 并停写，父逐项复核实际 diff：只改 artifact 测试体及默认 `None` 的同文件预览 surface/fixture，`artifact_card.rs` 唯一差异为四行 `cfg(test)` crate-private 只读 `test_preview_lines` 方法；移除此接点后普通构建源文与基线逐字节相同。默认无预览 surface 仍生成原渲染树，不改其余用例前置。
- 真实登记/apply 后运行中无 ArtifactGroup；前轮合法 Final 在 MessageFinished 之前，完成后唯一匹配聚合。当前轮开始后，精确 card/call/owner/group 及聚合行数前后不变；四行实际 paint、全 viewport bounds、原逐行几何比较与两行精确全文保留。真实 workspace Preview focus[0] 在首次 Final 前后均实际持焦，Enter 经既有 ArtifactActivate 产生精确 emitter EntityId 与 thread/project/card/file 请求字段，两次请求严格相等。当前轮真实 process_in_details 从 false 到 true，过程关闭，精确 Final 原文在外。
- 测试源码 SHA-256 `b5d4ca9a458b600d34ba6cac3dfc2bd19da085572f32493f6e440415cec39157`；ArtifactCard 源码 `0877d19ebd5db4149dc6a3a8a1f05de5add202f839a41179ebae43f3718d150e`；两源码 binary diff `19db48afcea186637557382f0139b520ca821d06a0558e499b5c1f563537ce45`。父读回全部匹配 writer 停写内容。
- 父实际 `cargo fmt --all -- --check`、`git diff --check`、词法边界静态范围检查以及唯一 accessor/字节保全检查均退出 0。28 个名称/顺序保留，其他 27 个测试体逐字节不变；四项旧修复、五类已知未决原断言、tool_card、workflow、Nextest 配置与依赖保持。静态 assert 宏计数 218→237，不是发现或运行计数；旧结构断言依据上述契约作当前身份/聚合观察等价替换，不放宽业务目标。
- 未本地运行 Vega、编译/构建/check/Clippy/test/nextest/bench/profile/CUA；新 head 编译、发现与行为结果及非写入者审查仍待安排，不宣称通过。规格偏离无；证据仍是实际渲染加渲染数据观察，不是像素正文或真实文件/权限/外部应用验收。
