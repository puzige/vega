# 结构化 `@file` 引用（方案 B）

## 1. 问题

Vega 把 `@` 引用实现为**发送时对纯文本重新扫描**（`vega_tools::reference::reference_tokens`），
任何「空白后的 `@word`」都被当成项目内路径，解析失败即拒绝整条消息。

后果：粘贴网页正文（含 `@账号`、代码块内 `@x`、带全角标点的 `@名字，`）无法发送，
并同时出现「引用文件不存在」与「检查模型、权限、工作目录」两条互相冲突的提示。

## 2. 目标语义（对齐 ChatGPT / Claude Code）

**引用只在选择时产生。** 用户从 `@` 候选选择器选中真实文件，才构成结构化引用；
正文里手打的 `@whatever` 永远是普通文本，不参与解析，也不会阻断发送。

已验证的对照实现：

- ChatGPT 桌面版：composer 内 `@` 是结构化节点（`insertMentionAtSelection` →
  `{type:'mention', path}`），候选来自索引，手打 `@foo` 不选候选即普通字符。
- Claude Code 2.1.292：`at_mentioned_files` 是独立附件类型；单个 mention 失败
  `return null` 或降级为 `{unread:"unexamined"}`，**消息照常发送**。

## 3. 契约

### 3.1 数据模型

`TextInput` 持有有序 mention 列表：

```rust
struct Mention {
    /// 覆盖 `@path` 的 content 字节范围（含 `@`，不含尾随空格）。
    range: Range<usize>,
    /// 项目相对路径，选择时来自有界索引。
    path: String,
}
```

不变量：

1. `range` 始终落在 `content` 的字符边界内，且 `range` 区间内的文本等于 `@path`。
2. mention 按 `range.start` 升序、互不重叠。
3. 任何让不变量 1 失效的编辑都**丢弃**该 mention，而不是尝试修补。

### 3.2 编辑联动（全部 content 变更路径）

| 路径 | 行为 |
|---|---|
| `complete_at_query` | 插入新 mention，范围 = `@` 到路径末尾 |
| `replace_text_in_range` | 与编辑区间重叠的 mention 丢弃；起点在编辑区间之后的整体平移 delta |
| `replace_and_mark_text_in_range` | 同上（IME 组合文本走这条） |
| `set_text` / `clear` | 清空全部 mention |
| `set_text_with_references` | 供草稿/队列回填，按保存的字节范围与路径恢复；验证边界、文本与不重叠，不扫描正文 |
| 粘贴、剪切、删除、预测接受 | 经 `replace_text_in_range` 或 `set_text`，自然覆盖 |

mention 的 `range` 覆盖 `@path` 文本本身，因此复制粘贴得到的是纯文本 `@path`，
不会跨消息携带引用身份。

### 3.3 渲染

mention 范围渲染为**单一 accent 色的 run**（`text_color = accent`），不新增背景块、
不加图标、不加文案。引用只是「这段文字是文件」，用颜色即可表达，符合
「优先用控件/状态表达功能」。

标记（IME）下划线优先级不变：marked range 仍按原逻辑切分，mention 颜色在
marked 之外的片段生效。

### 3.4 提交

`ComposerSubmitted` 增加 `references: Vec<String>`（路径，按出现顺序）及 `reference_mentions`（原始字节范围与路径），后者随队列持久化与失败草稿保留。恢复时不靠正文搜索猜测引用位置。

2026-10-27 续接核对修订：同一正文可同时包含手打与选择器产生的相同 `@path`；只有路径列表无法无损恢复，因此补充原始范围契约。旧队列缺少范围时保持普通文本，不猜测身份。

- `content` 仍是用户可见原文，**不含**注入块。
- 引用只来自 mention 列表，提交时不再扫描正文。
- 队列消息（`PendingMessage`）持久化 `references`，旧记录缺字段时按空列表读取
  （`#[serde(default)]`），不做数据库迁移。

### 3.5 解析

`vega_tools::reference` 用显式列表替换文本扫描：

