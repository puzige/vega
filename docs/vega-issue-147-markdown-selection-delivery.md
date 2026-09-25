# Issue #147 交付记录

## Freeze

- verified_at_utc: 2026-09-25T14:31:48Z
- verified_at_local: 2026-09-25 22:31:48 CST
- git_head: `feat/147-markdown-text-selection`（记录分支，不记录 raw OID）
- tracked_diff_sha256: `759e3fcba19d7fad618976d9b6cf8eb3e5658e1e3dfeaa64f6338a0e7d75a1ef`（相对 origin/master 的实现源码与测试，不含规格及本交付记录）
- task_contract: Issue #147；规格 `docs/vega-issue-147-markdown-selection.md`；spec SHA-256 `f7215363488dbf24987db18c4924b3a7103cd5221715cadd661234736b1c9671`
- os_arch: Darwin arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`
- git: `git version 2.55.0`
- diff-check: `git diff --cached --check` exit 0
- formatting: `cargo fmt --all -- --check` exit 0

## Results

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
|---|---|---|---|---|---|
| M147-1: 用户消息局部拖选，Unicode 字符边界与精确复制 | GPUI UI test | `cargo nextest run -p vega_ui issue147_markdown_selection` | PASS；拖选 `Alpha你好 😀` 并 Cmd+C | 0.040s test | Nextest 日志 SHA-256 `a59c3ac770d740a57630039bf1084c3eda1a53aa71576fff1d8a163917b1aec5` |
| M147-2: Cmd+C 与空选区 | GPUI UI test | `cargo nextest run -p vega_ui issue147_markdown_selection` | PASS；空选区 Cmd+C 保留剪贴板哨兵值 | 0.045s 总测试 | 同上 |
| M147-3: Markdown 跨块、软换行、列表、代码、引用、表格与可见文本投影 | GPUI UI test | `cargo nextest run -p vega_ui issue147_markdown_selection` | PASS；断言精确文本、换行、制表符及隐藏链接地址 | 0.043s test | 同上 |
| M147-4: 右键菜单保持选区；无选区时“复制”禁用 | 真实桌面待验 | 由主控 Computer Use 手测 | NOT RUN；测试上下文无法安全回收弹出的 PopupMenu entity | — | 需验有选区可复制及空选区禁用 |
| M147-5: 流式正文更新后旧选区不可复制 | GPUI UI test | `cargo nextest run -p vega_ui issue147_markdown_selection` | PASS；正文变更后清空投影并保留剪贴板哨兵值 | 0.044s test | 同上 |
| M147-5: 切换任务、历史恢复、行回收与滚动期间选择边界 | 真实桌面待验 | 由主控 Computer Use 手测 | NOT RUN | — | 检查旧索引不复制到新消息、裁剪行不崩溃 |
| M147-6: CJK/emoji | GPUI UI test | `cargo nextest run -p vega_ui issue147_markdown_selection` | PASS；用户与助手 Markdown 选择路径均含 CJK/emoji | 0.044s 总测试 | 同上 |
| M147-6: 浅/深主题、窄窗口、长代码与滚动 | 真实桌面待验 | 由主控 Computer Use 手测 | NOT RUN | — | 确认高亮可见、代码软换行不插入复制换行 |
| VegaWindow 生产接线可编译 | cargo check | `cargo check -p vega` | PASS | 2.06s | cargo 日志 SHA-256 `67a5e96868880f0449f741dbcc6b9f89fb0f01af7539a098fc3f494ba8182319` |
| 工作区静态分析 | cargo clippy | `cargo clippy --workspace --all-targets -- -D warnings` | PASS | 0.40s | 最终日志 SHA-256 `441f85dd571d3f14fd90ff182207064b919ea97aa0c4e54a90c610bc6fb20d77` |
| 格式与空白 | cargo fmt / diff check | `cargo fmt --all -- --check`; `git diff --cached --check` | PASS | — | 无格式或 whitespace 错误 |

定向 Nextest 原始输出：

```text
   Compiling vega_ui v0.1.0
    Finished `test` profile [unoptimized + debuginfo] target(s) in 10.98s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 7568d94c-c90c-4d34-bab9-41c089161842 with nextest profile: default
    Starting 3 tests across 1 binary (482 tests skipped)
        PASS [   0.040s] (1/3) vega_ui conversation_stream::tests::issue147_markdown_selection::issue147_user_text_drag_supports_unicode_cmd_c_and_empty_copy
        PASS [   0.043s] (2/3) vega_ui conversation_stream::tests::issue147_markdown_selection::issue147_assistant_markdown_drag_copies_visible_block_text
        PASS [   0.044s] (3/3) vega_ui conversation_stream::tests::issue147_markdown_selection::issue147_stream_append_invalidates_stale_copy_before_repaint
