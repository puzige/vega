# ✦ Vega — UI 规格与验收准线（UI Spec）

**版本** v0.11 · 2026-09-10 · 关联：[vega-features.md](vega-features.md)

> **规范分层**：跨任务的视觉语言、语义 token 与新 UI 默认规则以
> [Vega 设计守则](vega-design-guidelines.md)为入口；本文件继续承载组件行为和
> 可测量验收。遇到历史数值冲突时，以更新、更具体的冻结规格为准，并在实施任务中同步修订。

> **设计基线决策**：UI 风格对齐 **Codex Desktop / ZCode 默认风格**——极简、留白充足、浅灰层次、无重边框、内容居中；Vega Logo 的宝石蓝/冰蓝保留给主操作、焦点和 Agent/AI 品牌语义。普通导航的 hover/selected 均使用中性灰，不用品牌色标记当前位置。
> 本文件是验收准线：每条都可检查、可测量。S 级 Sprint 验收时逐条过。

> 当前 R8 实测视觉基线见 [R8 ZCode parity](vega-r8-zcode-parity.md)。该任务覆盖下列旧 R4 的几何、浅色中性 token、空态和 composer 分组；安全、数据与性能条款保留。

---

## 1. 布局解剖（Layout）

```
┌────────────────────────────────────────────────────────┐
│ Sidebar (304px*) │  Thread View (flex, max 820px 居中) │
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
| 侧边栏宽度 | 默认 304px，可拖拽于 240–365px，可折叠至 0（Cmd+B）；宽度与折叠状态分别记忆 |
| 主 Header | 46px；底部 1px 分隔线 |
| 会话内容列 | max-width 820px，水平居中，左右留白 ≥16px |
| Composer | 底部固定，max-width 736px、min-height 100px、bottom inset 16px；圆角 20px，边框 1px + 单层克制阴影；发送/停止 28×28px |
| Environment | 宽屏 rail 320px，内含 304px 卡片与 16px 右 inset，圆角 18px；默认 Sidebar 下 1229/1230px 切换 overlay/rail，断点随有效 Sidebar 宽度一对一移动 |
| 窗口最小尺寸 | 960×600；Sidebar 用户折叠、宽度驱动自动折叠与 Environment 选择彼此独立 |
| 触控栏/标题栏 | 原生 macOS 标题栏透明融合（traffic lights 内嵌），不自绘 |

## 2. 色彩 Token（Light / Dark 双套）

| Token | Light | Dark | 用途 |
|---|---|---|---|
| `bg-base` | #FFFFFF | #202020 | 主区背景 |
| `bg-sidebar` | #FAF9F9 | #191919 | 侧边栏背景 |
| `bg-elevated` | #FFFFFF | #2A2A2A | 卡片/composer |
| `bg-hover` | #F3F3F3 | #282828 | 中性悬停态 |
| `bg-active` | #EDEDED | #303030 | 中性选中态（当前会话/项目/普通导航） |
| `border-subtle` | #E8E8E8 | #383838 | 1px 分隔线/卡片边 |
| `text-primary` | #191C1F | #EDEDED | 正文 |
| `text-secondary` | #676767 | #ABABAB | 辅助信息/时间戳 |
| `text-tertiary` | #8A8A8A | #828282 | 占位符 |
| `accent` | #3478D8 | #8FC7FF | 主按钮、主要选中态（Logo sapphire / ice blue） |
| `success` | #1A7F37 | #3FB950 | 工具成功态、diff 新增 |
| `danger` | #CF222E | #F85149 | 错误态、diff 删除、危险操作 |
| `warning` | #9A6700 | #D29922 | 权限确认、预算告警 |
| `code-bg` | #F6F6F6 | #262626 | 代码块背景 |

R18 品牌补充 token：`brand-primary` = `#3478D8` / `#8FC7FF`，
`brand-primary-strong` = `#245AAF` / `#609DE1`，`brand-soft` =
`#EAF2FC` / `#203247`，`brand-on-accent` = `#FFFFFF` / `#13233A`。
它们均由 `vega_theme::ThemeColors` 提供，组件禁止写死色值。详见
[R18 品牌 UI 与 Icon 基线](vega-r18-brand-ui.md)。

> diff 遵循国际惯例（绿增红删）；这不是股票场景。所有颜色必须走 token，禁止组件内写死色值（验收时 grep 检查）。

## 3. 字体排版（Typography）

| 项 | 规格 |
|---|---|
| 正文字体 | 系统字体（SF Pro），13px/1.55 行高 |
| 会话消息正文 | 15px/1.65 |
| 代码字体 | SF Mono / JetBrains Mono，12.5px，等宽对齐 |
| 侧边栏条目 | 13px，行高 32px，超出省略号 |
| 标题层级 | 仅三级：页面 16px 600 / 区块 14px 600 / 卡片 13px 500 |
| 空态主标题 | 28px / 600 |
| 说明 / 元数据 | 12px |
| CJK 混排 | 中英文之间自动 1/4 字距（盘古之白）；CJK 渲染无豆腐块（验收用混排样本文本） |

## 4. 核心组件规格

