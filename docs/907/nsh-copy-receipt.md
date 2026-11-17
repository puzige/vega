# NSH 残余 CopyReceiptButton

## 范围与冻结验收

本路写入 `conversation_stream/copy_receipt.rs`、`copy_receipt/native*` 与本文件。本轮父追加独占授权：`render_rows.rs` 仅可信 owner 传播、`selection_menu/mounted.rs` 仅 `list_bounds` 可见性；实际 owner 接线须先证明生产可达，不为消除警告制造不可达绑定。不修改共享 render/mod/core、message_footer、权限业务、持久化、OS acquisition、依赖、vendor 或 Kit/Fast。保留工作树全部其他 dirty；父已确认 fetch/祖先，本路不运行 Git/网络/Cargo/native。所有实现先完成，再集中独审；本路交接不是 review。

- 复制图标、hover/pressed、键盘 focus、tooltip、Copy→Check 交换和回执生命周期必须由真实 NSH leaf 绘制；GPUI 只承担透明布局、原 builder 回调、焦点/AX 桥和 Surface 挂载观察，不保留可见 GPUI 按钮。
- 保留 `new(id, source, label, colors, style: FnOnce(Interaction) -> Stateful<Interaction>)`，style 原样执行一次，保留尺寸、id、事件回调能力。任意自定义 GPUI 绘制不作为原生绘制后门；标准 hover/focus 由 NSH 使用同一主题 token 表达。
- 旧 API 只有 Arc 原文，没有会话/根授权。增加 `with_owner(stream, source_id, code_identity)`，没有可信绑定时 fail closed，不能从 selector/index 猜 owner。父只负责最小接线。
- 当前会话必须匹配 ReadingOwner 的 route/root/epoch，当前 actions enabled、未 suspended，消息 identity 和 copy id/generation 必须匹配，代码 identity 与 Arc 原文必须仍属于该消息当前 StreamModel。源通知、路由/权限变化、卸载、窗口激活/几何变化失效旧动作；A→B→A 不复用展示或意图身份。
- pointer、AX、keyboard 共用 NSH 异步测量和可信出队处理；检查当前 owner/source/root/generation、mount lease、已 paint 投影、bounds/clip/list viewport/foreground/window/scale、实测 root/control 与测量 revision。NSH presentation 已死或测量过时拒绝；不以旧焦点、旧 index 或旧 lease 直接授权。
- 每 leaf 独立 Tab/Shift-Tab 焦点；Space/Enter 仅未修改、非 repeat、非 IME character-preference 且仍持焦点时触发，焦点离开立即取消排队键盘意图。AX Focus/Click 使用当前展示快照，未知 action 拒绝。
- 执行时才重新读取当前 MessageCopy 原文或当前 Arc 代码，空文本不写。沿用 `App::write_to_clipboard`，该接口没有 OS 成功返回值，因此 Check / `ui.copy_requested` 只表达已提交复制请求，不冒充系统剪贴板写入成功或外部读取完成。每次接受复制重置 Motion::COPY_RECEIPT_MS，到期、内容变化、卸载清除；Reduce Motion 不播放交换动画。

## 编译失败后续实施验收

本次重新取得独占写入权，仅限 `copy_receipt.rs`、`copy_receipt/native.rs`、`copy_receipt/native/pixels.rs` 与本文档；不再写 `render_rows.rs` 或 `selection_menu/mounted.rs`。父集中编译 `cargo check -p vega --offline --locked -j2 --target-dir target/official-nsh-validation` 的原始日志 `target/nsh-workspace-separator-check.stderr` 报告本路两个类型错误：`native.rs:281` 的 `IntoElement` 派生宏歧义，以及 `copy_receipt.rs:65` 向 `interaction_surface` 传入 `Hsla` 而非 `Rgba`。

本轮验收为限定派生宏路径、显式转换透明颜色，保持其余生产决策和全部现有 dirty 不变。增加有限开发回归，直接调用生产 `CopySource::same_source`、`generation`、`text` 及真实 `MessageCopy` 变更接口：消息 clone 保持身份而同文不同消息不共享身份；代码 clone 保持身份而同文重新分配的 Arc 不共享身份；消息 A→B→A 必须产生不同内容版本，不能因文本恢复而接受旧版本。测试不使用源码字符串检查或替身决策，不覆盖 owner/root/mount/clipboard/完整回执生命周期。新增测试本阶段保留，父集中运行并在合并前按仓库规则留痕清理；本路只做限域 rustfmt/static，不运行 Cargo。

## 集中验证要求

父统一 Cargo/check/Clippy/必要回归；三平台真实 clipboard、hover/focus/AX/Tab/IME/modified-repeat、连续复制、源变化/路由ABA、卸载、裁剪/缩放/窗口失活和 delayed AX/keyboard 必须集中验证。本路仅限域 rustfmt/static，未执行项不能写通过。required CI、集中非实现者独审、用户手测仍独立必需。

## 前轮集成验收与生产可达性

