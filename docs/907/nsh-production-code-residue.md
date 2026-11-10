# NSH 生产代码头部与 legacy 正文路径闭合

## 本路范围与结论

本轮是 #907 第四路实施任务，不是独立审查。父提供的源码基线为 `0b5ea426abfd8d4358489e916273d89474fb190f` 加现有 dirty；本路未运行 Git，因此不独立宣称核实 HEAD、分支或远端状态。遵守当前四路先完成实现、再集中独审的顺序。

授权写域仅 `conversation_stream/render_rows.rs`、`inline_code.rs`、新增同域文件及本文档。按用户要求，先核实实际生产调用链；无生产可见访问则只记录证据，不机械改造 benchmark/不可达消息分支。

**当前静态结论：`render_rows.rs::code_header` 及其 legacy 正文绘制没有正常产品界面的可达调用链。** 主时间线、展开的过程助手消息和 Thinking 均使用真实 NSH 正文。旧非选择正文仅由独立隐藏 benchmark 使用；子代理生产调用的旧入口固定接收权限卡，不进入用户/助手正文分支。因此本轮只新增本文档，两个授权 Rust 文件均逐字节保留，不添加替代渲染器、owner 绑定、export 或 runner 接线。

本结论仅闭合本任务指定的旧头部/legacy 正文可达性，不宣称 #907 全量验收、原生复制行为或其他作者实现通过。

## 实际生产路径图

以下为本轮读取时的符号及行号；其他作者修改共享正文文件后行号可能变化，以符号为准。

### 主时间线用户消息与助手消息

```text
ConversationStream 的生产 list 行
  render.rs:749 render_stream_entry(..., MessageInteraction { stream: Some(stream), ... })
    render_rows.rs:98 render_entry_internal(..., Some(interaction), ...)
      StreamEntry::User → selectable_user_message
        user_disclosure::preview → 仅计算折叠投影
        selectable_markdown_content
      StreamEntry::Assistant → selectable_markdown_message
        selectable_markdown_content
          render_rows.rs:474 selection_menu::body::render_model
            body.rs:1448 render → render_fading
              BodyState + Surface::new_transparent
              native_blocks → Block::CodeBlock → native_code_action
```

`render_rows.rs:192` 和 `:206` 依据外层 `interaction: Option<MessageInteraction>` 分派。主入口无条件传入 `Some(interaction)`；即使其中 `interaction.stream` 为 `None`，也不会切到旧正文，因为这不是外层 Option 的判定条件。`selectable_markdown_content` 没有调用 `markdown_blocks` 的 fallback。

### 展开的过程助手消息

```text
phase.rs:669 ConversationStream::render_run_details
  RunDetail::Assistant
    phase.rs:700 render_entry_with_selection(..., self.message_interaction())
      render_rows.rs:87 render_entry_internal(..., Some(interaction), false)
        selectable_markdown_message → selectable_markdown_content
          selection_menu::body::render_model → 真实 NSH Surface
```

`RunDetail::Operation` 走既有 `RunActivityGroup::render_child`；本任务不改工具/正文共享路径。

### Thinking

`thinking_body.rs:1` 将 `selection_menu::body::render_fading` 引入为 `render_native_body`；`:106` 的 `render_body` 在 `:115` 调用它，再套既有 `selection_context_menu` 和挂载观察。它不调用 `render_line`、`markdown_blocks` 或旧 `code_header`。

### 生产代码复制和安全边界保持原样

实际生产代码块在 `selection_menu/body.rs::native_blocks` 的 `Block::CodeBlock` 分支构造 NSH `view`、可选择 `text`、`native_code_action` 及 `scroll().horizontal().scrollbar(true)`。`native_code_action` 使用 `nsh_core::prelude` 的真实文本按钮、测量和命令，不嵌入旧 `code_header` 或 `CopyReceiptButton`。

本轮读取到的生产执行链为 `Request::Code` → `BodyState::copy_code`；透明键盘/AX桥使用 `copy_code_target` → `copy_code`。后者检查投影 session、`valid`、当前 mounted content，以及目标 code fragment 与当前 `display_text` 的 source mapping，再将 fragment 完整文本交给既有 `write_to_clipboard`。`copy_code_target` 另检查当前 lease geometry 和 ReadingSnapshot currency。`valid` 继续检查 ReadingOwner/frame、内容 generation/source、已确认 session、Surface 错误和挂载/窗口/裁剪等条件。

以上只定位既有实现，不是独立安全审查或行为验证。完整源复制、权限/路由、焦点/AX、挂载和失效判断全部未改；不触碰共享 copy receipt、图标、tooltip、body/source anchor、vendor、滚动协议、Cargo 或依赖。NSH 0.4 / Kit 0.7.3 / Fast 0.1.9 维持父指定版本，不运行包管理工具核验。

## 非选择 legacy 路径的完整调用闭包

