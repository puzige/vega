# Issue #296 · 长过程收起入口吸顶交付记录

对应 [S01–S06 规格](vega-issue-296-sticky-disclosure.md)。2026-10-13，本地基线为 `310340ffdeb12435c79ac4eed4f0d4845a710866`，沿用 `feat/296-stream-polish` worktree；旧 PR #318 已合并。记录只含合成测试内容，不附用户截图、真实会话或本机绝对路径。

## 实现

Final 本轮过程和 live stage 复用局部 `StickyDisclosure` 元素。原标题仍只有一个实体与一个布局占位；元素按本帧标题和连续操作区的 layout bounds 移动其绘制及命中位置。标题到达所属区域底部时推出，Artifact 原位打断吸顶，后续操作区可以恢复同一阶段入口。clock 分隔线仍在原文流。

标题通过 priority 1 deferred 绘制，并在 prepaint、paint 两阶段显式应用 viewport 与当前操作区的交集。使用主题页面底色，鼠标点击阻挡穿透，滚轮仍能传给现有滚动链。Thinking、Tool、ToolGroup 的独立开关和详情布局保持现状。

仅在用户通过已吸顶标题收起时，按当前 run/stage 找到稳定 entry identity，再解析当前索引返回收起入口；保持非追尾状态。普通开关、内容更新与常规滚动不额外定位。

代码涉及 `conversation_stream/sticky_disclosure.rs`、`run_activity.rs`、`content.rs`、模块注册及 `tests/issue296_sticky_disclosure.rs`。不修改后端、持久化、详情顺序、工具安全投影或全局列表实现。

## 验收证据

| 规格 | 实际证据 |
|---|---|
| S01 | Final/live 的 320px 明细内部真实 wheel 到中部、底部，标题 bounds 与外层 offset 不变；再向下滚动传给外层。普通收起标题和吸顶展开标题中心也发送真实 wheel，均不形成滚动死区。 |
| S02 | 两个生产入口先失败后通过：标题原本分别在 viewport 上方 84px、100px，修复后与 viewport 顶边相同。实际点击收起后入口可见、非追尾；下方所有工具仍未展开。收起状态和无详情标题不吸顶。 |
| S03 | 所属区域仅剩 10px 时标题被推出到顶边上方 15px；clip 外点击不收起。viewport 顶落在 Artifact 中、后续操作区已可见时，不出现旧标题或残留点击区；后续操作区恢复吸顶且收起后 Artifact 保留。Final 离开其区域后消失，第二轮独立接替且不改变第一轮展开状态。 |
| S04 | 浅深主题、480/1200px 窗口验证 bounds 裁剪、不透明页面底色、展开箭头离开鼠标后仍绘制。真实焦点导航后 Enter/Space 均能收起，原 focus handle 不变；同一个标题负责绘制与交互。 |
| S05 | 吸顶收起后入口可见且非追尾；增量正文和历史前插后，stage 的稳定 identity、手动展开及自然尺寸保留，收起定位到前插后的正确索引。 |
| S06 | 普通/吸顶的无缓存 entry 估高完全一致，stage 自然尺寸一致。受影响的 #296、#151、Thinking、#146/#147、#72/#98 定向回归覆盖原文选择/复制、工具定位、失败与阅读保护；不调整旧测试断言。 |

## 原始日志与命令

日志目录：`~/Workspace/artifacts/vega-296-sticky-disclosure/`。以下退出码均来自实际命令返回，未为补取退出码重复执行。

S02 先行红测及第一次绿测使用：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_sticky_s02_/)'
```

- `red-both.log`，退出码 **100**：`Summary [   0.087s] 2 tests run: 0 passed, 2 failed, 703 skipped`。Final/live 都在实际 header bounds 断言失败。
- `first.log`，退出码 **0**：`Summary [   0.096s] 2 tests run: 2 passed, 703 skipped`。
- 较早的合并 fixture 探索日志 `red.log` 也保留，退出码 **100**：`Summary [   0.079s] 1 test run: 0 passed, 1 failed, 703 skipped`。该 fixture 在 live 首次失败即退出，随后拆为上述两项，确保 Final 也取得独立红证。

扩展矩阵及最终增量命令：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_sticky_/)'
```