1. 实际生产链为 `render.rs:779` 的 `cx.entity().downgrade()` → `MessageInteraction { stream: Some(stream), .. }` → `render_stream_entry` → `render_entry_internal(..., Some(interaction), ...)` → `selectable_user_message` / `selectable_markdown_message` → `selectable_markdown_content` → `selection_menu::body::render_model`。真实 stream 同时传给 `selection_context_menu`；ReadingOwner 由既有控制器通过 `copy.reading_owner` 绑定。此链完全不调用 `markdown_blocks` / `code_header` / `CopyReceiptButton`。
2. `phase.rs:702` 的 `render_run_details` 同样调用 `render_entry_with_selection(..., self.message_interaction())`，仍走 NSH selectable 正文；默认 interaction 的 `stream: None` 不是可供本路绑定旧 header 的 stream。原生正文使用 `copy.reading_owner`，其代码复制执行链在 `body.rs` 的 `copy_code_target → copy_code → write_to_clipboard`，不经过本路。本轮只核对接线，不改正文路实现或宣称其完整行为已验证。
3. 全仓 Rust 调用清点确认：旧 `code_header` 的唯一调用在 `markdown_blocks`；后者仅由 `markdown_message_item` 和 `user_message_item` 调用，二者仅在 `render_entry_internal` 的 `interaction: None` 分支使用。唯一外部 `render_entry` 调用者是隐藏 `--vega-bench-render` 的 `BenchStreamView`（`bench.rs:738`，实体不是 ConversationStream）和 `subagents.rs:77` 的 `StreamEntry::Permission`（不会进入消息/header 分支）。因此没有真实生产 ConversationStream 路径可向旧 header 传播 WeakEntity；不修改 `render_rows.rs`，不从 selector、ordinal、copy id 或全局活动线程猜 owner，不改生产入口回退到旧渲染。
4. 本轮唯一 Rust 改动将 `selection_menu/mounted.rs:16` 的 `list_bounds` 扩大为 `pub(in crate::conversation_stream)`，函数体和其他可见性不变，使 `copy_receipt/native.rs` 可访问真实已挂载 NSH list bounds。它只解除已知 sibling 可见性阻塞，是否还存在类型错误须父集中编译确认。
5. 无需父改 `render.rs` 的函数签名或新增 owner 接线。既有 `mod copy_receipt` 不变，内部自注册 `mod native` 和 `mod pixels`；不修改 message_footer。保留原 `new(...)` / `with_owner(...)` API，隐藏 bench 中没有可信绑定的旧 header 仍 fail closed。本路交付不能被表述为生产正文已改用 CopyReceiptButton；生产正文复制由独立 NSH body 路负责。若未来新增真实生产调用，须从真实 ConversationStream 传 `.with_owner(stream_weak, copy.id, Some(code.identity))`，Message 来源为 `None`，并重新验证。

## 前阶段实现事实与验证记录

- 原 GlyphLayer/ReceiptSurface/GPUI icon_button 绘制已移除；style 只执行一次，透明 Interaction 保留布局/id/回调。NSH 独立绘制 SVG、hover/pressed/focus 边框、按需提示和 Copy→Check；透明桥捕获真实 hitbox 与 pointer down/up，避免 NSH Surface 抢走 builder 回调，随后使用 NSH leaf/root 实测与同一可信执行器。
- 当前 stream 的完整 entry identity、copy id/content generation、代码块 identity 与 Arc 指针、ReadingOwner 根/route/epoch、权限门禁及挂载/paint/clip/测量 revision 在出队重新检查。源通知和全局路由、设置、删除确认、窗口激活变化失效旧投影；焦点变化失效键盘意图。几何变化另有不复用 frame identity，避免 A→B→A 几何 ABA。
- 实际复制仍只调用现有 `write_to_clipboard`；没有新增 OS acquisition 或伪造 outcome。已请求反馈使用现有 `ui.copy_requested`；定时器到期、源版本变化、卸载取消。hover/focus 发布不重启回执交换。
- 用户提供基线 HEAD：`0b5ea426abfd8d4358489e916273d89474fb190f`，父本轮再次确认 remote branch HEAD 未变及 master `34555cec9c0c4c4c1f7a528d739752f414e53edf` 祖先；本路未调用 Git，故不是独立 HEAD/fetch 验证。保留全部其他 dirty；本轮只写本文件及 `mounted.rs` 的指定可见性，不改三个已交付 Rust 文件。
- 前阶段记录：三个限定 Rust 文件直接 rustfmt 和 `--check`、限域禁止项及行尾空白扫描退出 0；只读核对仓库 NSH 和本机已安装 GPUI Fast 的实际 API，非编译/行为证据。
- 本轮实际验证：`RUSTUP_TOOLCHAIN=1.98.0 rustfmt --edition 2024 --config skip_children=true --check` 指定 `copy_receipt.rs`、`copy_receipt/native.rs`、`copy_receipt/native/pixels.rs`、`selection_menu/mounted.rs`，两次退出 0；实际 rustfmt `1.9.0-stable (88d9e12ae1 2026-08-18)`。Python SHA-256 校验确认三个复制文件和 `render_rows.rs` 与本轮读取基线逐字节相同；将 `mounted.rs` 唯一新可见性字节还原后，SHA-256 精确等于原文件 `af0fcedb6fb907366c604d8ea6846983d6ab4169c4d93ee05db57c71c9c86086`。第一次组合扫描退出 1：注释正则把行首 `*lease` 等七处合法解引用误报，逐行核对后只修正扫描器，不改产品；修正后的 unwrap/expect、注释起始、Rust test 标记及行尾空白限域扫描退出 0。上述均为静态证据，不是行为测试。
- 未运行：Cargo/check/Clippy/Rust 测试、原生/clipboard/三平台交互、CI、独立审查。按用户限制不查询 Cargo/rustc 版本，不安装/升级工具链。不增加 Rust 测试。父须集中编译，真实 hover/tooltip 的 Surface 裁剪、透明桥自定义回调、AX/Tab/IME/modified-repeat、queued clipboard 拒绝/连续复制生命周期必须实际验证。

