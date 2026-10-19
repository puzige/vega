# #420 用户验收修订：置顶区自然增高

## 裁决与基线

用户对 #496 手测反馈：超过五条时应向下扩展已固定区域、推动项目区下移，不形成置顶区内部滚动。已再次确认“50个”为“5个”。本规格取代 v2 的五行视口、内部滚动及局部渐隐要求；旧文档与 RED/GREEN 保留为历史证据，不证明本轮行为通过。

基线 `dbb0f41d`；分支 `feat/420-pinned-natural-height`。保留 #496 的共享行 `.flex_shrink_0()`、32px 行高、13px 字号、排序、唯一投影、交互与持久化。整栏超出可用空间时使用既有 `sidebar-scroll` 外层滚动，不增加另一套滚动模型。不为项目区强设最小保留高度；项目按布局顺序在置顶之后，过长内容通过整栏滚动可达。

## 先行验收

| 场景 | 可观察结果 |
|---|---|
| 0条 | 不渲染置顶标题或内容，项目正常显示 |
| 1/5/6/7/20/51条 | 真实挂载 Sidebar；任务行均32px，置顶列表自然高度分别32/160/192/224/640/1632px；顺序和所有任务保留，项目分组位于完整置顶内容之后 |
| 同一挂载5→6→5 | 经现有服务置顶/取消置顶并刷新；内容160→192→160px，项目起点相对置顶起点向下移动32px再恢复；无空洞、重叠或行压缩 |
| 足够高窗口、7条 | 前置为整个 sidebar-scroll 的置顶、项目和最近内容均未溢出；指针在置顶区滚动，行相对置顶容器不移动，无局部溢出裁剪；项目起点不变。若整栏溢出，外层滚动属于正确行为，不误判失败。不是用只渲染前五行实现 |
| 有限高度、20/51条 | 从置顶区域发滚轮时，内容经既有外层滚动移动；逐段实测可见行高及任务身份；首末置顶和后方项目可达，点击打开预定任务ID；行相对自然内容布局不变，无内部第二滚动偏移 |
| 用户手测 | 更新后确认5→6→7置顶自然增高、项目下移、长列表整栏滚动；标题/选中/运行/操作正常，浅深色及窄栏代表检查 |

数字均为逻辑像素，允许0.5px浮点误差。6−5=1行=32px；7×32=224；20×32=640；51×32=1632。置顶标题高度和既有分组间距保持不变，不计入上述内容高度。视口外行先滚入再断言，缺少观测不能当作任务不存在。几何验收不冒充原生图标/字体像素验收。

## 实现与测试边界

先在 `crates/vega_ui/src/sidebar` 新建本轮最小真实挂载测试模块，使用迁移后的临时文件 Store 与现有 conversation/sidebar organization 服务、真实 `Sidebar::new`。允许在 sidebar/mod.rs、threads_block.rs、organization/render.rs、organization/reorder.rs 增加不改变布局和事件的测试条件观测；必要时 projects_block.rs 仅隔离无关分支探测。允许 vega_ui Cargo.toml 的 gpui_kit test-support 与 tempfile 开发依赖及必要锁文件边，不升级原有包。具体支持均在 GREEN 后独立删除。不整套恢复旧测试，不仿制布局，不以源码字符串或样式参数代替实际几何。用行为命名、禁止代码注释。

测试先行提交并经主会话审查，draft PR 使用现有 Actions 取得目标自然高度/项目位置失败的有效 RED；接线/编译/依赖/runner失败不算 RED。冻结同一测试后才修改生产：`render_pinned_pi` 移除五行 `max_h`、`overflow_y_scroll` 和该列表的 `scroll_edge_fade`，保留自然纵向 Flex、外层禁止收缩、横向补偿、标签与任务渲染。保留全栏滚动和其渐隐，不修改其他分组行为。

如果最小移除不能满足验收，记录反证，先修订独立验收与先行测试周期，不靠实现后补测或放宽断言。

## 验证与交付

遵守 AGENTS.md、执行指南及 Rust 测试开发/交付生命周期。本地只做fmt、diff、offline no-deps metadata等快速静态检查，不运行应用、编译、构建、Clippy或测试。

云端沿用现有 PR gate：fmt、全目标 Clippy、三组 Python、Nextest workspace；每5分钟查询CI。保留 RED提交/源码与原始失败 → 最小实现 → 相同测试GREEN 的准确计数和链接；独立删除Rust测试/独占支持，最终head完整门禁确认Rust0发现/0执行后转正式PR合并。Issue不自动关闭，合并后In review等待用户手测，任务分支/worktree保留。回滚只经PR恢复本轮布局差异，不清除用户置顶数据。

## 本轮云端证据

PR [#501](https://github.com/puzige/vega/pull/501)。首轮 [39255654694](https://github.com/puzige/vega/actions/runs/39255654694) 为观测包装导致 fade 类型不匹配的编译失败，不计RED；改用固定 selector 读取真实 bounds。随后测试参数的多余借用仅做lint纠错，断言不变。

- 有效RED：`4ca8da8f`，[39259001179](https://github.com/puzige/vega/actions/runs/39259001179)。4执行、0通过、4失败、0跳过；实际160px对比192/224/640/1632px。Clippy成功。
- 最小实现：`f190ba54`，仅删除局部max-height/overflow/fade；[39260153873](https://github.com/puzige/vega/actions/runs/39260153873)行为4通过，Clippy因obsolete import失败，后续仅删除无用导入。
- 完整带测试GREEN：`c54d4c52da8090570f5c1da7e9165ec8688445de`，[39261265115](https://github.com/puzige/vega/actions/runs/39261265115)，4执行、4通过、0跳过；Python5+15+14=34及fmt/Clippy/required check成功。
- [同一冻结测试源码](https://github.com/puzige/vega/blob/c54d4c52da8090570f5c1da7e9165ec8688445de/crates/vega_ui/src/sidebar/pinned_natural_height_tests.rs)：blob `407499d6c10078a910b80b174da47882350e1e95`，SHA256 `893c592997f7c48643fce201caa57e19f72b32bbc54cce38b1421f3a99b424e1`；有效RED与GREEN之间未改测试目标、数据和断言。
- 独立删测后最新空集完整门禁待CI，结果固定在PR；不把空集称为Rust测试通过。

本地仅fmt/diff/offline metadata等静态检查成功，未运行Vega、编译或测试。发布安装和本轮用户手测仍待进行。
