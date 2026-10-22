# #545 对话滚动：投影去重与估高/预览缓存

- 版本：v1，第一层 spec 待独立审查；不是 Ready 实现。本轮仅文档，无 RED/GREEN。
- 基线：`feat/545-scroll-cache`，HEAD `2ba904f7074115de5ee89cc18c376c083f803357`；parent 已 fetch+ff，本执行不操作 Git/index/refs。
- 依据：[Issue 正文](https://github.com/puzige/vega/issues/545)、[最新检查点](https://github.com/puzige/vega/issues/545#issuecomment-6346665537)，均已直接读取。

## 1. 验收矩阵（先于方案；均待未来 Actions/用户验证）

| ID | 前置与操作 | 必须观察的结果与终态 |
|---|---|---|
| A1 | 非空会话实际触发一次生产 render，含 notice/阶段间距/导航栏 | 同一份 notice 投影、stage insets 各至多计算一次，列表和 rail 消费同一快照；预览、间距与锚点目标正确；空会话无残留 rail |
| A2 | 长历史缓存预热后，纯滚动或无关刷新且实际发生父 render | 未变且仍驻留的历史 entry 不再扫描正文估高或重建 preview；冷/暖产物一致且符合独立预期；不声称所有滚动必触发父 render |
| A3 | 流式生成同时上翻，追加、替换正文并结束生成 | 仅变化 entry 的内容派生项重算；历史驻留项复用，预览及时更新，阅读锚点/偏移保留，不抢焦点或强制回底；原有底部跟随语义保持 |
| A4 | 插入/删除/编辑/重排消息、前插历史分页及重叠去重，直至清空 | key 不随索引漂移；导航顺序、预览、定位正确，无已删项；未变驻留项复用，分页保持当前阅读 entry 与项内偏移 |
| A5 | 有效宽度改变、恢复；展开/收起正文、代码/工具等显示状态变化 | 受影响估高/预览失效，位置重新派生；测量随后修正几何；选字、复制全文、展开及导航可用，不用截断预览替代正文 |
| A6 | notice 增删/状态转换；切换空/非空 thread、返回；未来 continuation 历史显示变化 | notice/insets 与累计位置同步，无旧 thread/旧显示态污染；continuation 改变可见内容或布局必须分别进入相应 revision |
| A7 | 历史量超过缓存预算、单条超长回复、表格/代码块；驱逐后重访并接收真实测量 | 容量有硬上限、旧 revision 不堆积；必要 miss 可重算且结果正确；真实测量优先，估高不得冒充真实尾部到达观测 |

覆盖组合至少包含：长历史暖滚动、单条长回复、表格/代码、流式同时上翻；不是以简化 BenchStreamView 通过替代生产路径。

## 2. 已知事实与范围

- `ConversationStream::render` 已有虚拟 list；调用 `refresh_run_notices`、`stage_top_insets` 后构建 rail。
- `message_anchors.rs::message_anchor_geometry` 又投影 notices/insets，遍历**当前已加载 entries**，未命中实测高度时扫描正文估高，并构建 preview。
- 流式同步主要更新活动助手项；基线不是每 delta 全历史 reparse。纯滚动父 render 频率、生产耗时/掉帧主因未采样，用户安装版本未知，不承诺收益数字。
- 已有 bounded measured cache 的扫描范围 256、窗口 512；不靠扩大它替代本卡缓存。保留 list、导航刻度、滚动锚点及脱离底部跟随状态。
- 不重做 #544、不引入长消息内部虚拟化、不改 Markdown 管线、不用动画/隐藏导航掩盖成本；跨 render 的全量投影增量化另凭生产证据决定。

## 3. 最小方案与缓存契约

1. `render.rs` 在既有刷新顺序上形成只读 render 快照，向列表与 rail/`message_anchor_geometry` 传入已刷新的 notices 和 insets；移除锚点分支的重复派生，不复制正文。快照只承诺本次 render 一致，不复用失效快照。
2. `message_anchors.rs` 的正文估高与 preview 构建走两个独立有界缓存；仍可遍历已加载 entry 的轻量元数据，不宣称滚动整体 O(1)。`core` 仅承载必要缓存/版本状态，具体接口待共享卡冻结。
3. 缓存 key 的契约为 `(thread identity, stable entry key, content revision, effective width, display state/metrics revision)`；估高/preview 可使用各自相关的显示子集，但不得漏掉影响产物的状态。禁止用位置索引或临时对象地址作 identity。
4. `content revision` 由正文/可展示 payload 的 owner 在加载、编辑、流式更新等变更点维护；查缓存只读便宜元数据，禁止每 render hash、比较或复制全文来判断命中。现有 durable owner revision 是否覆盖全部路径**未知**，不能假定已有可直接调用 API。
5. `geometry revision` 表示顺序、成员、notice/insets、宽度/显示布局及真实测量变化，重派生累计位置；不能把它当 content revision 导致所有历史 preview 重建。只缓存 entry 局部产物，不把绝对 y/索引当跨布局稳定值。
6. 有效宽度取实际正文约束（含容器/内边距/滚动条影响），不是窗口外宽；字体、行高、缩放等影响结果的 metrics 进入 key。无有效布局宽度时走既有保守路径，不把缺省宽度记录当有效命中。
7. 实测高度与估高分域：只在 thread/key/content/宽度/显示版本匹配时消费真实测量；新测量更新 geometry revision，不覆写正文 revision。估高仅用于未测项 fallback，不能替代 #344 的真实尾部布局观测。
8. 缓存使用固定 entry 上限和确定性驱逐（如 LRU）；preview 另有总保留字节上限，单项超限不驻留；同 entry 旧 revision 及时替换，删除/thread 释放清理。不保留渲染树或第二份完整正文。
9. 预算常量与具体 owner/revision 接口尚未冻结：测试前按实际存储成本明确 entry/字节上限，并写满载/超限断言；不能以无界 map、仅限 entry 不限超长 preview 或扩大 measured cache 交付。

## 4. key / 失效矩阵

E=估高，P=preview，G=累计位置/布局派生；“复用”均以仍驻留且相关 key 未变为前提。

| 事件 | identity / content revision | E / P | G 与实测值 |
|---|---|---|---|
| 纯滚动、hover、无关刷新 | 不变 | 命中，不扫正文/重建预览 | 可重算轻量位置；本 render 共用 notices/insets |
| 流式追加/修订/结束、消息编辑 | 稳定 key；实际 payload 变化才升 content；终态显示变化另升 display | 只失效变化项的相关产物 | 更新受影响后缀；旧版本测量不可用 |
| 新增/删除/重排、分页前插/去重 | 新项新 key，旧项不因 index 变而换 key/revision；删除退休 | 未变项复用；新增 miss，删除清理 | 成员/顺序变则重派生；分页保留阅读位置 |
| 有效宽度/字体/缩放变化 | content 不变，width/metrics 改变 | 相关 key miss；不得误用旧宽度 | 旧约束测量失效，等待真实新测量 |
| 展开/折叠、可见类型/工具状态变化 | payload 未变则 content 不变，display 改变 | 相关 E/P 失效 | 重派生并淘汰不匹配测量 |
| notice 增删、状态/阶段间距变化 | 正文 content 不变；notice/display 版本更新 | 未影响正文局部产物则复用；影响项失效 | 快照一致、G 更新，不能沿用旧间距 |
| thread 切换/清空、同 entry id 属于不同 thread | thread 隔离；释放旧 owner | 清理或隔离，绝不串用 | 重置相关位置，旧 thread 测量不可污染新树 |
| 真实测量到达/修正 | content 不变 | P 复用，E 不冒充测量 | 匹配版本的 measured 优先，G 更新 |
| #544 pending 确认/持久化 | 消费 stable entry key/durable owner revision 需求；映射待确认 | 内容变才升 content；身份替换须显式退休旧项 | 不因临时/持久身份漂移丢失阅读定位 |
| #535 continuation 历史显示变化 | payload 变化升 content；仅布局变化升 display | 按相关版本失效 | 更新 G；接口未知，不替该卡决定 |
| 驱逐/单项超字节预算 | 不伪造 content 变化 | miss 重算；空间仍有界 | 输出与未驱逐一致，不改变跟随状态 |

## 5. 先行测试与观测路线（未来执行）

- 使用真实生产 `ConversationStream` renderer、实际缓存和更新路径；GPUI `test-support` 的 `TestAppContext`/`VisualTestContext` draw 驱动布局。parent 已静态确认 registry `gpui-kit 0.6.1` / `gpui-pre 0.3.4` API；本轮未运行 harness。
- 从真实 `ListState::logical_scroll_top`、`bounds_for_item`、`viewport_bounds` 观测 entry/项内偏移、可见项与布局；draw 后结合 rail 实际产物、独立预期 preview/目标及真实测量断言。`painted_quads` 仅补充绘制结果，按 scale factor 换算；focus 用真实 focus 状态，不用截图猜。
- 最小只读计数探针限模块内 test-only seam，不扩大生产 public API；必须委托真实投影、正文扫描、preview 构建及行构建，记录缓存 hit/miss/保留量；不镜像 cache、不 mock 被测决策、不以源码 grep 代替行为。次数只证明 A1/A2 的必要不重复工作，并同时断言结果、位置和交互终态。
- 阳性对照先证明探针挂在真实路径且本轮实际 render；再分别改变正文/宽度/显示态，确认对应 miss 和可观察产物更新。未发生 render 的零计数、只有计数无结果，均不算命中证据；接线问题与产品缺陷分开诊断。
- A1–A7 的正常/失效/恢复/有界用例在生产修改前一次写齐，独立审查并冻结目标、数据、预期；不后补、不放宽、不 ignore、不重试换绿。新增边界/验收变化先停轮并重开 TEST-FIRST。
- 只复用 `.github/workflows/pr-check.yml` 现有 macOS Nextest：`cargo nextest list --workspace --message-format json`，非空执行 `cargo nextest run --workspace`；不新增 workflow/执行设施。先现有 Actions 有效 RED，再最小实现、同一冻结测试 GREEN；最小接口缺失须标注行为断言未执行，无关编译/runner 故障不算 RED。
- 固定基线、测试源码/补丁/完整 SHA、RED 原因及 run/job、实现顺序、同测试 GREEN 与实际发现/执行/通过/失败/忽略/跳过数；再独立提交删除 Rust 测试及独占接线、独立审查生产差异，最新 head 完整 required check 验证 `no Rust tests`（0 发现/0 执行）。空集不是 GREEN，fmt/Clippy/Python 等既有门禁不降低。
- 生产投影/估高耗时、缓存命中、行构建、真实帧间隔尚无证据；简化 bench 不含生产 rail。后续获授权的生产观测须绑定实际构建、场景、历史量/宽度并比较同条件结果，不能用测试平台帧间隔宣称用户丝滑。
- 合并后由用户手测上述四类场景，以及导航/选字/复制/展开、上翻不被抢位与底部跟随；先核对安装版本。本地不运行 Vega、编译/check/Clippy、测试、bench/profile 或 CUA。

## 6. 集成前提、下一验证与回退

- #344 活跃范围 `message_anchors/core/render`；#535 活跃范围 `content/run_activity/phase/core/message_anchors/pending/render_rows`。本 spec 可独立审查，代码/测试写入须等 parent 协调接口冻结或集成，不覆盖其实现。
- 下一具体验证：parent 独审本版矩阵；取得 #344 真实尾部测量接口与 #535 continuation 的显示/revision 契约，核对 #544 pending→durable 身份消费，再在变更 owner 逐项确认 revision 更新入口、缓存预算及真实 renderer 探针接线，才冻结先行测试包。未知接口不在本轮臆造。
- 尚无性能采样、RED/GREEN、独立审查、CI 或用户手测结论；本轮只做文档快速静态检查，不写源码/测试/Cargo/workflows/索引/Issue，不 commit/push/PR。
- 未来回退仅撤销本卡缓存与同 render 去重接线，保留既有 list、measured cache 及他卡接口/功能；若需改变已冻结行为，先记录验收变化再重开周期。
