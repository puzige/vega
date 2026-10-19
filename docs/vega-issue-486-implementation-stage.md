# Issue #486 · 实现阶段交接

2026-10-19。执行者 `link_preview_executor`，worktree `vega-486-markdown-link-preview`，分支 `feat/486-markdown-link-preview`。生产实现已写入工作树，待 Codex 主会话审查、提交和云端验证；未 commit/push，没有修改冻结测试，也没有进入删测阶段。

## 时序与验证状态

主会话确认冻结测试提交 `5d46a1955d0212182f128e46dec20c67898dce55` 的 Actions `39201250059` 为有效最小接口缺失 RED 后，明确授权本次生产实现。该 RED 的 42 个 Rust 用例断言未执行，不能计作行为失败数量；原始证据由主会话维护在交付页。

本阶段本地仅执行格式、差异和静态检查。未运行 Vega、编译、构建、Clippy、test/nextest、benchmark 或打包。实现编译结果、42 个冻结用例的执行结果及 GREEN 均为 **待云端 CI 验证**。

| 本地检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `git diff --check` | exit 0 |
| `cargo metadata --offline --locked --format-version 1 --no-deps` | exit 0；只读取清单，不编译 |
| lock package identity 静态比较 | 与冻结提交相同，924 个 name/version/source/checksum 身份均未变化 |
| 新增生产源码静态审阅 | 没有新增源代码注释，没有新增 `.unwrap()` / `.expect()`；没有同步 `runtime.block_on` |
| 四个冻结文件 SHA-256 | 与冻结记录逐项一致，见下表 |

## 实现落点

| 范围 | 修改路径与实现 |
|---|---|
| 服务及 DTO | 新增 `crates/vega_conversation/src/link_preview.rs`、`link_preview/metadata.rs`、`link_preview/ports.rs`、`types/link_preview.rs`；接入 conversation 的 `lib.rs` / `types/mod.rs` |
| 已批准依赖 | workspace `Cargo.toml`、`Cargo.lock`、`crates/vega_conversation/Cargo.toml`：直接复用 pinned html5ever/image/reqwest，启用生产共享 Tokio runtime 所需 `rt-multi-thread`；没有升级包 |
| 逻辑出现身份与文本 | `conversation_stream/model.rs`、`autolinks.rs`、`links.rs`：同标签样式片段共享 occurrence，相邻相同 href 保持独立；原始文本身份仍取消延迟点击，预览只按 scope/owner/occurrence/精确 target 保持 |
| 几何与选择 | `conversation_stream/inline_code.rs`、`selection.rs`、`thinking_body.rs`：在 flow wrapping 前仅为原始起点预算 20px，图标不进入文本；真实文本布局、选择及复制索引保留原字节 |
| 样式及图标 | `conversation_stream/render_rows.rs`、`icons.rs`、`crates/vega_theme/src/lib.rs`：链接最低 Medium、accent、无永久 underline；本地图标，真实 baseline 下偏移 2px 的 device-snapped 1/2/2 虚线 |
| 共享窗口协调器 | 新增 `crates/vega_ui/src/link_preview.rs`，接入 UI `lib.rs`、stream `mod.rs` 和 `crates/vega/src/window/render.rs`：每窗口一个非模态 defer-draw 卡，400/150ms、取消/generation/存活检查、键盘焦点代理与阅读 scope |
| 真实宿主接线 | `conversation_stream/render.rs`、`thinking.rs`、`file_preview.rs`、`settings/updater.rs`：会话、思考、文件和更新说明复用服务/窗口层，真实 ScrollHandle/ListState 签名参与关闭 |

服务使用同一生产请求策略处理网页及图片的每一跳。拒绝私网/混合 DNS/userinfo 等地址，固定全部已批准连接地址并核对 peer，禁代理、自动重定向和自动解压。HTML 解析使用 html5ever tokenizer 的 RCDATA/ScriptData/Rawtext，并处理 head 范围、实体、首个有效字段及 UTF-8 声明；图片仅接受已验证 PNG/JPEG/WebP，限制尺寸/分配，转换为不放大的 RGBA PNG 缩略图。图片失败或在整体截止时仍未返回时保留页面文字成功结果。

