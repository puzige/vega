# 消息与代码块复制

版本：v1.3，2026-10-19；复制验收沿用冻结 v1，追加 C9 菜单资源修复记录。Issue：[复制功能增强 #477](https://github.com/puzige/vega/issues/477)。

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

## v1.1 交付规则与冻结 C9 修复（#482 后）

用户要求同步 `efd92833488fb3432042682e81d7f4eec1a10d0d`（#482）。C1–C10 的行为验收、13个开发测试的 fixture/目标/断言不变；本节更新交付顺序和原 C9 的必要焦点接线，不追加测试或追认缺失证据。此前文档所述开发保留测试，以本节和 [Rust 测试生命周期](vega-rust-test-delivery-lifecycle.md) 为准。

冻结源码为 `crates/vega_ui/src/conversation_stream/tests/copy_actions.rs`，SHA-256 `688d2428efe401cd0b5eba4e615509e418fdc5a22398aacc600bdf819bdb1086`。完整源码已固定到 [PR 原始测试归档](https://github.com/puzige/vega/pull/479#issuecomment-6253908078)，并读回校验；完整测试提交 `3bbd02ef878e24c5e81d175b103a2c15f9b97933`、有效 RED [39155948614](https://github.com/puzige/vega/actions/runs/39155948614/job/117759089744) 和此前等价接线修正均保留在 PR/Issue。该 RED 发生在重放 `e14241237d00e8a666f96bcf69ab30e58286777d` 前；后续 GREEN 仍未取得，不把135/136通过写成GREEN。

带测试运行 [39156461571](https://github.com/puzige/vega/actions/runs/39156461571/job/117760583604) 中，C9 用户消息与最终回复的 Tab/Enter/Space 已通过，随后到达代码按钮的 Tab 检查失败。静态分析确认 `composer_actions.rs::move_composer_focus` 对内部控件取模环绕并停止传播，使焦点进入 Composer 后不能返回会话正文。修复仅保留内部控件顺序及 IME guard，在首尾边界或当前焦点不属于内部控件时交给 `window.focus_prev/next`；不改测试、不改变复制数据和提交行为。该文件加入本卡允许范围。

开发时仅保留本卡13项测试及必要几何观察、定位标记和UI dev依赖 `gpui_kit[test-support]` / `vega_runtime`。不恢复 master 已删除的123个旧测试、旧fixture/mock、runtime test-support或无关dev依赖。

交付顺序固定为：

1. 保留同一冻结测试，修复原 C9；现有 draft PR 通过 Actions 确认全部13项实际执行并 GREEN。冻结文件哈希保持不变，未知计数如实记录。
2. 在 PR 固定带测试 GREEN 的完整提交 SHA、原始源码/必要接线补丁、RED 原因与 GREEN run/job及准确计数，读回确认可找回；不依赖可删除分支、本机文件或过期artifact。
3. 用独立提交删除本卡Rust测试、`cfg(test)`模块及几何观察、仅测试使用的debug selector和dev依赖/锁包。逐项审查无生产/benchmark消费者；保留真实控件ID、可访问名称、焦点/事件/布局/滚动、Python测试及fixture、benchmark和通用CI。
4. 删测后不夹带生产行为变化，运行最终head完整云端门禁。fmt、全目标Clippy、Python以及Rust发现/构建成功；Rust报告 **0发现/0执行，`no Rust tests`**，不是“Rust测试通过”。出现基线整合导致实现变化时，先恢复同一冻结测试重新GREEN再删测。
5. 带测试GREEN与最终空集门禁均可核实后才转正式PR，并按原看板流程合并、保留资源与开放Issue等待用户手测。按用户要求保留真实开发历史和独立删测提交，不重排提交追认TDD。


## v1.2 等价平台激活接线修正

带测试 head `f1668bf61b6b3d2c9b56c8e174cfaa8c5d7a1e17` 的 [Actions 39157501853](https://github.com/puzige/vega/actions/runs/39157501853/job/117763655839) 实际执行13项，12通过、1失败、0跳过；fmt/Clippy成功。C9已通过三类按钮的键盘复制和选字/右键，最后URL断言失败。GPUI测试平台的open_window不激活窗口，active初始false，模拟输入也不会激活；原fixture缺少平台激活，因此被生产链接的前台窗口保护阻止。

主控先在Issue留存源码证据并批准接线例外，再由专职子agent在C9创建visual后仅增加activate_window和park两行，提交 `84db9971f137451d1501d35f919bb99b8b08050f`。生产保护、原操作/计时、测试数据、目标与全部断言不变，测试仍为13项；不增删测试，不直接聚焦按钮，不重排历史或事后补测。

当前重新冻结SHA-256为 `c1d8db023323402c7d19d87b1667eec45215e14c2c37b217d018c64d751162cf`。[原始完整源码](https://github.com/puzige/vega/pull/479#issuecomment-6253908078)与[完整等价修正补丁](https://github.com/puzige/vega/pull/479#issuecomment-6254089023)已固定在PR，组合可复原当前文件且校验哈希一致。既有有效先行RED照原始时序保留，本次平台接线失败不作为行为RED。当前13项仍待真正GREEN，取得并固定证据前不删除；后续严格执行v1.1的独立删测与最终空集门禁顺序。


## v1.3 冻结 C9 暴露的菜单资源生命周期修复

复制实现暂不继续变动，不删测。`dbc39a9973a3b49aace91a4f4afb63abd293b08b` 的 [Actions 39158417842](https://github.com/puzige/vega/actions/runs/39158417842/job/117766386812) 中，现有C9全部复制/草稿/选择/菜单/链接断言已通过，但退出时GPUI实体泄漏断言报告唯一残留 `gpui_component::menu::popup_menu::PopupMenu`。整体为13执行、12通过、1失败、0跳过，fmt/Clippy成功，不能作为GREEN。

本节在修复前记录独立的资源修复切片：打开选区右键菜单并用键盘复制、关闭菜单后，原交互结果保持，窗口结束时释放该菜单及其独占订阅。复用原先已经冻结并实际失败的C9（其GPUI退出检查已涵盖该资源结果），不追加测试、不改变数据或断言；该缺陷的真实RED为上述带测试运行，不用最初“复制入口缺失”的RED冒充菜单资源证据。当前冻结源码SHA-256仍为 `c1d8db023323402c7d19d87b1667eec45215e14c2c37b217d018c64d751162cf`。

只读根因：gpui-component 0.6.1 的ContextMenu在Dismiss回调强捕获shared_state，shared_state持有PopupMenu与Subscription，Subscription又持订阅表，形成强引用环。Dismiss只设open=false，未释放菜单/订阅；App正常清理窗口也无法解除该环。上游状态字段私有，不能从Vega公开接口直接修复内部状态；不通过测试清全局、忽略句柄、关闭泄漏检测或修改依赖缓存过关。

允许的最小生产修复：仅替换 `render_rows::selection_context_menu` 的生命周期包装，可放入同目录私有 `selection_menu.rs`；继续复用现有PopupMenu内容/禁用状态、样式、键盘处理、定位、点击外部关闭和焦点恢复。菜单状态强持菜单实体，Dismiss订阅仅弱持状态，关闭时释放菜单与订阅，避免状态和实体互相强持。保留稳定控件身份、正文选择/链接、焦点、事件和布局；如函数签名必须补窗口参数，仅修改现有调用点（含thinking_body），不扩展到其他菜单。

不改冻结测试、依赖/锁文件、上游源码、CI、Python或benchmark。先由专职子agent完成最小修复，主控审查引用所有权与差异；同一完整13项实际GREEN并留证后，才按v1.1独立删测、最终空集门禁和正式PR。原功能实现与该次资源修复的基线、RED和GREEN分别记录，保留真实提交顺序。