```text
render_rows.rs:77 render_entry
  render_entry_internal(..., None, false)
    User → user_message_item (:349)
    Assistant → markdown_message_item (:271)
      markdown_blocks (:504)
        唯一 code_header 调用 (:551)
        code_header 定义 (:693)
        render_line → markdown_text → inline_code::interactive_text
```

全 `crates` Rust 符号引用清点发现 `render_entry` 仅有两个外部调用者：

1. `conversation_stream/bench.rs:738` 的 `BenchStreamView`。`crates/vega/src/main.rs:47–54` 仅在显式 `--vega-bench-render` 参数存在时启动该独立基准窗口，随后 `return`，不进入正常应用启动。它确实可能绘制旧正文，但不是用户正常进入的生产正文，且本任务明确不把它当作 live 残余机械迁移。
2. `conversation_stream/subagents.rs:77` 的 `SubagentGroup::render`。参数现场构造为 `StreamEntry::Permission { card }`；`render_entry_internal` 的 Permission arm 直接调用 `PermissionCard::render_card`，不经过 User/Assistant、`markdown_blocks` 或 `code_header`。不能仅凭入口名称就把此权限卡调用算作旧正文可达。

`user_message_item` 没有其他外部引用；`markdown_message_item`、`markdown_blocks`、`code_header` 为本文件私有。`render_line` 的引用也全部局限在该 legacy 绘制链内。未发现把这些函数另存回调或别名后绕过上述入口的 Rust 引用。没有通过隐藏入口、删除正文或更改交互参数来制造“已迁移”。

## 必须保留的生产测量

`inline_code.rs` 不能整删，也不能因旧绘制仅用于 benchmark 就一并移除所有类型。

```text
selectable_user_message (render_rows.rs:394)
  user_disclosure::preview (:31)
    measure_text (:181)
      带链接/内联代码背景 → shape_flow + RunIndex
      普通文本 → shape_text → Fragment + FlowLayout
    table_row_heights → shape_flow + RunIndex
```

`user_disclosure.rs:189–232` 实际返回/构造 `FlowLayout` 和 `Fragment`；`:192`、`:311` 调用 `shape_flow`，`:194`、`:313` 构造 `RunIndex`。这些是正常产品用户消息折叠投影的生产测量，不是可删除的旧绘制尾巴。本轮保留 `shape_flow`、`RunIndex`、`FlowLayout`、`Fragment` 及其依赖，不改测量、截断、安全或滚动行为。

## 实际静态核对与验证边界

- 通过 `find . -name AGENTS.md -print` 确认目标 worktree 仅根级 AGENTS；读取根约束、`docs/vega-exec-guide.md`、设计守则、`nsh-full.md` 的本任务范围/实施顺序及 reading/copy receipt 对应契约。
- 使用 `rg -n` 对全 `crates` 的 Rust 文件清点 `render_entry`、`render_entry_with_selection`、`render_stream_entry`、`user_message_item`、`markdown_message_item`、`markdown_blocks`、`code_header`、`render_line`，再逐处读取真实生产调用和分派条件。
- 对全 `vega_ui/src` 清点 `inline_code` 外部引用，逐处核对 `user_disclosure` 的测量用途；读取实际 NSH 代码块、复制命令及 Thinking alias，而非只凭名称或旧文档判断。
- 使用 `shasum -a 256` 在调查开始和结束复核两个 Rust 输入文件，SHA-256 均保持下表身份。本文档之外没有本路写入。
- 这是源码静态证据，不是源码字符串行为测试。未新增 Rust 测试，不增加 allow/ignore，不改断言或测试预算。
- 未运行 Cargo、rustc、Clippy、应用、网络、Git 或委派；未取得本轮编译、行为测试、clipboard、三平台、CI 或独立审查结果。无 Rust 改动，因此无需格式化生产文件；文档执行 UTF-8、相对链接、末尾换行和行尾空白静态核对。

| 保持未改的文件 | SHA-256 |
|---|---|
| `crates/vega_ui/src/conversation_stream/render_rows.rs` | `b9ad8d74709c73646e3900f6237536139700c3036e353e0798a0e4b3d0200409` |
| `crates/vega_ui/src/conversation_stream/inline_code.rs` | `d22481b98434f6462c6fa17e3d0ba5df2c1fdeb5b8154078cc356205a69244f2` |

## 停写交接

本路无新增 API、export、runner、共享 copy receipt 或父代补丁请求。已按用户允许的“无生产访问只记录证据”分支闭合任务，不将 benchmark 改造或测量删除扩展成新工作。父集中验证其他生产修改；未来若新增 `render_entry(User/Assistant)`、直接 `user_message_item` 或 `render_line` 的产品调用，则需重新核实这份可达性结论，不能把本文当作永久许可。

停写后文档最终 SHA-256 在交付消息记录，避免将自身哈希写入自身。本轮实现交接不替代所有实现完成后的集中非实现者独审、最终 head required CI 和用户手测。