Rust 产物 SHA-256：

```text
eaa90787cc726564222c51388d6f48401d5b5836f327366d1d5e6a3df284f6f2  copy_receipt.rs
270ee8b30a6dc912ac6a26f15ab7e04e16758b067aa83f3bf2b5635cd04513a2  copy_receipt/native.rs
1c7ea546af2511ee9c7f76538daa6800d66e563cbb63f689a7b3b756ac954832  copy_receipt/native/pixels.rs
932dde509edd9cd3d3e33fe5492fe32b3853c29fd05c186a3c6f161cd7434749  selection_menu/mounted.rs
7da9cf285ab7625185015e086bc3cd447a1e894928a193f646cc4c242b7eb565  render_rows.rs（未修改）
```

前轮完成单行可见性集成与真实可达性核对后停写；本次仅按上方新授权恢复复制文件与本文档写入，不制造无生产路径的 owner。父无需补 `render.rs` / `render_rows.rs` 的 CopyReceiptButton owner 接线，仍须对全体实现最终字节集中编译、处理实际警告/错误、验证生产 NSH body 的代码复制及本路受信绑定下的行为，再统一独审、required CI 和用户验收。不以静态清点冒充编译或行为通过，本轮不是 review。

## 本次编译修复实际交付

- `copy_receipt.rs:60` 使用 `Rgba::from(transparent_black())`，三个 surface 参数共用真实 API 所需的透明 `Rgba`。
- `copy_receipt/native.rs:281` 使用 `#[derive(gpui_kit::IntoElement)]`，与现有 message_footer 用法一致，不改变 glob 导入或运行逻辑。
- `copy_receipt.rs` 保留三个新增开发测试：`message_identity_survives_cloning_but_not_equal_text_replacement`、`code_identity_requires_the_original_allocation`、`restoring_message_text_does_not_restore_a_queued_generation`。它们直接调用现有生产身份/版本决策和消息内容接口，不 mock 决策、不引入测试依赖、不扩大生产可见性。消息/代码跨类型身份也必须拒绝。测试源码当前保留供父集中编译与执行；执行结果未取得，不是通过。范围不包含 owner、queued action 完整拒绝链、定时器或系统剪贴板结果。
- 实际静态验证：`RUSTUP_TOOLCHAIN=1.98.0 rustfmt --edition 2024 --config skip_children=true` 仅格式化两个修改文件，退出 0；随后同配置 `--check` 指定三个复制 Rust 文件，退出 0。实际 rustfmt 为 `1.9.0-stable (88d9e12ae1 2026-08-18)`。
- Python 限域字节核验退出 0：移除新增测试并还原颜色单行后，`copy_receipt.rs` 精确恢复前交付哈希；还原 derive 单行后，`native.rs` 精确恢复前交付哈希；`pixels.rs` 哈希完全未变。注释起始、unwrap/expect、行尾空白扫描通过，三个开发测试均保留且无 ignore。静态扫描不运行或模拟 Rust 决策。
- 本次未运行 Cargo/check/Clippy/test/nextest、Git、网络、native app、CI 或独审，未接触任何共享文件或 body 警告。父集中运行最终生产与 test target 编译，再实际执行 `conversation_stream::copy_receipt::tests::` 下三个回归；此前 check 失败不能作为修复后成功证据。开发测试须在验证留痕后按仓库合并边界清理，当前阶段不得提前删除。

本次最终 Rust SHA-256（包含开发测试）：

```text
483aa23d4c7fe5e8c6297eda7f9d4156baac9cc6d0fe9eb55c0f755fadac0beb  copy_receipt.rs
ee1e09950eee83928e798b72c374e2b00dad8e921cc8e943771cfef72ed61e48  copy_receipt/native.rs
1c7ea546af2511ee9c7f76538daa6800d66e563cbb63f689a7b3b756ac954832  copy_receipt/native/pixels.rs
```

本次实施写入已结束，释放四个授权路径；本记录是实际类型错误的修复交接，不是 review 或编译通过声明。
