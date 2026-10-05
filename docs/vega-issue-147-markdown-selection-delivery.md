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

## Follow-up: active Markdown delta invalidates an established selection

| ID | requirement / risk | initial state | operation | expected observable result | evidence class | status |
|---|---|---|---|---|---|---|
| M147-5a | A stable selection in an active assistant Markdown message must not survive the next stream delta | GPUI has laid out an unfinished assistant Markdown stream and a drag selection has settled on `Alpha` | Apply the next `ConversationEvent::TextDelta` through `ConversationStream::apply_event`; issue Cmd+C before repaint, then let the pending Markdown tail repaint | `MessageCopy` has no selected text immediately; Cmd+C leaves the clipboard sentinel unchanged; repaint clears GPUI's selected text and exposes the updated Markdown projection | Production GPUI test context, no Provider | PASS |

### Verification

- verified_at_utc: 2026-10-04T10:18:23Z
- verified_at_local: 2026-10-04 18:18:23 CST
- branch: `feat/147-stable-selection-stream-delta`
- test_source_diff_sha256: `5423b6810c11adfd6ebaed2a5e8c54737c6d4b7a65f5ca54562c32ecd785f1d6` (relative to `origin/master`)
- exact command: `cargo nextest run -p vega_ui issue147_markdown_selection`
- result: PASS; 4 passed, 525 skipped; Nextest run ID `598b7ffa-d5aa-482c-b2ec-7c0771815c29`
- `cargo fmt --all -- --check`: PASS, exit 0
- `git diff --check`: PASS, exit 0
- first test build attempt exited 101 with E0499 in the test harness helper because `cx` was mutably borrowed in both the outer call and nested argument. The helper now stores the new stream in a local before opening the window. No production code changed.
- scope: regression test and this delivery record only; no Provider request and no 360px/table-copy expansion.

## Follow-up: restored long-code selection and complete clipboard readback（2026-10-06）

### 结论与原生观察更正

本次新增 6 项 owned GPUI/Store 回归，完整文字、选择绘制和 Composer 粘贴断言均通过；生产代码改动为 0，没有检出业务 RED，也没有宣称修复复制缺陷。

主控在官方 v0.1.53 的原生窗口选择 CODE-07 至 CODE-21 时，最初 Composer 画面只显示末尾 CODE-14 至 CODE-21。随后将原生 Cmd+C 的结果粘贴到确认空白的 TextEdit 文档，AX 完整原文包含全 15 行。将同一剪贴板粘贴到 Vega 后，画面仍显示末尾 8 行，但在 Composer 全选复制并粘贴到确认空白的 TextEdit，AX 原文仍为完整 15 行。初次“丢前缀 FAIL”的判断来自底部视口画面，完整值对照已更正该判断。最初观察和更正证据同时保留。

该原生结果只证明这次 CODE-07 至 CODE-21 复制及 Composer 保存完整文字；其内部滚动、Cmd+Up 动作及其它 Markdown/主题/窗口组合尚无完整原生结论。全卡继续等待剩余矩阵验收。

### Freeze

- verified_at_utc: 2026-10-05T22:20:30.823789+00:00
- verified_at_local: 2026-10-06 06:20:30 CST
- branch: `codex/147-native-copy-prefix-v053`；fresh fetch/rebase 到包含 PR #276 的最新 master 后验证。
- task_contract: M147-5b / M147-5c；[本卡规格](vega-issue-147-markdown-selection.md)。
- spec_sha256: `c2d7b2656934d7ff62faa3a60a25827b0f69791e71d2b2e39eba2b037f64f232`
- changed_source_diff_sha256: `9e34d5b46e0717e5365682d004776d0363ceaa11248ffa8b674e93974412afae`（相对最新 master；仅新增测试模块与一行注册）。
- new_test_source_sha256: `b600ac0cfea470ece2a461a6c5ea67d3cce4c4b5dc4dab5ef10bb192b1d12d0b`；与 rebase 前完整 6 项通过时完全一致。
- test_registry_sha256: `8db398b5a97128a9672ff8f47625a85c30d3fe22d9c8d63815d7b32da897e6b6`。
- all_tracked_input_manifest_sha256: `a52e4b9fa25861a328afa0fb5715d5ce6e698936d17c0bc15eeb7210f0bf610c`；828 个 tracked 输入，其中 2 个软链，逐项核对归档原字节及软链目标。
- source_archive_sha256: `97c5349c8bc11e6d7bf8f434269978df0273d17c9c2a3930e8a1938ca44dcd0b`；原始源码/配置、完整日志、全部失败与 rebase 记录保存在私有证据目录。
- os_arch: Darwin arm64；rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)；cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)；cargo-nextest 0.9.146。
- profile: default；`.config/nextest.toml` SHA-256 `9adc8b4f095e26d9f66f8c3ca13caf625d19db473010a0adafa4f58d5bca8846`，retries=0；无 profile、重试、并发或 target 环境覆盖。
- formatter: 仅格式化本卡新增 Rust 文件；`git diff --check` 通过；新增代码注释为 0。

