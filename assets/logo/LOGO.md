# ✦ Vega Logo — 设计理念与生成档案

追加比较（2026-09-06）：按用户要求完成[02修长星形切口稿](explorations/r16/vega-blue-02-slender-spark.png)，保留02整体轮廓及蓝色材质，内部单颗星孔更修长。此为追加预览，未替代01选择，继续保留为历史探索。

**最新设计状态（2026-09-08）**：用户批准将01单星微笑稿接入生产。R17正式资源为[浅色](vega-icon-r17-smile-light.svg)、[深色](vega-icon-r17-smile-dark.svg)和[单色](vega-symbol-r17-smile-mono.svg)：统一透明1024px画布、居中832px底板、四边96px留白，以及连接的 `>`、一颗修长曲边四角星和轻微上扬光标。正式SVG按统一几何手工维护，浅色使用宝石蓝、深色使用冰蓝；旧F1/F3、R16探索图和原始位图全部保留。

**R16历史选择（2026-09-06）**：用户选中01，明确只要一颗星，并希望整体略有笑脸感。[01单星微笑精修稿](explorations/r16/vega-blue-01-single-spark-smile.png)已交付：终端 `>`、单颗修长四角星与轻微上扬光标，保留蓝色和柔和材质。主Agent此前02推荐已被用户选择替代。R17生产矢量以此稿为构图和配色依据。

**当前设计方向**：B终端提示符＋导航星，浅色宝石蓝／深色冰蓝。[蓝色精修稿](explorations/r16/vega-logo-b-macos-terminal-blue.png)· 2026-09-06。用户的新配色选择优先于下述绿色历史记录；符号、高光和柔和材质保留。

**历史生产资产**：F1（浅色主标）+ F3（深色变体）· 2026-08-29；文件保留用于回退和演进对照。

2026-09-06：已完成[R16三方向探索](explorations/r16/README.md)。用户选择第二版B（终端折角＋导航星），并要求结合macOS设计风格；[B精修稿](explorations/r16/vega-logo-b-macos.png)采用柔白/石墨底板、祖母绿/薄荷绿图形及克制的分层、高光和玻璃质感。初轮A推荐已被用户选择替代，后续以B为准。该稿和下列F1/F3均保留为历史定稿与材质/回退参考，R17已替换生产安装图标。

## 设计理念

R16最新反馈：用户认可macOS精修的高光和柔和配色，要求增强左侧终端识别。[最新预览](explorations/r16/vega-logo-b-macos-terminal.png)使用连续 `>` 和短横光标，保留右侧导航星及已认可材质，后续图形以此版为当前迭代方向。

符号 = **终端提示符 `>` / `>_` 与四角 AI 星芒（✦ sparkle）的融合**：

- `>` — 开发者工具身份、agent 执行的命令行本源
- ✦ — 北极星（Vega）+ 当下 AI 能力的通用视觉语言
- 融合手法：星芒的左顶点与 chevron 的右顶点共享同一锚点，一体成型
- 当前色板：浅色宝石蓝 `#3478D8`（暗部 `#245AAF`）/ 深色冰蓝 `#8FC7FF`（暗部 `#609DE1`），分别置于柔白和石墨底板。
- 历史色板：祖母绿 `#1E9168 → #0F6B47`（F1）/ 薄荷绿 `#7FE3A5`（F3），深色底 `#201E1B`

**风格决策记录**：初稿曾探索 macOS 27 Liquid Glass（多层玻璃折射）方向，后主动放弃——第三方 app 图标生态仍是经典扁平 squircle（Dock 实证：Telegram/WeChat/VS Code 等均未用玻璃材质）。定稿为「经典 macOS 第三方图标」：轻渐变 + 微质感、无玻璃透明。

## 文件清单

| 文件 | 用途 |
|---|---|
| `vega-icon-r17-smile-light.svg` | **当前生产主图标**（柔白底板、宝石蓝单星笑脸），AppKit/ICNS源 |
| `vega-icon-r17-smile-dark.svg` | 当前生产深色变体（石墨底板、冰蓝单星笑脸） |
| `vega-symbol-r17-smile-mono.svg` | 当前生产单色符号，`currentColor`，菜单栏/wordmark通用 |
| `vega-icon-f1-light.svg` | 历史主图标（白底祖母绿），回退/演进对照 |
| `vega-icon-f3-dark.svg` | 历史深色变体（黑底薄荷绿 `>_`），回退/演进对照 |
| `vega-symbol-mono.svg` | 历史 `currentColor` 单色符号，演进对照 |
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

- [x] R15：直接从 F1 SVG 通过 AppKit 导出无水印透明画布，生成 `.iconset` 全套（16/32/128/256/512 + @2x）及 ICNS。该流程和尺寸基线由R17继续复用，见 [R15 导出证据](../../docs/vega-r15-icon-delivery.md)。
- [x] R17：从单星微笑浅色SVG通过AppKit导出无水印透明画布，保留居中832px底板、四边96px透明留白，并将生产源切换到 `vega-icon-r17-smile-light.svg`。浅/深/单色SVG共用同一几何，见 [R17 导出证据](../../docs/vega-r17-logo-delivery.md)。
- [ ] wordmark 横版（mono symbol + "Vega" 字标）
- [ ] README 顶图 / 社交分享图（可用落选极光版改造）
