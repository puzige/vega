# Vega R5 文件引用应用链 SDD

- **版本**：v0.1
- **日期**：2026-09-05
- **状态**：已冻结，可实现
- **范围**：原生 GPUI 的 `@file` 索引、候选选择、提交解析与请求注入
- **关联**：S8 T47、R5 应用评审、Zcode UI 参考

## 1. 决策与目标

R5 把现有输入框里的 `@` 从本地展示状态接成一条可验收的应用链：

```text
用户输入 @
  -> VegaWindow 订阅 FileIndexRequested
  -> 有界后台索引
  -> 带 owner/generation 的候选快照
  -> 键盘或鼠标选择候选
  -> ComposerSubmitted 携带结构化引用
  -> 提交前有界解析
  -> 成功后才创建 provider 请求
```

成功路径必须经过真实应用订阅和真实请求组装，不能只在 UI 模型里模拟下拉。索引与解析都必须失败关闭：结果不完整、身份过期、路径越界、读取失败或预算耗尽时，均不得把不完整内容交给 provider。

本卡不读取真实用户文件，不发送真实 provider 请求，不使用 pi，不修改权限确认逻辑，不增加依赖，不修改 DDL 或 `Cargo.lock`。测试使用隔离临时项目、内存或测试 store 和 MockProvider。

## 2. 已冻结的边界

### 2.1 索引预算

现有 512 条候选上限继续保留。达到 512 条时可以截断候选快照，但 UI 和文档不得声称已经穷举仓库；索引排序和过滤必须对这 512 条稳定可见结果保持确定性。

单个相对 UTF-8 路径最多 4096 字节。路径超过该值或无法表示为 UTF-8 时跳过，因为这类条目不能安全地成为可编辑、可回传的引用。

后台 job 最多保留 2 MiB 的候选路径数据。超过此预算返回 typed failure，不发布部分快照；不能以“已经收集到的前缀”冒充成功结果。

后台 job 最多检查 8192 个由串行 `ignore::Walk` 交付的 yielded item（包含其交付的目录、非文件和错误项；ignore 库在交付之前过滤的 hidden/gitignore 项不计入这个可观察计数）。若在自然完成或得到 512 个可展示候选之前达到上限，返回 typed failure，不发布部分快照。这个预算约束 yielded traversal 工作；库内部已经开始的 readdir、ignore 匹配或其他阻塞 filesystem syscall 不能被此 API 逐节点计数或强行中断。

单次 job 的 cooperative wall budget 为 2 秒。实现应在串行 walker `next` 前后、每个 yielded item 处理前后，以及可控的排序、转换阶段继续检查取消和 deadline。deadline 只承诺 cooperative 检查；它不承诺中断已经进入的阻塞文件系统 syscall。预算耗尽时返回 typed failure，不发布部分快照。

取消是逻辑取消：取消旧 job、递增 generation、清除 owner 后立即使其结果失效，不等待 worker join。worker 的 late result 必须经过 owner/generation fence，并在过期时丢弃。

### 2.2 引用解析预算

解析阶段继续执行已有安全边界：每次提交最多 8 个不同引用，每个文件最多 16 KiB，所有文件合计最多 48 KiB；必须重新 canonicalize、检查项目根目录 fence、拒绝 symlink/directory、拒绝二进制内容。索引结果只能提供候选，不能绕过提交时的全部检查；普通非 UTF-8 文本沿用现有 read 语义，以有界 lossy 文本注入，二进制 NUL 仍然拒绝。

解析阶段的错误必须是可识别的 typed failure。禁止使用 `unwrap_or(content)`、空内容或原始用户文本作为解析失败时的 provider 输入。

## 3. 模块边界

### 3.1 `vega_tools`

`vega_tools` 是索引和解析的唯一权威层，保持 headless、无 GPUI 和无 SQLite 依赖。它负责：

- 在给定项目根目录下执行有界 walk；
- 执行路径字节、visited item、retained bytes 和 cooperative deadline 检查；
- 生成只含相对 UTF-8 路径和必要显示元数据的 typed snapshot；
- 在提交时重新解析并执行根目录、symlink、大小、总量和二进制检查，并沿用 read 的文本编码语义；
- 返回稳定的失败 code 与简短安全 message，不返回绝对路径、文件内容或敏感诊断。

索引 worker 不访问 provider、store 或窗口状态。解析函数仍可被现有 agent worker 调用，但必须在 provider 构造和 `chat_stream` 之前完成。

### 3.2 `vega_ui`

`vega_ui` 只管理输入、候选快照、焦点和 typed UI 状态，不执行文件系统、SQLite、线程或 provider 操作。保留 TextInput 的 IME、`@file` token、keyboard guard、caret 和现有输入高度重排语义。

