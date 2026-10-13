# Issue #296 · 统一图片右侧预览交付

对应 [冻结规格](vega-issue-296-image-preview.md)。实现基线 `bb6997e2857bef7833b09f41a44af3f602d0a742`；已基于 master `6a668f5` 完成 rebase，结果见末节。实现者初次交接时未提交；主控提交及集成记录见下。本轮没有安装、启动 Vega 或调用真实外部应用。

## 实现与验收

统一的图片 entity 承接 Composer、UserImages 与 Screenshot 的实际点击/Enter/Space；App 通过可信 occurrence 事件打开唯一右侧 Workspace 页。草稿 ACK 建立新 occurrence，移除、history rebuild、route/project 变化退役旧页。行内截图保存与顶栏共用 busy，原始 bytes 和 create_new 保存规则保留。

macOS 后台 NSWorkspace 查询带 selector 可用性检查，使用真实返回的 bundle 路径组成固定 `/usr/bin/open` argv。会话级缓存覆盖所有窗口，0700 目录/0600 文件、32份/128MiB（含 reservation）、原字节复用校验、实际 spawn 后 pin、退出等待物化及实际清理。系统枚举/启动器测试全部使用进程内替身。

| 矩阵 | 已执行证据 |
|---|---|
| IP01 | 真 VegaWindow 截图点击、draft/history 相同bytes不同entity、缩略图键盘；共享事件旧截图测试；pane→focus_next→Enter打开真实菜单→Enter选择→直接Escape关闭 |
| IP02 | history 同key重建、删除A但同bytes B仍在；增强原 #63 App MockProvider→持久化→ACK 用例确认旧pane关闭；运行task A→B→A保留同stream，迟到inline picker零写入 |
| IP03 | 非模态Composer操作、外部滚轮；dock移动后重新右开、780→1403 resize恢复；宽窗原bottom-max临时普通分栏且关图恢复；有效source回焦与离屏source回Composer |
| IP04 | Light/Dark × landscape/portrait/strip/column/tiny，40高顶栏/28控件/间距边界/长名操作可达/图像位于padding内且不超过原尺寸；prepared前loading、真实异步decode后错误fallback |
| IP05 | 行内无需pane的picker，pane真实键盘保存、跨入口busy、取消、两种原bytes、拒绝覆盖、同卡提示/高度稳定、关图后旧picker零写入 |
| IP06 | 精确一次argv、真实controller默认项/鼠标及键盘选择/外部dismiss、空列表/失败仍可保存；超时pin、外部改写后新版本、32份/128MiB、正常退出；barrier覆盖reservation退出与spawn失败取消竞争；window cleanup取消 |
| IP07 | 原UI定向12项通过；追加 #63 UI occurrence退役与App ACK、旧 #75 loading/键盘目标均通过。原模态规则仅按spec迁移，thumbnail几何/字节/拒收/数量安全断言保留。集成后的Workspace/选择/锚点组合回归见末节39/39。 |
| M01 | **NOT RUN**：真实macOS关联、外部应用读取、原生picker外观及像素观感仍需用户手测。 |

阅读定位使用真实宽度改变的现有 GPUI List Proportional 合同：同identity、non-tail、offset按同一item实测高度比例恢复；定高截图打开仍严格保持绝对offset。未改通用List。菜单异步到达只在“打开方式”仍实际持焦时接焦首项，不抢已离开的焦点。

## 命令与原始结果

以下均在本卡 worktree 执行；日志位于 `~/Workspace/artifacts/vega-296-image-preview/`。每次 stdout/stderr 合并保留，不覆盖首失败。表中编号对应完整命令。

**A**

```sh
cargo nextest run -p vega -E 'test(issue296_image_preview_screenshot_opens_right_workspace)'
```

**B**

```sh
cargo nextest run -p vega_ui -E 'test(issue75_screenshot) | test(issue75_usability_screenshot) | test(issue63_) | test(issue78_image)'
```

**C**

```sh
cargo nextest run -p vega -E 'test(issue296_image_preview)'
```

**D**

```sh
cargo nextest run -p vega -p vega_ui -E 'test(issue296_image_preview) | test(issue75_screenshot_loading) | test(issue63_standalone_first_image_submit)'
```

**E**

```sh
cargo nextest run -p vega -p vega_ui -E 'test(issue296_image_preview_saves_exact) | test(issue296_image_preview_reading_anchor)'
```

