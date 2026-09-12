# R49 交付记录 — Composer 上下文栏（utility bar）

## Freeze

- verified_at_utc: 2026-09-12T20:12:42Z / verified_at_local: 2026-09-13 04:12 (Asia/Shanghai)
- git_head: `82f7e4b6577fe77e70d9d401a46c6ae8fcebfbda`
- tracked_diff_sha256 (range spec..head): `7061c1d5f79c20cd…`
- task_contract: `docs/vega-r49-composer-utility-bar.md`（本 worktree）
- 真值来源：Codex 实机原生 2x 抓帧（`/tmp/r49-codex-newchat-2x.png` 新建任务页、`/tmp/r49-codex-work-2x.png` 会话页）+ Codex WebView token 源码
- os_arch: darwin 24.6.0 arm64 · rustc/cargo: stable（rust-toolchain.toml 锁定）
- 前置冻结：R45 `d7cb2d6`、R46 `95c5456`、R47 `ed52b07`、R48 `08602fb`（均未回退）

## 人类裁决

1. utility bar **只做两格**（文件夹 + 分支）；Codex 的 `Local`（执行环境）**不移植**，不占位不置灰。
2. **移除** Environment 卡片的分支行，分支入口唯一化到 composer（符合 R45「同一动作只有一个入口」）。

## 根因

| ID | 根因 | 类别 |
|---|---|---|
| D1 | Vega 无 utility bar 层，也无「新建任务页 vs 会话页」状态区分——两个状态渲染同一种 composer | 组件层级 |
| D2 | 文件夹（项目）在 composer 内无入口，切换项目只能靠 sidebar | 状态所有权 |
| D3 | 分支入口挂在 Environment 卡片，与 Codex 的 composer chip 位置不同构；且与 R45「单入口」原则冲突 | 组件层级 |
| D4 | `BranchSelector` trigger 是 R19 的带框药丸，与 Codex 无框 chip 不同形 | 视觉 token |

## Results

| requirement | evidence class | exact command / 方法 | result | 证据 |
|---|---|---|---|---|
| utility bar 仅新建任务页渲染 | 生产测试 | `r49_utility_bar_mounts_above_the_card_only_on_the_new_task_page` | PASS | 9 个 R49 测试全绿 |
| bar 高 = 37 | E2E-REAL | 原生抓帧：bar top 704 → card top 741 | PASS：**37** | r49-vega-06.png |
| bar 左右内缩 = 19 | E2E-REAL | 原生抓帧：card 边框 385.5..1067.5，bar 405..1048 | PASS：**19.5 / 19.5** | r49-vega-06.png |
| 首 chip inset = 14.5 | E2E-REAL | 原生抓帧：chip1 图标左缘 419，bar 左缘 405 | PASS：**14** | r49-vega-06.png |
| 两 chip 顺序与内容 | E2E-REAL | 原生抓帧 + 交互 | PASS：`📁 r12-linked-project` / `⑂ r12-linked` | r49-vega-06.png |
| 文件夹 chip 打开项目菜单 | E2E-REAL | 真实光标点击 | PASS：向上展开，列出真实项目（r11-native-project / r13-alpha-project / sandbox / …），tooltip「切换项目」 | r49f-A-project-menu.png |
| 分支 chip 打开分支列表 | E2E-REAL | 真实光标点击 | PASS：`main` / `r12-linked Current` / `r12-live-check`，当前分支标 `Current`，tooltip「切换分支」 | r49f-B-branch-menu.png |
| Environment 卡片无分支行 | 生产测试 | 断言 `environment-branch` absent | PASS | 1 test |
| 会话 composer 几何不回退 | 生产测试 | 既有断言（卡宽 736 / 高 ≥100 / 发送 28） | PASS | 未改断言 |
| fmt | 门禁 | `cargo fmt --all -- --check` | OK | — |
| clippy | 门禁 | `cargo clippy --all-targets -- -D warnings` | 0 error / 0 warning | 仅既存 block v0.1.6 future-incompat 提示 |
| test | 门禁 | `cargo test --workspace` | **1055 passed / 0 failed** | /tmp/r49-test.log |
| package | 门禁 | `cargo xtask package` | OK | dist/Vega.app |
| 安装一致性 | 门禁 | `shasum -a 256` dist vs /Applications | 一致：`e4a289ad47534ffb055702bd19852d44632a364beee92af6fb8ce4bd0e8b4a9e` | — |

