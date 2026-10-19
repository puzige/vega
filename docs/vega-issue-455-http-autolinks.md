# Issue #455 · 普通 HTTP/HTTPS 地址可点击

> 2026-10-19：后续展示以 [#486 需求 v1](vega-issue-486-markdown-link-preview.md) 为准，替代本页 A02/C1 的常显实线与不新增图标要求，并加入完整悬停预览；该需求尚未实现。裸 URL 的识别边界、目标保真、原文/代码排除和增量处理契约不变，历史验收保留。

来源：[Issue #455](https://github.com/puzige/vega/issues/455)。2026-10-18 用户明确要求实现；参考图为用户消息中的裸 GitHub 地址。本卡是新增识别能力，Codex 冻结规格后组织专职子 agent 实现，主会话负责审查、PR、CI 和集成。

## 根因与基线

基线为 `6382c908e14f2ad267e6068c672080a735541f0b`。#357 已接通 Markdown 链接的地址、主题样式和字形命中，但刻意不识别裸 URL。

- `conversation_stream/model.rs::flatten_inlines` 将普通 `Inline::Text` 直接变为 `link: None`。
- `render_rows.rs::user_message_lines` 将每一行用户原文视为一个普通 span；可选用户消息调用 `append_styled`，没有传递链接范围。普通用户消息直接绘制 `StyledText`，也没有链接交互。
- `links.rs`、`inline_code.rs` 和 `selection.rs` 已有共享字形命中、手形指针、500ms 多击仲裁和拖选取消，复用即可。无需修改 Markdown 语法、存储、运行时或外部打开入口。

## 实现前验收矩阵

所有自动用例先编写，执行交给 Actions；当前为 NOT RUN。

| ID | 前置/操作 | 可观察结果 | 验证层级 |
|---|---|---|---|
| A01 | 用户消息含中文前后文和一个或多个裸 http/https 地址 | 精确原文保留；只有地址带各自目标；空行、行尾、换行和气泡几何契约保持 | 模型与 GPUI |
| A02 | Markdown 段落、标题、列表、引用、表格及展开思考含裸 URL | 可选/普通路径均有链接范围、主题 accent 和既有 1px 下划线；样式叠加保留粗斜体 | 模型与 GPUI |
| A03 | 地址含 query、fragment、percent escape、端口、IPv6、Unicode 路径、成对括号 | 目标逐字保真，不截断合法路径或配对括号；末尾句号/逗号/中英文句末标点、外围引号及未配对闭括号不进入目标 | 扫描器与模型 |
| A04 | 空地址、不完整 scheme、相对地址、www、email、ftp/file/javascript/data、嵌入字母词中的 scheme | 不制造自动链接；已有显式 Markdown/mailto 链接目标与白名单保持 | 扫描器与回归 |
| A05 | `[https://visible.example](https://target.example)`、`<https://example.com>`、带格式的显式标签 | 保留显式链接目标，不重复链接、不把标签裸地址覆盖为新目标 | 模型与 GPUI |
| A06 | URL 出现在 Markdown 行内代码或围栏/缩进代码中 | 继续作为代码展示和复制；不新增自动链接或改变语法高亮 | 模型 |
| A07 | 单击裸地址；拖选、双击/三击、Shift 扩选、右键、离开目标、失焦或文本替换 | 只有有效单击打开准确目标一次；保留 #357 的取消、选择与复制契约 | 生产 GPUI 事件 |
| A08 | 地址跨自然折行，链接前后含非链接文字；用户消息和普通阅读宿主 | 各视觉行地址末字形可点，前后普通文字不打开；原文与选文复制无装饰文本 | 生产 GPUI 几何与事件 |
| A09 | 流式 URL 分散在多个 delta，随后 finish 或历史恢复 | 最新模型目标正确；已冻结块不重新物化；没有每次绘制/全消息反复扫描 | 增量模型与回归 |
| M01 | 合并后的版本，浅/深主题，用户消息、模型回复、思考及窄窗口 | 用户手动核实下划线、手形、浏览器打开和选择复制 | 待用户手测 |

## 冻结契约

1. 自动识别带 `http://` 或 `https://` 的绝对地址，scheme 大小写不敏感。保留 URL 显示文本和目标字节，不规范化、不缩短、不新增图标、标题或说明。沿用 #357 的主题链接色、1px 下划线及字形命中。
2. 在消息模型物化时扫描，普通用户消息和 Markdown 文本共用一个内部扫描器。可使用独立的私有 `autolinks.rs` 模块，不新增依赖或公共 API。扫描为线性推进，不能在每帧绘制或每 delta 全量历史解析。
3. URL 以空白、控制字符、外围引号、尖括号和常用中文句读为边界；去除句末 ASCII 标点与没有对应开括号的尾部闭括号，保留 URL 内部标点、成对括号和 IPv6 authority。只接受已有 `allowed_link` HTTP/HTTPS 边界；不得把词内的 `abchttps://` 误识别为地址。短路非候选文本，所有范围均为有效 UTF-8 字节边界。
4. Markdown 显式链接优先，保留其标签与目标；格式化文本的原样式仍叠加。代码 span、代码块、高亮 token 和外部工具原文不新增自动链接。现有解析器将原始 HTML 降为普通可见文字，该文字沿用同一扫描规则，不解释 HTML 属性、不执行 HTML、不为区分来源扩展 Markdown 节点。此卡只扩展可见消息文本的 URL 识别，不改 Markdown 解析语法或源位置映射。
5. 两条用户气泡渲染路径均传递精确链接范围，沿用现有交互元素及稳定 ID。保留用户文字本身的纯文本语义，不将用户消息改成 Markdown 渲染。
6. 复用 #357 全部交互、安全及复制规则，不新增外部联网预览、tab stop、配置、成功提示或打开方式。悬停和消息到达不会打开地址。既有显式 mailto 及非白名单地址行为不变。

## 实现顺序与文件归属

专职实现 agent 是本卡唯一代码写入者，工作区 `~/Workspace/worktrees/vega-455-http-autolinks`，分支 `feat/455-http-autolinks`。Codex 主会话仅改规格/交付记录、审查和集成。开始修改前 fetch 并确认分支基线；保护他人文件，不 reset。

1. 在 `crates/vega_ui/src/conversation_stream/autolinks.rs`（或既有 `links.rs` 内）先写 A03/A04 扫描用例，再实现小型纯扫描器。
2. 在 `model.rs` 的普通文本物化接入自动链接，保留显式链接覆盖和格式样式。`render_rows.rs` 为用户消息建立 spans，并在普通/可选气泡路径复用 `markdown_text` / `append_styled_runs` 等既有交互入口。
3. 允许修改 `conversation_stream/mod.rs` 注册私有模块，`tests/mod.rs` 注册本卡测试；新增 `tests/issue455_http_autolinks.rs`。可在 #357 测试中将必要的夹具 helper 改成同测试模块可见，不能扩大生产公开 API或改写既有断言。
4. 只在确有接线需求时修改既有 `links.rs`、`selection.rs`、`inline_code.rs`，不重构 #357 交互状态机。遇到冻结契约缺口交回主会话，不扩到别的 issue。
5. 交付文档为 `docs/vega-issue-455-http-autolinks-delivery.md`，报告文件、实际静态命令/退出码、未执行的云端测试、风险及偏离。禁止新增代码注释和非测试 unwrap/expect。

## 验证、交付与回滚

本地只做 `cargo fmt --all -- --check` 和 `git diff --check` 等快速静态检查，不运行 Vega、不编译或测试。GitHub Actions 执行：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

保留 #357、#432、#147、#78 的全部已有断言，不新增真实浏览器/网络 E2E，不弱化断言或增加忽略。CI 启动后每 5 分钟查询一次。仅最新 head 的 required check 成功后由 Codex squash merge；Issue 保持开放并进入 In review，保留本卡分支/worktree，等待用户 M01 结论。代码合并不代表本机应用已更新。回滚为回退本卡集成提交。

## 实施前来源边界修订（v2）

源码复核确认 `vega_markdown/nodes.rs` 将 HtmlBlock/InlineHtml 降为 `Inline::Text`，UI 展平时没有 HTML 来源信息。v1 要求在这个接入点独立排除原始 HTML 不可执行。v2 按当前真实数据边界明确：可见的降级文字沿用普通文本扫描，不识别或执行 HTML；保留零解析器改动和原文字节，不为此增加节点、依赖或来源映射。其余验收与范围不变。

## 云端夹具隔离修订（v3）

首轮 [Actions 39083785375](https://github.com/puzige/vega/actions/runs/39083785375) 为 3024 passed / 3 failed / 5 skipped。三个失败均来自同一测试上下文内依次检查多个 renderer：前一组合的合法点击已写入平台的最近打开 URL，后一组合的新窗口仍读取该值，却断言为 None。

锁定 GPUI 的 `TestPlatform::opened_url` 为平台共享字段，`open_url` 覆盖其值，`TestAppContext::opened_url()` 只克隆、不清空；创建新窗口不会新建测试平台。v3 将用户普通/可选、Markdown 普通/可选、折行 user×selectable 各组合分别放到独立的 `#[gpui_kit::test]` 上下文，步骤可由私有 helper 共用。保留所有原有 None、精确目标、几何、原文复制和非链接区域不打开断言，不改成与此前值比较，不重置生产记录，不新增公共 API、忽略或豁免。该修订仅隔离测试夹具，生产契约不变。