**F**

```sh
cargo nextest run -p vega -E 'test(issue296_image_preview_reading_anchor)'
```

**G**

```sh
cargo nextest run -p vega -p vega_ui -E 'test(issue296_image_preview)'
```

**H**

```sh
cargo nextest run -p vega -E 'test(issue296_image_preview_open_with_real_controller)'
```

**I**

```sh
cargo nextest run -p vega -p vega_ui -E 'test(issue296_image_preview_open_with_real_controller) | test(issue296_image_preview_geometry) | test(issue63_paste_image_only)'
```

**J**

```sh
cargo check -p vega --bin vega
```

**K**

```sh
cargo fmt -p vega -p vega_ui -p vega_theme -- --check
```

**L**

```sh
git diff --check
```

| 原日志 | 命令 | Exit | 原始 footer / 编译结果 | SHA-256 |
|---|---|---:|---|---|
| red.log | A | 100 | Summary [   0.046s] 1 test run: 0 passed, 1 failed, 256 skipped | `79987503af8fe55964298db34e7bff72fa95d916659dda7c321cb2682846f4a9` |
| red-business.log | A | 100 | Summary [   0.159s] 1 test run: 0 passed, 1 failed, 256 skipped | `985f1a42f6f9742f8072cb8583df8068bf1f4a74bcdde7a47670fe65a9e654be` |
| host-first.log | A | 101 | error: command `/opt/homebrew/bin/cargo '--color=auto' test --no-run --message-format json-render-diagnostics --package vega` exited with code 101 | `944a82f77d7f14127daca3ac3960a74c4cf2bb3900bd9fff461d981bdfef3f73` |
| host-second.log | A | 101 | error: command `/opt/homebrew/bin/cargo '--color=auto' test --no-run --message-format json-render-diagnostics --package vega` exited with code 101 | `d21b8adbe0e1af427b6b21c9f2dc81ec62ad630be914122b30105d96effca449` |
| host-third.log | A | 0 | Summary [   0.167s] 1 test run: 1 passed, 256 skipped | `f1d2185403ac1a005a3f8a5a5acfafc40064f50cfde772462c127bd48bf6aa09` |
| host-fourth.log | A | 0 | Summary [   0.166s] 1 test run: 1 passed, 256 skipped | `e51e3ffc413068f353c02d6ed25fbcad01196b70aab3b23bb9faf4ef386be2c9` |
| native-first.log | A | 0 | Summary [   0.183s] 1 test run: 1 passed, 256 skipped | `9c9280082065242d1e686f98b1c4bfe130cc26b8c2c3e40cd1d423240d27f422` |
| ui-first.log | B | 101 | error: command `/opt/homebrew/bin/cargo '--color=auto' test --no-run --message-format json-render-diagnostics --package vega_ui` exited with code 101 | `1ae8a50ebdbfa54c3629674d7246108484a01bf1903e7934ad4d7ead8236ae06` |
| ui-second.log | B | 0 | Summary [   0.923s] 12 tests run: 12 passed, 718 skipped | `d874fea4b4f4b1c77a006ce22ce3a7cfd5001ed8c5bf83dcd0a3684286ffd8c6` |
| entries-first.log | C | 101 | error: command `/opt/homebrew/bin/cargo '--color=auto' test --no-run --message-format json-render-diagnostics --package vega` exited with code 101 | `73e61b74b0dd8f097ec8251f0e3d275bdb5b277ff00349114daea71838005f38` |
| entries-second.log | C | 101 | error: command `/opt/homebrew/bin/cargo '--color=auto' test --no-run --message-format json-render-diagnostics --package vega` exited with code 101 | `cafeedf4346214860b32c0969cbcc70d0fe0ae2cf19356b8f74fddd51234e736` |
| entries-third.log | C | 101 | error: command `/opt/homebrew/bin/cargo '--color=auto' test --no-run --message-format json-render-diagnostics --package vega` exited with code 101 | `ce2571ecb684b641649e7d3bd2927fd221c3dce4ab82f207ae307719080ee0b4` |
| entries-fourth.log | C | 100 | Summary [   0.548s] 5 tests run: 3 passed, 2 failed, 256 skipped | `8388508a9bcceb5584047d447d56acb64503c356ce2e8c8d50d2123a5d3a3841` |
| entries-fifth.log | C | 0 | Summary [   0.656s] 8 tests run: 8 passed, 256 skipped | `a16818aba9d5df7cbcecfd5c71fbe7ba5a82c0cc828366d9c0ec5f0ac3151cfa` |
| matrix-first.log | D | 101 | error: command `/opt/homebrew/bin/cargo '--color=auto' test --no-run --message-format json-render-diagnostics --package vega --package vega_ui` exited with code 101 | `d6c6e2e705b820a488f5ddb4e9690d43653d5757e48713a151fef38c3d039b2a` |
| matrix-second.log | D | 101 | error: command `/opt/homebrew/bin/cargo '--color=auto' test --no-run --message-format json-render-diagnostics --package vega --package vega_ui` exited with code 101 | `14c73974977591d1c4e51e3fc564419da02fa0e82f96a4050b0e4a506810ecbf` |
| matrix-third.log | D | 100 | Summary [   0.933s] 14 tests run: 13 passed, 1 failed, 984 skipped | `6a6b87c381a6e14f3a454d15bee32163a3e2b00a9ec3ab0826cf61a941ae8ae7` |
| matrix-fourth.log | D | 100 | Summary [   1.004s] 16 tests run: 14 passed, 2 failed, 984 skipped | `75fcdcd0edccd99f3d24b5211b3b5046b6ec0b7a6596551257e1a9397fb6524a` |
| fixture-final.log | E | 100 | Summary [   0.578s] 2 tests run: 1 passed, 1 failed, 998 skipped | `f0e5e7ad8af220b856a28598e29df5325944155bcbe1997ba3942351459a9935` |
| anchor-diagnostic.log | F | 100 | Summary [   0.255s] 1 test run: 0 passed, 1 failed, 268 skipped | `eab86836b89972e1293db60e7d361bf53857ef4d9a7218dfcfb7ea59a87f395f` |
| anchor-final.log | F | 0 | Summary [   0.260s] 1 test run: 1 passed, 268 skipped | `f50ee546ff2f17ae45ecfeda98f156890711e64341f6c3c6a19a0991db3a00f0` |
| controls-final.log | G | 100 | Summary [   0.706s] 14 tests run: 13 passed, 1 failed, 986 skipped | `9621c2f6c7ff45a05c208efd5f4bfc44c575088c86d1211e8308ea29cb9f946e` |
| menu-final.log | H | 100 | Summary [   0.236s] 1 test run: 0 passed, 1 failed, 268 skipped | `25175b1d32175dca9adc491d3b1909f892b99d2e15430801be5cb87cf2ef1d5e` |
| menu-keyboard-final.log | H | 0 | Summary [   0.391s] 1 test run: 1 passed, 268 skipped | `8f3715a0a50256e32bc21dc7066459bfe9fe58b973fc53e61c09f1e4fe7c299b` |
| menu-geometry-final.log | I | 0 | Summary [   0.521s] 3 tests run: 3 passed, 997 skipped | `809988a5ce43f3fd3757e3ebf2e0c6c5fa15c76df0b9e75afb98e725f0923ba4` |
| production-check.log | J | 0 | Finished `dev` profile [unoptimized + debuginfo] target(s) in 57.26s | `905fe4214a0059318b5cb9b6d5871091b025bd1e7c0140af2cb4ef80b96ab0f9` |
| format-check.log | K | 0 | 无输出 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| diff-check.log | L | 0 | 无输出 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

