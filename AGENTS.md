# AGENTS.md — Vega 仓库协作准则

Cross-agent instructions for Vega — a native AI agent desktop (Rust + GPUI).

## 最高原则：SDD（Spec-Driven Development）

**Spec 先行，代码不允许先于 spec。** 所有实现工作必须对应 [`docs/`](docs/) 中的具体规格章节。设计文档以本仓库 `docs/` 为准（主索引见 [README](README.md#状态)）。

- [`docs/vega-exec-guide.md`](docs/vega-exec-guide.md) 是**执行宪法**：红线清单、依赖白名单、遇阻上报协议、验收协议。任何 agent 开工前必读。
- 视觉/UI 工作必须先读 [`docs/vega-design-guidelines.md`](docs/vega-design-guidelines.md)。它统一设计语言与 token 使用；产品行为、安全契约和任务级冻结规格仍按各自更具体的 spec 执行。
- 发现 spec 缺陷 → 提 issue 或修改 spec 文档并注明变更记录，**禁止代码先行**。

## 工作流

1. **不在 `master` 上直接做功能开发**——用 feature 分支（`feat/<task-id>-<slug>`，如 `feat/t01-scaffold`）。
2. **动手前先 `git fetch && git rebase origin/master`**。
3. **任务来源**：默认从 [GitHub Project](https://github.com/users/puzige/projects/2/views/1?system_template=kanban) 的 Ready 卡取任务，用户指定任务优先；关联 `docs/vega-s*-tasks.md` 等规格。一张卡 = 一个 PR。卡外工作先问。取卡、实现与交付必须使用 [vega-kanban-delivery](.agents/skills/vega-kanban-delivery/SKILL.md)，验收、合并、任务分支/worktree 清理及卡片回写全部完成后才能关闭。
4. **主 agent 角色**：协调、验收、集成；**代码实现委托给专用 subagent**，主上下文不被实现细节污染。
5. **遇阻**：按 exec-guide §6 用 `[BLOCKED]` 格式上报，禁止自创方案绕过。
6. **验收强制 E2E-first**：优先以真实 production 入口、owned temp repo 与真实 controller 的端到端证据验收；test-only seam 仅保留无法由 E2E 稳定证明的安全不变量，证据分级与留存格式见 [exec-guide §7](docs/vega-exec-guide.md#7-验收协议每个任务卡通用)。

## 问题与需求收集

- 按 [Issue 与看板工作流](docs/vega-issue-workflow.md) 收集明确要跟踪的 Vega 问题和需求。**登记不等于承诺立即修复**：暂时无法处理时，先查重并记录 Issue，给用户链接和真实状态，不把它假装成进行中。
- GitHub Issue 是执行事项与证据的单一入口，GitHub Project 只管理流转状态；Notion PRD 管产品目标，本仓 `docs/` 管实现规格。Issue 本身不替代上述 spec，也不授权卡外代码实现。
- 无法访问 Project 时，不得声称已入看板；如已获授权且可创建 Issue，先保留 Issue 并明确说明待关联。仓库公开，截图、日志和复现信息必须脱敏。

## 提交与 PR

- 提交格式：`feat(A2-09): <一句话>` / `fix(A3-07): <一句话>`（功能点 ID 见 [vega-features.md](docs/vega-features.md)）
- 小步提交，一个任务卡 ≤3 个 commit
- PR 必须附：验收命令原始输出 + 与 spec 的偏离说明（必须为无）
- 合并方式：squash merge，合并后删除功能分支（2026-08-29 决策）

## 验证门禁与并发构建（2026-09-21 用户裁决）

按 [Issue #107 规格](docs/vega-issue-107-test-workflow.md)执行。**单卡不默认运行 `cargo test --workspace`**，不得因 push hook 或主/子 agent 交接重复执行同一份有效验收。

- 统一入口：`python3 scripts/verify.py --plan` 先查看范围；`python3 scripts/verify.py` 执行。默认相对 `origin/master` 的 merge-base，包含本地改动；`--base <ref>` 可冻结验收基线。
- Rust 变更选择受影响 workspace 包和全部传递依赖方，运行格式、对应 clippy 与测试。任务规格仍须写清 production-root 回归；不能只按改动文件挑几个测试冒充完整影响分析。
- 纯文档运行适用检查；开发脚本/hook 运行工具链回归。根依赖/工具链或无法识别的构建输入变更要求明确选择 `--full`，不能静默漏检。`--full` 用于需要全量覆盖的集成，不是每卡默认。
- 成功证据仅在源码内容、基线、命令、工具链和相关环境一致且日志完整时复用。失败、源码在验证中变化、日志缺失均不得复用；主 agent 负责检查证据，不例行重跑子 agent 刚通过的同树测试。
- pre-push 调用同一入口，校验实际推送的当前干净 HEAD；不再无条件全量 clippy/test/build。测试已编译目标，不每次 push 追加重复 build。打包和安装在需要应用交付的节点执行。
- 保留既有安全断言、失败输出及任务验收矩阵；不得为提速删测试、加 ignore、放宽断言或自动重试到绿。

### 构建目录和资源调度

**新 worktree 不再链接共享 target。** `scripts/cargo-lock.sh <cargo-args>` 使用按 worktree 隔离的持久构建目录，限制全仓构建并发（默认 2），同一个实际 target 全程互斥。首次冷编译仍有成本，后续沿用该任务自己的缓存。显式 target 也必须经调度器，并绑定单一 worktree；已有且无法确认归属的缓存不能自动接管。

- `scripts/cargo-lock.sh --wait test -p <package>` 明确排队；不带 `--wait` 快速报告冲突。
- `scripts/cargo-lock.sh --status` 查看资源；格式检查无需构建锁。
- 旧测试中尚有共享状态，当前所有 `cargo test` 调用继续跨任务互斥；编译可独立并发。不要宣称已实现所有测试并行，也不要用绕过入口的 Cargo 命令规避限流。
- `scripts/cargo-share-target.sh` 仅用于状态/迁移说明，不再创建共享链接或删除缓存。旧主检出 target 和未知目录不自动删除。
- 旧 worktree 中的旧版 wrapper 不认识新调度锁；迁移后必须更新脚本再运行，不能混跑两种协调协议。
- 用 `scripts/cargo-lock.sh --target-path` 查询实际产物目录；不要硬编码 `target/release`。
- 证据在 worktree 外保存，清理任务时先确认无运行进程、代码已交付或已归档；只清理本任务的构建缓存。不要每次验证后 `cargo clean`，它会丢掉下一次增量编译收益。
- 全局安装/原生 UI 操作仍独占，与 Cargo 编译槽位无关。

## 固定应用安装入口（2026-09-21 用户约定）

- 用户日常使用的应用固定为 `/Applications/Vega.app`，bundle ID 固定为 `ai.vega`。新版本替换这一位置；不要创建带版本号的安装名、指向 worktree 的符号链接或从 `dist` 启动日常应用。
- `dist/Vega.app` 仅是打包产物。备份优先保存为 zip，附原二进制哈希和构建 commit；不要长期散落可被系统索引的 `.app` 备份。既有验收证据不得擅自删除。
- 安装须独占，先检查正在运行的任务；未经确认空闲不得强退。候选构建签名/哈希验证完成后再替换固定位置，并核对安装后二进制身份。
- 替换后运行 `/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/Vega.app`，通过 bundle ID 查询确认解析到此路径，后续启动使用 `open /Applications/Vega.app`。
- 若发现重复入口，只对确认 bundle ID 为 `ai.vega` 的旧构建/备份路径逐个 `lsregister -u <exact-path>`；先留存清单，不删除备份文件，不重置整个 Launch Services 数据库或 Launchpad 布局。注册验证不等于已经目视确认 Launchpad。
- “代码已合并”和“应用已安装”分别报告，不能把旧安装当作最新 master。纯文档约定不触发重编译或应用安装。

## 原生 UI 验收：合成输入事件无效（工具限制）

**不要用合成键盘/鼠标事件验证本应用的焦点行为——它们驱动不了 GPUI 的焦点链。** 这是工具限制，不是产品缺陷；写成规则以免每轮重复踩坑。

2026-09-13 实测，四种方法全部失败：

| 方法 | 结果 |
|---|---|
| Quartz `CGEventCreateKeyboardEvent` 发 Tab | 焦点不移动（连续 10 次停在同处） |
| AppleScript `key code 48` | 同上 |
| Quartz unicode 键盘事件打字 | 字符不进入输入框 |
| macOS AX 查询 `AXFocusedUIElement` | 恒返回 `AXWindow`；窗口 AX 子元素仅 4 个 |

根因：GPUI 应用不把内部焦点暴露给 macOS 辅助功能树，且 `focus_visible` 只在键盘导航路径下渲染。

**推论与正确做法：**

- **焦点归属不能靠截图判断**。composer 等控件用 `focus_visible`（仅键盘导航时可见），鼠标点击后的截图看不出焦点在哪。要验证焦点，读**代码链路**或用 GPUI `TestAppContext`（见下）。
- **焦点行为的权威验证方式是生产测试**。GPUI 测试可直接注入并断言真实焦点状态：`window.update(cx, |_, w, cx| handle.focus(w, cx))` 后用 `handle.contains_focused(w, cx)` 断言，`window.focus_next(cx)` 可验证键盘导航路径。范例：`crates/vega/src/window/workspace.rs` 的 `r51_inactive_close_follows_parent_focus_and_group_hover`。
- **合成鼠标事件仍可用于**：悬停（tooltip 会出现）、点击按钮触发无焦点依赖的动作。但依赖焦点状态的交互（tab 键盘激活、`focus_visible` 样式）不可用此法验证。
- **原生像素验收仍有效**，用于颜色/几何/布局：截图 + 像素测量是可靠证据（如暗色下 tab pill 填充 `(39,39,39)` 与 `0.03×255+0.97×32=38.61` 吻合）。不可靠的只有**依赖焦点的**那部分。
- 验收报告须区分「行为已验证（生产测试）」与「像素外观已验证（截图）」；不要把前者当作后者。

同类记录：`docs/vega-r47-panel-structure-alignment-delivery.md:80`。

## 原生验收的操作方法：六个必踩的坑（2026-09-14/15 实测）

R62 验收耗时远超预期，全部时间花在**定位窗口和坐标**上，与产品无关。以下每条都真实发生过一次，照做可省一整轮。

### 坑 1 · `pgrep -f '<path>'` 会匹配到自己

`pgrep -f '/Applications/Vega.app/Contents/MacOS/vega'` 恒返回命中——模式字符串出现在执行 pgrep 的 **shell wrapper 自己的命令行**里。

**改用**：`pgrep -x vega`（精确进程名），或 `ps aux | grep '[V]ega'`。

### 坑 2 · ZCode 客户端窗口标题就是 "Vega Desktop"

ZCode 自己的窗口标题是 `Vega Desktop`，界面上也有 `Full access` 等相同字样。**只看截图无法分辨**，点击会落到 ZCode 上。

**改用**：`CGWindowListCopyWindowInfo` 按 owner name + bundle id 定位目标窗口。

### 坑 3 · `screencapture -l <winid>` 的阴影边距会变

窗口 ID 截图会把投影拍进去，边距**随窗口阴影状态在 34～56 逻辑像素间变化**。用它做像素→坐标换算必然错位（这是本轮坐标反复失准的真正原因）。

**改用**：`screencapture -R x,y,w,h`（x/y/w/h 取自 `CGWindowListCopyWindowInfo` 的 bounds），输出严格 2× 且无边距，`px = 2 × window-rel`。

### 坑 4 · shell 往返会让终端抢回焦点

`open -a Vega` 与随后的点击若分属两次 Bash 调用，中间的终端窗口会重新成为前台。

**改用**：把「激活 → 等成为前台 → 点击 → 截图」写进**同一个进程**。`scripts/native-drive.swift` 是为此写的驱动（`NSRunningApplication` + `CGWindowListCopyWindowInfo` + `CGEventPost` + `screencapture -R`）。

注意：`NSRunningApplication.activate()` 在本机被系统拒绝（返回后仍非前台），**必须用 `open -a` 激活**，然后轮询 `NSWorkspace.shared.frontmostApplication?.bundleIdentifier`。

### 坑 5 · 用窗口内相对坐标，不要用屏幕绝对坐标

窗口位置在验收过程中会变（本轮实测 origin 从 (318,53) 变到 (346,52)）。写死屏幕坐标会在下一次窗口移动时静默失效。

**改用**：每次都从 `CGWindowListCopyWindowInfo` 现读 origin，再加窗口内相对偏移。

### 坑 6 · 点不开某个 UI 时，先查数据前提，再调坐标（R66 实测）

R66 验收时我反复打不开滑块卡片，**花了十几轮在调整点击坐标**，全是白费。真实原因是数据前提不满足：`~/.config/vega/reasoning.toml` 只给 `glm-5.3-flash` 声明了档位，而当时选中的模型是 `deepseek-v4-flash` —— 按 R57 R12，**无档位的模型不渲染滑块卡片**。

**规则**：某个 UI 元素反复点不开时，**先确认它的渲染前提**（该模型的配置、该路由的状态、该 feature 开关），再去调坐标。前提不满足时，坐标怎么调都不会有反应。

配置位置速查：reasoning 档位在 `~/.config/vega/reasoning.toml`（**不在** `~/Library/Application Support/ai.vega/` 的数据库里，也不在任何 `.toml` 仓库文件里）。

## GPUI 圆角：`overflow_hidden` 不裁圆角，且半径会被钳制（R65 实测）

两条都必须记住，各自踩过一次坑。

### 1 · `overflow_hidden` 只裁矩形，`rounded` 不影响子元素

`ContentMask`（`gpui-pre-0.3.4/src/window.rs:2081`）是 `struct { bounds: Bounds<P> }`——**没有圆角字段**。`overflow_mask`（`style.rs:638`）据此构造遮罩，所以：

**给父元素加 `.rounded(N).overflow_hidden()` 不会把子元素裁成圆角。** `rounded` 只作用于元素**自己的背景**。

需要圆角的子元素必须**自己带 `rounded`**。R65 的滑块填充左端方角就是这个原因——它一直指望父级轨道裁剪，而那条路在 GPUI 里不存在。

### 2 · 圆角半径按 quad 尺寸钳制

`clamp_radii_for_quad_size`（`geometry.rs:2467`）：

```rust
let max = cmp::min(size.width, size.height) / 2.;
// 每个角实际半径 = min(设定值, max)
```

**窄元素上设大半径会被静默压小。** R65 首轮我写"12px 宽的填充设半径 12，钳到 6 就是半圆"——错。真半圆需要半径 = 高度的一半（12），钳到 6 只能得到 6px 圆角，于是填充戳出轨道 6px。

**正确做法**：先把元素宽度撑到 `>= 2 × 目标半径`（本例撑到高度 24），再设半径。多出的宽度要确保被后绘的元素盖住。

## 规格里出现数值断言时，必须当场验算（R65 教训）

R65 首轮规格里我写了"半径钳到 6.0 视觉上就是左半圆"，**没有验算**。实际是填充左界 x=6 而圆点左界 x=12，露出 6px。这个未验算的断言直接导致首轮修复无效，浪费一整轮实现 + 打包 + 验收。

**规则**：规格中凡是出现半径、坐标、宽度、差值等**数值断言**，必须在写规格时就逐行算一遍，并把算术表附在规格里。不要用"视觉上""大概"这类词替代计算。

## 改「形状/对称性」类缺陷时，先列全所有产生该形状的代码路径（R65 教训）

R65 修滑块填充左端时，我没问"另一端呢"。实际上产生这个形状的有 **flat + gradient 两个分支 × 左右两端 = 4 个角**，我只改了其中一个，于是最强档渐变右端仍是方角（用户第二次复验才发现）。

**规则**：这类缺陷开工前先枚举所有代码路径与所有端/角，逐一核对，并在验收判据里**为每一个都写一条**。

## GPUI 绘制顺序：弹出层必须 `deferred`（R64 实测）

**在带边框的卡片里挂浮层，卡片边框会画在浮层之上。** 这是 GPUI 的绘制顺序决定的，不是 z-index 问题，`occlude()` 解决不了。

`gpui-pre-0.3.4/src/style.rs:688` 的 `Style::paint` 顺序：

```rust
window.paint_quad(background);   // 1. 背景
continuation(window, cx);        // 2. 子元素（浮层在这里画）
if self.is_border_visible() {
    window.paint_quad(border);   // 3. 边框 ← 在子元素之后
}
```

**`occlude()` 只影响鼠标命中测试**，不影响绘制顺序——`div.rs:1208` 的 `Div::occlude` 只做 `interactivity().occlude_mouse()`，即设 `HitboxBehavior::BlockMouse`。R61/R62 给每个浮层都加了 `occlude()` 并据此认为层级已处理，**这个推断是错的**（R64 §2 记录了错误链条）。

**正确做法**：浮层包一层 `gpui_kit::deferred(...).with_priority(2)`。语义是*"delay the painting of its child until after all of its ancestors, **while keeping its layout as part of the current element tree**"*（`elements/deferred.rs:11`）——布局不变、只把绘制挪到祖先之后，由 `window.rs:3343` 的 `paint_deferred_draws()` 在 `root_element.paint()` 之后执行。

注意两点：

- **`deferred` 不改变坐标**。`defer_draw` 会记录 `absolute_offset`（`window.rs:4105`）。R59 曾记录"deferred 导致卡片落在 x 1232.5"——那是它把 deferred 包在 `max_w + mx_auto` 的**外层盒子**里所致（R61 已删该结构），不是 deferred 本身。
- **仓库里已有正确先例**：`branch_selector.rs:997`、`render_file_dropdown`（`render.rs:702`）、侧栏菜单。新增浮层时照抄这个模式。

**验收方法**：浮层类改动不能只看"内容对不对"。必须做一次**像素行扫描**：取浮层中部一行，确认其中没有其他层画上来的元素（R64 用 `vfind`/`vscan` 定位到 `rgb(232,232,232)` = `border_subtle` 的连续 run）。

## GPUI hover：`.hover()` 不带 `.id()` 就永不重绘（R67 实测）

**给一个匿名 div 加 `.hover(...)`，它永远不会变色。** 必须同时给它 `.id(...)`。

原因：GPUI 只在元素拥有 **element state** 时才注册 hover 翻转的重绘监听（`gpui-pre-0.3.4/src/elements/div.rs:2783`），而 element state 只在 `Element::id()` 返回 `Some` 时存在（`div.rs:1848`，id 来自 `interactivity.element_id`，即 `.id(...)`）。绘制分支（`div.rs:3391-3405`）在有 hitbox 时读 `hitbox.is_hovered(window)`，无 hitbox 时退化到 element state——匿名 div 没有 element state。

R67 的临时探针实测（同一份代码，只差一个 `.id()`）：

```
ANONYMOUS: base        renders=1 bg=BLUE
ANONYMOUS: hover-child renders=1 bg=BLUE   ← 永不变化
ANONYMOUS: hover-block renders=1 bg=BLUE   ← 永不变化
STATEFUL:  base        renders=1 bg=BLUE
STATEFUL:  hover-child renders=2 bg=RED    ← 生效
STATEFUL:  left        renders=3 bg=BLUE   ← 退出还原
```

**这条最阴的地方是它静默**：不报错、不 panic，只是永远不亮。若把某个原本常驻的底色改成 hover 态（R67 就是），漏掉 `.id()` 会得到一个**比改动前更糟**的结果。

**加 `.id()` 不等于加交互。** id 只注册一个 `HitboxBehavior::Normal` 的 hitbox，不注册任何监听器；R66 R5 约束的是**点击行为**，`r66_r5_the_highlight_container_is_inert` 在加了 id 之后仍通过。

**顺带**：hover 命中父块时，指针落在带自己 hitbox 的**子元素**上也一样有效——`hit_test` 收集指针下所有 hitbox，`HitboxBehavior::Normal` 不遮挡后面的（`window.rs:1095`）。所以"整块 hover"不必给每行单独加 hover。

## 测试平台能观测绘制：用 `painted_quads()`，不是 `capture_screenshot`（R67 实测）

旧注释说"测试平台没有 headless renderer，所以绘制结果不可观测"——**只对了一半**，别再用它当借口跳过绘制断言。

- `VisualTestContext::capture_screenshot` → `window.render_to_image()`：**确实**需要 `HeadlessRenderer`，未配置时 `bail!`（`gpui-pre-0.3.4/src/platform/test/window.rs:441`）。
- **`Window::painted_quads()`**（`window.rs:2618`）：直接读 `rendered_frame.scene.quads`，**不需要渲染器**。每个 `Quad` 带 `bounds` / `corner_radii` / `background`。

```rust
let quads = visual
    .update_window(window, |_, window, _| window.painted_quads())
    .expect("painted quads");
```

两个坑：

- **`painted_quads` 的 bounds 是缩放像素**（本机 2×），`debug_bounds` 是逻辑像素。要按 `window.scale_factor()` 换算后才能互相比较。
- **`Background` 的 `tag`/`solid` 字段是 `pub(crate)`**，外部无法 `match Background::Solid(..)`。用公开的 `Background::as_solid()` 取 `Hsla`，再比色。

R67 因此把"hover 才亮"从"看截图差不多"变成了可证伪的生产测试（`r67_a3..a6`）。

## 验证 hover 类改动不能用点击（R67 实测）

`scripts/native-drive.swift` 只做点击。**hover 态不能靠它验证**——点标题行会进二级列表，点别处又测不到 hover。需要一个 **move-only** 驱动：激活 → `CGEvent(mouseMoved)` 移动（不要 down/up）→ 截图。

另外：hover 是**翻转**触发的，所以移动要分两步（先到块外的起点，再到目标点），一步直接落在目标上可能不触发。R67 的驱动先移到窗口左下角再移到目标。

## GPUI 点击外部关闭：`on_mouse_down_out` 会与 mouse-up 触发器互相打架（R68 实测）

`on_mouse_down_out` 是"点击外部关闭"的正确原语。三条实测结论：

- **不需要 `.id()`**（与 `.hover()` 相反，见上一节）；
- **在 `gpui_kit::deferred` 包裹下照常工作**，不影响 R64 的绘制顺序修复；
- 跑在 **capture 阶段**，且只在指针位于该元素 bounds **之外**时触发。

### 坑：触发器的开关在 mouse-up，会被 out 处理器吃掉

若触发器的开关跑在 mouse-**up**（本仓库两个 composer 弹窗都是），而 out 处理器跑在 mouse-**down**，点触发器会变成「down 先关 → up 再开」，**看起来完全没反应**：

```
1. click chip        -> open=true
2. click chip again  -> open=true   ← 错，关不掉
```

**修法**：触发器改用 **`capture_any_mouse_down(|_, _, cx| cx.stop_propagation())`**，在 capture 阶段抢先声明手势。

注意 **bubble 阶段的 `on_mouse_down(stop_propagation)` 挡不住**——它比 capture 晚。这是最容易踩错的一步。

### 坑：别把 out 处理器挂到「包含触发器的外层」

那样能修好触发器，但会**破坏弹窗内部点击**：弹窗是 `absolute` 定位，落在 wrapper 的布局 bounds **之外**，内部点击会被当成「外部」。实测点弹窗内部会把弹窗关掉。**用 capture 方案，不要用外层方案。**

### 弹窗内部点击要防穿透

弹窗自己仍要保留一个 bubble 阶段的 `.on_mouse_down(stop_propagation())`，否则点弹窗内部会穿透到**祖先**的外部点击处理器（如 composer 卡片自己的）。它与 capture 阶段的 out 处理器**阶段不同、互不冲突**，两个都要有。

## 对齐参考实现时，先确认 CSS 变体作用域（R68 教训）

参考实现的 CSS 里有大量 `[data-vega-window-type=browser]` / `=electron` 变体块。**同一组 token 在不同变体下值不同**，取错会得出完全反向的结论。

R68 就踩了：从 `[data-vega-window-type=browser]` 块取了 `--menu-item-height: 36` 等值，写下"行高该 32→36、内边距差一倍"；而 Codex **桌面截图**实测行高 28.5、行内边距只差 2px——**真正的缺陷是卡片宽度差 30%**。若照第一版做，会把行高改大（反向优化）而没修宽度。

**方法**：

1. 取 token 时**连 selector 一起记录**，确认它属于哪个变体；
2. **用截图交叉验证**：先找一个已知量校准截图的缩放比例（R68 用 Vega 自身的 32px 行高：实测 64px 行距 ÷ 32 = 2×），再按比例读其它量；
3. 两者矛盾时，**以产品截图为准**，并把这个矛盾写进报告。

## 架构红线（速记，详见 exec-guide）

- `vega_runtime` 禁止依赖 GPUI/任何 UI crate（headless 可测）
- 跨 crate 共享类型只放 `vega_conversation::types`
- API key 只存配置根下独立的 owner-only 明文凭据文件（R10），不写 config.toml/项目文件/日志；不访问旧 Keychain
- 非测试代码禁止 `unwrap()`/`expect()`
- schema 只增不删，走 `migrations/` 递增文件

## 行为技能

- [vega-kanban-delivery](.agents/skills/vega-kanban-delivery/SKILL.md)：看板取卡到关闭的工程交付流程；测试用例先行、真实 E2E 与持久截图、并行隔离、master 集成及清理的统一完成标准。

- [karpathy-guidelines](.agents/skills/karpathy-guidelines/SKILL.md)：写/审/重构代码时的
  行为准则（最小改动、表面化假设、可验证成功判据）。**冲突时以本文件与 exec-guide 为准**，
  适配边界见该 SKILL.md 的「Vega 适配」一节。canonical 在 `.agents/skills/`，
  `.claude/skills/` 为软链；新增 agent 入口时按同样方式链接。
