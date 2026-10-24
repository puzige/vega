# #551 原位复制回执 — 接管实现规格 v3

## 基线与权威

2026-10-24 用户明确选择接管本卡；占用评论：<https://github.com/puzige/vega/issues/551#issuecomment-6387147578>。

基线 `0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`，分支 `feat/551-copy-receipt-takeover`。
旧文档保留在 [v2 完整版本](https://github.com/puzige/vega/blob/967ad9f6165c19b5118e23b1842d6b0d99812c9f/docs/551/vega-issue-551-copy-receipt.md)；不修改旧工作树。
本规格保留其 C01–C11 产品验收，以现行 AGENTS/执行总纲 §7.2 替代强制先行测试/RED 时序；不是取消回归或降低验收。

## 目标与非目标

现有用户消息、可信 Native Final、正文及 Thinking 中实际存在的代码复制按钮，在非空复制调用执行后，原位 Copy→Check→Copy。
不改变剪贴板来源、文本字节、Native/Codex 谓词、选择/菜单复制、Fork 或其他卡片行为；不新增 toast、可见说明条、依赖或动画引擎。
#517/#519 当前 OPEN、无评论，不在本卡实现；其将来集成须保留相同显隐/复制边界。

## 已核源码与方案

- `render_rows.rs::message_with_copy`：现有逐按钮 opacity/group_hover/focus_visible，固定页脚；保留原按钮 ID、24px 热区、页脚和布局。
- `render_rows.rs::code_header`：目前 owner/ordinal 按钮 ID，非空源调用 clipboard。代码 header 仍常驻，不移动复制标题。
- `model.rs::MessageCopy`：稳定 ID 和 content_generation，原文由 RefCell 持有；`copy()` 当前返回 `()`。可窄改为返回是否实际执行非空调用，现有无反馈消费者忽略返回值，不读回剪贴板。
- GPUI `App::write_to_clipboard` 返回 `()`，不能证明系统接受或粘贴成功。Check 仅代表非空调用已执行；可访问描述据此命名，动作名称始终为复制，不冒充 OS 成功。
- `interaction::{decorative_transition,decorative_value}` 已在 master 提供共享 Reduce Motion/失活策略；不再等待 #549 的历史接口缺口。不改该共享机制，不复制自建引擎。
- GPUI `Window::with_element_state` 仅在绘制阶段调用；`use_keyed_state`/element state 提供局部状态，`code_drag.rs` 已有 MountLease Drop、弱引用与有界 Task 范例。必须尊重阶段约束，不在普通 render 中直接调用仅供 prepaint/paint 的 API。

实施范围：新增 `conversation_stream/copy_receipt.rs`（或同级窄组件）并在父模块接线；`render_rows.rs` 的两个按钮入口；`model.rs` 必要返回值/代码身份；`icons.rs` 仅增加支持动态 child 的窄组合点，保留普通按钮原行为；`vega_theme::Motion` 增加 `COPY_SWAP_MS=80`、`COPY_RECEIPT_MS=1000`。不改通用 CI/Cargo/其他卡片文件。

### 身份

消息：使用稳定 MessageCopy.id 与来源 content_generation；点击读取最新源，内容换代立即使旧回执失效。
代码：使用消息 owner + mdstream block_id + 块内 node_identity（flatten_node 已有计数），不能只用可变的跨块 ordinal。在 StreamCodeBlock 增加必要身份并传给 header；同一源的多行共用同一身份，插入其他块不得串状态。内容版本变化清除回执，不每帧对全文 hash；可复用 Arc 源身份/等价有界版本判断，重新物化时宁可失效也不显示错误回执。
不同按钮/线程/窗口隔离，无全局 registry；元素卸载后旧任务不能延长 owner 生命周期或通知新对象。

### 时序与动效

真实非空复制调用返回后设置 deadline=executor.now()+1000ms；重复按 Check 仍复制并更新 deadline。
每按钮至多一份到期 Task；续期不不断派发任务，旧期限唤醒重读最新 deadline，未过期则等待剩余时间。只捕获弱 owner，Drop 取消；不得 detached 无界任务。
图标槽内 80ms 有限过渡，超时直接恢复 Copy，不重播成功效果；动画只覆盖 glyph，不改变焦点、命中、布局或父 opacity。
使用共享 decorative 策略/Kit sampler；隐藏、失活、Reduce Motion 不播放装饰帧，恢复仅显示未过期状态，不重播过期动效。1000ms 期限不是连续帧循环。
消息回执绝不覆盖 group_hover/focus_visible；hover-out/blur 后照常隐藏，重入仅显示剩余有效回执。代码按钮仍常驻。

## 验收（C01–C11）

1. C01：现有用户消息、可信 Final、代码按钮 mouse/Enter/Space 均执行精确非空复制，再显示 Check；重复点击继续复制。
2. C02：空源/没有执行不显示回执；无 OS 返回值时不声称系统成功，不回读或记录用户剪贴板。
3. C03：1000ms 后恢复；900ms 续期不能被旧 timer 提前清除；任务数有界。
4. C04：两条消息/两个代码块独立；身份重排不串状态，来源更新失效。
5. C05：hover-out/blur 隐藏、重入只显示未过期状态；Fork 显隐及行为不变。
6. C06：代码常驻；按钮 ID/热区/页脚/正文几何与焦点稳定；浅深/窄宽无布局跳动。
7. C07：虚拟列表回收、卸载、线程切换、销毁后旧任务无串状态/复活。
8. C08：Reduce Motion 直接切状态仍保留短回执；隐藏/失活不持续调帧、恢复不补播。
9. C09：Tab/Enter/Space、可访问复制动作不变；Check 的非颜色/非运动反馈可理解，必要节点描述仅表示调用已执行，VoiceOver 交用户验证。
10. C10：复制原文/语言/尾换行/空行不变；Native/Codex 原可信边界、选择复制及 Fork 保留。
11. C11：流式/长历史普通重绘不重启回执、不触发正文重排；不以静态检查声称性能改善。

## 按风险验证与交付

本地仅 diff/格式/静态检查，不运行 Vega/编译/测试/Clippy，不使用 CUA，不安装或重启应用。
生命周期/身份/续期是主要回归风险，可选少量开发 Rust 回归，自动执行只经现有 Actions；不恢复历史集合，不为流程制造无意义测试，不用源码字符串断言替代行为。开发测试若使用，固定完整 SHA/原始源码/实际云端结果，合并前清理测试及专属支持。
独立非实现者审查同一不可变 head，最终 head 必须完整 required CI 成功且 `no Rust tests`（0 发现/0 执行）；Python/生产安全断言/CI 不变。CI 每 5 分钟查一次。
原生复制/粘贴、键盘、VoiceOver、系统 Reduce Motion、浅深/窄宽、长会话实际体验由用户手测，不以 CI 代替。未验证写待验证。
最新 required CI/独立审查通过后 squash merge，Issue 保持 OPEN，状态 In review 等用户手测。回滚仅本卡按钮回执/身份接线及新增未被其他消费者使用的 token，不动复制数据与共享机制。
