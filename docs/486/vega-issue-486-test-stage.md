# Issue #486 · 先行测试阶段交付

2026-10-19 12:45 Asia/Shanghai（04:45 UTC）。状态：测试补丁已写齐，待 Codex 审查、提交 draft PR 并确认 Actions 有效 RED。尚未进入生产实现。

## 基线与授权

- 方案：[技术方案 v2](vega-issue-486-markdown-link-preview-implementation.md)，需求验收 A01–A12 保持原范围。
- 生产未改基线与方案提交：`35d443dbade4f0766cadc81e915613719b358279`。
- 分支：`feat/486-markdown-link-preview`。开始修改前已执行 `git fetch origin`、`git rebase origin/master`，结果为已是最新。
- 专职实现者当前仅获准写测试、临时 cfg/dev 接线及本页。没有提交、push、GitHub 评论或生产实现。
- 主会话拥有需求/技术方案文档；本阶段未修改其内容。工作树中技术方案的独立文档差异属于主会话。

## 变更清单

| 路径 | 本阶段内容 |
|---|---|
| `crates/vega_conversation/src/link_preview_tests.rs` | 22 个服务行为测试，调用待实现的真实服务、分类、解析、网络策略、缓存和取消边界 |
| `crates/vega_conversation/src/link_preview_test_support.rs` | 仅测试使用的脚本 DNS/transport、受控网络 gate 与单调时钟；不实现 URL/IP、解析、重定向、缓存或队列决策 |
| `crates/vega_conversation/src/lib.rs` | 两个条件测试模块入口 |
| `crates/vega_conversation/Cargo.toml` | 临时 `test-support` feature 与图片 fixture 的 dev dependency |
| `crates/vega_ui/src/conversation_stream/link_preview_tests.rs` | 20 个模型/真实 GPUI 行为测试，使用生产正文、选择、文件预览、设置发行说明和思考宿主 |
| `crates/vega_ui/src/conversation_stream/link_preview_thinking_fixture.rs` | 在 thinking 子模块内调用已有构造/append/展开方法的测试 fixture，不新增生产可见构造 API |
| `crates/vega_ui/src/conversation_stream/{mod,thinking}.rs` | 条件测试模块入口 |
| `crates/vega_ui/Cargo.toml` | dev-only GPUI test-support、conversation test-support 及图片 fixture dependency |
| `Cargo.lock` | GPUI test-support 解析出的 10 个测试支持包及 dev 边；既有 914 个包的身份、版本与 checksum 均未变 |

未建立服务/UI 的生产模块、stub、URL helper、解析器、状态机或公共兼容 shim。除上述条件模块接线外，生产 Rust 源保持基线内容；正常 dependencies 尚未加入 html5ever/reqwest/image。

## 自动验收覆盖

共 42 个测试定义：服务 22 个；UI/模型 20 个（其中 18 个使用真实 GPUI 测试上下文）。这只是静态定义数量，**全部 NOT RUN**，不是发现或执行计数。

| 验收 | 已写的可观察断言 |
|---|---|
| A01 | 浅深 accent、最低 Medium、强语义/斜体/代码/标题保留；默认无线；hover 真实短 quad 且同排有 2px 空隙；pointer 离开后的 focus 仍绘制；文本/图标几何和源文不变 |
| A02 | GitHub 主机/子域、伪后缀、userinfo、未知站点与 mail 分类；宿主图标固定 16px、与文字间隔 4px；正常物化/paint 零 DNS/零请求 |
| A03 | 一个混合样式 occurrence 保持一个目标；相邻同 href 独立；普通/可选/用户/真实 FilePreview/真实 Settings updater/Thinking 宿主；窄表格跨行；普通行内/围栏代码与相对/不支持目标无交互 |
| A04 | 图标/正文各有效点击一次；499/500ms；1px 微移、5px 拖动、多击、Shift、右键、不同目标 down/up、blur/卸载/失活取消；真实选择内容、生产复制到剪贴板与原始 Markdown 字节 |
| A05 | 399/400ms；跨折行片段保持既有 dwell；快速划过不抓取；进入卡片后可保留；149/150ms 离开；Escape 后不自动重弹 |
| A06 | 真实服务读取实体/属性顺序/大小写、7-byte 网络断块、重复字段、title RCDATA、script/style/template/comment/body 伪 meta、UTF-8/BOM、相对图片和 effective host；卡片真实标题/摘要/图片/Host/Open |
| A07 | 缺标题/摘要/图片不虚构；网页标题回退原目标；坏图、错误 MIME、5MiB/8MP 边界、非 HTML、401/403、编码错误；有效 PNG/JPEG/WebP 与公共图片 redirect；图片区比例且无缺失项占位 |
| A08 | Network 失败的 Retry 绕过负缓存取得真实内容；Open 始终可用且只打开原目标；Blocked/Unsupported/RequiresLogin/mail 不形成自动重试循环 |
| A09 | A 未完成切 B，旧 A 不覆盖；active request 未完成时 Escape、取消后 gate 完成也不重开；wheel/程序滚动、scope、卸载、外点、失活即时关闭；普通流式旁文保留卡，但文本身份变化取消 pending click |
| A10 | IPv4 特殊区段、localhost、IPv6 特殊区段、mapped/private、数字/十六进制 loopback、空/混合 DNS、userinfo；批准地址固定一次、异常 peer 拒绝、逐 redirect/图片 hop 重新检查；公共 HTTP:80/IPv6/mapped-v4 正例；自动受限地址仍允许原有主动 Open |
| A11 | fragment 共用、query 独立、正负 TTL、Retry 只绕过负缓存、64 条 LRU/32MiB weight、2 active/8 queued/Busy 不缓存、取消 queued 不连接、单飞单订阅离开不取消他人、最后等待者取消、2s DNS/10s 总时限、8192 UTF-8 bytes 精确边界 |
| A12 | 真实阅读焦点 → link → Open/Retry/地址展开 → 后续链接；Enter、Escape、回焦、卸载回阅读区域；鼠标 hover 不抢 Composer；真实 Link role/label/目标描述、无图标焦点与非模态退出路径 |

