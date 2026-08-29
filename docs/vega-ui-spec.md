# ✦ Vega — UI 规格与验收准线（UI Spec）

**版本** v0.3 · 2026-08-30 · 关联：[vega-features.md](vega-features.md)

> **设计基线决策**：UI 风格对齐 **Codex Desktop / ZCode 默认风格**——极简、留白充足、浅灰层次、无重边框、内容居中。不发明新设计语言，把精力放在渲染性能和工具卡片信息密度上。
> 本文件是验收准线：每条都可检查、可测量。S 级 Sprint 验收时逐条过。

---

## 1. 布局解剖（Layout）

```
┌────────────────────────────────────────────────────────┐
│ Sidebar (260px)  │  Thread View (flex, max 820px 居中) │
│  - 新建任务       │   ┌──────────────────────────┐     │
│  - 搜索           │   │ 消息流（滚动区）           │     │
│  - 自动化(P3)     │   │  - 用户消息                │     │
│  - 项目列表       │   │  - Agent 消息              │     │
│    - 会话列表     │   │  - 工具卡片                │     │
│  - 用户/设置      │   ├──────────────────────────┤     │
│                  │   │ Composer（底部固定）        │     │
│                  │   └──────────────────────────┘     │
└────────────────────────────────────────────────────────┘
```

| 项 | 规格 |
|---|---|
| 侧边栏宽度 | 260px，可折叠至 0（Cmd+B）；折叠状态记忆 |
| 会话内容列 | max-width 820px，水平居中，左右留白 ≥24px |
| Composer | 底部固定，与会话列同宽；圆角 12px，边框 1px（不使用阴影堆叠） |
| 窗口最小尺寸 | 960×600；小于此时侧边栏自动折叠 |
| 触控栏/标题栏 | 原生 macOS 标题栏透明融合（traffic lights 内嵌），不自绘 |

## 2. 色彩 Token（Light / Dark 双套）

| Token | Light | Dark | 用途 |
|---|---|---|---|
| `bg-base` | #FFFFFF | #1E1E1E | 主区背景 |
| `bg-sidebar` | #F7F7F5 | #252525 | 侧边栏背景 |
| `bg-elevated` | #FFFFFF | #2D2D2D | 卡片/composer |
| `bg-hover` | #EFEFED | #383838 | 悬停态 |
| `bg-active` | #E9E9E7 | #404040 | 选中态（当前会话） |
| `border-subtle` | #E5E5E3 | #3A3A3A | 1px 分隔线/卡片边 |
| `text-primary` | #1A1A1A | #ECECEC | 正文 |
| `text-secondary` | #6B6B6B | #9C9C9C | 辅助信息/时间戳 |
| `text-tertiary` | #9E9E9E | #6B6B6B | 占位符 |
| `accent` | #1A1A1A | #ECECEC | 主按钮（黑底白字/反色） |
| `success` | #1A7F37 | #3FB950 | 工具成功态、diff 新增 |
| `danger` | #CF222E | #F85149 | 错误态、diff 删除、危险操作 |
| `warning` | #9A6700 | #D29922 | 权限确认、预算告警 |
| `code-bg` | #F6F8FA | #282C34 | 代码块背景 |

> diff 遵循国际惯例（绿增红删）；这不是股票场景。所有颜色必须走 token，禁止组件内写死色值（验收时 grep 检查）。

## 3. 字体排版（Typography）

| 项 | 规格 |
|---|---|
| 正文字体 | 系统字体（SF Pro），13px/1.55 行高 |
| 会话消息正文 | 14px/1.6 |
| 代码字体 | SF Mono / JetBrains Mono，12.5px，等宽对齐 |
| 侧边栏条目 | 13px，行高 32px，超出省略号 |
| 标题层级 | 仅三级：页面 16px 600 / 区块 14px 600 / 卡片 13px 500 |
| CJK 混排 | 中英文之间自动 1/4 字距（盘古之白）；CJK 渲染无豆腐块（验收用混排样本文本） |

## 4. 核心组件规格

### 4.1 侧边栏会话条目
- 单行：会话标题（省略号截断）+ 右侧相对时间（"2h"）；选中态 `bg-active` + 左侧 2px 强调条
- 未读：标题 500 字重 + 右侧圆点
- 项目分组可折叠，折叠状态记忆

### 4.2 工具调用卡片（信息密度核心）
```
┌─ ⚙ bash · 已完成 · 1.2s ─────────────── [展开▾] ┐
│ $ cargo test --workspace                        │
│ （折叠时仅显示命令一行，输出默认收起）              │
└──────────────────────────────────────────────────┘
```
- 状态色：执行中=旋转指示器+`text-secondary`，成功=`success` 图标，失败=`danger` 图标+退出码
- 写操作卡片头部显示 `路径 +12/-3`，点击展开内嵌 diff
- write/edit 卡只消费 tech-spec §2 的 strict 安全成功/失败投影：成功显示规范项目相对路径、bytes_written 与 edit replacements 摘要，不显示 checkpoint ref；失败只显示稳定、脱敏 code/message。missing/extra/wrong-type、非法 u64/replacements/ref 必须 fail closed 为损坏结果，禁止从 raw provider input、绝对 checkpoint path 或 preimage 补数据
- invalid write/edit 显示 rejected 工具卡与 stable validation code，不生成权限卡，不显示/保留 raw path、body 或 JSON
- 卡片间距 8px，圆角 8px，边框 1px `border-subtle`，无阴影