### 4.1 侧边栏会话条目
- 单行：会话标题（省略号截断）+ 右侧相对时间（"2h"）；选中态 `bg-active` + `text-primary`，不使用品牌色标题或重色左边条
- 未读：标题 500 字重 + 右侧圆点
- 项目分组可折叠，折叠状态记忆；项目行不显示独立 disclosure Chevron，使用 Folder / FolderOpen 图标直接表达收起与展开
- 项目、会话和 Settings 导航行使用 8px 圆角；Settings 保持 32px 高并位于 Sidebar 的 12px 左右/底部 inset 中

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
- 多行自适应（1~8 行，超出内滚）；placeholder `描述任务，或用 @ 引用文件`
- 主输入区在上；下方只有一行真实操作：添加上下文、Ask/Plan/Execute、权限（只读 / 确认 / 自动）、模型、thinking 与 send/stop；空间不足时允许自适应收紧或换行
- 模式与权限当前值持续可见；菜单、键盘路径、loading/error 和提交 guard 继续使用真实 controller 状态
- branch 只在 project-backed task 的 Environment 中显示，并使用 live branch projection；普通 Composer 不重复放分支选择器
- 普通 Composer 不显示常驻 token / 成本仪表；用量只在拥有真实计数或账单来源的专门界面中展示

### 4.5 Diff 视图
- 统一视图（unified）默认，可切左右分栏
- 新增行 `success` 8% 透明度底色，删除行 `danger` 8% 底色；行号 `text-tertiary`
- hunk 头 `@@` 行 `code-bg` 背景

### 4.6 空态 / 加载态 / 错误态
- 空会话：居中显示“想在这个项目里完成什么？”与真实的项目选择 / 添加 / 新建入口，不显示无功能模板或大 logo 插画
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
| P7 | 冷启动到首屏可交互 <50ms（KPI）——测量语义冻结于 [vega-s8-sdd.md](vega-s8-sdd.md) §2/C1：`process_start_to_first_rendered_interactive`，20 进程 nearest-rank p95 <50.000ms，next-frame 语义（≠ 物理 present） | bench（C1 协议） |
| P8 | 空闲内存 <100MB（无任务、单窗口）——测量语义冻结于 [vega-s8-sdd.md](vega-s8-sdd.md) §3/C2：release RSS raw bytes（`proc_pidinfo pti_resident_size`），20 进程 +5/+10/+15s median 的 nearest-rank p95；阈值单位 OPEN(OWNER: human)，裁决前字面权威 100,000,000 bytes（decimal MB），裁决后测后永不换 | bench（C2 协议） |

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
- v0.4 (2026-08-31) S8-T42 契约冻结回写：§5 P7/P8 测量语义指向 [vega-s8-sdd.md](vega-s8-sdd.md) C1/C2；P8 阈值单位为 OPEN(OWNER: human)（裁决前按 decimal MB 字面权威，见 SDD §3.1/§10）。
- v0.5 (2026-09-05) R4 客户端 UI 翻新：依据 [R4 客户端 UI 翻新 SDD](vega-ui-refresh-sdd.md) 同步 Codex / ChatGPT 工作区式层级、Light/Dark token、34px 侧栏行高、15px/1.65 会话排版、20px Composer、两行控件分组与真实空态；S3 演示、跟随诊断和无功能模板移出普通产品流，安全、controller、性能冻结条款保持不变。
- v0.6 (2026-09-08) R18 品牌 UI 基线：采用批准的 R17 蓝色终端/单星/微笑光标作为产品 UI 与 icon 的几何和强调来源；新增品牌 token，active/selection 改为低对比蓝灰，统一 16px、1.45px 柔角 vector icon；success/danger/warning 语义色与 R15 IA 保持不变。
- v0.7 (2026-09-10) R20 设计守则收口：以 [Vega 设计守则](vega-design-guidelines.md)、[R19 主窗口壳层](vega-r19-codex-parity.md)与 `vega_theme` 当前值校正 Light 表面色、32px Sidebar、28px 空态、壳层几何及单操作行 Composer；外部解包 token 不作为输入。
- v0.8 (2026-09-10) R21 当前截图对标：Sidebar 改为默认 304px、240–365px 可拖拽并独立记忆；主壳层采用平直分栏，Environment rail 320px，Settings 使用同宽导航 rail 与 744px 内容列；Composer 冻结 736px 与 28px 发送/停止控件。精确状态与证据见 [R21 screenshot parity](vega-r21-screenshot-parity.md)。
- v0.9 (2026-09-10) R23 Sidebar footer 修正：常驻设置入口使用完整 32px 行高并铺满 Sidebar 的 12px 内边距内容列；不得叠加额外水平缩进或固定宽度。精确验收见 [R23 Sidebar footer fill](vega-r23-sidebar-footer-fill.md)。
- v0.10 (2026-09-10) R24 Sidebar footer 复验修正：用户确认 R23 视觉未解决；设置入口保持 32px，但 hover/点击面改为与 Sidebar 左右及底部 edge-to-edge，普通内容继续保留 12px inset。精确验收见 [R24 Sidebar footer edge-to-edge](vega-r24-sidebar-footer-edge-to-edge.md)。
- v0.11 (2026-09-10) R25 Sidebar 导航复验修正：普通 hover/selected token 改为中性灰；项目行去除独立 Chevron 并以 Folder open/closed 表达展开状态；导航行圆角统一为 8px；Settings 撤销 edge-to-edge 条带并回归 12px inset。精确验收见 [R25 Sidebar navigation](vega-r25-sidebar-navigation.md)。
- v0.12 (2026-09-10) R26 Sidebar 分组与渐进显现：生产投影固定为 `PINNED / PROJECTS / RECENTS`，任务只出现一次；section、project 与 task 的辅助操作仅在所属区域 hover、键盘 focus 或菜单打开时可见，且保持既有 hitbox 与无布局跳动。精确验收见 [R26 Sidebar sections](vega-r26-sidebar-sections-hover.md)。
- v0.13 (2026-09-11) R27 Sidebar 行栅格修正：Pinned 行移除重复 Pin 图标与空槽；项目 Folder 到名称使用 8px gap；项目名称与所有任务标题统一 32px 内容 inset；Pinned 项目元数据固定 85px 列宽。精确验收见 [R27 Sidebar row alignment](vega-r27-sidebar-row-alignment.md)。