服务核心保持 async、runtime 无关；实际 IO/计时 ports 使用单个共享 Tokio runtime（2 个 worker、最多 2 个 blocking worker），GPUI 可在受控网络等待时停放。单飞按独立 flight id 追踪等待者，最后一位离开取消；2 active / 8 queued、缓存 64 项/32 MiB、LRU/TTL/Retry 依照冻结规格。timer future 在尚未首次 poll 就被丢弃时也会取消其真实计时任务。

最终真实 DNS 资源审查补正：不再调用取消后可能遗留 blocking 工作的 `tokio::net::lookup_host`。真实 resolver 先可取消地取得共享的 2 个 DNS permit 之一，再将 permit 移入执行 `ToSocketAddrs/getaddrinfo` 的 blocking closure。已开始的系统解析不能强行中断，其 permit 一直保留到原生调用结束；尚未开始的 job 可 abort，等待 permit 的任务可取消。resolver future 即使尚未首次 poll 就被丢弃也会取消其子 token。这样超时、关闭和反复取消不会绕过 permit 积累原生 DNS 工作或其 blocking 队列，服务的 DNS 2s/整体 10s 截止与调用者结果失效规则保持不变。补正前重新 fetch，`origin/master...HEAD` 为 0/3，无需变更基线；下列静态检查和四个 hash 已重新核对。

窗口层的真实代理共同消费并记录 role、label、description；测试观察读取同一登记值。图标不单独占焦点。鼠标动作在按下至释放及重入前抑制预览，保留原 500ms 点击仲裁与 4px 拖选屏障。Escape 也抑制仅 Armed 的链接；焦点离开同样走 150ms 路径；窗口释放取消 Armed/Leaving/Active 全部 token。卡片固定站点/Open 头部，内容及长地址在卡内滚动，标题/摘要采用明确行高与 2/3 行限制。

## 冻结测试完整性

| 文件 | SHA-256 |
|---|---|
| `crates/vega_conversation/src/link_preview_tests.rs` | `cd688119fe429bc05cb592cfbf09164dc472b1b931c76d7dbfbf073efac1909b` |
| `crates/vega_conversation/src/link_preview_test_support.rs` | `b43ae6426450ca724611ef39021b7c70b24ee89fc2b3e56cd7089adaf8eee3be` |
| `crates/vega_ui/src/conversation_stream/link_preview_tests.rs` | `40c1d0cd5b87a38ecc43f85448c9453e578a42fd4aa890e1bbb87d00bebd167f` |
| `crates/vega_ui/src/conversation_stream/link_preview_thinking_fixture.rs` | `6683f075a1fcfd3185958d60393abc19a9d5e60573a01725513344ba07080ca6` |

A01–A12 的测试定义、矩阵和冻结清单位于测试阶段页；本阶段未增加、删除、更名、放宽、ignore 或重试任何测试。UI 临时观察位于 `link_preview::test_support`；服务临时 re-export/factory 仍按已批准 `test-support` cfg 隔离，没有第二套策略、解析或缓存逻辑。

## 下一步与边界

Codex 主会话审查完整树，提交到现有 draft PR #491 并运行原门禁。生产编译和行为仍可能由 CI 揭示需要修正的实现问题；不能将上述静态检查称为 GREEN。真实字体/像素、外网站点及默认浏览器结果仍待用户 M01/M02 手测。

仅在相同冻结测试 GREEN 且证据固定、主会话明确授权后，独立删除四个冻结文件、cfg 模块、conversation 临时 feature/re-export/factory、UI dev-only feature/跨 crate dev edge、注入 opener 与 UI 观察模块/登记标记。保留真实生产 ports、服务依赖、窗口焦点及可访问语义。随后由主会话验证最终空集 head 完整门禁；本阶段没有清理资源或推进 Issue/PR 状态。

主会话同时修改的 README、需求/技术方案与 `vega-issue-486-delivery.md` 已保留，不属于执行者本次文档写入。
