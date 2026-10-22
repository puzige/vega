# #551 消息与代码复制按钮原位短回执

状态：SPEC/API/先行测试设计，v1 候选；不表示已实现、RED 或 GREEN。

- 生产基线与本 worktree HEAD：`871f91f246e90753743a256cee1eec315e74effc`。
- 分支：`feat/551-copy-receipt`；worktree：`vega-551-copy-receipt`。
- 仅写本文件；不改生产、Cargo、测试、workflows、`docs/README.md`；不 commit/push/PR/Issue 写入、不嵌套委派。
- 父已 fetch/创建/领取，远端只读核对 master 与本地 origin/master 均为上述 SHA；未发现新远端影响。根 AGENTS 一致且目标路径无嵌套规则；已读 exec-guide、issue-workflow、design-guidelines、kanban skill、test-delivery-lifecycle。
- 已读 [#551](https://github.com/puzige/vega/issues/551) 正文及 [2026-10-22 16:27:05Z 最新评论](https://github.com/puzige/vega/issues/551#issuecomment-6345796987)，并读取 [#517](https://github.com/puzige/vega/issues/517)、[#519](https://github.com/puzige/vega/issues/519) 正文和评论（读取时均 OPEN、无评论）。后二者是未实施的独立范围，不是本卡授权。

## 1. 场景与验收矩阵（先于方案）

A=Actions 生产路径验证，M=用户真实系统手测；所有结果尚未取得。

| ID | 前置/操作 | 必须观察的结果 | 证据路线 |
|---|---|---|---|
| C01 | 现有可复制用户消息、可信 Native Final、代码块，各 mouse/Enter/Space 激活 | 原位 Copy→Check→Copy；真实非空复制路径执行后才产生回执，重复点击 Check 仍复制 | A：生产按钮事件、测试平台剪贴板原文、实际绘制边界 |
| C02 | 无源/空文本、动作未执行或可观测拒绝；平台不能报告写入结果 | 不预演回执、不伪报成功；API 不可观测失败必须如实披露，不读回用户剪贴板诊断 | A：空源/未激活负例；M：真实系统集成边界 |
| C03 | 一次复制、约 1s 超时；到期前多次重按 | 按最新一次执行时间续期，旧 timer 不能清新回执；无禁用、无无界任务/队列 | A：测试 executor 时钟、生产 timer/lifetime 和结果 |
| C04 | 两条消息与同消息两个代码块先后复制 | 回执按稳定身份独立；一项续期不改变另一项，不随列表索引重排串状态 | A：真实列表、原文与多 owner |
| C05 | 回执中 hover-out/blur；稍后 hover/focus 重入；Fork 并列（其卡集成后） | 消息按钮未 hover/未聚焦时全部按原规则隐藏，不能为了回执留下 Copy/Check；重入只显示未过期回执；Fork 不变 | A：生产 hover/逐按钮 keyboard focus、绘制；集成回归不可省 |
| C06 | 代码常驻按钮；消息/代码浅深主题、窄宽窗 | 代码仍常驻；按钮 ID/24px 热区、28px 消息页脚、正文位置和焦点稳定；Check 不改变动作含义 | A：debug_bounds/focus/布局；M：视觉 |
| C07 | 卸载、虚拟列表回收、线程/页面切换、旧 timer 到期 | 无旧状态串入/旧 owner 通知，无已卸载实体复活；重新进入不重播过期回执 | A：真实 owner 路由与弱引用生命周期 |
| C08 | Reduce Motion 开前/运行中、隐藏/失活后恢复 | 直接切状态但保留短回执；隐藏/失活不持续装饰帧，不补播过期动画 | A：测试门控/时钟/帧；M：OS Reduce Motion |
| C09 | Tab、Enter/Space、辅助技术 | 可访问动作名称仍是复制；反馈以非颜色/非运动语义可理解，不新增可见反馈文案/toast；具体 a11y 观测限制见后文 | A：真实 focus/事件、生产节点描述；M：VoiceOver |
| C10 | 用户原文、Native 可信 Final、代码语言/尾换行/空行、选区/上下文菜单、Codex 各状态 | 剪贴板逐字保持当前来源；#529 vendored mdstream 基线保留；不改 Native 边界、Codex 谓词、选择复制或 Fork | A：合成精确文本；#517/#519 集成后按其冻结规则覆盖 |
| C11 | 长会话/流式重绘同时复制，期间操作其他按钮 | 回执不重启、不触发正文重排，不添加成功条、朗读/点赞等动作；不得把本地静态检查当性能证明 | A：生产 renderer；M：真实体验 |

## 2. 本轮补齐与真实源码边界

2026-10-22 续接修订 v2：主 Agent 在保留原矩阵后补齐方案。任务树已从上述审计基线快进至 `2ba904f7074115de5ee89cc18c376c083f803357`；差异仅 #554 文档，生产源码不变。本轮仍只有文档，没有测试或生产补丁。

| 符号（当前生产基线） | 已核事实 | 本卡边界 |
|---|---|---|
| `conversation_stream/render_rows.rs::code_header`（822–859） | 当前代码源是 `Arc<str>`；非空后写 clipboard；按钮 ID 由 owner/ordinal 拼成；代码按钮常驻 | 不改原始代码字节、语言、尾换行或常驻规则；序号不能单独充当反馈身份 |
| 同文件 `message_with_copy`（1475–1527） | `MessageCopy` 克隆到按钮闭包；固定页脚；逐按钮 opacity/group_hover/focus_visible | 不把 opacity 放到整个操作栏，不因回执改变 footer、Fork 或原复制谓词 |
| `model.rs::MessageCopy::{has_text,copy,content_generation}` | 原文可为空；非空才写入；有稳定本地 ID 和内容代次，copy 返回 `()` | 空源无回执；内容变化后旧回执无权声称新内容已复制；不改选区复制 |
| `icons.rs::icon_button`（293–330） | 24px 默认框、Tab、Enter/Space、aria_label、tooltip，当前 child 固定 Copy | 如需可变 glyph，只增加真实消费者所需的窄组合点；不重写交互或依赖 |
| 锁定 `gpui-pre 0.3.4::App::write_to_clipboard` | 调用平台写入并返回 `()`，无系统成功/失败回执 | Check 仅表示本应用已执行非空复制调用；不声称系统接受、其他程序可粘贴或粘贴成功；不读用户剪贴板做生产核验 |

## 3. 单一候选方案与寿命

- 使用 `Motion::COPY_SWAP_MS = 80` 与 `COPY_RECEIPT_MS = 1000` 候选；由 #549 单一写入者提供，曲线用锁定 Kit 默认 ease_out_cubic。它们是产品值，不是官方规定或实测收益。本卡不加私有时间常量、引擎或全局反馈 registry。
- 仅在真实非空复制调用返回后，记录该按钮身份、来源内容代次、最后执行时间及失效代次；同事件使按钮进入 Check。重复 Check 仍复制当前来源并把期限续至最近执行后 1000ms，不禁用按钮。
- 第一阶段只对图标像素做 80ms 有限过渡；固定槽、命中、可访问动作及焦点不淡入。超时回 Copy 不播放第二轮成功效果。Reduce Motion 直接换图标但保留限时反馈。隐藏或失活时不播放装饰，期限仍按真实时钟流逝，恢复只读尚有效状态。
- 消息反馈身份使用现有 MessageCopy 本地稳定 ID 加窗口/来源命名空间，不用行 index、文本、标题或每帧随机 key。代码反馈至少使用消息 owner 与 mdstream block identity，不把可变 ordinal 当唯一身份；来源改变后旧 receipt 失效。block_id 在 pending→committed/重排的稳定性须在先行测试前核实，不能每 render 全文 hash 来补救。
- 首选用按钮局部 element state 持有一份 deadline、generation 与至多一个到期任务。续期更新 deadline；已存在任务到旧期限唤醒时只重读期限，未到期则等待剩余时间，不为每次点击再派一个任务。卸载后不延长 entity/窗口/线程生命；任务仅保留弱 owner，失效或销毁后不再通知。若锁定 element state 不支持随卸载清理，先明确最小 owner/token 接线，再冻结测试，不改为无界 timer 列表。
- 截止状态与视觉调度分开：1000ms 是一次性期限，不是连续 redraw；隐藏按钮可以保存未过期期限，但绝不覆盖现有 hover/focus 显隐。鼠标移开后 Copy/Check 与 Fork 按 #519 原规则消失；代码按钮保持常驻。C05 的集成回归不能因 #517/#519 尚未合并就删除。
- 动作 aria label 始终指向“复制”；反馈状态可在同一节点描述短暂“复制操作已执行”，不增加持久提示、toast、第二个焦点目标或每次重复播报的独立 live region。不得使用暗示系统已确认的措辞。实际平台无障碍支持范围在先行测试时核实，完整 VoiceOver 交用户。

## 4. 验收对应的先行测试包

生产未改时一次写齐 C01–C11 的必要测试及最小观测接线，经非写入者独立审查后在现有 draft PR 执行 Actions；不等实现后补测，不用矩阵代替测试。

| 包 | 必需行为与对照 | 被测边界 |
|---|---|---|
| T1 原位反馈/精确来源 | 三类真实按钮 mouse/Enter/Space；非空执行后 Check、1000ms 到期恢复；空源/未激活不产生反馈；逐字 clipboard 对照 | 真实 message_with_copy/code_header/MessageCopy 与图标 paint；测试平台 clipboard 仅含合成内容，不读生产剪贴板 |
| T2 续期/身份/销毁 | 900ms 再次点击后旧期限不清新状态；两消息两代码块独立；内容换代、虚拟列表回收、A→B→A、销毁后旧任务无效果；大量重复点击任务数有界且终态正确 | 实际 element state/任务及弱 owner；不测试一个镜像 receipt helper 来替代生产按钮 |
| T3 显隐/几何/焦点 | hover-out 与 blur 隐藏，重入仅未过期回执；Fork 同规则；常驻代码；Tab 焦点/24px 默认框与 Layout 实际尺寸、28px footer 和正文 bounds 均不变 | 生产组 hover、逐按钮 focus、布局和实际输入接收者；不以 screenshot 判断焦点 |
| T4 动效生命周期/真实渲染 | 80ms 中间图标绘制、终态；Reduce Motion 即时；hidden/inactive 不连续装饰帧；长历史/流式 notify 不重启或重排 | 锁定 Kit executor 时钟、真实 glyph paint 只读 observer，加按钮 bounds/语义；quads 本身不证明 glyph |
| T5 范围回归 | 用户/可信 Native Final/代码精确原文、语言换行、选择复制、Codex 与 Fork 最新冻结谓词、浅深和窄宽 | 真实生产 renderer 与已有路由；#517/#519 集成时恢复其可访问冻结回归，不恢复历史测试全集 |

同轮阳性对照：先证明基线 Copy glyph、真实按钮事件、精确测试剪贴板内容、有效 FocusHandle 及布局可观测；启用 test-support 的 executor 时钟可驱动一次真实到期任务。新行为缺失（没有 Check/超时回执）是目标 RED；空 recorder、无效 focus fixture、误读背景、依赖/编译接线错误不是。真实图标 alpha 观测需在实际 child paint 边界，不能只断 sampler 返回 0.5。必要帧观察要隔离本装饰来源，不把 Thinking/流式帧计为泄漏。

## 5. 实施顺序与未决项

1. 主 Agent 先冻结上述寿命、代码稳定身份和现有 API 的可行性；#549 提供共享策略，本卡消费。#550 可能同改 icons.rs；#535 可能同改 model/render_rows。按精确符号及同步基线串行集成，不从旧 ref 整文件覆盖。#517/#519 未完成时不在本卡顺带实现其复制谓词/Fork。
2. 下一专职 writer 先写齐上述完整测试与最小 cfg(test)/test-support 接线，不改生产行为；主 Agent 独审测试后创建/复用现有 `pr-check` draft PR。有效 RED 后再写最小产品实现，保持同一冻结测试 GREEN。
3. 固定可访问测试源码/原始 patch/完整 SHA、RED 与 GREEN run/job 和准确计数，再独立删除 Rust 测试及独占支持；最终 head 完整门禁要求 0 发现/0 执行并明确 `no Rust tests`。Python、生产与现有 CI 能力保留；不另建测量服务或真实系统 E2E。

| 未决 | 下一验证 | 当前不允许宣称 |
|---|---|---|
| 代码 block identity/内容换代 | 从生产 flatten→code_header 核实际传递，先行测试覆盖插入/重排/pending转committed | ordinal 永远稳定或反馈已不会串对象 |
| element state卸载、单任务及弱owner通知 | 核锁定生命周期/Task Drop；同补丁销毁阳性负例经 Actions 验证 | 无泄漏/无迟到通知已通过 |
| glyph/a11y/装饰帧观察 | 只读生产 paint/节点接线独审、首轮现有 Actions 对照；VoiceOver 用户手测 | helper/quads 等于完整系统无障碍或像素证据 |
| 真实平台 clipboard | API无结果返回，本卡仅如实表达调用；不新增剪贴板权限/回读/日志 | 系统写入或后续粘贴成功 |

本轮纯文档静态检查，不制造 RED/GREEN，未运行 Vega、构建、测试、bench/profile、CI 或 CUA。用户手测保留真实系统复制/粘贴、Reduce Motion、浅深窄宽、键盘与 VoiceOver；未验证不标通过。回滚仅撤本卡局部 glyph/receipt 接线，保留复制原文、显隐、Fork 和共享机制的其他消费者。
