# R47 交付记录 — 面板结构对齐 Codex（token 化 + Terminal 单层化）

## Freeze

- verified_at_utc: 2026-09-12T05:12:00Z / verified_at_local: 2026-09-12 13:12 (Asia/Shanghai)
- git_head: `406e9630b50838f54ac54d2bb71635a40b1a30b2`
- tracked_diff_sha256 (range master..head): `7fb721d3e1891297…`
- task_contract: `docs/vega-r47-panel-structure-alignment.md`（本 worktree）
- 真值来源：Codex WebView 层 token 源码 `/Users/puzige/Workspace/vega-design-reference/app/webview/assets/app-initial-*.css`（人类提供）+ 原生 2x 实机测量
- os_arch: darwin 24.6.0 arm64 · rustc/cargo: stable（rust-toolchain.toml 锁定）· git: 见上 head
- 前置冻结：R45 `d7cb2d6`、R46 `95c5456`（均未回退）

## 根因（R47 修复的四个）

| ID | 根因 | 类别 | 证据 |
|---|---|---|---|
| D1 | 右 pane 动作组与窗口槽位带间距 0px（两层归属合并成一条按钮带） | 组件层级 | 两种独立测量：token 推算 + 原生截图 |
| D2 | pane 本地「移到底部」与外壳「切换终端」同形（同一 `Icon::DockBottom`） | 视觉 token | 逐像素比对 26% 差异仅来自抗锯齿 |
| D3 | Terminal 面板三层（tab 行 + 状态行 + body）各带分隔线，共 3 条 | 组件层级 | R47 前实测 585 / 627 / 659；R47 后仅 585 |
| D4 | 终端 body 用 `code_bg`(#f6f6f6)（内联代码块的 inset 面）当背景 | 视觉 token | Codex `--color-vega-terminal-background = --color-background-surface = #fff` |

## Codex 真值（本轮新采纳）

| Codex token | 值 | R47 落地 |
|---|---|---|
| `--padding-toolbar` | 8px | `Layout::TOOLBAR_TRAILING_INSET = 8.0` |
| （实测归属间隔） | ≈32px | `Layout::SHELL_SLOT_GUTTER = 32.0` |
| `--color-background-surface` | #ffffff | 终端 body = `colors.bg_base` |
| `--height-toolbar-pane` | 40px | 已有 `WORKSPACE_HEADER_HEIGHT = 40`（未变） |
| 终端无状态行 | — | 状态行仅在 `status != Running` 渲染 |
| 终端面板单分隔线 | — | 移除 pane header `border_b_1` |

## Results

| requirement | evidence class | exact command / 方法 | result | 证据 |
|---|---|---|---|---|
| 六态槽位中心恒定 | E2E-REAL | 原生 2x 抓帧 + 字形簇测量（closed / env-overlay / bottom / bottom+right / 空路由 / 带线程） | PASS：1315.2 / 1347.8 / 1381.8 全态一致 | r47s-A/B/C、r47-click3-c、r47f-tooltip |
| pane 动作组 gutter = 32±2 | E2E-REAL | `right-workspace-maximize` 命中框右缘 → 槽位带左缘（token 1404−104） | PASS：**32.25** | r47-click3-c.png |
| 图标去重（同形不同义消除） | E2E-REAL | 原生 hover 读 tooltip | PASS：新图标 tooltip =「移到底部」；外壳 ⊟ =「切换终端 ⌘J」 | r47f-tooltip-dockmove.png |
| Terminal 单分隔线 | E2E-REAL | 全宽分隔线扫描（内容区 ≥90% 列） | PASS：仅 y=585（R47 前 585/627/659） | vega-r47-b2.png |
| Terminal body = surface | E2E-REAL | body 采样 | PASS：`(255,255,255)`（R47 前 `(246,246,246)`） | vega-r47-b2.png |
| 状态行仅异常态 | 生产测试 | `r47_terminal_running_hides_the_status_toolbar` / `..._exited_...` / `..._failed_...` | PASS | 3 test |
| 顶带预留 + gutter 断言 | 生产测试 | `r47_top_band_pane_actions_keep_the_slot_gutter` | PASS | 1 test |
| token 冻结 | 生产测试 | `r47_panel_alignment_tokens_are_frozen`（104=3×28+2×6+8；136=RESERVE+GUTTER） | PASS | 1 test |
| R44 契约不回退 | 生产测试 | 既有 r44 测试断言**零改动**全绿 | PASS | 见下 |
| fmt | 门禁 | `cargo fmt --all -- --check` | OK | — |
| clippy | 门禁 | `cargo clippy --all-targets -- -D warnings` | 0 warning / 0 error | 仅既存 block v0.1.6 future-incompat 提示 |
| test | 门禁 | `cargo test --workspace` | **1042 passed / 0 failed** | /tmp/r47-test.log |
| package | 门禁 | `cargo xtask package` | OK | dist/Vega.app |
| 安装一致性 | 门禁 | `shasum -a 256` dist vs /Applications | 一致：`f7aad85054a4fa19c77b34192cb9eef7840d3334a62ba6f52707f9dff24159f4` | — |

## 原生验收截图（均来自最终安装的候选包）

| 状态 | 文件 | 结论 |
|---|---|---|
| 无面板（closed） | `/tmp/r47s-A-closed.png` | 槽位中心恒定；三槽全部 enabled |
| Environment overlay | `/tmp/r47s-B-env-overlay.png` | overlay 正常，槽位不动 |
| 底部终端面板 | `vega-r47-b2.png` / `r47-01-bottom.png` | 单分隔线、白底 body、无状态行 |
| 底部 + 右侧面板 | `r47-click3-c.png` | gutter 32.25；pane 动作组与槽位带分离 |
| DockMove tooltip | `r47f-tooltip-dockmove.png` | 「移到底部」，与外壳 ⊟ 语义分离 |

安装包 SHA-256（packaged == installed）：`f7aad85054a4fa19c77b34192cb9eef7840d3334a62ba6f52707f9dff24159f4`
回滚位置：`~/.Trash/Vega-before-r47.app`（R46 二进制 `9267d4936f9c6f49b58825105e3387397d451d58b7b686c6e70e724fb16cdaf1`）

## 实现改动面

- `crates/vega_theme/src/lib.rs`：`TOOLBAR_TRAILING_INSET=8.0`（新）、`SHELL_SLOT_GUTTER=32.0`（新）、`SHELL_SLOT_CLUSTER_RESERVE 108→104`、冻结测试
- `crates/vega/src/window/render.rs`：槽位簇 `.pr_3()` → `.pr(TOOLBAR_TRAILING_INSET)`；注释同步
- `crates/vega/src/window/workspace.rs`：顶带 pane header 预留 `RESERVE+GUTTER`；移除 pane header `border_b_1`；pane dock 动作改 `Icon::DockMove`
- `crates/vega_ui/src/icons.rs`：新增 `Icon::DockMove` + `DOCK_MOVE_SVG`
- `crates/vega_ui/src/terminal.rs`：body bg → `bg_base`；`paint_terminal` 默认 cell 背景同步 → `bg_base`；状态行改 `.when(status != Running)`；canvas frame `.px_3()` → `.px_2()`
- `docs/vega-design-guidelines.md`：v1.30 变更记录

## Residuals

- **ACCEPTED** `bg_active`(#ededed) 与 Codex tab fill(#f4f4f4) 差异 —— 全局选中态 token，影响面大，另轮处理。
- **ACCEPTED** pane header 控件数 4（chevron/+ /dock/maximize）多于 Codex 2（+/×）—— R44 契约冻结，不删功能。
- **LIMIT** 终端字体域不同（Vega mono 12.5px，advance 7.94 vs Codex 8.12 px/char）。
- **LIMIT** Codex 侧面板是 browser 面板、Vega 是 Review/diff 面板——功能域不同，仅对齐结构模型与 token。
- **ACCEPTED** 合成窗口事件（CUA `left_click` / 合成 ⌘J）无法驱动此 app 的 GPUI 命中测试；原生验收一律使用 Quartz 真实光标/键盘事件。这是工具限制，不是产品缺陷。
- **NOTE** 规格 §2.1 初版写死的槽位绝对中心值有算术错误（inset 12→8 应为右移而非左移），已由实现 subagent 发现并按 `窗口右缘−22/−56/−90` 修正，规格文档同轮修订（commit `406e963`）。
