# Issue #486 · 技术方案 v2 与测试先行交接

2026-10-19。承接 [需求 v1](vega-issue-486-markdown-link-preview.md)，用户已明确授权补齐技术方案并实现。需求范围不变；本页冻结工程边界与执行顺序。当前状态：方案已完成，待先行测试与 Actions 有效 RED；任何 NOT RUN 项不表示实现已通过。

## 1. 代码事实与修改边界

方案基线为需求提交 `13739db02c5a47fbc5c90353ac3fa797ce14a2a1`，其生产树基于 `f054c35874513bab4c65d7e179fdac6758503182`。开工前已 fetch/rebase；后续基线变化须重新审查生产差异与测试证据。

- `model.rs::StreamSpan` 只有目标地址，没有链接出现位置的身份。`coalesce` 会合并同样式/同目标 span，`links.rs::link_ranges` 又按样式 span 切分链接：同一标签含粗体时不能按现状逐 span 加图标，也不能把相邻同 href 的不同标签合为一个链接。
- 可选正文与普通正文已共用 `links.rs::register_link_interactions`。行内代码的 `InlineFlow/shape_flow/position_fragments` 已有不进入文本的留白几何；装饰槽沿用此路径。
- 展开思考的 `thinking_body.rs::prepare_line/PreparedFragment/FragmentsElement` 有独立的预布局与片段消费，需要一起接入，不能只改常规正文。
- pinned GPUI 为 `gpui-kit 0.6.1 → gpui-pre 0.3.4`。`UnderlineStyle` 只有 thickness/color/wavy；本卡自绘真正的虚线，不升级或修改 GPUI。
- 窗口根为 `VegaWindow`，已有 `TextSelectionLayer`；不能依赖没有挂载的 gpui-component Root。共享预览层挂到真实窗口根，所有阅读宿主复用。

允许修改：`vega_ui` 的正文/思考/表格/普通阅读链接管线、图标、Markdown 预览浮层；`vega_theme` 的本卡共享 token；`vega_conversation` 的无 UI 预览服务及 `types`；`vega/src/window/render.rs` 根层接线与必要阅读宿主；上述 crate 的必要依赖/临时测试接线和本卡文档。不得改运行时、数据库、用户设置、解析语法、CI 拓扑或无关产品功能。

## 2. 模块与契约

| 模块 | 责任 | 不允许承担的工作 |
|---|---|---|
| `vega_conversation::link_preview`（新） | URL/站点分类、连接策略、元信息解析、图片校验/缩略、取消、单飞与有界缓存 | GPUI、窗口/焦点、模型调用、凭据、SQLite |
| `vega_conversation::types::link_preview`（新） | 跨 crate 的站点种类、真实目标/有效目标、元信息、已净化图片、稳定失败类型 | 窗口对象、解析器 DOM、reqwest response 或原始 HTML |
| `vega_ui::link_preview`（新） | 单窗口协调器、触发/关闭状态、异步投影、非模态卡和键盘控件 | 直接抓取 URL、重新决定网络白名单、把远程 URL 交给 GPUI 图片加载器 |
| `conversation_stream/{model,autolinks,links,inline_code,selection,render_rows,thinking_body}.rs` | 出现身份、装饰槽、文字样式、命中/选择、可见焦点代理、根层注册 | 改写消息/复制字节、每帧扫描全消息、每个链接独立弹层 |
| `vega/src/window/render.rs` | 真实窗口中挂载一次 `MarkdownLinkPreviewLayer` | 复制一份卡片/网络逻辑给文件预览或更新说明 |

服务的生产边界为：纯目标分类；可取消的 `load(original_target, retry, cancellation)`；返回真实 requested/effective 地址、host、可选 title/description/image 或稳定不可用原因。跨 crate 数据一律位于 `vega_conversation::types`。内部 resolver/transport/clock 边界可在开发测试中替换，公开 API 不为测试扩大；业务校验、解析、缓存及取消决策必须仍运行真实生产代码。

