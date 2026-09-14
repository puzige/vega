# R62 实机验收报告（三行卡片 + 权限/工具栏下拉）

> 日期：2026-09-14
> 基线：`master @ 28f6c98`（R62a `a86bac3` + R62b 三个提交）
> 受测构建：`dist/Vega.app` → `/Applications/Vega.app`，可执行文件 `md5 f1c6f7e41fe78cbaa105480648a4ad69`
> 签名：`codesign --verify --deep --strict` 通过
> 证据目录：`/tmp/accept-*.png`

---

## §1 结论

R62 的两半（R62a 三行卡片、R62b 权限与工具栏下拉）**全部通过实机验收**。发现 **1 个新缺陷（UI-05，P1）**，与 R62 契约无关，已记入 `docs/vega-ui-backlog.md`。

| 验收项 | 结论 | 证据类型 |
|---|---|---|
| A9 权限 chip 打开三行下拉 | ✅ 通过 | 实机截图 |
| A10 选中项生效 | ✅ 通过 | 实机截图 |
| A11 `+` 菜单权限组一致 | ✅ 通过（生产测试） | 生产测试 |
| A12 项目/分支下拉列表结构 | ✅ 通过 | 实机截图 |
| A13 搜索过滤不改语义 | ✅ 通过（生产测试） | 生产测试 |
| A14 两个下拉视觉对齐 | ✅ 通过 | 实机截图 |
| A15 门禁 0 失败 | ✅ 1144 passed / 0 failed | 生产测试 |
| R62a 三行卡片 + 阴影分离 | ✅ 通过 | 实机截图 |
| R62a 二级下钻（档位名 → 模型列表） | ✅ 通过 | 实机截图 |
| UI-05 权限 chip 图标随模式变化 | ❌ 不通过（新缺陷）→ **已由 R63 修复并复验** | 实机截图 |

> **后续**：验收发现 UI-05（权限 chip 图标/颜色不随模式变化，P1）。已立项 R63 修复并合入 master（`3642047`），复验见 §7。

---

## §2 门禁结果（合并前，独立执行）

在 `feat/r62b-menus` rebase 到 `master` 之后、合并之前执行：

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 退出 0，无输出 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 退出 0 |
| `cargo test --workspace` | **1144 passed / 0 failed / 9 ignored** |

合并方式：`git merge --ff-only feat/r62b-menus`。合并后 `git diff master feat/r62b-menus` 为空，**受测提交与 master 内容逐字节一致**。

---

## §3 实机验收明细

### §3.1 权限 chip 三行下拉（A9 / A14）

点击底部控制行的 `⚠ 确认` 后，卡片出现在 chip 上方，内容为：

```
操作应如何获得批准?                          了解更多
──────────────────────────────────────────────────
✋  只读
    每次都询问
🛡  确认                                      ✓
    仅对潜在不安全操作询问            ← 浅灰圆角选中背景
⚠  自动
    不受限访问                        ← 橙色文字 + 橙色图标
```

逐条对照 R62 §7 的契约 R8：标题行 ✅、三行 ✅、每行"图标 + 标题 + 描述" ✅、当前选中项带勾选 ✅、第三项橙色 ✅。**契约全部满足。**

证据：`/tmp/accept-permission-picker.png`（整窗）、`/tmp/accept-permission-picker-crop.png`（放大）。

### §3.2 选中项生效（A10）

打开下拉 → 点 `只读` 行 → chip 文案由 `确认` 变为 `只读`，下拉关闭。**行为正确。**

证据：`/tmp/accept-perm-chip-readonly.png`。

> 这条同时暴露了 UI-05：chip 变成 `只读` 后仍是橙色警示三角，而 picker 里 `只读` 行是手掌图标。两个表面自相矛盾。

### §3.3 项目下拉（A12 / A14）

```
🔍 搜索项目
📁 r12-link...                               ✓    ← 浅灰圆角选中背景
📁 r13-alph...
📁 r13-beta...
📁 r11-nativ...
📁 sandbox
📁 r11-ope...
📁 r14-fold...
📁 r14-fold...
📁 vega-e2...
──────────────────────────────────────────────
+ 新建项目                                          ← 置灰
× 不关联项目
```

R10 的五个结构元素**全部具备**：搜索框 ✅、行图标 ✅、选中项勾选 + 背景 ✅、分隔线 ✅、额外操作项 ✅。

证据：`/tmp/accept-project-dropdown.png`、`/tmp/accept-project-dropdown-crop.png`。

### §3.4 分支下拉（A12 / A14）