## 原生验收截图（均来自最终安装的候选包）

| 状态 | 文件 | 结论 |
|---|---|---|
| 新建任务页（utility bar） | `/tmp/r49-vega-06.png` | bar 高 37、左右内缩 19.5、两 chip 就位 |
| 文件夹 chip 菜单 | `/tmp/r49f-A-project-menu.png` | 向上展开，真实项目列表 |
| 分支 chip 菜单 | `/tmp/r49f-B-branch-menu.png` | 真实分支列表 + `Current` 标注 |
| Environment 卡片 | `/tmp/r49-vega-04.png` | 已无分支行（标题 + 项目行 + Local terminal） |

安装包 SHA-256（packaged == installed）：`e4a289ad47534ffb055702bd19852d44632a364beee92af6fb8ce4bd0e8b4a9e`
回滚位置：`~/.Trash/Vega-before-r49.app`（R48 二进制 `3b21f669f606fd7bddb66b1971b22034ae8571a410d786316ec5b34f420f4327`）

## 实现改动面

- `crates/vega_theme/src/lib.rs`：5 个 token（`COMPOSER_UTILITY_BAR_HEIGHT=37` / `_INSET=19` / `_RADIUS=12` / `COMPOSER_UTILITY_CHIP_GAP=28` / `_CHIP_INSET=14.5`）+ 冻结测试
- `crates/vega_ui/src/conversation_stream/utility_bar.rs`（新建 250 行）：bar 层 + 两个 chip + 项目菜单
- `crates/vega_ui/src/conversation_stream/render.rs`：bar 作为卡片前兄弟节点（`.when(utility_bar_visible)`，零重叠、无负 margin）
- `crates/vega_ui/src/conversation_stream/core.rs`：菜单状态 + `set_chip_chrome(true)`
- `crates/vega_ui/src/branch_selector.rs`：新增 `set_chip_chrome(bool)`（默认 false 保持既有药丸样式；语义/disabled 降级不变）
- `crates/vega/src/window/workspace.rs`：删除 `environment-branch` 节点及其接线
- `crates/vega_ui/src/conversation_stream/tests/utility_bar.rs`（新建 183 行）：9 个 R49 测试
- `docs/vega-design-guidelines.md`：v1.32

## 与规格的偏离

**无。** 两处规格预留自由度按最小改动落地：
1. §4 残差「项目选择器弹层定位」→ 菜单**向上展开**（composer 贴窗口底边），代码注释已说明。
2. §2.6 样式开关命名取 `set_chip_chrome`（规格原文允许「或等价最小改动」）。

## Residuals

- **ACCEPTED** Vega bar 只有两格，Codex 三格（缺 `Local`）—— 人类裁决 1。
- **ACCEPTED** chip 墨迹间距实测 34 vs Codex 28：**不是布局错误**。bar 的 `gap` 布局值确为 28；差异来自图标字形宽度不同（`Folder` 墨迹满 16px，`ArrowUpDown` 墨迹仅 8px 且居中，导致墨迹起点右偏约 4px）。按图标框对齐后间距 ≈30，与 28 的差属字形内边距。
- **ACCEPTED** 卡片实测宽 682 而非 `COMPOSER_MAX_WIDTH`(736)：当前窗口内容区被 Environment rail 压缩后可用宽度不足 736，`max_w` 按预期退让。**这不是 R49 引入的**——会话 composer 一直如此，且 `max_w` 语义正确。
- **LIMIT** 项目菜单的弹层定位为「向上」，若未来 composer 不贴窗口底边需重新评估。
- **NOTE** 首次全量 `cargo test --workspace` 曾报 `vega_conversation` 的 `selection_noop::clean_and_normalized_noop_are_no_staged_changes_without_commit` 失败（`process_control_failed`）；隔离与后续两次全量重跑均通过，`vega_conversation` 不依赖本次改动文件 —— 既存 flake。
- **NOTE** 既有 R21 Environment 断言按新真值更新（`environment-branch` 由 bounds 断言改为 absent）；R44/R45/R46/R47/R48 契约断言一律未改，全绿。
- **NOTE** 原生验收中，合成窗口事件（CUA `left_click`）无法驱动此 app 的 GPUI 命中测试；一律使用 Quartz 真实光标事件。此为工具限制，非产品缺陷。
