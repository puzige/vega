# Issue #486 · 实现阶段交接

2026-10-19。执行者 `link_preview_executor`，worktree `vega-486-markdown-link-preview`，分支 `feat/486-markdown-link-preview`。首轮生产实现已由 Codex 主会话提交并 push 为 `b5b7faed5ea6cfe0048e0e07517a2d80f8949313`；执行者未 commit/push。当前补丁修正云端发现的 GPUI API/交互问题和主会话明确批准的测试接线例外，待复审/云端验证，没有进入删测阶段。

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
| 四个冻结文件 SHA-256 | 首轮与冻结记录一致；批准的 import/ReleaseNotes setup 例外及各阶段 hash 另列，其他三个不变 |

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

## 原始冻结测试完整性

| 文件 | SHA-256 |
|---|---|
| `crates/vega_conversation/src/link_preview_tests.rs` | `cd688119fe429bc05cb592cfbf09164dc472b1b931c76d7dbfbf073efac1909b` |
| `crates/vega_conversation/src/link_preview_test_support.rs` | `b43ae6426450ca724611ef39021b7c70b24ee89fc2b3e56cd7089adaf8eee3be` |
| `crates/vega_ui/src/conversation_stream/link_preview_tests.rs` | `40c1d0cd5b87a38ecc43f85448c9453e578a42fd4aa890e1bbb87d00bebd167f` |
| `crates/vega_ui/src/conversation_stream/link_preview_thinking_fixture.rs` | `6683f075a1fcfd3185958d60393abc19a9d5e60573a01725513344ba07080ca6` |

A01–A12 的测试定义、矩阵和冻结清单位于测试阶段页；本阶段未增加、删除、更名、放宽、ignore 或重试任何测试。UI 临时观察位于 `link_preview::test_support`；服务临时 re-export/factory 仍按已批准 `test-support` cfg 隔离，没有第二套策略、解析或缓存逻辑。

## CI 首轮编译修正与批准的 import 例外