首次 `red.log` 是 fixture 未使用已登记 Git capture 的失败，不是业务证据；改用 owned temp store 后 `red-business.log` 真正证明旧截图只能打开模态、没有统一右栏。host/entries/ui/matrix 的编译失败均保留，属于构造/借用/名称/测试宏问题。

`entries-fourth` 两项分别为实际目录0755（修为显式0700）与错误的旧bottom selector；`matrix-third` 的decode error断言缺少GPUI asset完成后排定的下一帧，补真实next-frame后通过。`matrix-fourth` 暴露实际持久FocusHandle未设置tab_stop（现已修复），另一项及 `fixture-final`/`anchor-diagnostic` 是错误的绝对offset假设：24→19.512194来自既有Proportional重测，不是clamp。`controls-final`/`menu-final` 的固定全窗Tab次数假设不适用于deferred菜单；真实trigger仍持焦时首项接焦后，`menu-keyboard-final`严格验证完整键盘链、鼠标选择和外部dismiss。

最终证据分散在增量日志，不能把 `matrix-fourth` 写成全绿：该批14通过/2失败；后续 `controls-final` 为13通过/1失败；失败项最终由 `menu-geometry-final` 3/3 与 `menu-keyboard-final` 1/1闭合。`production-check.log` 编译了 cfg(not(test)) 平台适配和真实launcher，exit0；未运行真实app。仅已有依赖 `block v0.1.6` 的 future-incompat 提示，无本轮源码warning。未运行本地Clippy或全量测试。

