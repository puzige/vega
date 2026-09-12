# R48 交付记录 — Sidebar 缩进阶梯对齐 Codex

## Freeze

- verified_at_utc: 2026-09-12T14:59:18Z / verified_at_local: 2026-09-12 22:59 (Asia/Shanghai)
- git_head: `5e487c2a783059c2e69e52fbe1b5ecd2f51ed72c`
- tracked_diff_sha256 (range master..head): `a0941208258cc5dc…`
- task_contract: `docs/vega-r48-sidebar-indent-ladder.md`（本 worktree）
- 真值来源：Codex 实机 AX 控件盒 + 原生 2x 像素测量 + R47 同源 token 库
- os_arch: darwin 24.6.0 arm64 · rustc/cargo: stable（rust-toolchain.toml 锁定）
- 前置冻结：R45 `d7cb2d6`、R46 `95c5456`、R47 `ed52b07`（均未回退）

## 根因

| ID | 根因 | 类别 | 证据 |
|---|---|---|---|
| D1 | 项目行的 `.px_2()`(8) 把 folder 图标从 base(16) 推到 24.5，与 section 标签列(17.0)错位 7.5px | 组件层级 | 原生 2x 实测 + 算术链推导 |
| D2 | folder 文本(48) 与 child 文本(48) 同列是数值巧合：一个靠 `px_2+16+gap_2` 累加，一个靠 `NAV_CONTENT_INSET(32)`，非共享基准 | 状态所有权 | 代码路径比对 |
| D3 | section 标签列靠隐式继承容器，非显式阶梯 token | 视觉 token | 代码审查 |

Codex 真值模型（逐行核实）：**folder 图标与 section 标签同列（都贴 base）**，folder 文本与 child 文本同列。

## Results

| requirement | evidence class | exact command / 方法 | result | 证据 |
|---|---|---|---|---|
| folder 图标列 == 标签列 | E2E-REAL | 原生 2x 字形测量（5 态） | PASS：图标 16.5 / 标签 17.0，**差 0.5px**（R48 前差 7.5px） | r48-01..05 |
| folder 文本列 == child 文本列 | E2E-REAL | 原生 2x 字形测量 | PASS：41.0 / 40.5（同列） | r48-vega-after-2x.png |
| ROW_INSET 导出关系 | 生产测试 | `r48_sidebar_indent_ladder_is_frozen`（断言 `== 16.0 + 8.0`） | PASS | vega_theme 16/16 |
| 图标与标签同列 | 生产测试 | `r48_project_folder_shares_the_section_label_column` | PASS | tests.rs |
| 文本与 child 同列 | 生产测试 | `r48_project_name_shares_the_child_text_column` | PASS | tests.rs |
| 折叠态图标不变列 | 生产测试 | `r48_folder_icon_keeps_its_column_when_the_project_collapses` | PASS | tests.rs |
| hover/selected 不变 | 生产测试 | 既有断言全绿（未改） | PASS | — |
| fmt | 门禁 | `cargo fmt --all -- --check` | OK | — |
| clippy | 门禁 | `cargo clippy --all-targets -- -D warnings` | 0 warning / 0 error | 仅既存 block v0.1.6 future-incompat 提示 |
| test | 门禁 | `cargo test --workspace` | **1046 passed / 0 failed** | /tmp/r48-test.log |
| package | 门禁 | `cargo xtask package` | OK | dist/Vega.app |
| 安装一致性 | 门禁 | `shasum -a 256` dist vs /Applications | 一致：`3b21f669f606fd7bddb66b1971b22034ae8571a410d786316ec5b34f420f4327` | — |

## 原生验收截图（均来自最终安装的候选包）

| 状态 | 文件 | 结论 |
|---|---|---|
| rest | `/tmp/r48-01-rest.png` | 图标 16.5 == 标签 17.0 |
| selected（打开线程） | `/tmp/r48-02-selected.png` | 对齐保持 |
| expanded（展开项目） | `/tmp/r48-03-expanded.png` | 对齐保持 |
| collapsed | `/tmp/r48-04-collapsed.png` | 对齐保持 |
| hover（悬停项目行） | `/tmp/r48-05-hover.png` | 对齐保持 |

