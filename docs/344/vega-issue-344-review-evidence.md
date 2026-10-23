# #344 独立审查记录

## 方案审查（生产未改）

- 生产基线：`871f91f246e90753743a256cee1eec315e74effc`。
- v1 规格 SHA-256：`154fad30278fbb4a3208e29871bc666b5532ce7455bad82506db87ce341e81c5`。
- 独立 reviewer 为非实现写入者，仅静态读取固定生产基线及规格；未读取正在写入的测试 diff，未编译或运行。
- v1 阻塞点：GPUI `bounds_for_item(last)` 不保证 top 到 tail 中间已测量，未知高度可在坐标前缀中按零贡献；生产先计算 rail 后布局列表，没有布局后重新计算的保证。
- 另需闭环：未布局/无有效阅读视口时隐藏的明确验收；marker active 宽度同时代表 current 与 hover/keyboard，必须在 neutral 输入状态观察当前位置，并用不同的顶部/末条 ID 制作负例。

## v2 增量审查

- v2 规格 SHA-256：`3d2cbd53f810042e9fc5330a670a8ee7ef9678c7c5d1fd898152be06ff10fec4`。
- 结论：方案增量可接受，无新增方案级阻塞；不代表测试/实现已验证。
- `canvas` 第一回调在 prepaint，普通父 div 按子节点顺序 prepaint，故 list 后的观测节点可读本轮列表布局结果；公开 `Div::on_children_prepainted` 可作等价入口。
- ListState 没有公开 padding getter。观测必须捕获本帧传给 `.pb(...)` 的同一个 foreground 值，与本轮 viewport 配对；不能 defer 后再读取新 foreground 与旧几何混算。
- post-layout 当场形成一致快照；defer 中比较/提交，只有投影变化才 notify，通知应驱动真实生产实体重绘。不允许测试用额外 notify 或强制重复 draw 掩盖时序。
- 连续测量验证必须接受合法零高隐藏行，不把 height == 0 误当未知；未测量/无有效宽度与零高已测量应区别处理。
- 测试审查待写入者 READY 停写后进行，云端 RED/GREEN 尚未取得。

## 主 Agent 附加静态证据

锁定 gpui-kit 0.6.1 的 `TestWindowExt::render_frame` 内部直接 `refresh()` 后 `draw()`，不是只推进已调度帧。进一步核对发现 `TestAppContext::test_window()` 为 `pub(crate)`，不能从本 crate 使用底层测试窗口的 scheduled-frame 方法。公开可用路线是正常 `App::update` 在 test-support 下通过 `flush_effects` 绘制 dirty window、排空 deferred effects；必要时公开 `Window::simulate_next_frame` 只执行已排队回调。不能用强制重绘证明生产布局后自主刷新。

## 原始测试 payload 独立审查

- reviewer：`01M5J3M8XE2KQE5QRN5TXD5BGP`，execution `01M5J6W1C1N4HJC0X5HVDV65RS`；只读完成，未运行代码或测试。
- 输入：方案 v2，原测试清单 SHA-256 `da70196e0562a9152e9874993564fe897ee0543012f7178bc778ac7f0948570d`；测试源码 SHA-256 `a58101c1ad60b01a216fa762b14a01f7c08c5ec316e5b8e5ce395fd75816b142`；patch SHA-256 `8d22db2786740bfe6dfd3c35359960835b43d1736d8f2ca87b17c39c802fcf05`（34,690 bytes）。前后哈希一致。
- 结论：暂不通过；尚不能用于冻结/有效 RED。四项阻塞均为先行测试的接线或既定覆盖缺口，不是生产修复，也不改变方案 v2 验收。

| 项 | 原始证据 | 修订条件 |
|---|---|---|
| B1 | `settle` 两次 `render_frame` 无条件 refresh/draw，掩盖布局后通知缺失 | 使用自然事件/update/executor，必要时有界推进已排队回调；禁止强制刷新和断言前补通知。深色主题切换无宿主订阅，改为初始化主题或明确独立主题动作，不把刷新混入几何验证 |
| B2 | 测试宿主绕过生产 `render.rs` | 冻结前固定共享观测入口。优先由既有 `render_message_anchor_rail` 的返回元素携带布局后观察器，生产与宿主均在 list 后挂载，隐藏轨道时观察器也存在；RED 前不实现判底算法 |
| B3 | A2 的 unknown 仅在 top 之前，未覆盖 A3 的 cached tail + unknown middle | 使用真实 ListState，明确 gap/tail bounds 与索引关系；保留不同顶部/末锚点、neutral pointer 和非键盘状态，不注入判底布尔值 |
| B4 | 无效几何负例后直接结束，未验证同实例恢复 | 恢复 mount/尺寸/foreground 的一次正常动作后自主显示轨道；不靠额外鼠标移动、notify 或 draw |