真实 dash 像素、字体/图标视觉、窗口边缘与真实外网/浏览器效果仍按 M01/M02 交用户手测；不把 GPUI 逻辑/scene geometry 当成视觉或外网验收。

右键测试沿用 [#357 已记录的测试 fixture 收尾](../357/vega-issue-357-markdown-links.md)：保留生产选择菜单，先断言 PopupMenu 实际显示、Escape 后消失且不打开链接，再销毁窗口，对观察所得仍存活的弱菜单发送既有 DismissEvent，最后断言全部弱句柄释放；不禁用 LeakDetector，不声称修复上游菜单环。

## 预期接口缺失 RED

所有行为断言均已写出。当前基线没有下列目标 API，draft PR 预期首先报告最小接口缺失；在 Actions 实际取得日志前不声明已 RED，也不声明任何行为断言已经执行。

| 缺失入口 | 冻结测试使用的契约 |
|---|---|
| `vega_conversation::link_preview::{LinkPreviewService,classify_target}` | `load(&str, retry: bool, CancellationToken)` 异步返回 typed data/failure；分类返回原目标/site |
| `vega_conversation::types` 的预览 DTO | data 字段为 original_target/effective_target/host/site/title/description/image；image 为 png bytes/width/height；site 为 Github/Web/Mail；稳定失败为 Blocked/Unsupported/RequiresLogin/Network/Timeout/TooLarge/Malformed/Busy/Cancelled |
| 临时 `link_preview::test_support` | 仅重导出真实生产内部端口及 `service_with_ports` 构造；不另写测试决策 |
| `PreviewResolver` | owned host/port/token → `BoxFuture<Result<Vec<SocketAddr>, Failure>>` |
| `PreviewTransport` | 已批准的 PreparedRequest/token → status/headers/peer/有界可消费 byte-chunk stream；请求包含 url、pinned_addresses、headers |
| `PreviewClock` | 单调 `Duration now()` 与 `sleep_until(Duration)`；服务中的真实 TTL/deadline 使用同一时钟 |
| `vega_ui::link_preview::{MarkdownReadingScope,MarkdownLinkPreviewLayer}` | 阅读 scope/focus/真实 ScrollHandle 包装与单窗口根层，不使用缺失的 GPUI component Root |
| UI 临时 `test_support::{install_dependencies,visible_links,LinkObservation}` | 仅注入真实服务与外部 opener，并只读实际登记 occurrence/target/label/description/role、文字/图标几何、真实 FocusHandle；不计算替代几何或预览决策 |
| 结构化 `links::LinkRange` | 持有真实 occurrence 与原文字节 range；当前 tuple 无这些字段 |

端口是生产服务真实 requester/解析/缓存的外部边界；测试 transport 只能提供 DNS、peer、response、错误和完成顺序。返回的 pinned_addresses 由被测生产策略决定。UI 使用同一真实服务，未替换成伪 preview loader。

## 临时支持清理清单

带相同冻结测试云端 GREEN 且源码/commit/原始输出固定到 PR 后，独立删除：

1. 两个测试模块、共享 network/clock fixture、thinking fixture 及其条件模块入口。
2. conversation `test-support` feature、生产模块中仅为跨 crate 测试重导出的端口/factory；UI 仅测试使用的依赖注入/只读观察出口。保留生产真正使用的端口、状态、控件 ID、无障碍、焦点/事件/scroll/paint 与图像能力。
3. dev-only GPUI feature、conversation test-support dev 边和无生产消费者的图片 fixture dev 边。重新解析 lock，只清理测试独占包；之后生产已使用 image 时保留其正常边。

不得通过删除失败或未执行测试换取空集绿色；删测前后审查生产投影一致，最终完整门禁明确 `no Rust tests`，与带测试 GREEN 分别留证。

## 内容标识与静态结果

| 文件 | SHA-256 |
|---|---|
| conversation service tests | `cd688119fe429bc05cb592cfbf09164dc472b1b931c76d7dbfbf073efac1909b` |
| conversation external fixtures | `b43ae6426450ca724611ef39021b7c70b24ee89fc2b3e56cd7089adaf8eee3be` |
| UI/model tests | `40c1d0cd5b87a38ecc43f85448c9453e578a42fd4aa890e1bbb87d00bebd167f` |
| thinking fixture | `6683f075a1fcfd3185958d60393abc19a9d5e60573a01725513344ba07080ca6` |

本地只执行以下低 CPU 静态工作，退出码均为 0：

- `cargo fmt --all` 与 `cargo fmt --all -- --check`。
- `cargo metadata --offline --format-version 1`（仅依赖解析，更新 dev 支持 lock）及 `cargo metadata --offline --locked --no-deps --format-version 1`。
- `git diff --check`；锁文件包身份/checksum 对比；测试定义与新 Rust 文件无注释的静态清点；文件 SHA-256。

未运行 Vega、编译、Clippy、Rust/Python 测试、benchmark 或打包。未获得 Actions run/job、实际发现/执行数或行为失败原文；这些均待 Codex 建立/复用 draft PR 后取得。若云端先发现与缺失目标接口无关的测试接线错误，先保留原始证据，经审查修正接线，不把无关错误算有效 RED。

下一步：Codex 审查本补丁并固定内容 → 提交/push/draft PR → 核实最小目标接口缺失 RED → 明确授权实现者进入生产实现。当前在测试交接点停止写入。