- `matrix-first.log`，退出码 **101**，未运行测试：新 fixture 直接比较没有 `PartialEq` 的 `ListOffset`，且调用 `window.focused` 漏传 context。改为分别比较 index/offset，并传实际 context。
- `matrix-second.log`，退出码 **100**：`Summary [   3.133s] 7 tests run: 6 passed, 1 failed, 703 skipped`。第二轮 fixture 缺少第一轮 `MessageFinished`，所以生产入口正确拒绝另一个 `MessageStarted`，查不到第二轮。补齐合成事件生命周期；不改运行门禁。
- `final-sticky.log`，退出码 **0**：`Summary [   3.120s] 9 tests run: 9 passed, 703 skipped`。包含审查发现的标题滚轮修复，以及最终点击不穿透、clip、键盘、前插和几何断言。

受影响回归命令：

```sh
cargo nextest run -p vega_ui -E 'test(/conversation_stream::tests::issue296_/) | test(/issue151_latest_activity::/) | test(/thinking::/) | test(/issue147_/) | test(/issue146_/) | test(/issue72_/) | test(/issue98_/)'
```

`regression.log`，退出码 **0**：`Summary [   4.011s] 148 tests run: 148 passed, 563 skipped`。

这 148 项通过后，独立审查发现标题 wrapper 的 `occlude()` 同时阻挡滚轮。此前内部滚轮用例只操作详情中心，没有覆盖标题中心。新增生产事件回归并保留失败证据：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_sticky_s01_header_wheel/)'
```

`review-wheel-red.log`，退出码 **100**：`Summary [   0.058s] 1 test run: 0 passed, 1 failed, 711 skipped`。普通收起标题中心滚动后，外层 index/offset 未变化。两个标题 wrapper 最小改用 `block_mouse_except_scroll()`，最终仅重跑上述 9 项 sticky 集合并全绿；**148 项属于这次滚轮修复之前的回归结果，不声称最终代码重新整批跑过 148 项**。

最终静态检查：

```sh
cargo fmt -p vega_ui -- --check
git diff --check
```

两条退出码均为 **0**。没有本地全量测试或 Clippy。Nextest 构建只出现既有依赖 `block v0.1.6` 的 future-incompatibility warning，没有新增源码 warning。

### 日志 SHA-256

| 日志 | SHA-256 |
|---|---|
| red.log | `1f61efe46819c7af56e38721d65c6a5a9bae75f6f1afba0989cffe98749b0385` |
| red-both.log | `2d6d1a9df9eaa98c6ca9df7661a195333549c306e7e402e410dff311f975d551` |
| first.log | `b1fb2e0bc88f50cc27da53df15fb9f95a4d942430531c4e04c894a31aa3d9301` |
| matrix-first.log | `1483511641a463a8e420abab6da48c3e70fea7d61dddd1147d681eb9f67482ae` |
| matrix-second.log | `a0d32e8d7fa86158d0d5dd51348c2191d8eca1969c2b066d8306bbca2034585e` |
| regression.log | `796165f802f50b399c2923484a7d83cdc2b499daade608489f49a925fb77a9d5` |
| review-wheel-red.log | `505a31616b375b23d689ec6a194f61eff787608209223f30b4f4fdebd18dbcdd` |
| final-sticky.log | `8f42438a96d4a94b097f854870b8d62472a8dafd30d4a2497aeeed5f02f126e2` |

## 审查与待手测

独立只读审查已核对单一 header/focus、两阶段 mask、底界 push-out、Artifact 间隙、前插后身份定位与最终滚轮修复，无剩余阻塞项。生产 toggle 只由标题鼠标或键盘入口触发，未发现离屏旧 pinned 状态引起程序性收起定位的调用路径。

M01 原生像素、触控板惯性和连续拖动时的闪烁仍需用户在新版实机手测；本轮没有启动、安装或操作 Vega。建议在长 Final 和 live 过程内、外分别滚动，在标题吸顶后点击/键盘收起，再检查产物前后以及浅深主题和窗口缩放。GPUI 测试证明逻辑几何与事件行为，不等价于原生观感验收。全量 CI 和合并由主 agent 接管。

## 更新基线后的验证

集成时 `origin/master` 前进至 `a47abb004026a19802c1a98893189167e275fea1`（PR #320），主 agent 已 fetch 并成功 rebase。验证提交为 `7a117d2c871888d56922b72a8e51a3220dbe694f`；sticky 提交内容在 rebase 前后无差异。此次只追加本交付记录，没有生产或测试补丁。

新基线在会话中加入独立截图 entry 与 CUA notice，涉及同一列表的外显内容和权限/接管入口。因此复跑原定向筛选，并加入现有 `cua_usability` 四项进程内测试。独立审查确认：截图仍在 run/stage 之外，notice 作为列表外的布局 sibling 自动缩小 viewport，未改变吸顶区域或稳定身份。

```sh
cargo nextest run -p vega_ui -E 'test(/conversation_stream::tests::issue296_/) | test(/issue151_latest_activity::/) | test(/thinking::/) | test(/issue147_/) | test(/issue146_/) | test(/issue72_/) | test(/issue98_/) | test(/cua_usability::/)'
```

原始日志 `integration.log`，退出码 **0**：

```text
Summary [   4.329s] 153 tests run: 153 passed, 563 skipped
```

153 项由原筛选的最终 149 项（包括新增标题滚轮与完整 9 项 sticky）及 4 项 CUA 组成。四项 CUA 覆盖截图外显/重播、actionable notice、新任务清理、接管恢复、权限及浏览器批准入口。未访问真实外部 App。仅出现既有 `block v0.1.6` future-incompatibility warning。

`integration.log` SHA-256：`66fdb6d813209cdf7eb580edb1b01a9a952c3f0d4d29db30d5dee33dffd821de`。旧红绿日志均保留，没有覆盖。新基线再次执行 `cargo fmt -p vega_ui -- --check`、`git diff --check`，两条退出码均为 **0**。

### Freeze 与环境

- 代码冻结取证时间：**2026-10-13 01:13:42 UTC** / **2026-10-13 09:13:42 Asia/Shanghai（UTC+08:00）**。
- `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`。
- `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`。
- `git version 2.55.0`。
- OS/架构：macOS **15.8.1**，`Darwin arm64`。

最终代码文件 fingerprint 包含本次变更的全部六个 Rust 文件，包括两个新源文件。按下面仓库相对路径字典序，对每个文件执行 `shasum -a 256`，将其标准 `SHA256`、两个空格、相对路径、LF 行结果保存为 `integration-source-fingerprint.txt`，再对该 UTF-8 清单计算 SHA-256。文档不参与代码 fingerprint，后续主 agent amend 交付记录不会改变它。

| 相对路径 | 文件 SHA-256 |
|---|---|
| crates/vega_ui/src/conversation_stream/content.rs | `2d538fd6a29c89d3756e6c83c8cff3db9c20ecb21e9b43e612f5528adae3490f` |
| crates/vega_ui/src/conversation_stream/mod.rs | `d1b930ec3df1f3459609581f328049f3e6692563e2bee49f482b5246b1c0a56b` |
| crates/vega_ui/src/conversation_stream/run_activity.rs | `63166442a78f4ed52439ab1f77fe41be7f7b5fe5a1fe7fa4df2788531b4f2f22` |
| crates/vega_ui/src/conversation_stream/sticky_disclosure.rs | `ad672030f476a2bc48d5bdace744c1ecfdcb07115c757dbf8aad63010278fed5` |
| crates/vega_ui/src/conversation_stream/tests/issue296_sticky_disclosure.rs | `69a3134d31df34083f7e2a5f447ff58d26a60b4d5827df9f1edc9982eb1cc662` |
| crates/vega_ui/src/conversation_stream/tests/mod.rs | `7ca64a81dc1cdd149e8e7209fb46cf8fee3b55af0a006e2d7d5b113d000156d0` |

最终清单 SHA-256：`a5f64de6fd3d3245482275620d2f00fa6071895ee39027e4e6befb1e5fdad254`。清单与日志均位于前述 artifact 目录。实现 agent 已停止代码写入；提交 amend、CI 与集成由主 agent 接管，M01 仍待用户手测。

## PR #321 首轮 CI 与测试帧适配

首轮发布提交 `5e7fc1330190e6ba2c7d2c5fcc1def27aa205122` 的云端 run `38331280870` 中，Clippy job 通过，Nextest job `115081336565` 失败。原始 `ci-first-nextest.log` 的退出码为 **100**，footer 为 `Summary [ 128.221s] 2361 tests run: 2360 passed, 1 failed, 5 skipped`。唯一失败是 `ten_k_mixed_items_trunk_e2e` 的 `attempted to dereference an ArenaRef after its Arena was cleared`。该云端失败保留，不能用前述定向绿测代替全量 CI 结论。

修改前再次 `git fetch origin`、`git rebase origin/master`，两条退出码均为 **0**，基线没有漂移。按以下命令本地单例复现：

```sh
RUST_BACKTRACE=1 cargo nextest run -p vega_ui -E 'test(/ten_k_mixed_items_trunk_e2e/)'
```

`ci-repro-red.log`，退出码 **100**：`Summary [   1.171s] 1 test run: 0 passed, 1 failed, 715 skipped`。调用栈为 `AnyElement::prepaint → Window::prepaint_deferred_draws → Window::draw → App::flush_effects`，来源是旧 fixture 的 `VisualTestContext::draw`。

锁定 GPUI 的 `app/test_context.rs:893–920` 只对子树执行 layout/prepaint/paint，随后清理 arena，并未完成 deferred 队列。sticky 的正常 deferred header 因而在下一次真实窗口帧中已过期。生产 `Window::draw_roots` 在清理前执行 deferred prepaint/paint，`Window::draw` 完成 finish/swap/clear；独立审查与本地调用栈一致，没有生产元素生命周期缺陷证据。

按新增的 S06 测试帧适配规格，仅修改 `tests/e2e_variable_height.rs` 的 draw helper：真实测试窗口 resize 为 1200×800，每帧严格检查 viewport 尺寸，并从 `VisualTestContext::update` 调用 `window.refresh()` 和完整 `window.draw(cx).clear(cx)`。保留全部 10,000 条混合消息、正文保真、贴底/脱离、前插像素锚点、回底、稳定身份与冻结区 rematerialization 断言；没有忽略、缩减 fixture、清空 pending 队列或修改生产代码。

最终只跑该用例与 9 项 sticky：

```sh
cargo nextest run -p vega_ui -E 'test(/ten_k_mixed_items_trunk_e2e/) | test(/issue296_sticky_/)'
```

`ci-fix-green.log`，退出码 **0**：`Summary [   3.017s] 10 tests run: 10 passed, 706 skipped`。10k 用例自身通过耗时 1.138s。只出现既有 `block v0.1.6` future-incompatibility warning。`cargo fmt -p vega_ui -- --check` 和 `git diff --check` 退出码均为 **0**；没有额外本地全量或 Clippy。

| 证据 | SHA-256 |
|---|---|
| ci-first-nextest.log | `9599d58886476692fadb1aa09c46a170da1408dc9a1c39fd73ae8c05214c0e97` |
| ci-repro-red.log | `b7b9fde1e870aeb34c3bdd61be7d43adaafeff38d92231fd492e8f3c6d320a9c` |
| ci-fix-green.log | `3ef3f26f10c33ad98e61bb5e02383738f4779287b33ddfb1ef24cd0a64ba8534` |
| tests/e2e_variable_height.rs | `eb1a2322017f5ccfb59303e30b811fe0e2d0d2818c6f60427227e6ddca379284` |

修复后的代码冻结取证时间为 **2026-10-13 01:27:02 UTC / 09:27:02 Asia/Shanghai**，环境版本与前一节相同。新的 `ci-fix-source-fingerprint.txt` 沿用相同字典序清单格式，包含此前六个 Rust 文件及本次 `tests/e2e_variable_height.rs`，共七个文件；清单 SHA-256 为 `9bf7009bfb04dc002b6a132f614550ece89c454f26b3a9e5bc37b8bda61aecb6`。此前六个文件内容与上一冻结清单完全相同。实现 agent 不提交或推送，主 agent 接管第二提交与下一轮 CI；M01 仍待用户手测。
