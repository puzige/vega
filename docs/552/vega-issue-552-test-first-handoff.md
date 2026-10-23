# #552 恢复后的先行测试交接

## 1. 当前阶段与精确基线

- 内容版本：`v1.2-test-first-recovery-ready`。基线/最终格式核对记录时间：`2026-10-22T19:30:52Z`；随后完成本交接与清单核验。
- 唯一任务工作树：`vega-552-workspace-motion`；分支：`feat/552-workspace-motion`；HEAD：`6615748f91273bfb8ee424011b3b5a09b3777d2b`。writer 交接时内容尚未提交；精确未提交版本由 §5 清单与交接消息绑定，不等同于 HEAD。父发布前仅将本机绝对路径改为任务工作树名称；源码和测试断言未变。
- 父 Agent 已 fetch＋ff；恢复 writer 开工核对 17 文件初始清单 SHA-256 `14d48972284a84f1fe6936cbbe14edfb47f74f69e3e1fd61d769070d2f88212f` 及四个关键 hash 均匹配。原 writer 已由工具确认 failed 并终止，不存在两个 writer 同时写入。
- **READY 仅表示测试/支持源码与交接已补齐、可供非写入者独审；不是已审、编译通过、有效 RED、GREEN 或可开始生产实现。** 原 16 个 GPUI 用例保留，新增 6 个；源码静态属性数为 22，不是发现/执行/通过数。W1–W12 逐项入口、断言与证据边界见 [spec §5.7](vega-issue-552-workspace-motion.md#57-恢复完成的先行测试映射与证据边界)。
- 本文取代首次 `v1.1-test-first-preflight` 部分交接；该版 HEAD、hash、缺口和 BLOCKED 状态属于历史，不代表当前内容。[恢复 checkpoint](vega-issue-552-recovery-checkpoint.md) 保留原故障/初始完整性证据，并在文末给出当前结果。
- 本 writer 未执行 commit/push/reset/stash/index 修改，未改 Issue/PR/Actions、未嵌套委派、未写其他工作树；暂存区为空。未触碰 #550、#344/#535、#553/#554 或 #549 共享实现。
- 没有实现生产过渡/token/helper、selected 填色、aria_selected、导航/focus/reveal 或业务算法。保留生产即时填色及既有缺陷；共享 helper 不存在的 HEAD 不用缺失导入遮住行为断言。

## 2. 实际产物与等价边界

| 范围 | 当前变化 |
|---|---|
| `crates/vega/src/window/workspace/motion_tests.rs` | 真实 Workspace 根，保留原 16 用例；新增 opacity 阳性对照、无 PTY 终端、Terminal/Diff/文件反复切换与输入/滚动/生命周期、文字/close/focus 几何、无 inactive draw 快恢复、Workspace＋TerminalView＋ConversationStream 业务组合，共 22 个属性 |
| `crates/vega/src/window/workspace.rs` | 仅 cfg(test) 测试模块、真实 tab/body selector、透明文字/body paint 观察及真实关闭按钮内部 paint 探针；普通分支仍用原 body/文字/按钮，无 motion/selected/focus 算法修改 |
| `crates/vega_conversation/src/terminal/test_support.rs` | 默认关闭 feature 下使用原 Session/State/Control、bounded channel、vt100 parser 与 update；driver 在外部进程边界投递 output、消费真实 Input/Resize/Scroll、记录关闭；无 PTY/worker/shell/网络。Diff fixture 生成实际 projection，不复制渲染或导航算法 |
| `crates/vega_conversation/src/terminal.rs` | 仅 feature-gated test_support 模块导出 |
| `crates/vega_ui/src/terminal.rs` | 普通 for_target 仍先调用 start_target，再调用私有 with_session；原 30ms spawn/poll 与字段初始化机械移动到 with_session。测试入口只注入真实无进程 session，不启动 shell。只读快照和成功 line.paint 后的观察默认关闭 |
| `crates/vega_ui/src/test_support.rs` | PaintLog 记录真实叶子 paint 的 bounds/clip/style；同 paint 边界的测试专用微小 quad 从公开 painted_quads 读取框架实际乘过的 opacity，不把 style alpha 当继承 opacity |
| `crates/vega_ui/src/conversation_stream/{core.rs,mod.rs,selection_observer.rs}` | 默认关闭的只读正文/viewport 观察及 SelectionDocumentBuilder 转发模块；委托原 selection.rs 的构造、append、wrap、selection 与 paint，原 selection.rs 未改、renderer 未复制 |
| `crates/vega_ui/src/{file_preview.rs,diff_view/state.rs,diff_view/render.rs,lib.rs}` | 默认关闭的实际 scroll/文字 paint 观察、真实 Retry selector 与 test_support 导出；真实 handler 和控件保留 |
| 三个 `Cargo.toml` 与 `Cargo.lock` | Vega dev-dependencies 显式启用既有 test-support；UI/Conversation feature 默认关闭；仅闭合现有测试 feature 的 10 个间接包，所有原 package 的 name/version/source/checksum 保留，无第三方升级 |
| 三个 `docs/552/*.md` | 当前 W1–W12 映射、恢复历史、精确交接与阶段门禁 |

Terminal constructor 的生产调用顺序和原 poll/初始化已对照 HEAD 静态审查为机械等价；这不是执行验证，非写入者仍须独审。normal build 的新增 API 均受 feature/cfg 门控；没有以公开常规 API 绕过 crate 边界。

### 观察与夹具的证据边界

- `WorkspaceRoot` 委托生产 `render_workspace`，不调用会启动 updater 的完整 `VegaWindow::new`。生产 Sidebar 仍创建原有内部服务/GPUI timer，使用隔离空 store 的错误边界；未验证完整应用启动、真实 Git/PTY/模型/网络或显示器 present。
- 文件/Diff/Terminal 的 scroll 对照经真实 wheel/action 与内容可见范围变化建立，不只读 offset；输入隔离经真实 Enter/Paste/Diff action 验证；实体与 weak handle 检查保留生命周期。Diff snapshot/projection、terminal output 与 stream event 属于外部业务输入，未替代生产 UI handler。
- W6 原键盘 Retry 断言保留，并增加实际 loading/error/空态正文、真实 Retry click 与恢复。W7 新 fixture 必须先点击实际 Diff 文件行展开；三次真实 NextDiffHunk 提供滚动阳性对照，不假设第二 hunk 必然离屏。reorder 只输入生产投影的新顺序，不声称有拖拽 handler。
- 锁定 GPUI 的 text_style color 不含祖先 opacity，element_opacity 是 crate-private。PaintLog 在真实 paint 边界增加 1px、alpha=`1/65536` 的 quad，并读取实际 scene alpha；缺失或不唯一探针失败，不猜测 1。该探针不是零光栅写入，也不是 glyph/SVG 像素捕获。父 opacity 1→0.25→1、style alpha 仍为 1、隐藏正文无记录的对照已写，待 Actions 执行。close 探针在实际按钮内部，避免错过其内部 opacity。
- W8 文字颜色、close bounds/opacity、真实 focus border quads 与 hit bounds 的 0/30/60/120ms 断言已写；不是截图、显示器对比度或用户外观已通过的证据。
- 无 inactive draw 用例先记录真实 RenderCount，在 deactivate→推进 1ms→activate 后、显式 frame 前要求 render 未发生；不能以主动 inactive draw 替代该场景。
- W11 使用真实 ConversationStream::apply_event、生产 TerminalView poll/input/paint 和 Workspace handler，保留 draft/回焦/阅读锚点；complete/reduce/hide/inactive 后继续消费业务输出。恢复后要求终端实际正文，ResumeTail 后要求最终对话正文实际 paint；未采用 toy stream 或 snapshot 字段冒充绘制。

## 3. RED 与调度归因：不能绕过的门禁

1. **没有有效 RED。** 本地未编译、未执行，阳性对照未证实；独立审查也未完成。任何编译/依赖/API/runner/夹具/观察失败先归正确责任层，不能称为产品 RED，不能据此开始生产实现。
2. **SelectionSamples 尚无生产填色 sampler。** 每帧对 pane/key 清为 None，只允许未来实际填色 sampler 把真实 MotionStatus 传入 observe_selection_sample；观察器不合成目标、不调用旁路 sampler、不驱动帧。测试要求 Running 阳性与 Idle/Finished 停止，None 明确失败。缺少采样观察本身不能单独证明 motion 产品 RED；必须结合真实 quads 行为、阳性对照和独审归因，不能先合成记录求通过。
3. **W10/W11 不要求全应用零帧。** 静态窗口保留 simulate_next_frame 有帧/停止对照；并发 Terminal/stream 的业务定时器合法继续。组合用例按 pane/key 的真实填色状态分层；隐藏 pane 没有采样。锁定 Kit transition_with_status 仅在 Delayed/Running 请求帧的源码证据不等同于本补丁的运行证据。
4. **W12 是独立的既有语义缺陷。** Kit 原生 observer 支持 selected；此 HEAD 的生产 tab 未设 aria_selected。测试保留 Some(false)/Some(true)，None 不能算 pass；该问题不冒充背景 motion/性能 RED，父 Agent 独立归因与界定最小范围。
5. **共享与版本边界不变。** #549 由唯一 writer 提供未来 Motion::TAB_SELECTION_MS 与 decorative_transition(duration, visible, window, cx) -> Transition；本包不实现 fallback 引擎、不写共享文件。生产实现前由父 Agent 同步最新接口/增量，保留 W1–W12 和 120ms/default ease_out_cubic 冻结断言。

## 4. 实际本地验证与未取得项

本地只做低 CPU 静态检查。**未运行** Vega、编译/build/package、cargo check、Clippy、test/nextest、bench/profile 或 CUA。cargo metadata 仅离线解析依赖，不编译或执行 Vega。

| 当前内容检查 | 实际结果 |
|---|---|
| 13 个 Rust 文件 rustfmt `--edition 2024 --check --config skip_children=true`（命令见下，已在最后一次 source 编辑后执行） | exit 0 |
| `git diff --check` | exit 0 |
| `cargo metadata --offline --locked --format-version 1`，管道启用 pipefail 后 JSON 解析 | exit 0；workspace_members=13 |
| Python 静态审计：20 路径白名单、暂存为空、原 lock identity/checksum 保留＋新增 10 包、默认关闭 feature、Vega 仅 dev 启用、原 16 名称与 22 属性、无新增注释/ignore/should_panic/外部进程 fixture、W1–W12 在 spec 中 | exit 0；属性数不是测试运行数 |
| 清单/文档最终核对 | 见 §5 及最终交接消息；只核对当前字节、链接/范围与 Git 状态，不代表编译/测试 |

```sh
rustfmt --edition 2024 --check --config skip_children=true \
  crates/vega/src/window/workspace.rs \
  crates/vega/src/window/workspace/motion_tests.rs \
  crates/vega_conversation/src/terminal.rs \
  crates/vega_conversation/src/terminal/test_support.rs \
  crates/vega_ui/src/lib.rs \
  crates/vega_ui/src/test_support.rs \
  crates/vega_ui/src/terminal.rs \
  crates/vega_ui/src/file_preview.rs \
  crates/vega_ui/src/diff_view/state.rs \
  crates/vega_ui/src/diff_view/render.rs \
  crates/vega_ui/src/conversation_stream/core.rs \
  crates/vega_ui/src/conversation_stream/mod.rs \
  crates/vega_ui/src/conversation_stream/selection_observer.rs
```

历史失败不改写为成功：首次部分包 locked metadata exit 101（测试 feature lock 闭包未齐）后离线补齐，旧 rustfmt exit 1 仅格式差异，均在生产实现和首次云端执行前纠正；恢复阶段格式预检也曾 exit 1，格式化后当前 exit 0。原 writer 的工具执行 failed 无编译/测试证据，属 Agent/环境故障；历史 #553 只读 hash 保护 exit 1 属外部树变化，本 writer 未写/恢复该树。以上都不是产品 RED。

| 云端/交付项目 | 状态 |
|---|---|
| 独立审查 | 待父 Agent 组织非写入者审查本停写内容 |
| Actions run/job | 未触发；无 run/job |
| 自动发现/执行/通过/失败/忽略/跳过计数 | 全部未取得 |
| 阳性对照通过、有效 RED、同一冻结测试 GREEN | 未取得 |
| 生产实现、独立删测、最终空集 gate、合并、用户手测 | 未进入 |

## 5. 精确内容标识

当前共有 **20 个** changed/untracked 文件，13 tracked modified＋7 untracked。下列清单只排除本交接文件自身，其余 **19 个** 全部纳入，不漏 checkpoint/spec 或新增支持。清单定义：按相对路径字典序排序，每行 `SHA256`＋两个 ASCII 空格＋`relative_path`＋LF，UTF-8；不包含代码围栏和说明行。该清单字节的 SHA-256：

`54b121bd5b775161a4783a3435d7fa0dcfc8da54a5829c2154e981fe8baa2557`

```text
d7c701e424bae4802ca725b937899e3875d0d0293b7d3be5558ff707849ab888  Cargo.lock
81a5a6fefe3dd036e4e8660deb84b87477c380a00cebbb96ba9f929d385b179b  crates/vega/Cargo.toml
717bdaa7e80a9ccaee1f63cb07b1e1afe17cbccfe8f5420a5808740eaa5e2779  crates/vega/src/window/workspace.rs
ac11a1d03e441b2ec090886d7ae25f096760d7663fe3296a5f136580f4e47298  crates/vega/src/window/workspace/motion_tests.rs
981e0ffcf73ce223ae1ec65ecc63d74ef1d3fda80b249cb75ae94d306491a55d  crates/vega_conversation/Cargo.toml
447ca657f223fa8c4efa76560d97d88268553e76b3d2724080b486ebabef3249  crates/vega_conversation/src/terminal.rs
683c56c492a9ece62c71d68f033ccc0a7af383997dd0a3f2848ba4ad813b9539  crates/vega_conversation/src/terminal/test_support.rs
25d5a4fb1f0d41abc69f6aed1b32eb848bb15faceefc857b9d20612a8d941545  crates/vega_ui/Cargo.toml
3c1a1f221ee21d0bbe0707b63671260036967b2fb7fd9ee3deda10a6b264ace8  crates/vega_ui/src/conversation_stream/core.rs
399ea06835a6071a650b1987e525a02628e1f60148af873b41fd41f0d91b8171  crates/vega_ui/src/conversation_stream/mod.rs
626b9618d3144d0b20647d87fef7b47656202e2eb029ef0f5560b04eb15fe956  crates/vega_ui/src/conversation_stream/selection_observer.rs
9a56b9eab8877f8ce981cb1db6919d09861cb171c4f3b686e53e505000ddc2a5  crates/vega_ui/src/diff_view/render.rs
f83d938295b6498f1fdf118c7e4b015a0f0cb0b585a028ade94cbdfe55acd23d  crates/vega_ui/src/diff_view/state.rs
a6efb07684d73cc2bfe8e3b8b4a5ff4365eb293162aab02203bd9e8fe0be4c49  crates/vega_ui/src/file_preview.rs
67a3746e271077bffcb63d3323b3c41185ad3532345244800409c613b0398c35  crates/vega_ui/src/lib.rs
1dabaf6b74b0edb515fcf9dc8f1a845f6a61ac81053b5d6db7e510ba5fc2818a  crates/vega_ui/src/terminal.rs
8921fdd36c29f6bd24339f2ffff36b3e1f3e991d33d71de5d03b7673fa35422e  crates/vega_ui/src/test_support.rs
cee1160542f67ed379724c9924135cb1ef426b0ebf0985702fc4bc4cb0f2d396  docs/552/vega-issue-552-recovery-checkpoint.md
6c2f0715c6dbf475c9575e3baed5b5c30590364cf0ed03ae4eb529655acffd1e  docs/552/vega-issue-552-workspace-motion.md
```

本交接自身 SHA-256 与**包含本交接的 20 文件整包清单 SHA-256**由最终交接消息给出，避免自引用。tracked diff 不能包含未跟踪源码，独审必须连同此清单所有文件核对，而不只看 git diff。

## 6. 下一责任链与停写

恢复 writer 在最终 READY 消息后**停止全部写入**；没有本任务启动的编译/测试或外部进程在后台运行。父 Agent 负责 Git、独审、草稿 PR 和外部状态；本包不自动触发 Actions。

父 Agent 下一步：非写入者独审这个精确版本的完整 W1–W12、默认 feature 边界、constructor 等价性及观察阳性对照，再用既有 `.github/workflows/pr-check.yml`。保留 fmt、全 workspace/all-targets Clippy、nextest list JSON 与正常非空 workspace run、现有 Python 门禁；不得新建 CI 或为失败删断言/ignore。只有真正空集才能使用 `--no-tests=pass`，不是当前 22 属性源码包的替代执行方式。

阳性对照与责任层核清、有效 RED 经独审确认后，才可进入最小生产实现；同一冻结测试 GREEN 后固定可访问源码/完整 SHA/run/job/计数，随后独立删除 Rust 测试及独占 test-support 并执行最终空集完整 gate。当前均未进入，不因 READY、缺失 sampler 或编译失败跳过门槛。