### 4.3 权限确认卡片
- `warning` 左侧 3px 竖条；操作描述 + 命令全文（等宽）
- 按钮三枚：[允许一次]（主按钮） [总是允许] [拒绝]；拒绝可附言输入
- 普通卡键盘：初始焦点 [允许一次]；Enter=允许一次，Cmd+Enter=总是允许，Esc=拒绝
- 危险命令卡 override（2026-08-30 人类裁决）：初始焦点必须是 [拒绝]；Tab/Shift+Tab 在三按钮间双向循环；Space 激活当前焦点按钮（包括 [允许一次]）。bare Enter 无论当前焦点在哪都必须拒绝，Cmd+Enter=总是允许当前次并保存 exact rule，Esc=拒绝。危险 always 不跳过下次危险确认，卡片须明确提示
- key binding 仅在当前权限卡 scoped context 生效；重复按键只提交一次。卡片消失、线程切换、窗口关闭或 10 分钟超时均视为拒绝，绝不隐式批准

### 4.4 Composer
- 多行自适应（1~8 行，超出内滚）；placeholder `text-tertiary`
- 工具条（底部一行）：[+] [@引用] [权限模式] ··· [模型选择器] [发送]
- 模式胶囊：Ask/Plan/Execute 三态 segmented control，状态全局可见（不只藏在菜单）
- token 计数器：右下角常驻 `12.4k tok · ¥0.17`，流式期间实时跳动

### 4.5 Diff 视图
- 统一视图（unified）默认，可切左右分栏
- 新增行 `success` 8% 透明度底色，删除行 `danger` 8% 底色；行号 `text-tertiary`
- hunk 头 `@@` 行 `code-bg` 背景

### 4.6 空态 / 加载态 / 错误态
- 空会话：居中引导语 + 快捷模板按钮（对标 ZCode 快捷任务），不显示大 logo 插画
- 加载：骨架屏（不转全屏 spinner）
- 错误：内联条（`danger` 图标 + 描述 + [重试]），不弹模态

## 5. 动效与性能准线（可测量）

| # | 准线 | 测量方式 |
|---|---|---|
| P1 | 万行会话滚动稳定 120fps（允许瞬时不低于 100fps） | `xtask bench` 帧率直方图 |
| P2 | 流式 token 上屏延迟 <16ms（收到→渲染） | bench 埋点 |
| P3 | 流式追加时，已渲染区域**零重排**（无视觉跳动） | 走查 + 帧对比测试 |
| P4 | 滚动锚定：贴底时自动跟随；用户上翻>1 屏后不再自动跳转，回到底部恢复 | 走查 |
| P5 | 所有交互反馈 <100ms（点击、折叠、切换会话） | 走查 |
| P6 | 动效仅用于：卡片展开/收起（150ms ease-out）、权限卡片滑入（120ms）。禁止装饰性动画 | 走查 |
| P7 | 冷启动到首屏可交互 <50ms（KPI） | bench |
| P8 | 空闲内存 <100MB（无任务、单窗口） | bench |

## 6. 验收 Checklist（每个 Sprint 末过一遍）

- [ ] 颜色/字体全部来自 token，无硬编码（`rg "#[0-9a-fA-F]{6}" crates/vega_ui` 白名单除外）
- [ ] Light/Dark 切换无闪烁、无遗漏组件
- [ ] CJK 混排样本文本渲染正确
- [ ] 键盘全可达：不碰鼠标完成「建会话→发消息→批准权限→看 diff→提交」全流程
- [ ] 960×600 最小窗口无布局破裂
- [ ] P1-P8 性能准线达标
- [ ] 与 Codex/ZCode 并排截图对比：信息密度与视觉风格不违和（走查项）

---

## 变更记录

- v0.1 (2026-08-28) 初版定稿。
- v0.2 (2026-08-30) S5 安全裁决回写：§4.2 补 invalid write/edit 的脱敏 rejected card；§4.3 区分普通/危险权限卡默认焦点与 Enter 语义，危险卡补 Tab/Shift+Tab 焦点循环、Space 激活焦点，并固定 bare Enter 在任意焦点均拒绝；两类卡保留 Cmd+Enter/Esc 及重复提交、超时与视图销毁的 fail-closed 行为。
- v0.3 (2026-08-30) 人类批准 S5 wire schema 回写：§4.2 固定 write/edit 工具卡只消费 strict 安全成功/失败投影，隐藏 checkpoint ref，并对损坏 shape fail closed。