────────────
     Summary [   0.045s] 3 tests run: 3 passed, 482 skipped
```

`cargo check -p vega` 退出码 0，bounded footer：

```text
    Checking vega_ui v0.1.0
    Checking vega v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.06s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, run `cargo report future-incompatibilities --id 1`
```

工作区 Clippy 退出码 0，原始 bounded footer：

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.40s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, run `cargo report future-incompatibilities --id 1`
```

失败迭代保留：首次 Clippy 退出码 101，报告 `unnecessary_unwrap`、`if_same_then_else` 和测试内 `redundant_closure`；第二次报告 `collapsible_if`。修复后最终命令通过。失败日志 SHA-256 分别为 `2e9dde83d1c4ab194945f5630ad77dd4614a62b08f8b59493b833e0cdf5c1245` 与 `04cd94d329647b4aca51667aa7151a8f30452b2d7e639ced44ae3e2a37691d8d`。

## 实现范围

- `crates/vega_ui/src/conversation_stream/selection.rs`：将每条消息映射为独立选区文档，记录渲染 run 几何、绘制选区并把 UTF-8 安全的局部范围投影到可复制文本。
- `crates/vega_ui/src/conversation_stream/render_rows.rs`：为用户正文和助手 Markdown 增加可选正文渲染路径；复制投影按可见格式文本与冻结的块间分隔符生成；添加消息局部“复制”菜单。
- `crates/vega_ui/src/conversation_stream/model.rs`、`core.rs`、`render.rs`：保存消息选择状态；Cmd+C 仅响应有效正文选区；流式正文变化与新 ConversationStream 生命周期清除旧索引。
- `crates/vega_ui/src/lib.rs`：注册正文选区 Cmd+C action，并保留 Composer 的 key context 行为。
- `crates/vega/src/window/render.rs`：在 VegaWindow 根首个 child 接入 `TextSelectionLayer`。应用没有 GPUI Component Root；生产接线需要该层才能接收拖选事件。
- `crates/vega_ui/src/conversation_stream/tests/issue147_markdown_selection.rs`：新增 3 个生产渲染路径 GPUI 测试，覆盖 Unicode 局部选择、空选区剪贴板不变、跨 Markdown 块规范化复制及流式更新失效。

规格偏离：无。

## Residuals

- ACCEPTED：本机未运行 workspace 全量 Nextest；本卡只运行定向过滤器。`cargo check -p vega` 与格式检查已通过。
- NOT RUN：主控在实际 Vega 桌面使用 Computer Use 手测右键菜单、Composer 焦点、浅/深主题、窄窗口、长代码、滚动、切换任务与历史恢复。自动 GPUI ContextMenu 测试触发测试上下文 PopupMenu entity teardown 泄漏，因此右键菜单验收留给真实 UI。
- NOT RUN：云端 PR check 尚未完成；本地静态分析与定向测试不替代云端门禁。分支保持未合并。
- LIMIT：编译存在既有依赖 `block v0.1.6` 的 future-incompatibility warning；本卡没有修改该依赖。
- 本次验证中的一次静态注释扫描误把 Rust 解引用表达式中的 `*` 识别成注释；修正扫描规则后通过。实现源码和测试未新增注释或诊断输出。
