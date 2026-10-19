# Issue 66 · Composer Enter 发送键位契约（SPEC FROZEN）

> 状态：**SPEC FROZEN**
> 冻结日期：2026-09-20（Asia/Shanghai）
> 来源：GitHub Issue [#66](https://github.com/puzige/vega/issues/66)「现在期望按回车键自动发送对话，现在必须要按 Command 键回车才会发送。」
> 用户裁决（2026-09-20）：**Enter 发送 · Shift+Enter 换行 · Cmd+Enter 发送**
> 基线：`master @ 8c1d874`
> 关联：`docs/vega-features.md` A2-11 · `docs/vega-ui-spec.md` §4.4

---

## §0 这不是新行为，是修复代码对冻结功能表的偏离

`docs/vega-features.md:44` 冻结的 P0 功能点 **A2-11 Composer 输入**：

> | A2-11 | Composer 输入 | 多行、**Shift+Enter 换行**、历史消息 ↑ 召回 | P0 | 全员 |

A2-11 明文规定 **Shift+Enter = 换行**，其逻辑蕴含 **Enter = 发送**。而当前实现
（`crates/vega_ui/src/lib.rs:213-218`）是：

```rust
KeyBinding::new("enter", text_input::InsertNewline, Some("Composer")),   // Enter = 换行
KeyBinding::new("cmd-enter", conversation_stream::SendMessage, Some("Composer")),
```

注释自认来源是「T18 架构师裁定，ui-spec §4.4 **未定项**」。也就是说 ui-spec §4.4
从未冻结键位，T18 拍板了 Enter=换行，与 A2-11 冲突且一直未被发现。

**本 Issue 按 A2-11 冻结 §4.4 键位，并让代码与之一致。** 不新增产品行为。

---

## §1 键位契约（冻结）

| 键 | 行为 | 依据 |
|---|---|---|
| `Enter` | **发送**（无浮层、非 IME 组合态、提交 guard 通过时） | A2-11 蕴含 + #66 |
| `Shift+Enter` | **换行**（在 Composer 多行输入内插入 `\n`） | A2-11 明文 |
| `Cmd+Enter` | **发送**（保留，兼容既有肌肉记忆与全部现有测试） | 不删旧路径 |

### R1 · Enter 发送
`Enter` 在 `Composer` key context 下绑定到 `SendMessage`，走既有
`ConversationStream::on_send_action` → `submit_message()`。发送仍受全部既有 guard 约束
（`actions.running` / `composer_submit_pending` / `attachment_import_pending` /
`pending_mode` / `approved_not_started` / `trusted_action_busy` /
`skill_mutation_pending` / `model_selection_pending` / 空文本且无附件），**不新增任何旁路**。

### R2 · Shift+Enter 换行
`Shift+Enter` 在 `Composer` key context 下绑定到 `text_input::InsertNewline`。
多行输入插入 `\n`；行为与当前 Enter 完全一致（`replace_text_in_range(None, "\n", …)`）。

### R3 · Cmd+Enter 保留发送
`Cmd+Enter` → `SendMessage` 绑定**保持不变**。三个键位并存，不删除 Cmd+Enter。

### R4 · 作用域浮层优先（不得被发送抢走）
以下 scoped context 比 `Composer` 更深（渲染为 Composer 壳层内层），GPUI
`bindings_for_input` 按 context 深度排序、深者优先，且动作在 bubble 阶段默认
`propagate_event = false` 被消费。因此浮层打开时 `Enter` 仍归浮层：

| 浮层 | context | Enter 行为 | 证据 |
|---|---|---|---|
| `@file` 候选下拉 | `FileSelect` | 接受候选（`AcceptFile`） | `render.rs:201`、`lib.rs:260` |
| `@file` 失败重试 | `FileSelectRetry` | 重试（`RetryFile`） | `render.rs:168`、`lib.rs:267` |
| `+` 菜单（含 `/` 命令） | `ComposerActions` | 接受高亮行（`AcceptComposerAction`，显式 `stop_propagation`） | `render.rs:204`、`composer_actions.rs:461` |
| 模型选择器 | `ModelSelector` | 开合/接受（`ActivateModel`） | `render.rs:806`、`lib.rs:289` |
| 停止按钮 | `ComposerStop` | 停止（`StopComposer`） | `composer_actions.rs:681` |
| 权限卡 | `PermissionCard` | 允许一次 / 危险卡拒绝 | `permission_card.rs`、`lib.rs` |

**验收不变量**：任一浮层可见时，`Enter` 绝不触发发送。

### R5 · IME 组合态绝不发送
`Enter` 在输入法组合中（`TextInput::is_composing()` 为真）**必须**交给输入法确认候选，
不得发送。实现两层防护：

1. **平台层（既有，无需改）**：`gpui_macos` 在 `is_composing` 时把按键先交给
   `inputContext handleEvent:`；IME 消费则 `return YES`，按键永不进入 keymap
   （证据：`gpui_macos/src/window.rs:2460-2510`）。
2. **handler 层（本卡新增）**：`on_send_action` 增加 `is_composing()` guard，
   组合态下 `cx.propagate()` 并返回，与同文件 `accept_composer_action` /
   `close_composer_actions` / `stop_composer_action` 等既有守卫一致。
   这是纵深防御：即使平台层在某布局/事件路径下把按键漏进 keymap，也不会误发。

### R6 · 非 Composer 的多行输入保持不变（非目标）
`ProviderSettings`（模型 ID 列表）、`CommitPanel`（commit message）、MCP 设置的多行字段
仍为 **Enter = 换行**。本卡只改 `Composer` scope，不动其它 scope 的绑定。

---

## §2 实现面（最小改动）

| 文件 | 改动 |
|---|---|
| `crates/vega_ui/src/lib.rs` | `Composer` scope：`enter` → `SendMessage`；新增 `shift-enter` → `InsertNewline`；`cmd-enter` → `SendMessage` 保留。更新 doc comment。 |
| `crates/vega_ui/src/conversation_stream/composer.rs` | `on_send_action` 增加 `is_composing()` guard（R5）。 |

无新依赖、无 schema 变更、无公共 API 变更。

---

## §3 验收矩阵（测试先行）

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 层级 | 状态 |
|---|---|---|---|---|---|---|
| E1 | R1 Enter 发送 | Composer 聚焦、有文本、无浮层 | 按 `Enter` | 发出 `ComposerSubmitted`；`composer_submit_pending` 置位 | GPUI 生产 handler | ✅ `issue66_enter_sends_through_key_dispatch` |
| E2 | R2 Shift+Enter 换行 | 同上 | 按 `Shift+Enter` | 不发送；输入框文本追加 `\n` | GPUI 生产 handler | ✅ `issue66_shift_enter_inserts_newline_without_sending` |
| E3 | R3 Cmd+Enter 仍发送 | 同上 | 按 `Cmd+Enter` | 同 E1 | GPUI 生产 handler | ✅ `issue66_cmd_enter_still_sends` |
| E4 | R4 FileSelect 优先 | `@` 下拉有候选 | 按 `Enter` | 接受候选、不发送 | GPUI 生产 handler | ✅ `issue66_enter_accepts_file_candidate_instead_of_sending` |
| E5 | R4 ComposerActions 优先 | `+`/`/` 菜单打开 | 按 `Enter` | 接受高亮行、不发送 | GPUI 生产 handler | ✅ `r11_composer_context_and_slash_keyboard_use_real_mode_and_file_handlers` |
| E6 | R5 IME 组合态不发送 | `is_composing()` 为真 | 直接调用 `on_send_action` | 不发送、零事件 | GPUI 生产 handler | ✅ `issue66_enter_never_sends_during_ime_composition` |
| E7 | R1 提交 guard | `composer_submit_pending` / running | 按 `Enter` | 不重复发送 | GPUI 生产 handler | ✅ `issue66_enter_respects_the_submit_guard` |
| E8 | R6 ProviderSettings 不受影响 | Provider 多行字段聚焦 | 按 `Enter` | 换行，不发送 | 回归（已有） | ✅ 未改 `ProviderSettings` scope 绑定 |

**测试迁移**：把「在 Composer scope 下按 `Enter` 期望换行」的既有测试改为 `Shift+Enter`。
已知一处：`crates/vega_ui/src/conversation_stream/tests/core_flow.rs:553`
（`failed_file_index_keys_restore_composer_scope`，断言 `"@missing\n"`）——已迁移为 `shift-enter`。

### 门禁结果（2026-09-21，worktree `vega-issue66-enter-to-send` @ `8c1d874`）

| 门禁 | 命令 | 结果 |
|---|---|---|
| 格式 | `cargo fmt --all -- --check` | ✅ `FMT_OK` |
| 静态检查 | `scripts/cargo-lock.sh --wait clippy --all-targets -- -D warnings` | ✅ exit 0，0 error，全 crate 真实重编（41.28s） |
| vega_ui 测试 | `scripts/cargo-lock.sh --wait test -p vega_ui` | ✅ `392 passed; 0 failed` |
| 全量测试 | `scripts/cargo-lock.sh --wait test --workspace` | ✅ exit 0，35 个 `test result: ok`，0 FAILED |

**fallout 记录**：全量测试出现过两次与本卡无关的环境抖动，均在单测隔离复跑通过、全量复跑通过后判定为并发资源争抢：

1. `window::workspace::terminal_tests::r44_terminal_entry_points_and_creation_menu_preserve_explicit_focus`
   —— PTY 子进程启动失败 + 焦点断言失败（`left: ""` / `right: "explicit-focus"`）。
2. `tests::diff::diff_refresh_intents_keep_content_during_background_and_retry`
   —— `diff refresh stage retry reached terminal state without success: refresh_error=Some(GitFailed)`，
   即 fixture git 子进程在并发负载下失败。

两者都不经过 Composer key context，与本卡键位改动无关；隔离复跑与全量复跑均绿。

---

## §4 非目标
- 不改 `ProviderSettings` / `CommitPanel` / MCP 多行字段键位。
- 不新增「设置里可切换 Enter 行为」的开关。
- 不改 `@file` 下拉「无候选时 Enter 归属」的既有语义。
- 不触碰 PermissionCard 的危险卡 Enter=拒绝安全裁决。

---

## §5 变更记录
- v0.1 (2026-09-20) 冻结：Enter 发送 / Shift+Enter 换行 / Cmd+Enter 发送；依据 A2-11 与 #66。