### 验收矩阵

所有新用例使用 owned 临时 Store、真实 VegaWindow、1403×860 布局和独立配置。恢复视口通过既有 `restore_scroll_anchor` 及任务切换缓存路径，再注入真实 GPUI 滚轮和鼠标事件。无害助手消息的 `plaintext` 围栏内容精确匹配原生记录的 1392 字节及 SHA-256；不读取用户数据库。

| ID / requirement | 前置状态与实际操作 | 预期可观察结果 | evidence class | status |
|---|---|---|---|---|
| M147-5b / 正向拖动 | 恢复已滚动代码视口，小幅向上滚动，再分 32 次移动从 CODE-07 拖至 CODE-21 | 15 个生产绘制选区矩形、精确选区文本和 Cmd+C 原始剪贴板均完整匹配 15 行与 14 个换行 | 进程内 GPUI 生产路径、TestPlatform scene | PASS |
| M147-5c / 反向拖动 | 相同恢复/滚动，从 CODE-21 反向拖至 CODE-07 | 相同 15 个绘制矩形、完整文本及原始剪贴板 | 同上 | PASS |
| M147-5b / 批量输入 | 同一更新批内 down、32 次 move、up，随后绘制 | 完整 15 个选择矩形、15 行投影和剪贴板 | 同上 | PASS |
| M147-5c / 选择后再滚动 | 建立完整 15 行选择，再向下滚动 40px | 生产消息几何精确移动 40px；选择绘制、文字和剪贴板仍保留相同 15 行 | 同上 | PASS |
| M147-5c / 单行对照 | 建立并复制多行选区，清除后只拖选 CODE-07 | 1 个绘制选区矩形、精确单行投影和剪贴板 | 同上 | PASS |
| M147-5b / Composer 完整值 | 完整 15 行 Cmd+C；实际点击空 Composer、Cmd+V、Cmd+Up，读取完整 InputState 文本 | 复制原文、粘贴后完整文本及 Cmd+Up 后完整文本均相同；无 Send，最终清空输入 | 同上；不证明真实 macOS 输入框像素/内部滚动 | PASS |
| M147-1/2/3/5a / 相关回归 | 既有 Unicode、空选区、跨 Markdown 块、稳定消息变更和流式 delta 失效 | 精确文本及旧选区失效，剪贴板哨兵不被陈旧选择覆盖 | 既有进程内 GPUI 测试 | PASS；4 项 |

### 实际命令与结果

| exact command | result | Nextest run ID | command duration / test footer | raw_sha256 |
|---|---|---|---|---|
| `cargo nextest run --offline --locked -p vega issue147_native_copy` | exit 0；6 passed / 223 skipped | `21bbdb0b-3ed8-4a4b-b630-8c6ceb8f9db6` | 14.256163s / 1.699s | `5224149d98518e19e692e0fc3f1098cea46ae59799aeb0016161fe3811969be5` |
| `cargo nextest run --offline --locked -p vega_ui issue147_markdown_selection` | exit 0；4 passed / 549 skipped | `bc5649e9-4df5-4323-b904-c54dc42ded2a` | 45.161533s / 0.050s | `52cf26b9ba50494c4648f5567838760bea8cd58791c0794ea589dd8ca67ab307` |

新 6 项的原始有界输出：

```text
 Nextest run ID 21bbdb0b-3ed8-4a4b-b630-8c6ceb8f9db6 with nextest profile: default
    Starting 6 tests across 2 binaries (223 tests skipped)
        PASS [   0.575s] (1/6) vega::bin/vega tests::issue147_native_copy::issue147_native_copy_restored_small_scroll_batched_input_keeps_all_fifteen_lines
        PASS [   1.145s] (2/6) vega::bin/vega tests::issue147_native_copy::issue147_native_copy_restored_small_scroll_reverse_keeps_all_fifteen_lines
        PASS [   1.150s] (3/6) vega::bin/vega tests::issue147_native_copy::issue147_native_copy_restored_small_scroll_forward_keeps_all_fifteen_lines
        PASS [   1.191s] (4/6) vega::bin/vega tests::issue147_native_copy::issue147_native_copy_selected_rows_survive_another_small_scroll
        PASS [   1.248s] (5/6) vega::bin/vega tests::issue147_native_copy::issue147_native_copy_clipboard_and_composer_paste_keep_the_same_fifteen_lines
        PASS [   1.698s] (6/6) vega::bin/vega tests::issue147_native_copy::issue147_native_copy_single_line_control_after_clearing_multiline_selection
────────────
     Summary [   1.699s] 6 tests run: 6 passed, 223 skipped
```

相关 4 项的原始有界输出：