`FileSelectorModel` 的应用事件携带 owner/generation；UI 不自行推断当前项目或启动线程。query 改变时只在已经被接受的快照上本地过滤，不为每个字符启动 job。

### 3.3 `vega`

`VegaWindow` 订阅 `FileIndexRequested`，负责从 store 和当前路由解析项目根目录，维护每个 stream 的单一 active job，并把 worker 结果回投 GPUI 主线程。它负责 owner/generation、取消、路由切换、设置页切换和 stream entity 生命周期 fence。

窗口层不得把一个旧 stream、旧 thread、旧 project 或旧 generation 的结果投递给当前 UI。窗口关闭、切换 thread/project、离开编辑路由、打开 Settings 或显式取消时都必须取消并清除对应 job。

## 4. 事件、身份与状态机

### 4.1 请求与结果 envelope

索引请求至少包含：

```rust
FileIndexRequested {
    thread_id: ThreadId,
    project_id: ProjectId,
    generation: u64,
}
```

取消和重试沿用同一身份字段：

```rust
FileIndexCancelled { thread_id, project_id, generation }
FileIndexRetryRequested { thread_id, project_id, generation }
```

窗口内部 envelope 至少保存：

```rust
FileIndexOwner {
    stream: Entity<ConversationStream>,
    thread_id: ThreadId,
    project_id: ProjectId,
    generation: u64,
}

FileIndexJob {
    owner: FileIndexOwner,
    cancel: CancellationToken,
}

FileIndexOutcome {
    owner: FileIndexOwner,
    result: Result<FileIndexSnapshot, FileIndexFailure>,
}
```

具体类型名可以按现有模块命名调整，但不能丢失这四个身份维度：stream entity、thread、project、generation。failure code 必须稳定、可测试，并且不能包含绝对路径、文件内容或 provider 凭证。

### 4.2 接受结果的 fence

主线程接受 outcome 前必须同时满足：

1. active job 的 owner 与 outcome owner 完全相同；
2. 当前 `OpenedThread` 仍是同一个 thread；
3. 当前选中 project 仍是同一个 project；
4. 当前 stream entity 仍存活且是当前正文 stream；
5. 编辑路由仍允许索引，Settings 未覆盖当前编辑页；
6. stream 的 generation 与 outcome generation 相同。

任意一项不满足都静默丢弃结果，并清理已经失效的 job 引用；不得重新打开 selector、覆盖新快照或改变当前输入。

### 4.3 UI 状态

selector 的可观察状态为：

```text
Closed
Loading
Ready(candidates, query, highlighted)
Empty(query)
Failed(code, query)
```

输入 `@` 且没有可用快照时进入 Loading，并显示紧凑的索引提示；job 成功后进入 Ready 或 Empty。失败保留原始可编辑 query，并提供 Retry；Retry 只取消并替换当前 owner/generation 的 job，不复用未知完整性的部分结果。

Esc、删除 `@` token、光标离开 token、切换 thread/project、打开 Settings 或关闭 stream 都会关闭 selector 并使当前 job 失效。job 完成后不得因为旧 query 重新打开已关闭的 selector。

已成功发布的快照可以在当前 stream 内存中短暂缓存，供 query 变化过滤；切换 thread/project 或 stream 后丢弃旧缓存。缓存不是持久化协议，也不改变数据库 schema。

### 4.4 选择与焦点

候选选择保持现有键盘和鼠标行为：上下移动高亮，Enter/Space 接受，Esc 取消，点击候选接受。selector 打开时焦点必须仍可达输入和候选；事件捕获不得吞掉输入框的 IME、`@` token 或 keyboard guard。

接受只写入结构化相对路径引用并保留尾随空格语义；不在 UI 层读取文件，不在 UI 层拼接文件内容。鼠标和键盘必须走相同的 `AcceptFile` 处理路径。

## 5. 提交与 provider 顺序

Composer 提交事件携带原始可编辑文本和已选结构化引用。应用层先解析引用，再决定是否启动 provider：

```text
submit
  -> 清理当前 submit owner/pending 状态
  -> 解析并校验所有引用
  -> 失败：typed ReferenceRejected，保留输入，返回 UI
  -> 成功：生成注入后的 content
  -> 创建 provider / chat_stream
```

解析必须发生在 provider 构造、认证调用和 `chat_stream` 之前。失败时：

- provider call count 必须为 0；
- 清理 pending/run owner，避免旧请求状态卡住下一次提交；
- 保留用户原始输入和引用 token，用户可以编辑后重试；
- 不为失败的新提交新增 user echo 或 durable run；
- 不删除已有持久化历史，不修改 DDL；
- 只向 UI 返回稳定的 typed rejection 与可读错误。