[Actions 39204406299](https://github.com/puzige/vega/actions/runs/39204406299) 在首轮实现提交 `b5b7faed5ea6cfe0048e0e07517a2d80f8949313` 失败：fmt 成功，Python 34 项成功；Rust 尚未完成发现/执行，没有 GREEN。原始错误已由主会话固定于下方 PR 批准记录，`/tmp/vega-486-ci-39204406299.log` 仅为本地诊断副本。读取 pinned GPUI/AccessKit 后，仅作如下生产 API 修正：为 `ReadingScopeElement` 派生 `IntoElement`；将代理的 `.size(Size<Pixels>)` 改为同值 `.w/.h`；四个文本节点使用 pinned StyledText 本身采用的真实 `Role::Label`，保留原 label/内容与可访问语义。

测试接线唯一例外在修改前获主会话明确批准并公开留证：[批准记录](https://github.com/puzige/vega/pull/491#issuecomment-6260399046)。只从 `crates/vega_ui/src/conversation_stream/link_preview_tests.rs:5` 的 `gpui_kit` import 删除未使用的 `App, `，没有 lint allow，没有改 fixture、数据、定义或断言。静态逐字比较确认当前文件恰好等于原始冻结文件删除这一处 import；42 个用例定义和断言全部保留。

| 文件 | 原始冻结 SHA-256 | 本次仅 import 修正后的 SHA-256 |
|---|---|---|
| `crates/vega_ui/src/conversation_stream/link_preview_tests.rs` | `40c1d0cd5b87a38ecc43f85448c9453e578a42fd4aa890e1bbb87d00bebd167f` | `ac95026c77e216a344fb9af5a73c7b3c7206b31b214e286a00ead50312bdfcab` |

其余三个冻结文件继续与原始表中的 hash 逐项一致。修正前工作树干净，已 fetch 并更新远端任务分支，无 history rewrite。修正后 fmt、diff、offline locked metadata 静态检查重新 exit 0；未本地编译/执行测试，待主会话提交后交云端复验。

第二轮 Actions `39205170482` 在 `d6ec15f81e1bc25aff611693b2c07801276a74e5` 仍因 `ReadingScopeElement` 编译失败，未发现/执行 Rust 测试：`IntoElement` 派生实现要求 `RenderOnce`，不适用于此自定义 Element。按主会话明确指示及仓库 `MarkdownLinkPreviewLayer` / `InlineCodeText` 的现有模式，删除该 derive，显式实现 `type Element = Self; fn into_element(self) -> Self { self }`；没有实现 `RenderOnce`，没有改布局或交互。原始日志 `/tmp/vega-486-ci-39205170482.log` 保留；本轮仅该生产修正和本段报告，测试/fixture 零改动。修正前 fetch/update 干净分支；fmt、diff、hash 静态检查重新通过，UI 测试继续使用已批准的 `ac95026c77e216a344fb9af5a73c7b3c7206b31b214e286a00ead50312bdfcab`，其他三个仍为原始冻结 hash。未本地编译/测试、未 commit/push，GREEN 仍待云端验证。

第三轮 Actions `39205794474` 在 `c1e8c64072102eec7110578ab4502fdff64fc7f0` 已完成编译、发现并执行 42 个 Rust 用例：28 passed / 14 failed / 0 skipped。其中服务 22 项全部成功，UI 为 6 成功 / 14 失败；这仍是失败门禁，不是 GREEN。原始日志 `/tmp/vega-486-ci-39205794474.log` 保留。12 个卡片场景在 pinned GPUI `element.rs:453` 报 `must call request_layout before prepaint`：`Window::defer_draw` 仅入队，后续 `prepaint_deferred_draws` 直接调用 prepaint。生产修正是在入队同一个 AnyElement 前调用 `layout_as_root`，以 viewport AvailableSpace 完成 request/computed layout；不提前 prepaint，不重复 prepaint。

第三轮另一个 UI 失败为点击后重新聚焦 Composer 没有触发 blur 取消：此前代理 `prevent_default` 使普通点击一直保持 Composer 焦点。生产修正移除该抑制，普通单次无修饰左键点击按真实可见逻辑目标转焦（覆盖图标与后续片段），仍只在鼠标按下转焦，hover 不转焦；鼠标转焦期间保留 dismissed-until-reentry，避免点击同时重新触发预览。Clippy 的唯一报告 `Option<Rc<dyn Fn(&str)>>` type-complexity 改用 `LinkOpener` alias，没有 allow。上述修正待云端重验，未改断言。

剩余 ReleaseNotes 场景的可见性诊断：fixture 的 340×240 阅读 viewport 内，208px label + 16px gap + 220px control basis 超过 group 内宽度，两组设置字段折行，加上 header/actions/disclosure 后，发行说明位于 viewport 下方。生产登记继续遵守 content mask，不登记屏外目标。最小 fixture 方案在修改前获主会话明确批准并公开留证：[ReleaseNotes setup 批准记录](https://github.com/puzige/vega/pull/491#issuecomment-6260646734)。只在既有 typed `read_with` 返回 tuple，增加外层 ScrollHandle clone；仅 ReleaseNotes 的初始首帧之后调用真实 `scroll_to_bottom` 并再 `render_frame`。`update_window` 的首参保持 AnyView，没有把它误当 Host。viewport、数据、所有 42 个用例函数体/断言/事件保持不变，其他宿主没有滚动动作。

| UI 测试源阶段 | SHA-256 |
|---|---|
| 仅批准 App import 修正之后 | `ac95026c77e216a344fb9af5a73c7b3c7206b31b214e286a00ead50312bdfcab` |
| 再应用批准的 ReleaseNotes setup 修正之后 | `9b5feb4a71a628a061168dc521c9cb1bf5bd95b44d73e8ab3ed67beca254dd7a` |

本轮静态比较确认自首个测试定义起的整个 UI 源尾部与原始冻结提交逐字一致，服务测试及两个 fixture 的原始 hash 也一致。fmt、diff、offline locked metadata 检查重新 exit 0；本轮没有本地编译/运行测试，没有 commit/push。原始 hash、两次批准及失败证据均保留，不能将 setup 修正或静态检查称为 GREEN。

第四轮 Actions `39206888765` 在 `dceb3d4edc25e895750621040e30256af4938535` 实际执行 42 项：41 passed / 1 failed / 0 skipped，耗时 12.102s；22 个服务用例及 19 个 UI/模型用例成功。fmt、Clippy 与 Python 34 项成功。唯一失败为关闭生命周期用例 line 934 的 `wheel` reason；该用例在此终止，不声称其后 reason 分支已执行。原始日志 `/tmp/vega-486-ci-39206888765-nextest.log` 保留，仍无完整 GREEN。

只读诊断确认 pinned GPUI Kit `scroll_target` 会先 render，再取目标 bounds 的中心并发送 MouseMove/ScrollWheel；340×240 正文区域中心约 `(170,144)` 被合法可进入的预览卡覆盖。原本输入实际滚向卡内，生产继续保留卡内滚动、不关闭，符合规格。静态布局给出卡宽 360、首行下方定位及无图片内容高度，正文右下内点约 `(339,263)` 仍暴露；这些是源码尺寸推导，没有冒充本地实际渲染测量。

修改前主会话已公开批准唯一 wheel 输入坐标修正：[wheel setup 批准记录](https://github.com/puzige/vega/pull/491#issuecomment-6260786505)。仅该 reason 分支用与 pinned helper 完全相同的真实 render → MouseMove（无按键/default modifiers）→ render → ScrollWheel（原 `-30px` delta）→ render 序列；保留 `move_pointer` 内部在 MouseMove 后的帧，[序列精度批准记录](https://github.com/puzige/vega/pull/491#issuecomment-6260800193) 也在修改前公开。坐标取 `body-frame.bounds().bottom_right() - point(px(1.), px(1.))`；ScrollWheelEvent 完全限定，无新增 import。没有生产修改，没有改其他 reason、数据、断言、等待时长或 viewport。静态逐字比较确认工作树测试文件恰好为本轮基线加这一处批准的输入替换；其他三个文件保持原始冻结 hash。

| UI 测试源阶段 | SHA-256 |
|---|---|
| import + ReleaseNotes setup 例外后 | `9b5feb4a71a628a061168dc521c9cb1bf5bd95b44d73e8ab3ed67beca254dd7a` |
| 再应用批准的 wheel setup 例外后（完整 helper 帧序列） | `e58d73ee6c783a4a65e88e4f1720334b09d85114f07e068b982ba89d028f387d` |

本轮修改基于主会话正常 merge 提交 `4a688fff326da7a94ff3d6e52bbe35e94e42cb67`，已包含 `origin/master` 的 `84baff54`（消息驱动会话排序），无冲突、无 history rewrite，fetch 后 `origin/master...HEAD` 为 0/8。fmt、diff、offline locked metadata 重新 exit 0；原始冻结 source、三次批准的接线/输入例外与每阶段 hash 均保留。没有本地编译/测试，没有 commit/push，也没有删测；主会话待云端验证完整门禁。

## 下一步与边界

Codex 主会话审查完整树，提交到现有 draft PR #491 并运行原门禁。生产编译和行为仍可能由 CI 揭示需要修正的实现问题；不能将上述静态检查称为 GREEN。真实字体/像素、外网站点及默认浏览器结果仍待用户 M01/M02 手测。

仅在相同冻结测试 GREEN 且证据固定、主会话明确授权后，独立删除四个冻结文件、cfg 模块、conversation 临时 feature/re-export/factory、UI dev-only feature/跨 crate dev edge、注入 opener 与 UI 观察模块/登记标记。保留真实生产 ports、服务依赖、窗口焦点及可访问语义。随后由主会话验证最终空集 head 完整门禁；本阶段没有清理资源或推进 Issue/PR 状态。

主会话同时修改的 README、需求/技术方案与 `vega-issue-486-delivery.md` 已保留，不属于执行者本次文档写入。