```rust
pub fn resolve_references(
    root: &Path,
    paths: &[String],
    max_files: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
) -> Result<Vec<ResolvedReference>, ToolError>
```

保留全部既有安全围栏且语义不变：根目录 canonicalize、`resolve_in_root` 的
绝对路径/`..`/符号链接逃逸拒绝、末段 symlink 拒绝、目录拒绝、NUL 二进制探测、
单文件 16 KiB、总量 48 KiB、最多 8 个引用。任一条失败仍 fail-closed ——
因为此时用户**明确引用**了该文件，静默不注入会让模型基于缺失内容作答。

删除 `reference_tokens` 与 `resolve_bounded_references`（无其它调用方）。

### 3.6 失败与恢复

- 引用在**创建提交之前**校验（`submit_ordinary` / `enqueue_composer` 入口）。失败时
  只调用 `reject_composer_submission` + `apply_reference_error`：草稿、mention、
  附件都还在输入框，没有清空、没有用户回声、没有失败提交行，也就没有那条永远
  不会成功的 `Retry`。
- 引用文件在选中后被删除/移走：仍然拒绝发送，错误文案沿用
  `domain.reference.missing`，因为这次是真实的显式引用失败。
- 提交前校验通过但 worker 侧读取仍失败（提交与读取之间文件被删，或队列消息过期）：
  worker 仍是权威读取并 fail-closed，同时把提交时的草稿快照（文本 + references +
  图片）回填输入框，避免用户重打整段正文。该快照在提交被接受时立即丢弃，
  所以已提交消息不会被重新当作草稿。
- 引用错误与「模型/权限/工作目录」通用提示**互斥**：`reference_failure` 存在时
  不再叠加 `preparation_failed`。
- 正文里的 `@word` 不再产生任何错误。

## 4. 非目标

- 不改数据库 schema（不新增表/列；队列 payload 是 JSON 文本字段）。
- 不改权限模型、provider 重试、索引预算（512 条 / 2 s / 8192 item 保持不变）。
- 不做 mention 的鼠标悬停预览、点击打开文件、拖拽重排。
- 不实现符号索引、watcher、远程文件。
- 不改变 `FileReferenceFailureCode` 的隐私约束（仍不暴露绝对路径、errno、内容）。

## 5. 影响模块

- `crates/vega_ui/src/text_input/{state,element}.rs` — mention 模型、编辑联动、渲染
- `crates/vega_ui/src/conversation_stream/{composer,mod,file_reference}.rs` — 提交载荷、选择器写入
- `crates/vega_conversation/src/{ordinary_submissions,pending_messages}.rs` — 传递与持久化
- `crates/vega_conversation/src/types/pending_messages.rs` — `PendingMessage.references`
- `crates/vega_tools/src/reference.rs` — 列表式解析
- `crates/vega/src/{app_agent.rs,window/{ordinary_send,pending_messages,agent}.rs}` — 接线

## 6. 验收

1. 正文含 `@不存在的名字`、代码块内 `@xxx`、`@名字，`：**正常发送**，请求不含注入块。
2. 选择器选中真实文件后发送：注入块内容正确，原文 `@path` 保留。
3. 选中后文件被删除再发送：拒绝发送，提示引用不存在，provider 调用数 0，
   **且草稿仍在输入框**（含 mention、图片）。
4. 选中后在 mention 前面插入文字：引用仍然生效（范围已平移）。
5. 选中后删除 mention 内任一字符：该引用失效，不再注入，消息正常发送。
6. 队列消息（queue/steer）携带引用时行为与直接发送一致。
7. 旧队列记录（无 `references` 字段）仍可读取。
8. 引用错误不再叠加通用「模型/权限/工作目录」提示。
9. 8 文件 / 16 KiB / 48 KiB / symlink / 目录 / 二进制 / 越界围栏全部保持不变。

## 7. 验证方式

本地只做静态与格式检查，不编译、不跑测试（仓库规则）。行为验证交云端 `pr-check`；
粘贴长文、选择器、队列回填、草稿恢复等真实交互由用户手测。