图片输出只允许服务重新编码后的 PNG bytes 与校验后的宽高，UI 用本地 bytes 构建图像。不得把未校验的原始响应或任何远程图片 URL 传给原生图片加载器。

## 3. 依赖裁决

Codex 在本卡按架构审查职责批准以下精确范围；不升级既有依赖：

- `html5ever = 0.27.0` 作为直接依赖加入 workspace 与 `vega_conversation`；该版本及传递依赖已在 lock 中。使用 tokenizer，不引入 DOM、CSS selector、浏览器或另一个 HTML parser。
- `vega_conversation` 复用 workspace 的 `reqwest = 0.12.24`、`image = 0.25.10`、`futures`、`tokio`、`tokio-util`；URL 使用 reqwest 重导出的 `Url`，不引入第二份 URL 实现。
- 开发期间允许 `gpui_kit` 的 dev-only `test-support`，以及现有 workspace 测试辅助能力；最终 GREEN 后清理测试独占依赖/feature，保留生产、benchmark、Python 与通用 CI 能力。
- 不新增 favicon、GitHub API、第三方 unfurl、HTML regex 扫描器或新的配置依赖。遇到额外 crate 需求先交 Codex 审查，不能自行扩依赖。

## 4. 网页与图片服务

### 4.1 URL 与实际连接