对照图：`/tmp/r48-after-vs-codex.png`（Vega R48 后 vs Codex，红线=标签列，绿线=文本列，两者同构）。

安装包 SHA-256（packaged == installed）：`3b21f669f606fd7bddb66b1971b22034ae8571a410d786316ec5b34f420f4327`
回滚位置：`~/.Trash/Vega-before-r48.app`（R47 二进制 `f7aad85054a4fa19c77b34192cb9eef7840d3334a62ba6f52707f9dff24159f4`）

## 实现改动面

- `crates/vega_theme/src/lib.rs`：新增 `SIDEBAR_LABEL_INSET=0.0`、`SIDEBAR_ROW_INSET=24.0`（含"= 图标16 + gap8"导出关系 doc comment）+ 冻结测试
- `crates/vega_ui/src/sidebar/threads_block/organization/render.rs`：`render_pi_project` 的 `.px_2()` → `.pr_2()`；两个 progressive control → `SIDEBAR_ROW_INSET`；Pinned/Projects 标签 → `SIDEBAR_LABEL_INSET`
- `crates/vega_ui/src/sidebar/threads_block/organization/projections.rs`：`render_project_thread` → `SIDEBAR_ROW_INSET`；Recents 标签 → `SIDEBAR_LABEL_INSET`
- `crates/vega_ui/src/sidebar/threads_block.rs`：活路径 `render_pi_row` 的 inset → `SIDEBAR_ROW_INSET`（见下）
- `docs/vega-design-guidelines.md`：v1.31 变更记录 + §7 阶梯条款

## 与规格的偏离

**一处，已复核并接受。** 规格 T3 点名 `organization/projections.rs::render_project_thread` 为子任务行，但该函数只在 `render_project_projection` 内被调用，而后者无调用方（`organization.rs` 为 `#![allow(dead_code)]`）；挂载中的 sidebar 实际走 `render_organization → render_projects_pi → render_pi_project → render_pi_row`（`threads_block.rs`）。

子代理判断"只改点名的函数会让实测 UI 不动、t2 必然失败"，因此**在点名的死路径与活的 `render_pi_row` 上都做了同一 token 替换**。主 agent 复核确认：`render_project_projection` 确实只被自身调用（grep 无其它引用），两个改动点都是正确的；活的调用点在 `threads_block.rs:1014`。属实现者按证据修正规格的合理判断，非越权。

## Residuals

- **ACCEPTED** `threads_block.rs` 会话行（Recents 区块的 thread row）仍用 `SIDEBAR_NAV_CONTENT_INSET`(32)；R48 只改了 project child row 的调用点，Recents 行不在本轮范围。
- **ACCEPTED** `projections.rs::organization_thread` 的项目名 sublabel（第 550 行）仍为 32px —— 死路径，未在任务卡点名。
- **ACCEPTED** `render.rs::organization_control` 的 `.px_2()`（group/timeline projections）未动 —— 超出本轮范围。
- **ACCEPTED** Codex 侧栏无 Pinned 分区标题（置顶项直接列在最上），Vega 保留独立 Pinned 标签 —— 产品差异，不追平。
- **ACCEPTED** 分区间距（垂直）Vega 31.5 vs Codex 19 未处理 —— 属垂直节奏，另轮。
- **NOTE** 首次全量 `cargo test --workspace` 出现 `vega_conversation` 的 `trusted_git_rejects_intent_to_add_and_hidden_delete_form` 失败；隔离重跑通过，且后续两次全量运行均通过。`vega_conversation` 不依赖 `vega_theme`/`vega_ui`，本 diff 未触及 —— 既存 flaky。
- **NOTE** 原生测量中 folder 图标(16.5) 与标签(17.0) 恒差 0.5px：图标字形自带 0.5px 内缩，五态恒定，属字形内边距而非布局偏差。
