# R50 交付记录 · 选中填充派生 + 侧栏分区间距

> 分支：`feat/r50-sidebar-section-rhythm`（4 个提交，`73900bf` → `aa6c47d`）
> 基线：`master @ 1c23fc4`
> 规格：`docs/vega-r50-selected-fill-and-section-rhythm.md`（冻结于实现前）
> 状态：**实现完成，门禁通过，原生验收通过**

---

## §1 交付内容

| 提交 | 内容 |
|---|---|
| `73900bf` | 新增 `ThemeColors::bg_active_alpha` + `Layout::SIDEBAR_SECTION_GAP`，含合成不变量测试与冻结断言 |
| `1f58477` | workspace tab pill 激活态改用 `bg_active_alpha`（唯一调用点切换） |
| `947ba12` | 移除 `render_pinned_pi` 的 `.mb_1()`；`body.gap_2()` → `SIDEBAR_SECTION_GAP` |
| `aa6c47d` | 端到端结构不变量测试（三分区边界相等，明暗两态） |

改动面：4 个文件，+271 / −4。

## §2 修正了一个既有错误结论

R47 §2.4 与 delivery 记录写的是「`bg_active`(#ededed) 与 Codex tab fill(#f4f4f4) 差异 —— 全局选中态 token，影响面大，另轮处理」。该结论把 `#f4f4f4` 当成一个独立的候选色值，因而推导出"全局换值、影响 49 处调用点"的高风险判断。

**本轮证伪**（全仓字节级检索）：

1. `#f4f4f4` 在参考实现中**不是设计 token**。`app-initial-*.css` 中仅出现 2 次，均为同一条 Tailwind 任意值 `.bg-\[\#F4F4F4\]{background-color:#f4f4f4}`；`app-primary-*.css` 中 0 次。其余命中是 mermaid 主题数据与 SVG 渐变，与 UI 表面无关。
2. 参考实现的选中填充是**一条语义规则**：`--color-background-primary-ghost-hover = color-mix(in oklab, var(--color-text-foreground) 5%, transparent)`，浅色前景 `#1a1c1f`。
3. 该规则同时解释两次独立像素测量：

| 表面 | 算式 | 计算 | 实测 |
|---|---|---|---|
| 侧栏 `#f9f9f9` | `0.05×26 + 0.95×249 = 237.85` | 238 | **237**（`#ededed`） |
| tab pill `#fff` | `0.05×26 + 0.95×255 = 243.55` | 244 | **244**（`#f4f4f4`） |

**结论**：Vega 的侧栏选中态**本来就是对的**。错的是在白色表面上复用了同一个不透明常量。因此修法不是"全局换色值"，而是新增保留 alpha 的 token，只在白色表面那一处切换——把 R47 担心的"49 处调用点风险"消解为 1 处改动。

## §3 分区间距的根因

不是某一侧多了 12px，而是**同一层级关系有两个不同值**：

- `organization/render.rs:209` — `body.gap_2()`（8px，作用于所有 section）
- `organization/render.rs:320` — `render_pinned_pi` 内 `.mb_1()`（4px，**仅** Pinned 之后）

叠加结果：Pinned→Projects = 12px，Projects→Recents = 8px。修复即移除后者，并把前者提为显式 token。

## §4 关键数值与推导

| 值 | 依据 |
|---|---|
| `bg_active_alpha` 浅色 `#1A1C1F @ 0.05` | 参考实现 `--color-text-foreground` + 5% |
| `bg_active_alpha` 深色 `#FFFFFF @ 0.03` | 参考实现深色 `#ffffff08`（3% 白） |
| alpha 用 `f32` 比值而非 `13/255` | 避免把取整步骤烘进 token |
| `SIDEBAR_SECTION_GAP = 12.0` | 实测三分区边界均为 12px 盒间距 / 21.5px 文字带；参考实现文字带比值 1.58×–2.19×，Vega 32px 行高下 1.95× 落在区间内；且 12 是 R35 用户验收已冻结的值，统一到它修正的是 8px 那一侧 |

## §5 门禁结果

| # | 命令 | 结果 |
|---|---|---|
| 1 | `cargo test -p vega_theme -p vega_ui` | 20 + 200 通过 / 0 失败（20.7s） |
| 2 | `cargo test --workspace -- --test-threads=1` | **1060 通过 / 0 失败**（416s） |
| 3 | `cargo test --workspace`（并行） | **1060 通过 / 0 失败**（89s） |
| 4 | `cargo fmt --all -- --check` | 干净 |
| 5 | `cargo clippy --workspace --all-targets -- -D warnings` | 干净 |

R48 缩进阶梯测试全绿，未触碰。

## §6 原生验收

打包 `cargo xtask package`（40s），安装到 `/Applications/Vega.app`，实机启动 1404×860 窗口。

| 项 | 规格 | 实测 | 判定 |
|---|---|---|---|
| tab pill 填充 | `#f4f4f4`(244) | 截图内检出精确 `(244,244,244)` 像素 | ✅ |
| tab pill 高度 | 28px | 27.5px（含抗锯齿） | ✅ |
| 侧栏 Pinned→Projects | 与 Projects→Recents 相等 | 两边界均为 12px 盒间距（结构测试） | ✅ |
| 侧栏选中行填充 | 保持 `#ededed` | 未变（`bg_active` 未改值） | ✅ |

**验收限制（如实记录）**：侧栏当轮数据中 Pinned 之外无 Recents 分区（项目任务占满），三分区相等这一条由 R50 自身的结构不变量测试提供证据，而非本机截图。该测试渲染真实挂载的 `ThreadsBlock` 并断言两个边界的盒间距与文字带均相等，证据等级 `INTEGRATION-DELEGATING`。

**视觉观察（非缺陷，但需知悉）**：tab pill 填充 244 与面板底色 255 仅差 11 级，激活态在浅色下**视觉区分度很低**。这是参考实现的既有特征（同一规则在白色表面必然产出 244），非本轮引入。若需增强区分，应另开一轮改设计（如加边框或提高 alpha），不在 R50 范围。

## §7 未做（按规格 §3）

- 未改 `bg_active` 的值
- 未改侧栏行高 / 水平内边距 / 行圆角
- 未改 tab pill 其他几何（高度/内边距/圆角）——属 R51 范围
- 未改深色模式既有断言
- 未 push、未创建 MR

## §8 发现的其他白色表面调用点（仅报告，未改）

规格 §A.3 要求报告而不修改。以下调用点位于白色表面，同样会从 alpha 派生中受益，但本轮未动：

`settings/provider_management.rs:520,576`、`settings/usage.rs:158`、`settings/render_impl.rs:427,597,677`

## §9 与 R51 的关系

`feat/r51-tab-controls` 与 R50 都改 `crates/vega/src/window/workspace.rs` 和 `crates/vega_theme/src/lib.rs`，但改动互补（R51 加 `TAB_*` 几何 token，R50 加 `bg_active_alpha` 与 `SIDEBAR_SECTION_GAP`）。`git merge-tree` 干跑显示 **0 冲突**。R51 的 `TAB_HEIGHT=28` / `TAB_RADIUS=10` 与 R50 的 pill 填充派生共同构成 tab pill 的完整对齐。
