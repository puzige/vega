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

