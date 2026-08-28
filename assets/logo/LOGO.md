# ✦ Vega Logo — 设计理念与生成档案

**定稿版本**：F1（浅色主标）+ F3（深色变体）· 2026-08-29

## 设计理念

符号 = **终端提示符 `>` / `>_` 与四角 AI 星芒（✦ sparkle）的融合**：

- `>` — 开发者工具身份、agent 执行的命令行本源
- ✦ — 北极星（Vega）+ 当下 AI 能力的通用视觉语言
- 融合手法：星芒的左顶点与 chevron 的右顶点共享同一锚点，一体成型
- 色板：祖母绿 `#1E9168 → #0F6B47`（F1）/ 薄荷绿 `#7FE3A5`（F3），深色底 `#201E1B`

**风格决策记录**：初稿曾探索 macOS 27 Liquid Glass（多层玻璃折射）方向，后主动放弃——第三方 app 图标生态仍是经典扁平 squircle（Dock 实证：Telegram/WeChat/VS Code 等均未用玻璃材质）。定稿为「经典 macOS 第三方图标」：轻渐变 + 微质感、无玻璃透明。

## 文件清单

| 文件 | 用途 |
|---|---|
| `vega-icon-f1-light.svg` | 主图标（白底祖母绿），Dock/浅色场景 |
| `vega-icon-f3-dark.svg` | 深色变体（黑底薄荷绿 `>_`），深色 Dock/营销 |
| `vega-symbol-mono.svg` | `currentColor` 单色符号，菜单栏/wordmark 通用 |
| `raster/vega-icon-f1-original.png` | **F1 原始 AI 生成图（后续迭代的底图，勿删）** |
| `raster/vega-icon-f3-original.png` | F3 原始 AI 生成图（同上） |

SVG 为手工矢量重绘（非自动描摹），形状微调直接改 path 坐标。

## 生成 Prompt 档案（复现/迭代用）

> 生成工具：WorkBuddy ImageGen（当前走 hunyuan，右下角带「AI 生成」水印——定稿图标使用 SVG 矢量版不受影响；如需重新生成位图，换模型时用下列 prompt）。尺寸均 1024×1024，quality high。

**概念 3（中选方向，终端+星融合）**：
```
macOS app icon, flat minimal vector design: a four-pointed north star whose left half is formed by a terminal command prompt chevron symbol (>) merged seamlessly into the star shape, white and soft green accent on a dark slate rounded square (squircle) background, clever negative space, coding tool identity, no text, crisp geometric flat style
```

**F1 定稿（经典 macOS 图标风，白底绿标）**——以概念 3 为底图 image-to-image：
```
Redesign this app icon in classic macOS third-party app icon style (like Telegram, VS Code, Antigravity icons in a Mac Dock): keep the exact same symbol — terminal chevron (>) fused with a four-pointed sparkle star — in solid emerald green with subtle vertical gradient and very soft drop shadow, on a clean white-to-light-gray subtly rounded square (squircle) with a faint inner edge highlight, flat design with only a hint of depth, NO glass transparency effects, crisp vector look, no text, no watermark
```

**F3 定稿（深色开发者工具风，黑底薄荷绿）**——同上底图：
```
Redesign this app icon in classic macOS third-party developer-tool app icon style (like Zed editor, iTerm, dark-themed Mac Dock icons): keep the exact same symbol — terminal chevron (>) fused with a four-pointed sparkle star — in bright mint-green with subtle glow-free flat finish, centered on a near-black charcoal rounded square (squircle) with very subtle dark vertical gradient, flat modern design, NO glass transparency effects, crisp vector look, no text, no watermark
```

**落选方向存档**：①折纸星（白青渐变四角星+伴星）；②轨道星芒（极简线条+轨道环）；Liquid Glass 三版（深色玻璃/浅色玻璃/极光渐变——风格被否但极光配色青→紫→绿可留作未来营销素材）。

## TODO

- [ ] 换非 hunyuan 模型重新生成无底标位图（或直接用 SVG 导出）→ `.iconset` 全套（16/32/128/256/512 + @2x）
- [ ] wordmark 横版（mono symbol + "Vega" 字标）
- [ ] README 顶图 / 社交分享图（可用落选极光版改造）
