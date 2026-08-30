# ✦ Vega — 竞品设计参考（Electron 解包调研）

**日期** 2026-08-30 · **方法**：本机安装包 asar 解包（ZCode 3.10.1 / WorkBuddy 5.3.14），提取样式表与入口 HTML 做静态分析
**用途**：为 S6/S7/S8 的 UI 打磨与远期架构提供对照参考；不构成 spec 变更——采纳需走正常修订流程

---

## 1. 技术栈对照

| 项 | ZCode 3.10.1 | WorkBuddy 5.3.14 | Vega（现状） |
|---|---|---|---|
| 框架 | Electron + **React 19**，依赖仅 32 个（vendor 进 /out） | Electron + 组件化 CSS（agent-chat-pane/artifact/FileTabs 等按组件拆分） | GPUI（D1 锁定，非 Electron） |
| AI 层 | **Vercel AI SDK**（@ai-sdk/gateway）+ 内嵌 12.5MB agent CLI（zcode.cjs）+ MCP 插件包 | 内置插件系统：builtin-plugins/**skills** 目录（html-to-docx 等技能，与我们 agents-skills 同构） | 自研 Runtime（D6）+ ACP（Phase 2） |
| 数学公式 | KaTeX 全家 | KaTeX | 未做（远期需求信号：两家都带） |
| 品牌字体 | — | **Assistant**（woff2 四字重） | 系统字体（ui-spec §3） |

## 2. 设计 Token 对照（vs vega-ui-spec §2）

| Token | ZCode | WorkBuddy | Vega 现行 | 备注 |
|---|---|---|---|---|
| 背景 light | **#F8F8F8**（neutral-50，非纯白） | -apple-system 白 | #FFFFFF | ZCode 微灰底更柔；是否跟进属主观，S8 决策 |
| 背景 dark | **#161616**（neutral-900） | — | #1E1E1E | — |
| 卡片 | #FFF / #2B2B2B | — | #FFFFFF / #2D2D2D | 近似 |
| **边框** | **alpha 色**：light 黑 10%、dark 白 **6%**；hover 加深到 20%/30% | — | 实色 #E5E5E3 / #3A3A3A | **可借鉴**：alpha 边框在深浅两态过渡更细腻（S8） |
| 品牌强调 | sky-400/500 蓝 + accent 淡蓝底 | — | 黑白反差（§2 accent） | 不照搬：Vega 单色 accent 是既定设计基线 |
| 主题分层 | neutral 基础层 + `.theme-zai-light/dark` **品牌主题层**叠加 | VSCode 风格变量（--vscode-*） | 单层 14 token | 双层结构（基础/品牌分离）远期可借鉴 |
| 圆角 | xs .125 / sm .25 / md .375 / lg .5 / xl .75 rem（=2/4/6/8/12px） | — | 12px（composer）/8px（卡片） | 尺度对齐良好；可补 xs/sm 档 |
| 阴影 | **无 shadow token** | — | 无阴影（§4.2） | 两家一致：无阴影设计确认 |
| 动效 | 仅 spin/pulse/ping 三种工具动效 | — | 禁装饰动效（P6） | 哲学一致 |

## 3. 字体与 CJK（高价值发现）

| 项 | ZCode | WorkBuddy | Vega 现行 | 建议 |
|---|---|---|---|---|
| sans 栈 | ui-sans-serif, system-ui + emoji fallback | -apple-system 栈 | 系统字体（SF Pro） | 一致 |
| **mono 栈** | ui-monospace…**末尾接 "PingFang SC"/"Noto Sans CJK SC"** | Menlo, Monaco（14px） | SF Mono / JetBrains Mono 12.5px | **强烈建议采纳**：mono 栈尾部补 CJK fallback，解决代码块内中文注释/字符串的字体断裂（S6/S8） |
| 聊天面板 CJK | — | **'PingFang SC' 置于栈首** | 未显式 | 会话正文 CJK 优先可参考（S8） |
| 基础字号 | — | **13px** 基准 + 代码 14px | 13px 正文 / 12.5px 代码 | 一致 |

## 4. 架构与功能参考（远期）

1. **ZCode 内嵌 agent CLI**：Resources/glm/zcode.cjs（12.5MB 完整 CLI bundle）+ packages/（MCP 插件：browser-use-plugin、android-emulator-plugin）——桌面壳内嵌 headless agent 运行时的先例。Vega 的 vega_runtime headless 化（S4 已实现）未来可复用为嵌入式 CLI（远期）。
2. **WorkBuddy 插件 + skills 架构**：builtin-plugins/<name>/skills/<skill>/ 结构化技能包——Phase 3 插件系统（A8）设计时的直接参考。
3. **双主题层**：ZCode = 中性基础 token + `.theme-zai-*` 品牌层叠加——若 Vega 未来需要 OEM/主题市场，参考此分层（远期）。

## 5. 不建议照搬

- ZCode 的 sky 蓝品牌色与 accent 淡蓝底（Vega 黑白反差基线，ui-spec §2 accent 行是刻意选择）
- WorkBuddy 的 VSCode 变量命名（--vscode-*，属其 webview 集成包袱）
- 两家的 KaTeX 全家桶（等真实数学公式需求出现再评估，避免体积）

## 6. 行动项（映射到 Sprint）

| 行动项 | 目标 Sprint | 优先级 |
|---|---|---|
| mono 字体栈尾部补 CJK fallback（PingFang SC / Noto Sans CJK SC） | S6（diff/代码高亮时顺手）或 S8 | 高 |
| 边框改 alpha 色（light 黑 10% / dark 白 6%，hover 翻倍） | S8 主题打磨 | 中 |
| 补 xs/sm 圆角档（2px/4px）到 token | S8 | 低 |
| 会话正文 CJK 字体显式声明 | S8 | 低 |
| WorkBuddy 插件+skills 结构、ZCode 内嵌 CLI 架构存档 | Phase 2/3 设计输入 | 存档 |