保留已审可行的真实 quad/token/mask、body bounds、预览 label/panel、稳定 ID request→reveal 和焦点断言。补齐长轨道在鼠标移出/滚动后跟随当前位置及再转键盘路径，避免四锚点 fixture 无法区分九项窗口锁定。合法零高已测量行沿用 v2 方案义务。

生产隔离独立结果：去掉三个 test-only wrapper 与模块接线后源码逐字节等于基线；定位、token、CI 未改；test-support 闭包未升级既有依赖。9 个测试为源码清点，不是发现/执行数。下一步由单一接续作者修订，固定新版本后增量复审，再交现有 Actions。

### B2 共享入口决策

同一独立 reviewer 的后续 execution `01M5J8XWWC9SSWJYWKX5XH9SZH` 确认共享入口规划成立：生产 `render.rs` 先挂 body/list、后挂 `message_anchor_rail_overlay`；测试 `AnchorHost` 同样先 list、后 `child(rail)`。RED 阶段保留现有函数签名与生产行为，在有效行为 RED 后让 `render_message_anchor_rail` 返回元素携带观察器即可；不新增 API，不需要实现后追改测试接线。

此项仅关闭 B2 的入口规划义务，不批准未见测试或实现。实现须始终挂载观察器（隐藏分支也不能空返回或 display:none）、不占正文布局或拦截输入、不改变 rail 定位参照，使用同帧 padding 并在 post-list 回调形成投影，defer 只比较/提交变化。B1/B3/B4 与长轨道增量仍待新 payload 审查。

## 修订测试增量审查

- 非实现 reviewer `01M5JB0VZGJH448QXX457Z6KNC` / execution `01M5JB0VZG095BJPSQT53FFHJM` 已只读完成并停写；结论：静态增量可接受，无阻塞提交 draft PR 的测试接线或 fixture 缺陷，不代表编译、行为测试通过或已取得 RED。
- HEAD `2ba904f7074115de5ee89cc18c376c083f803357`，仅在旧基线后新增 docs/554；测试 SHA-256 `5d5f7d00482143fe83cf747e06d8366b753dab36c69e9d7e90c63ed27897656f`，repair evidence SHA-256 `5e85753824d507c0b174ffeef03080df50ccace9363846cb65c5130c567c6618`，manifest `d9b4bd8193cb4521d7406f2ce51449b364fca058abf0814adcac7607f374ee6d`。reviewer 独立重算、前后身份一致。
- B1：settle 只消费已排队回调/正常 dirty-window update，无强制 refresh/draw 或补 notify；主题在 fixture 初始化，静态可行。
- B2：既有返回元素在生产与测试均位于 list 后挂载，保留共享入口方案及隐藏常驻、同帧 padding、changed-only notify 义务。
- B3：splice 保留 tail 缓存并清除中段 hint；真实 overdraw 下五个 260px 行达到 1300，超过 viewport+overdraw 的 1200，gap 可保持未知。该负例可用于本轮冻结，但 tail 位于视口下方，不单独证明连续测量防护。v2 的 top→tail 连续有效测量、遇未知拒绝、接受合法零高、提前停止不扫描全部历史，仍是后续生产独立静态审查义务，不豁免或改验收。
- B4：四种无效状态同实例恢复仅一次正常 host notify，后续无鼠标移动/补绘制；stream observer→host 重绘路径可达，实际自主恢复待 CI。
- 长 rail：总高 3980，click12 后 top3120，上滚2000 后 top1120=index4/offset80；九项窗口由7..16转0..9，再按键盘可从保留 ID12转11，前提成立。
- 零高尾行：总高1080、阅读420，回填 top=index0/offset660；消息2在340..420、零高尾行420..420，Measured高0不是unknown，负例顶部与末锚点区分成立。
- 生产隔离、既有锁定包不升级与 test-support 范围重新核对。reviewer 执行静态身份/差异及 `git diff --check`，未编译/运行/测试。组合测试未到达断言不算执行；只在 Actions 原始日志证明目标行为缺失后才能记有效 RED。