## 冻结

UTC `2026-10-13T05:37:35Z`；本地 `2026-10-13T13:37:35+08:00`。

`rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`；`cargo 1.98.0 (797e8a9bc 2026-08-05)`；`git 2.55.0`；`Darwin arm64`。

全部变更/新增 Rust 源文件按路径排序、逐文件 SHA-256 后再哈希，包含未跟踪新文件；manifest `source-fingerprint.txt` 的 SHA-256 为 `1720469129cf160176ad3a7fe8f168db94ab29ec1ff1b70f3d22ae44a5c0d99b`。fmt与diff检查 exit0。

```text
crates/vega/src/tests/r69.rs
crates/vega/src/window/image_preview.rs
crates/vega/src/window/image_preview/native.rs
crates/vega/src/window/image_preview/native/macos.rs
crates/vega/src/window/image_preview/tests.rs
crates/vega/src/window/mod.rs
crates/vega/src/window/render.rs
crates/vega/src/window/workspace.rs
crates/vega_theme/src/lib.rs
crates/vega_ui/src/conversation_stream/attachments.rs
crates/vega_ui/src/conversation_stream/composer.rs
crates/vega_ui/src/conversation_stream/core.rs
crates/vega_ui/src/conversation_stream/mod.rs
crates/vega_ui/src/conversation_stream/render.rs
crates/vega_ui/src/conversation_stream/render_rows.rs
crates/vega_ui/src/conversation_stream/screenshots.rs
crates/vega_ui/src/conversation_stream/tests/attachments.rs
crates/vega_ui/src/conversation_stream/tests/cua_screenshots.rs
crates/vega_ui/src/conversation_stream/tests/cua_usability.rs
crates/vega_ui/src/conversation_stream/tests/issue78_message_bubbles.rs
crates/vega_ui/src/image_preview.rs
crates/vega_ui/src/lib.rs
```

## 更新基线后的验证

主控将初次冻结实现提交为 `f46808f`，安全 abort 一次瞬时 tree 读取错误后重新 rebase 成功且无冲突。集成 HEAD 为 `078a49c33ba0be07ac3fb96270b4c09c2e6935c4`，base 为 `6a668f56d6bc50fd3fcf2a8aade70161ca0a0842`。本次未修改生产代码或测试。

仅执行一次如下组合：

```sh
cargo nextest run -p vega -p vega_ui -p vega_conversation -E 'test(issue296_image_preview) | test(issue63_) | test(issue75_screenshot) | test(issue75_usability_screenshot) | test(issue75_readonly_permissions_screenshot) | test(issue78_image) | test(workspace_root_preserves_draft) | test(r51_workspace_tabs) | test(terminal_workspace_production_handlers) | test(width_remeasure_preserves_anchor_identity) | test(issue296_phase_final_preserves_manual_reading_focus_and_selection)'
```

`integration.log`，exit **0**，原始 footer：

```text
Summary [   1.602s] 39 tests run: 39 passed, 1550 skipped
```

SHA-256：`76878a38d334b2d939d12ff673cca145984f5be720afef00f2f278b9d7ac8e1e`。仅原依赖 `block v0.1.6` 的 future-incompat 提示；未重跑production check、Clippy或全量测试。

独立重算原 manifest 所列22个 Rust 文件，逐文件摘要全部相同；包含新增 source，`source-fingerprint.txt` 聚合 SHA-256 仍为 `1720469129cf160176ad3a7fe8f168db94ab29ec1ff1b70f3d22ae44a5c0d99b`。`cargo fmt -p vega -p vega_ui -p vega_theme -- --check` 与 `git diff --check` 均 exit0；原始空输出保留在 `integration-format-check.log`、`integration-diff-check.log`。

集成交接冻结 UTC `2026-10-13T05:43:12Z`，本地 `2026-10-13T13:43:12+08:00`；M01 仍 **NOT RUN**。