```
🔍 搜索分支
⑂ r12-linked                                 ✓    ← 浅灰圆角选中背景
⑂ r12-live-check
```

搜索框 ✅、分支图标 ✅、选中项勾选 ✅。**通过。**

证据：`/tmp/accept-branch-dropdown.png`、`/tmp/accept-branch-dropdown-crop.png`。

### §3.5 模型卡片三行 + 阴影（R62a）

卡片内容自上而下：`low ›` → `glm-5.3-flash` → 滑块。**三行结构正确**，档位名与模型名是上下两级，**无闪电图标**（R62 用户决策）。

阴影：卡片外缘有可见的柔和投影，与下方 composer 卡片形成分离，视觉上"浮在上层"（R62 用户决策 B：允许重叠但必须浮起）。**通过。**

证据：`/tmp/d1.png`、`/tmp/d1-crop.png`。

### §3.6 二级下钻（R59 遗留未测项）

点击卡片内的档位名 `low ›` 后，弹出 `选择模型` 列表：

```
选择模型
glm-5.3-flash                                ← 蓝色（当前项）
deepseek-v4-flash
gpt-5.6-luna
```

**这一级此前从未在实机验证过，本轮补上并通过。**

证据：`/tmp/d3.png`、`/tmp/d3-crop.png`。

---

## §4 本轮发现的新缺陷

### UI-05 · 权限 chip 的图标与颜色不随模式变化（P1）

详见 `docs/vega-ui-backlog.md` 的 UI-05 条目（含根因代码位置与修复契约）。摘要：`render_permission_status` 硬编码 `Icon::Warning` + `colors.warning`，未使用 R62b 已建立的 `permission_icon(mode)` / `permission_is_warning(mode)` 投影，导致 chip 与 picker 对同一模式显示不同图标。

---

## §5 验收方法：本轮踩到的陷阱（重要）

本轮验收耗时远超预期，原因是**方法错误**，不是产品问题。记录如下，避免重复。

### §5.1 陷阱一：`pgrep -f '<path>'` 会匹配到自己

我用 `pgrep -f '/Applications/Vega.app/Contents/MacOS/vega'` 判断应用是否退出，得到持续的 "STILL RUNNING"。**假阳性**：该模式字符串出现在执行 pgrep 的 shell wrapper 自己的命令行里，pgrep 匹配到了那个 shell。

**正确做法**：`pgrep -x vega`（按精确进程名），或 `ps aux | grep '[V]ega'`（方括号技巧避免自匹配）。

### §5.2 陷阱二：ZCode 自己的窗口标题是 "Vega Desktop"

我的点击多次落在 **ZCode 客户端窗口**上而非 Vega 应用——ZCode 的窗口标题就是 `Vega Desktop`，且它的界面上也有 `Full access` 字样。两者外观相近，仅看截图无法立即分辨。

**正确做法**：用 `CGWindowListCopyWindowInfo` 按 **owner name + bundle id** 定位目标窗口，不要靠"屏幕中央那个窗口"的直觉。

### §5.3 陷阱三：窗口 ID 截图的阴影边距会变化

`screencapture -l <winid>` 会把窗口投影一并拍进去，且**边距随窗口阴影状态变化**：同一窗口我实测到 34 和 56 两种逻辑像素边距。用它做像素→坐标换算必然错位。

**正确做法**：用 `screencapture -R x,y,w,h` 按窗口 bounds 精确截图，输出严格是 2× 且无边距。

### §5.4 陷阱四：shell 往返会让终端窗口抢回焦点

`open -a Vega` 与随后的 `vclick` 若分属两次 Bash 调用，中间终端窗口会重新成为前台，点击就落到了错误的应用上。

**正确做法**：把「激活 → 等成为前台 → 点击 → 截图」写进**同一个进程**（本轮用 Swift 写了一个 `vdrive` 驱动：`NSRunningApplication` + `CGWindowListCopyWindowInfo` + `CGEventPost` + `screencapture -R`）。另外 `NSRunningApplication.activate()` 在本机被系统拒绝，须改用 `open -a`。

### §5.5 工具局限（既有记录，本轮再次确认）

`vega/AGENTS.md` 已记录：合成输入事件驱动不了 GPUI 的焦点链。本轮**未尝试**用合成键盘输入验证任何依赖焦点的行为，全部验收都是**鼠标点击 + 像素截图**，这类交互不受该局限影响。

**证据分级**（本轮严格执行）：

