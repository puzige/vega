# 消息与代码块复制

版本：v1，2026-10-19。Issue：[复制功能增强 #477](https://github.com/puzige/vega/issues/477)。

## 范围与基线

基线为 `3a97f4d9ff7e6c12e3ea746247161367733e2d12`，任务分支 `feat/477-copy-actions`。本卡是 Feature，由 Codex 组织专职子 agent 实现；主控负责规格、审查、PR、Actions 和合并。

用户提供的三张截图只作为可见交互参考，不复制截图正文、私有资产或第三方内部实现。新增三种入口：用户消息下方的复制、Agent 每轮最终回复下方的复制、Markdown 代码块右上角的复制。消息动作只在 hover 或键盘 focus 时出现；代码块入口常驻。

本规格替代 #78 中消息动作禁用状态，以及旧规格允许所有流式助手正文显示整条复制的规则；不改变其他消息、选择、权限、存储及最终回复可信度契约。只增加复制，不增加截图中的时间戳、评价、朗读、展开编辑器或更多菜单。

## 先行验收矩阵

下表是冻结目标，当前均待 Actions / 用户手测，不代表已经验证。自动测试必须经过生产渲染、事件或解析投影边界，不以源码检索、只测 mock 调用或内部字段存在替代行为。

| ID | 前置状态与操作 | 可观察结果 | 验证层级 |
|---|---|---|---|
| C1 | 用户消息含中文、空行、Markdown 和末尾换行；移入正文、移到下方按钮、点击，再移出 | 按钮在气泡外右下方；移动路径不断开；剪贴板精确等于该消息安全源文；移出隐藏，正文及邻项几何不变 | GPUI 生产渲染、真实 handler 与测试剪贴板；用户手测 |
| C2 | 同一回合有 Commentary、工具及可信 Final；分别悬停与复制 | 只有最终回复末尾有整条助手复制入口，左对齐；复制完整 Final Markdown 原文，不含 Commentary、工具、错误或相邻消息 | 生产事件、渲染和剪贴板 |
| C3 | Final 含 UTF-8，或跨多个渲染片段；之后开始下一回合 | 同一最终回复只有一个入口，复制可信字节范围的完整文本；前后回合互不混入 | 生产投影、事件与剪贴板 |
| C4 | 助手仍在流式输出、只有 Commentary、finish 了一个中间片段、空回复或失败且无可信 Final；元数据重叠/越界/冲突 | 不根据最后 entry 或 finish 猜 Final，不提供误导的最终回复整条复制；原来的正文、错误及选择复制仍可用 | 生产事件与渲染 |
| C5 | 从历史恢复含多回合的用户消息、Commentary、Final 和代码块；历史无有效 Final 元数据 | 有可信 Final 的历史与实时行为一致；缺失/无效元数据时仍保留用户消息、选区/右键及代码块复制，不伪造 Final | 生产 hydration、渲染与剪贴板 |
| C6 | 同一文档有两个代码块，含中文、缩进、空行、尾换行；一个有语言，一个无语言/未知语言 | 每块右上角各有复制按钮，只复制对应 `CodeBlock.code`，不含围栏、语言标签或邻块；已知语言可见，未知语言不阻止复制 | Markdown 生产解析/投影、GPUI handler 与剪贴板 |
| C7 | 未闭合代码围栏持续收到增量、闭合并提交；最后一行有/无换行；空代码块 | 有非空代码时复制当前已接收的完整代码，不包含显示补全产生的换行或围栏；后续点击使用最新内容，提交后不串到旧块；空块没有可执行空复制、不覆盖剪贴板 | 生产流式事件、投影和 handler |
| C8 | 长代码横向滚动、窄窗口、浅深主题 | 复制按钮固定在代码容器标题右侧，不被代码滚走或遮挡；代码保持横向滚动、语法样式和缩进；控件不溢出 | GPUI 几何/交互；用户视觉手测 |
| C9 | 用 Tab 聚焦三类按钮，Enter / Space 激活；复制前 Composer 有草稿及正文选区 | 图标有可访问名称与 tooltip，焦点可见并使消息动作显现；剪贴板正确，草稿不变、不提交；选择、右键复制和链接原有行为不被拦截 | GPUI 焦点、键盘、选择与事件回归 |
| C10 | 对同一长消息重复移入/移出；过程展开/收起、历史列表重新虚拟化 | 动作显隐不改变布局高度、不启动滚动跟随；控制身份不会串到另一条消息或另一块代码 | GPUI 几何、列表/事件回归；用户手测 |

## 交互与数据契约

### 消息动作

- 用户消息复制完整的安全文字源文；纯图片消息不增加文字复制，图文消息的文字可复制。助手复制完整可信 Final 的安全 Markdown 原文，保留围栏、链接 URL、缩进与换行。
- Final 归属复用 `phase.rs` 已有 `RunResponses::validated` / `AssistantResponseMetadata` 校验，按消息身份和 UTF-8 字节范围确定。每轮最终回复只在末尾挂一个入口；跨片段要复制完整范围。不把中间 `MarkdownStream::finish()`、最后 entry、最后一行或状态标签当作 Final。
- 消息入口有稳定对象 ID，hover 区域覆盖正文到按钮的连续路径。默认不显示动作背景或说明，hover/focus 才显示按钮；按钮本身 hover/pressed/focus 使用现有主题与交互组件。不得重新引入常驻动作文案或不相关菜单。
- 控件固定 24px 命中尺寸，复用 16px Copy 图标。必要的动作槽固定 24px，正文与动作间距 4px；显隐只改变绘制，不添加/删除占位。24 + 4 = 28px，须通过布局安排与已有消息尾部留白协调，不能 hover 后临时加 28px。新增几何常量进入 `Layout`，颜色和字号沿用 token；这些是 Vega 的共享基准，不声称是截图精确测量结果。
- 点击或键盘激活只写本地剪贴板，不提交或清空 Composer、不触发父层动作、不修改对话/数据库。保留正文拖选、右键复制、链接、历史锚点和滚动行为。

### 代码块

- 范围是会话中由同一 Markdown 渲染器呈现的代码块，包括流式、最终回复及展开后的 Markdown 正文；不扩大到工具命令、工具输出、终端、diff 或附件预览。
- 顶部左侧显示解析器提供的语言（有则显示，无则留空），右侧为常驻 Copy 图标按钮。复用代码背景、圆角、内距、Typography；标题与代码内容分离，水平滚动只作用于代码内容。
- 复制源必须来自该块 `RenderNode::CodeBlock { language, code }` 的安全原文解析投影；不能从高亮 spans 或可见行拼接，因为当前 flatten 会吞末尾空行。保留实际已接收代码的全部缩进、空行、Unicode 与尾换行，不包含围栏、info string 或显示补全产生的合成换行。若 pending 展示解析补了换行，先在原文到节点边界区分并剔除合成部分，不能猜测删掉真实尾换行。
- 多块、列表嵌套、重复代码文本须保持各自身份。对当前模型没有作为独立代码块呈现的引用结构，不在本卡重写引用解析或新增呈现语义。未知语言不影响复制；空块不覆盖剪贴板。
- 流式按钮使用当前模型中的最新代码，不保存会过期的初次快照。pending → committed 后仍绑定正确代码；不为每次绘制重新解析整条消息，也不对每个 token 触发全量重排。

### UI 与安全边界

只增加「复制消息」「复制最终回复」「复制代码」等必要可访问名称/tooltip；不增加常驻说明或成功提示条。共享控件已有的 hover、按下及焦点反馈继续有效。浅深主题、窄窗口、鼠标到按钮的移动路径及键盘操作均须可用。

所有源文沿用既有隐私/安全投影，不重新读取原始存储去绕过脱敏。复制不发送网络请求，不记录正文，不创建持久化字段，不改变 Provider、工具权限、价格、配置或数据库 schema。无新依赖，不为测试扩大跨 crate 公共 API；生产代码不新增注释。

## 已确认的静态事实

- `MESSAGE_COPY_ACTIONS_ENABLED = false`，旧 `message_with_copy` 是禁用分支；真实 `render_entry_with_selection` 走 `selectable_user_message` / `selectable_markdown_message`，未调用旧 wrapper。单独翻开关不足以交付。
- `MessageCopy` 已保存安全源文和共享更新缓冲，支持本地剪贴板与选择复制，不能用显示文本覆盖它。
- `phase.rs` 已校验 Final/Commentary 元数据及历史字节边界；本卡沿用这些可信度规则。
- `flatten_node(CodeBlock)` 当前把代码拆成高亮行并丢掉尾换行形成的末行，`StreamLine` 没有完整块代码及语言的复制投影；`markdown_blocks` 当前只有横向滚动内容，没有块标题/复制入口。
- 这些结论来自源码审查；没有在本地运行 Vega、编译、测试或进行原生验收。

## 实现顺序与允许范围

1. 专职子 agent 在生产未改时写齐 C1–C10 的必要进程内测试，复用已有 GPUI 上下文、事件、测试剪贴板与 hydration 工具。优先通过真实渲染/handler 断言行为；UI 外部依赖用现有测试平台隔离。测试模块使用行为名，不含 Issue 编号。
2. 主控审查测试补丁、生产未改的基线及覆盖，提交为测试先行 draft PR。Actions 得到目标行为缺失的有效 RED 后冻结测试目标、数据、预期与补丁内容。已有行为测试可在基线通过；不能用 runner、依赖或无关编译错误冒充 RED。
3. 修改 `conversation_stream/render_rows.rs` 的真实 selection / 非 selection 路径及 `render.rs` / `phase.rs` 必要的 Final 归属传递。最小复用 `MessageCopy` 和共享图标控件，避免为恢复旧 wrapper 引入不可信整条助手复制。
4. 在 `conversation_stream/model.rs` 的 Markdown 块投影中保留语言、完整代码和独立代码块身份，流式同步沿用原增量管线。允许 `vega_markdown/src/stream.rs` / `nodes.rs` 为区分原始代码与 pending 显示补全做最小修正，不改变围栏、列表或引用的 Markdown 业务语义。`markdown_blocks` 给已呈现的代码表面增加固定标题与独立内容滚动，按对应块绑定复制。必要时调整 `vega_theme::Layout`、现有图标/交互接线和 `message_anchors.rs` 的行高估算，维持列表锚点；Final 复制缓存随历史替换、回滚、失效和新回合正确维护，不在每帧拼接。
5. 同一冻结测试验证 GREEN，主控审查所有新增可见文字、默认/hover/focus/运行/空态/失败和选择交互；云端最新 head 全部门禁绿后 squash merge master。保留本卡分支/worktree、Issue 开放，Project 改 In review 等用户手测。

允许范围为上述会话 UI/投影、Markdown pending 原文保真、任务相关测试、必要共享 token/控件以及本规格和设计守则的当前入口。其他卡修改不合并进本卡；不删除旧有效测试、不恢复外部 E2E、不改 CI 拓扑。

## Actions、证据与回滚

本地只允许 `git diff --check`、格式与低 CPU 静态检查，不运行 Vega、不编译/构建/打包、Clippy 或任何测试。RED/GREEN 都使用现有 PR workflow：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/release_tests.py
python3 scripts/prepare_computer_use_tests.py
python3 -m unittest discover -s scripts/tests -v
cargo nextest list --workspace --message-format json
cargo nextest run --workspace
```

required check 为 `check (fmt, clippy, test)`。只有实际清点为 0 才按现有 workflow 使用 `--no-tests=pass`，不能把空集写成测试通过。CI 启动后每 5 分钟查询；完成/失败通知或用户即时请求除外。

PR 保留基线、测试补丁/提交、有效 RED 的 run/job 与原始失败、冻结标识、随后实现提交、同一测试 GREEN 与最新 head 门禁证据。没有有效 RED 不进入实现；实现期间不补测、不改预期。发现遗漏或新 Bug，先记录独立验收变化并重新开始先测后修。

用户在合并与云端发布后手测：对含中文、空行、两个代码块的真实会话，分别用鼠标和键盘复制用户消息、最终回复及代码块，粘贴核对原文；确认过程不出现整条复制、移动到按钮不闪退、横向滚动保留按钮、浅深主题与选字/草稿均正常。应用更新/安装由用户正常入口完成，不为本卡在本地启动 Vega。

回滚为 revert 本卡 squash 提交，无数据迁移。用户明确手测通过前不关闭 Issue、不标 Done、不清理任务分支或 worktree。