若现有持久化时序无法同时满足“失败不 echo”和“历史不删除”，应停止实现并先修订本 SDD；不得通过删除已有历史规避问题。

成功时，解析得到的 fenced reference block 必须按既有约定注入原始用户内容之前，随后才创建并调用 provider。注入内容只来自通过全部限制的文件读取结果。

## 6. 失败、重试与可观测行为

failure code 至少覆盖：

- `Cancelled`：用户或路由主动取消；
- `DeadlineExceeded`：2 秒 cooperative budget；
- `VisitedLimitExceeded`：8192 item 上限；
- `RetainedBytesExceeded`：2 MiB 快照上限；
- `PathTooLong` 或 `InvalidPath`：无法成为安全相对 UTF-8 引用；
- `ProjectUnavailable`：当前 project 无法解析；
- `ReferenceOutsideProject`、`SymlinkRejected`、`NotRegularFile`；
- `FileTooLarge`、`TotalBytesExceeded`、`BinaryContent`；
- `ReadFailed`：文件读取失败。

索引失败和解析失败都不得发布部分结果。UI 错误提示只暴露稳定 code 对应的短文案；详细 errno、绝对路径和内容只留在受控测试诊断中，不进入事件、历史或 provider prompt。

Retry 必须创建新 generation，并在启动新 job 前让旧 generation 失效。旧 worker 即使随后完成，也只能被 fence 丢弃。

## 7. 测试设计

### 7.1 `vega_tools` headless 测试

使用隔离临时目录和可控 fixture 覆盖：

- 正常索引、确定性排序、512 条截断语义；
- 4096B 路径、2MiB retained、8192 visited item、2 秒 deadline；
- 取消、预算失败不发布部分 snapshot；
- hidden/gitignore、symlink、非 UTF-8、非 regular item；
- 8 文件、16KiB 单文件、48KiB 总量和根目录 fence；
- 解析失败返回 typed error，绝不生成 fallback content。

测试不得读取用户真实目录，不依赖网络或真实 provider。

### 7.2 `vega_ui` 状态与焦点测试

覆盖输入 `@` 的 Loading/Ready/Empty/Failed 状态、query 过滤、键盘/鼠标选择、Retry、Esc/关闭、焦点可达性，以及以下 late-result 情形：

- 关闭 selector 后旧成功结果不得重新打开；
- 切换 thread/project 后旧结果不得覆盖新 stream；
- Retry 后旧 generation 不得覆盖新 snapshot；
- 输入清空和 IME 提交不破坏现有 TextInput 行为。

不写颜色或布局镜像测试；只验证状态、事件和可观察行为。

### 7.3 真实应用订阅 E2E

至少一组测试通过真实 `VegaWindow` subscription 触发 `FileIndexRequested`，使用 owned temporary project/store 和 MockProvider：

1. 输入 `@`，由 app subscription 启动有界 worker；
2. 选择候选，提交真实 composer event；
3. 断言 MockProvider 收到带 reference block 的请求，并且原始用户文本仍存在；
4. 对超界、解析失败、取消、切换 route 和过期 generation 分别断言 provider call count 精确为 0；
5. 断言失败后输入仍可编辑，pending/run owner 已清理，既有 durable history 未被删除；
6. 断言 Retry 只产生当前 owner 的结果，旧结果不改变 UI。

E2E 不使用真实 provider key、真实用户文件或生产 store。MockProvider 的调用计数必须在测试边界直接读取，不能用日志推断“没有调用”。

## 8. 实施顺序与非目标

实施顺序固定为：

1. 在 `vega_tools` 抽出带取消和预算的 typed index/resolver API；
2. 在 `vega_ui` 增加 owner/generation 事件和 Loading/Failed/Retry 状态；
3. 在 `vega` 接上 subscription、named worker、主线程投递和 fence；
4. 把提交前 resolver 移到 provider 构造之前，移除 fail-open fallback；
5. 增加 headless、UI 和真实 app subscription E2E；
6. 编译、聚焦回归和原生验收按 exec guide 执行。

R5 不实现仓库全文搜索、符号索引、实时 watcher、远程文件、权限模型、provider 重试策略、持久化索引缓存、数据库迁移或新的 UI 设计系统。R5 也不承诺在阻塞文件系统 syscall 中途强行终止线程；只保证每个可控阶段的 cooperative 检查和 late-result fence。

## 9. 变更记录

- **v0.1 / 2026-09-05**：冻结 R5 应用链、owner/generation fence、索引与解析预算、失败关闭语义、真实 app subscription E2E 和零 provider negative tests。
