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
3. **任务来源**：`docs/vega-s*-tasks.md` 任务卡（如 T01-T08）。一张卡 = 一个 PR。卡外工作先问。
4. **主 agent 角色**：协调、验收、集成；**代码实现委托给专用 subagent**，主上下文不被实现细节污染。
5. **遇阻**：按 exec-guide §6 用 `[BLOCKED]` 格式上报，禁止自创方案绕过。
6. **验收强制 E2E-first**：优先以真实 production 入口、owned temp repo 与真实 controller 的端到端证据验收；test-only seam 仅保留无法由 E2E 稳定证明的安全不变量，证据分级与留存格式见 [exec-guide §7](docs/vega-exec-guide.md#7-验收协议每个任务卡通用)。

## 提交与 PR

- 提交格式：`feat(A2-09): <一句话>` / `fix(A3-07): <一句话>`（功能点 ID 见 [vega-features.md](docs/vega-features.md)）
- 小步提交，一个任务卡 ≤3 个 commit
- PR 必须附：验收命令原始输出 + 与 spec 的偏离说明（必须为无）
- 合并方式：squash merge，合并后删除功能分支（2026-08-29 决策）

## 验收底线（本地 hooks 强制；云端 CI 延后）

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

门禁由本地 git hooks 执行（`.githooks/`，见 [vega-s1-tasks.md](docs/vega-s1-tasks.md) T03；一次性安装 `git config core.hooksPath .githooks`）。
外加 exec-guide §3 红线检查（`cargo tree` 依赖方向、色值硬编码 grep 等）。

## 共享构建缓存（新 worktree 必做）

**本仓库所有 worktree 共用一个 `target/` 目录。** 主检出保留真实目录作为构建缓存，其余 worktree 的 `target` 是指向它的符号链接。这样新 worktree 的首次构建是增量的，而不是从零编译 900+ 依赖。

实测（2026-09-13，`cargo check --workspace`）：

| 场景 | 耗时 |
|---|---|
| 全新 worktree，独立 `target/` | 52 s |
| 全新 worktree，接上共享 `target/` | **6 s** |
| 改一行后重新 check | 0.8 s |

**新建 worktree 后必须接上，否则第一次构建要等十几分钟：**

```sh
# 在主检出里跑（脚本会把所有 worktree 都接上，不只是新建的那个）
git worktree add -b feat/<task-id>-<slug> ../vega-<slug> master
scripts/cargo-share-target.sh
```

`scripts/cargo-share-target.sh` 幂等，随时可重跑。无参数时作用于当前仓库；也可显式传仓库路径。`--status` 看当前接线，`--unshare` 恢复独立目录。

**代价与约束见下一节**——共享 target 意味着同一时间只能有一个 worktree 在构建或测试。

## 并发构建与测试：同一仓库同时只跑一个

**本仓库所有 worktree 共用一个 `target/` 目录**（见上一节）。代价是**同一时间只能有一个 worktree 在构建或测试**。

cargo 的独占锁**只覆盖编译阶段**，测试二进制一旦构建完成就在锁外执行。所以两个 worktree 的测试套件可以真的同时跑，已实测到两种故障：

1. **产物串味**：一个 worktree 源码构建出的 crate 被另一个源码不同的 worktree 复用，报出源码里明明存在的符号找不到（实测 `E0599`）。普通重建即可恢复，但极易误判为真 bug。
2. **共享状态竞争**：触碰全局状态的测试偶发失败、重跑就过（实测 3 个 `trusted_git` 测试争抢 git 全局配置）。

**因此：构建/测试前先取锁。**

```sh
scripts/cargo-lock.sh test --workspace        # 被占用时快速失败并打印持有者
scripts/cargo-lock.sh build --workspace --all-targets
scripts/cargo-lock.sh --status                # 当前谁在跑
scripts/cargo-lock.sh --wait test --workspace # 明确选择排队等待
scripts/cargo-lock.sh --release               # 清理残留锁（进程已死时）
```

锁标记放在 `git rev-parse --git-common-dir` 下，该路径在**所有 worktree 中解析为同一处**，因此天然是仓库级共享的。进程崩溃留下的锁会在下次取锁时被自动识别并清理。

**默认快速失败，不静默排队**——静默排队会让 agent 看起来卡死；明确报错才能让它去干别的活，或显式改用 `--wait`。

> 如果某个 worktree 的 `target/` 不是指向共享目录的符号链接（独立构建），它不受此约束。用 `scripts/cargo-lock.sh --status` 之外的判断依据是：该 worktree 下 `target` 是否为符号链接。

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

## 原生验收的操作方法：五个必踩的坑（2026-09-14 实测）

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

## 架构红线（速记，详见 exec-guide）

- `vega_runtime` 禁止依赖 GPUI/任何 UI crate（headless 可测）
- 跨 crate 共享类型只放 `vega_conversation::types`
- API key 只存配置根下独立的 owner-only 明文凭据文件（R10），不写 config.toml/项目文件/日志；不访问旧 Keychain
- 非测试代码禁止 `unwrap()`/`expect()`
- schema 只增不删，走 `migrations/` 递增文件