```text
 Nextest run ID bc5649e9-4df5-4323-b904-c54dc42ded2a with nextest profile: default
    Starting 4 tests across 1 binary (549 tests skipped)
        PASS [   0.045s] (1/4) vega_ui conversation_stream::tests::issue147_markdown_selection::issue147_user_text_drag_supports_unicode_cmd_c_and_empty_copy
        PASS [   0.048s] (2/4) vega_ui conversation_stream::tests::issue147_markdown_selection::issue147_stream_append_invalidates_stale_copy_before_repaint
        PASS [   0.049s] (3/4) vega_ui conversation_stream::tests::issue147_markdown_selection::issue147_streaming_markdown_delta_invalidates_stable_selection_before_repaint
        PASS [   0.049s] (4/4) vega_ui conversation_stream::tests::issue147_markdown_selection::issue147_assistant_markdown_drag_copies_visible_block_text
────────────
     Summary [   0.050s] 4 tests run: 4 passed, 549 skipped
```

### 首次执行与失败分类

每次修改 harness 都保留修改前完整源码/配置归档和首次日志，未自动重试到绿，未弱化 15 行文字、剪贴板或绘制断言。

| attempt | 实际结果与分类 | retained evidence |
|---|---|---|
| owned-attempt-01 | 编译 exit 101；测试访问 private scroll-cache field，且误把 `Window::draw` 返回值当 Result。改用既有公开恢复接口和实际任务切换，正确处理绘制返回值；Nextest 未运行 | raw SHA `f1c0f72c81e9e52f1ba8f2c113a3dbff20ef941547c72ff27822514801d448d7`；完整 freeze/归档保留 |
| owned-attempt-02 | 3 个测试在前置等待超时；fixture 错把 hydrated entry count 写成 2，而生产正常恢复 2 条消息加 1 张终态摘要卡。依照生产契约修正为精确 3 条；复制断言尚未到达 | run `ce844ef0-5a39-4a39-a0e3-f5d55d3a00ab`；raw SHA `b733075bb9a7f50754392a148b593eae71e5dd0bfbc0aa20e90f6ac0a0c7e577` |
| owned-attempt-03 | 首批 3 项有效业务断言全 PASS，生产改动 0；没有业务 RED | run `4d340493-7ca4-483a-8171-e0c3ba4f1114`；raw SHA `7d3a08a0ed389eb321231b9a667543e5fcfe6058881043a1fae56012092440e7` |
| owned-attempt-04 | 5 项 PASS；选择后再滚用例的前置断言错误要求 scroll-top message 保持不变。滚动前首项为残留的 prompt，40px 后首项自然变为 code；该用例尚未进入滚动后的复制断言 | run `7310ca0a-3cbb-495d-a351-4b18f9f25d32`；raw SHA `c40dd72151d9c4b04a89d746d7ea94d893c3e8a2d4c2226ce4fe08240d262461` |
| owned-attempt-05 | 修正前置条件为消息几何精确移动 40px、首项为 code；保留完整 15 行与绘制断言，6 项 PASS，生产改动 0 | run `b84d3de0-8fa1-4f1e-abc7-7ddcb02f2239`；raw SHA `034f8c867f18fc598c79fce27a866e3a4193679185eec88af849c224ae408829` |
| rebase preflight | 两个测试模块注册相邻冲突；完整保留 #64/#147 注册。一次 runner 提前执行在 `git write-tree` 拒绝 unresolved index 后退出，Cargo/Nextest 未启动；不完整冲突 snapshot 单列 | rebase 原始输出、未运行说明保留；不作为有效 freeze 或测试结果 |
| rebased-owned-run-01 | 同一 6 项源码在最新 master 首跑 PASS；相关 4 项随后首跑 PASS | 以上最终有界输出；同一 source archive/manifest，7 份完整 freeze 与 raw 全部逐项核对 |

### 实现范围、限制与回滚

- 新增 `crates/vega/src/tests/issue147_native_copy.rs` 与一行测试注册；补充本卡规格及本交付记录。production diff 为 0；无新增依赖、公共 API、迁移或配置契约。
- 原始实现交付和历史 native/diagnostic 证据继续保留。旧 dirty worktree 未修改；没有安装或启动诊断包。
- NOT RUN：本分支云端 required check、PR 合并、其它原生 Markdown/表格/列表/主题/窄窗口/右键菜单矩阵，由主控继续推进。定向绿色不能代表全量 gate 或整卡完成。
- LIMIT：owned clipboard 原文与 InputState 完整文本可断言；native Composer 视口和 Cmd+Up 内部动作未测，不把仅显示末尾 8 行认定为丢数据。
- LIMIT：既有 `block v0.1.6` future-incompatibility warning 保留，本卡未改该依赖。
- rollback：撤回本次测试与文档提交即可；没有需要回滚的生产算法或用户数据变更。
- 规格偏离：无；此次为回归覆盖和证据更正。