1. 保留用户 original_target，Open 永远使用该原值。抓取用解析后的 http/https 绝对 URL，去掉 fragment；不根据 `og:url` 或 canonical 字段更换打开目标。
2. 预览拒绝 userinfo、无效 host、控制字符、超长地址、localhost 及其子域。mailto 只作本地目标投影，不进入网络服务；不影响旧的主动打开白名单。
3. 每次实际请求均独立解析 DNS。只要结果为空或包含非公网地址，就拒绝整次请求，不能过滤掉私网结果后偷偷继续。IPv4 文本变体与 IPv4-mapped IPv6 先规范到真实地址再检查。
4. 公网策略排除 IPv4 特殊/不可路由区段（含 0/8、10/8、100.64/10、127/8、169.254/16、172.16/12、192.0.0/24、192.0.2/24、192.88.99/24、192.168/16、198.18/15、198.51.100/24、203.0.113/24、224/4、240/4）；IPv6 仅允许 2000::/3 内正常公网单播，排除 2001::/23、2001:db8::/32、2002::/16、3ffe::/16、3fff::/20 等特殊/转换/文档地址。保守拒绝这些特殊区段是自动预览策略，不冒充所有地址的通用路由判断。范围依据 [IANA IPv4](https://www.iana.org/assignments/iana-ipv4-special-registry/)、[IANA IPv6](https://www.iana.org/assignments/iana-ipv6-special-registry/)，查阅于 2026-10-19。
5. 建立无代理、无 cookie、无凭据、无 Referer、禁自动 redirect 的独立 reqwest client；将通过检查的 SocketAddr 用 pinned `ClientBuilder::resolve_to_addrs` 固定到原 host，保留 Host/SNI 与正常 TLS 校验。不得换成 IP URL，不能在实际连接时走第二次未校验 DNS；连接目标/可读 peer 必须属于本跳批准结果。
6. 手动处理 301/302/303/307/308 的 Location，相对跳转基于本跳 URL；每跳重新执行同一校验/固定。网页图片、图片重定向完全复用此请求器。非 http(s)、缺失/无效 Location、跳转上限与不满足策略的地址均终止，不访问新目标。
7. 请求只发匿名 GET 与最少 Accept/User-Agent，要求 identity encoding；不从环境代理、浏览器、Provider/MCP 或会话取认证上下文。网络错误仅投影稳定原因，不记录 URL/query、响应正文或凭据。

### 4.2 元信息

- 页面响应必须成功且为 HTML（`text/html` 或 `application/xhtml+xml`）；401/403 是需登录/禁止访问，非网页是 Unsupported，不解析错误页作成功预览。
- 有界读取 bytes；UTF-8/BOM 正确处理。不能确定支持的声明编码时按不可解析回退，不输出已知乱码。不得用缺少元信息的失败页标签冒充目标网页标题。
- html5ever tokenizer 只收集 head 元信息与 title，按 HTML 原有顺序选择首个非空有效值。`og:title/og:description/og:image` 优先，文字回退标准 title/meta description；仅选择一张候选图，不请求任意正文图片。字段来源与多值优先级参照 [Open Graph](https://ogp.me/)。
- token sink 正确进入 title 的 RCDATA、script 的 ScriptData、style 等 raw-text 状态；忽略注释、script/style/template 等中的伪 meta。属性顺序、大小写、引号、实体、断块与重复字段不能靠脆弱字符串匹配处理。
- 相对图片地址基于最终响应 URL；meta 不能覆盖实际站点主机。仅把纯文本与候选图片 URL 交到下一阶段，不输出 HTML，不执行脚本。

### 4.3 上界与图片

| 项 | 固定上界/行为 |
|---|---|
| URL | 8192 UTF-8 bytes；超限只禁自动预览，原链接照旧可打开 |
| 时间 | 一次 load 总时限 10s（含排队/DNS/网页/图片），DNS 2s，connect 3s；取消立即使结果失效 |
| 重定向 | 每种资源最多 3 跳；最多初始请求加 3 次重定向请求 |
| HTML | 2 MiB；先检查 Content-Length，仍逐 chunk 检查累计值；不按服务端声明预分配无界内存 |
| 元信息 | title 512 字符、description 2048 字符；值规范空白/去控制符，按 Unicode 边界截断 |
| 图片输入 | 5 MiB，仅 PNG/JPEG/WebP；MIME 与实际解码格式一致，不解 SVG、GIF、视频、HTML |
| 图片解码 | 最大 8,000,000 pixels，decoder allocation limit 64 MiB；先验算宽高乘积再解码，错误只移除图片 |
| 图片输出 | 保持比例，最长容器 512×288，不放大；重新编码 PNG，不保留原始 metadata |
| 服务缓存 | 最多 64 个结果且总 weight ≤32 MiB；weight 包含字符串与图片 bytes，超限 LRU 淘汰 |
| 有效期 | 成功 30min；可恢复失败 30s；Cancelled/Busy 不缓存；Retry 跳过该目标失败缓存 |
| 并发 | 服务最多 2 个 active load、8 个排队的不同目标；同目标单飞，不占重复名额；超限返回可重试 Busy |

缓存 key 为网络目标（fragment 不入 key，query 保留）；返回给每位调用者的 original_target 单独保留。单飞请求记录等待者：一个调用者关闭仅取消其订阅，最后等待者离开才取消底层工作；迟到结果不能留存/复活已取消的卡。缓存不持久化，UI 的本地图片缓存同样有归属和释放时机；错误重试不等于自动无限重试。

服务只在后台 executor/工作线程中驱动 Tokio IO，不在 GPUI paint/prepaint 中创建网络工作。首个 hover/focus 延迟达成后才提交 load；绘制、物化、历史恢复与滚动本身保持零 load。

## 5. 链接出现身份、布局与绘制

1. `StreamSpan` 增加稳定的逻辑出现身份/开始信息；显式链接与裸 URL 在物化阶段分配。它与原始 occurrence 的字节范围关联；不是 URL 相等就合并。多样式同标签共享 occurrence，相邻同 href 两个标签分别保留。
2. `LinkRange` 改为带目标、出现身份、原范围与片段开始标记的结构。`fragment_link_ranges`、思考 `PreparedFragment` 与可选文本位置映射一并传递；裁到链接中间的 fragment 不重复添加图标。
3. `InlineFlow` 使用零文本字节的装饰 fragment，图标前置宽度 16+4=20px，在 shaping/wrapping 前计入；图标后与正文保持同一行盒。强制同一套 flow 覆盖有链接但无 code 的普通/可选/思考路径，不从 opaque StyledText 反读私有字段。
4. 文本 `content_range/logical_range/TextLayout` 仍只对应原内容，装饰槽只参与几何。选择、复制、点击的字节换算不能插空格、U+FFFC 或图标字符补洞。
5. `apply_span_style/block_text_runs` 保留语义格式，允许链接最低 Medium，去掉永久 underline；所有图标、槽位、焦点和虚线几何来自共享 token。
6. 虚线按实际文本 LinkRegions 画 snapped quads：厚度 1px、dash 2px、gap 2px、下偏移 2px。hover/focus 更新装饰绘制，不改 text runs 的几何；跨行分别裁在文字范围内。图标和尾部空白不画线。

数值核对：图标槽 16px + 间隔 4px = 20px（只在原出现起点预算一次）；虚线周期 2+2=4px，末段裁到 label 右界；512×288×4=589,824 bytes 为最大 RGBA 缩略图未编码体积，不代表服务可以忽略 32 MiB 总缓存上界。

## 6. 窗口层、焦点与状态机

- 新 `MarkdownLinkPreviewLayer` 从 `vega_ui` 导出，挂在 `VegaWindow::render` 的现有 TextSelectionLayer 邻近，**每个 WindowId 恰好一个**。根层拥有 Entity，注册表只留弱引用；链接注册可见几何，不各自持有卡片/网络服务。
- 协调器键为阅读 scope + occurrence + target，而非整段文本。根层 prepaint 完成可见目标登记并清扫已卸载触发器；会话 scope 切换关闭旧卡，相邻内容的普通流式更新保留有效锚点。
- 根层使用 `Window::defer_draw`（仅在 prepaint 调用）绘制，Positioner side 负责避让/钳制；卡阻断自身点击穿透，但不阻断正文选择，不在每条链接的局部裁剪区域里画浮层。
- 卡默认宽 360px、边距 12px、内距 12px；最大宽高始终小于窗口可用范围，高度最多 420px，超出内容局部滚动。内容宽 360−2×12=336px；图片按容器实际宽度等比收敛，不填充占位空白。
- 状态为 Idle → Armed → Loading → Ready/Unavailable；加上离开延迟与 dismissed-until-reentry 标记。show 400ms、leave 150ms，由可控 executor 时钟测试。卡内停留或焦点保持时取消离开计时；切目标时更新 generation 并取消旧订阅。
- capture mouse-down 在拖选/多击处理前抑制预览；wheel 关闭正文滚动触发的卡。scope 记录 ScrollHandle/ListState 的真实滚动签名覆盖滚动条和程序滚动，不能把普通锚点几何变化当成用户滚动。失活、Escape、外点、scope/target 身份变化及 liveness sweep 均关闭。
- 每次异步结果同时检查 window/scope/occurrence/target/generation 与取消状态。关闭不会被结果反向打开，旧目标不能覆盖新目标；网页先成功但图片失败时保留文字成功投影。
- 可见链接出现各有一个真实 `.track_focus/.tab_index/.role(Link)/aria_label/aria_description` 代理；折行片段与图标共享它。仅 set_focus_handle 不算可键盘导航。reading scope、FilePreview、更新说明与思考阅读面使用同一注册路径和存活的回焦 handle。
- Enter 激活原链接；focus 的 400ms 触发同卡；Tab/Shift-Tab 可进入/离开 Open/Retry/地址详情，Escape 关闭且不立即重弹。鼠标触发不调用 focus；触发器卸载时键盘回焦当前仍存活的阅读 scope，非模态卡没有焦点陷阱。

正常卡只显示真实内容和 Open；图片/摘要缺失移除对应区域；网页失败提供局部简短错误与 Retry。Blocked/Unsupported/RequiresLogin 说明当前不能预览并保留 Open；mail 卡只投影地址。所有地址/标题可查看复制，长地址展开有键盘路径；不把整卡点击当成 Open，不加成功 toast 或解释内部阶段的文案。

## 7. 先行测试冻结范围

先一次写齐需求矩阵 A01–A12 的自动测试，再进入任何生产实现。测试分两组，互相不能代替：

| 范围 | 必须验证的生产结果 |
|---|---|
| 模型/几何/选择 | 单个混合样式 occurrence 仅一次图标；相邻同 href 不合并；代码 URL 不扩展；两条正文路径/表格/思考用相同装饰槽；hover/focus 前后布局与复制字节不变 |
| 真 GPUI 交互 | 原点击仲裁、1px 微移、超过 4px 拖选、多击/Shift/右键/失活；400/150ms 边界、卡进入与切换、Open/Retry、Tab/Enter/Escape/回焦、滚动/卸载/迟到结果；mouse hover 不抢 Composer 焦点 |
| 生产解析与投影 | 真实 html5ever：实体/属性顺序/重复字段/title RCDATA/script/style/template 假 meta/相对图片/标准回退；缺字段与错误的真实卡内容，不制造文本 |
| 生产网络策略 | 外部 resolver/connector 替身只能提供 DNS/响应，不替换 URL/IP/peer/redirect/image 决策；私网/混合 DNS/映射地址/跳转越界/图片越界/超限必须不连接禁止目标 |
| 资源与异步 | 取消、旧结果屏障、单飞、最后等待者取消、正/负缓存/Retry、TTL/容量/内存淘汰、active/queue 边界；普通绘制零抓取 |

真实 dash 像素、字体视觉、站点真实外网兼容性与默认浏览器结果仍由 M01/M02 用户手测，不用 helper/静态源码检查冒充。GPUI 场景用真实生产控件/状态；测试替身只隔离外部打开/网络/时钟边界，不跳过真实判断。

测试编写阶段只允许新增测试及最低限度 cfg/dev 接线，不写生产逻辑或先造 stub 追求编译绿。新服务/API 不存在时可接受最小目标接口缺失 RED，必须逐项列出哪些行为断言未执行；无关编译错误必须先纠正测试接线，不能算 RED。测试补丁由 Codex 审查后提交 draft PR，记录冻结清单/内容 hash；实现阶段不补测试、不变期望。

## 8. 执行归属、命令、交付

| 执行者 | 文件归属/动作 |
|---|---|
| Codex 主会话 | 本卡 docs、方案/测试审查、Issue、PR、Actions 证据、集成与合并；不写生产实现 |
| `link_preview_executor` 专职子 agent | 先行测试与必要接线；Codex 明确确认有效 RED 后才独占允许范围的生产实现；经审查且 GREEN 后再独立删测。不得和主会话同时写同一文件 |

任务 worktree 继续使用 `vega-486-markdown-link-preview`、分支 `feat/486-markdown-link-preview`。实现者每阶段回报 changed paths、commit/是否 push、静态命令/退出码、冻结测试列表/内容 hash、未执行项、风险及阶段交付文档路径；未被明确授权的下一阶段停在当前交接点。不得自行开 PR、发布 Issue 评论、merge、改状态或清理本卡资源。

本地只做 diff/格式/静态读取；不得运行 Vega、编译、Clippy、测试、benchmark 或打包。供 Actions 的完整门禁保持原样：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、现有三组 Python 检查与 Nextest 清点/运行。CI 每 5min 查询，通知或用户即时要求可例外；不改 workflow 提速或恢复真实外部 E2E。

顺序：测试补丁 → draft PR 有效 RED → Codex 冻结测试 → 生产实现 → 同一测试 GREEN → PR 固定测试源码/提交及原始 RED/GREEN → 独立删 Rust 测试/独占支持 → 最终 head 空集完整门禁 → 正式 PR/squash merge → In review 等用户 M01/M02。提交证据必须在 squash/删分支后仍可访问；最终空集不是带测试 GREEN。无用户手测通过前不关闭 Issue、不 Done、不删任务分支/worktree。

回滚为整体撤销本卡实现，恢复旧链接展示并停止所有预览提交/后台请求，保留原文、点击与选择契约。遇 API/方案冲突、缺先行覆盖或新边界先交 Codex 修订，不能放宽断言或补测追认。下一页交付记录据实记 RED、实现、GREEN、删测与最终门禁，当前全部待执行。