| 类别 | 本轮用于 |
|---|---|
| 生产测试已验证 | A11（`+` 菜单与 picker 一致）、A13（搜索过滤不改语义）、A15（门禁）、焦点/键盘导航 |
| 像素外观已验证 | A9、A10、A12、A14、R62a 三行布局与阴影、二级下钻 |
| **未验证** | 依赖焦点的样式（`focus_visible`）、键盘激活路径 |

---

## §6 证据文件清单

| 文件 | 内容 |
|---|---|
| `/tmp/accept-permission-picker.png` | 权限三行下拉（整窗） |
| `/tmp/accept-permission-picker-crop.png` | 权限三行下拉（放大） |
| `/tmp/accept-perm-chip-readonly.png` | UI-05 证据：`只读` 模式下仍是橙色警示三角 |
| `/tmp/accept-project-dropdown.png` | 项目下拉（整窗） |
| `/tmp/accept-project-dropdown-crop.png` | 项目下拉（放大） |
| `/tmp/accept-branch-dropdown.png` | 分支下拉（整窗） |
| `/tmp/accept-branch-dropdown-crop.png` | 分支下拉（放大） |
| `/tmp/d1.png` / `/tmp/d1-crop.png` | 模型卡片三行 + 阴影 |
| `/tmp/d3.png` / `/tmp/d3-crop.png` | 二级下钻：模型列表 |

驱动工具：已正式入库为 `scripts/native-drive.swift`（见 AGENTS.md「原生验收的操作方法」）。用法：

```
swiftc -O scripts/native-drive.swift -o /tmp/vdrive
/tmp/vdrive <out.png> <relX> <relY> [<relX> <relY> ...]   # 窗口内逻辑坐标，依次点击后截图
```

裁剪辅助：`sips -c <h> <w> --cropOffset <y> <x> <src> --out <dst>`（参数是**像素**，即窗口内逻辑坐标 ×2）。

---

## §7 UI-05 修复复验（R63）

> 修复提交：`3642047`（`feat/r63-perm-chip-icon` → ff 合入 master）
> 受测构建：`dist/Vega.app` → `/Applications/Vega.app`，可执行文件 `md5 d7709cf92fcf52d5016a203e0af060e0`
> 签名：`codesign --verify --deep --strict` 通过

### §7.1 修复内容

`render_permission_status` 原本硬编码 `Icon::Warning` + `colors.warning`，改为读取与 picker 相同的投影：`permission_icon(mode)` 与 `permission_is_warning(mode)`。非警示模式取 `colors.text_secondary`（与同一行的 `+` 按钮、模型触发器一致）。

### §7.2 独立门禁（合并前，我本人执行）

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 退出 0，无输出 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 退出 0 |
| `cargo test -p vega_ui` | 280 passed / 0 failed |
| `cargo test --workspace` | **1145 passed / 0 failed**（较 R62b 多 1，即新增的 R63 测试） |

### §7.3 测试非平凡性证伪（我本人执行）

把 `let glyph = permission_icon(mode);` 临时改回硬编码 `Icon::Warning` 后重跑新测试：

```
test ...r63_permission_chip_glyph_follows_the_mode ... FAILED
panicked at ...:295:5:
R63: a `confirm` thread's chip must paint the shield
test result: FAILED. 0 passed; 1 failed
```

**测试确实能抓住这个缺陷**，不是空跑。随后已还原代码（`git diff` 为空）。

### §7.4 三模式实机复验

| 模式 | chip 显示 | 结论 |
|---|---|---|
| `确认` | `🛡 确认`（灰色盾牌） | ✅ |
| `只读` | `✋ 只读`（灰色手掌） | ✅ |
| `自动` | `⚠ 自动`（橙色警示三角） | ✅ |

三种模式依次通过生产 picker 路径切换，chip 的图标与颜色均正确跟随。**UI-05 关闭。**

证据：`/tmp/accept-r63-confirm.png`、`/tmp/accept-r63-readonly.png`、`/tmp/accept-r63-auto.png`、`/tmp/accept-r63-auto-chip.png`。

### §7.5 未能验证的部分

chip 的**颜色值**只有像素证据（截图），没有生产测试断言——GPUI 测试平台不带 headless renderer（`render_to_target` 报 "no HeadlessRenderer configured"），图标经 sprite atlas 绘制，测试里读不到像素或解析后的文字颜色。测试断言的是**图标身份**（debug selector 标记，由 `permission_icon(mode)` 的返回值直接派生，因此标签与所画图标必然是同一个值），颜色规则则由构造保证：与 picker 行用的是同一个 `permission_is_warning` 调用。
