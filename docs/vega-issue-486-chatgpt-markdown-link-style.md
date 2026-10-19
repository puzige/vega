# Issue #486 · Markdown 链接样式：ChatGPT App 的实现与效果

来源：[Issue #486](https://github.com/puzige/vega/issues/486)（「[Bug] Markdown链接样式优化」）。正文给了两张截图（GitHub issue 会话正文、ChatGPT 里的 GitHub issue 预览卡），并列出三个待答问题：hover 才显示下划线、GitHub logo 怎么展示、codex 预览 block 怎么实现。

本页只做**取证与结论**：回答三个问题、给出 ChatGPT App 的真实实现规则，并与 Vega 现状对照。不含实现改动，也不改变 #357/#455 已冻结的链接契约；是否落地另开卡片。

## 取证方式与边界

- 对象：本机 `/Applications/ChatGPT.app`（`CFBundleShortVersionString = 26.1013.40934`）内的 Electron 前端产物 `Contents/Resources/app.asar`。只读解析 asar，未启动应用、未截屏、未用 CUA、未发送任何请求。
- 文件：`webview/assets/{app-shared-dc7535e633d7.css, markdown-de681c624464.css, app-initial-4d5df9210cbc.css, app-30b4fba457b3.css, es6-a0739f375c3d.css, es6-32b20093825a.js, app-initial-b21bd554b363.js, response-f556de04e01e.js, recommendation-link-card-ff3151dfb9de.js}`。
- 局限：产物是压缩后的生产代码，类名带内容哈希（如 `_TextLink_myd0v_1`）；下面的引用按**选择器/组件语义**定位，不承诺哈希名跨版本稳定。主题 token 有 `lightningcss-light/dark` 双值，本文给浅色值，深色值在括号内。

## 结论先行

| 问题 | ChatGPT App 的实际做法 |
|---|---|
| 链接默认样式 | 当前会话正文的外链默认就是 **inline mention 外观**：`favicon 图标 + 蓝字 + font-weight 500`，不画常显下划线（`app-primary` 的 `CO` 组件，`appearance` 默认 `"inline-mention"`）。旧的 `TextLink`（`es6` 包）才是 `always` 实线下划线 + 继承正文色 |
| hover | **正文链接 hover 出现虚线**：`text-decoration: underline dashed` + `thickness: .5px` + `offset: 2px`，由 `data-underline-on-hover` + `data-inline-mention-interactive` 两个属性同时成立时触发。旧 `TextLink` 是实线，`color-mix(in oklab, currentcolor 70%, transparent)`，offset `.1em` |
| 下划虚线 | 就是**正文链接**（以及 `@` mention、文件/会话引用）hover 的默认样式，不是某个独立组件的特例。虚线与蓝字同属一个 `Mention` 组件 |
| GitHub logo | 不是图片资源，是**内联 SVG 图标**（20×20 viewBox 的 GitHub mark，`fill: currentColor`）；卡片站点行用 `Favicon` 组件，取 `https://www.google.com/s2/favicons?domain=<origin>&sz=32`，失败降级 globe |
| 预览 block | 两条不同通路：代码围栏 → `CodeBlock`（1px 边框 + `radius-md` + 独立底色 + 右上角悬浮 Copy）；链接卡片 → GenUI `RecommendationLinkCard` / `CardLinkItem`（1px 边框 + `radius-xl`，hover 只加 `shadow-200`） |

## 一、Markdown 链接：`TextLink` 组件

组件实现（`es6-32b20093825a.js`）：

```js
({children, primary = false, underline = !primary, className, forceExternal, as, href, to, ...rest}) => {
  const external = forceExternal ?? /^https?:\/\//.test(href ?? to ?? "");
  const props = {
    ...rest,
    className: cx(styles.TextLink, className),
    "data-primary": primary ? "" : undefined,
    "data-underline": underline === false ? undefined : underline === "hover" ? "hover" : "always",
  };
  // 无 href/to → <span role="button">；外链 → target=_blank + rel="noopener noreferrer"
}
```

要点：

1. **三态下划线**由 `data-underline` 承载：不设（无下划线）/ `hover`（仅 hover）/ `always`（常显）。默认 `underline = !primary`，所以不传参就是 `always`。
2. 旧版 Markdown 的 `a` 渲染器就是 `TextLink` 包一层：`_M = (props) => <TextLink as="a" {...props} />`（`es6-32b20093825a.js`）。它**不传 `primary`**，所以是 `always` 下划线 + 继承文字色，不是蓝色。
3. 蓝色链接是另一个开关：`data-primary` → `color: var(--link-primary-text-color)`，hover → `--link-primary-text-color-hover`。
4. **新版渲染器已经换路**：`app-initial-b21bd554b363.js` 里 `FMa` 的组件表是 `{a: QMa, code: eNa, img: Zka, pre: Fragment}`，`QMa` 对路径类 href 走 inline mention（`aka`）而不是 `TextLink`。也就是说当前版本正文链接是「蓝 + hover 下划虚线」，`TextLink` 更多用于非路径链接与按钮式链接。
5. 两代并存说明：**下划线默认开还是 hover 开，是产品在迭代的选项，不是固定结论**；抄的时候要选一种并写进 spec。

样式（`markdown-de681c624464.css`、`es6-a0739f375c3d.css`）：

```css
._TextLink {
  align-items: center; gap: var(--link-gap); border-radius: var(--link-radius);
  color: currentColor; cursor: var(--cursor-interaction, pointer);
  font-weight: var(--link-font-weight); overflow-wrap: anywhere;
  transition-property: opacity, color, box-shadow, text-decoration-color;
  transition-duration: var(--transition-duration-basic);
  transition-timing-function: var(--transition-ease-basic);
  display: inline-flex;
}
._TextLink:focus-visible { outline: 2px solid var(--color-ring); outline-offset: 1px; }

._TextLink[data-underline=always] {
  text-decoration: underline;
  text-decoration-color: color-mix(in oklab, currentcolor 70%, transparent);
  text-underline-offset: var(--link-underline-decoration-offset); /* .1em */
}

@media (hover: hover) and (pointer: fine) {
  ._TextLink:hover { color: color-mix(in oklab, currentcolor 80%, transparent); }
  ._TextLink[data-underline=hover]:hover {
    text-decoration: underline;
    text-decoration-color: color-mix(in oklab, currentcolor 70%, transparent);
    text-underline-offset: var(--link-underline-decoration-offset);
  }
}

._TextLink[data-primary] { color: var(--link-primary-text-color); }
@media (hover: hover) and (pointer: fine) {
  ._TextLink[data-primary]:hover { color: var(--link-primary-text-color-hover); }
}
```

token 取值（`app-shared-dc7535e633d7.css`）：

| token | 浅色 | 深色 |
|---|---|---|
| `--link-primary-text-color` | `--blue-500` `#0169cc` | `--blue-300` `#339cff` |
| `--link-primary-text-color-hover` | `--blue-800` `#013566` | `--blue-400` `#0285ff` |
| `--link-underline-decoration-offset` | `.1em` | 同左 |
| `--link-font-weight` | `inherit` | 同左 |
| `--link-radius` | `--radius-sm` | 同左 |

几点值得抄的细节：

- 下划线颜色是**当前文字色的 70%**，不是单独一条灰线；这样蓝链接和正文链接的下划线都自然。
- hover 只改 `text-decoration-color`（已在 `transition-property` 里），不会引起重排；`display: inline-flex` 让下划线只覆盖标签文字，不跨行拉伸。
- 键盘可达：`:focus-visible` 有 2px ring；外链固定 `target="_blank"` + `rel="noopener noreferrer"`；`http(s)` 判定走 `forceExternal`，站内路由才用 `to`。

## 二、下划虚线：就是正文链接的 hover 态

**先纠正本文首版的一处错误**：首版把虚线归为「只有 `@` mention 才用」。实际不是。当前会话正文的 Markdown 外链默认按 inline mention 外观渲染，hover 时显示的就是这条虚线——issue 截图里「favicon + 蓝字 + 虚线」正是它。

链路（`app-primary-d63a2421d501.js`）：

1. Markdown `a` 由工厂 `yA` 生成；非文件/非本会话引用时落到 `CO`。
2. `CO` 的 `appearance` **默认值就是 `"inline-mention"`**：`{appearance:n, ...}` 解构时 `n === void 0 ? "inline-mention" : n`。
3. 同一个 `CO` 里：`const w = n === "inline-mention"`，然后 `<ju appearance={n} icon={S} underlineOnHover={w}>`；`icon` 在 `appearance === "inline-mention"` 且不是 `codex:` 协议时取 `vO`，即 **Favicon 组件**。
4. 外层 `q_` 加上 `data-inline-mention-interactive`，`ju`（导出名 `aka`/`MU`）再按 `underlineOnHover` 打上 `data-underline-on-hover`。两个属性同时成立，虚线才生效。

CSS（`app-initial-4d5df9210cbc.css`，`_Mention_exp8z_2`）：

```css
@media (hover: hover) {
  ._Mention[data-inline-mention-interactive][data-underline-on-hover]:hover,
  [data-inline-mention-interactive]:hover ._Mention[data-underline-on-hover] {
    text-underline-offset: 2px;
    text-decoration: underline dashed;
    text-decoration-thickness: .5px;
  }
}
```

同一个类上的默认排版（这三条决定了「蓝色 + 加粗」的观感）：

```css
._Mention {                       /* 蓝 */
  color: var(--inline-mention-resolved-base-color, var(--inline-mention-base-color));
  font-weight: var(--font-weight-medium);   /* 500，不是 700 */
  padding-inline: calc(var(--spacing) * .5);
}
._Mention {                       /* 基础色 */
  --inline-mention-base-color: color-mix(in srgb, var(--color-text-info) 80%, var(--color-text) 20%);
}
._IconContainer { width: calc(var(--spacing) * 4); height: 1lh; position: relative; }  /* 16px 图标槽 */
._Icon { width: calc(var(--spacing) * 4); height: calc(var(--spacing) * 4);
         position: absolute; top: 50%; transform: translateY(-50%); }
[data-layout=inline-flow] ._IconContainer { vertical-align: bottom; margin-inline-end: 3px; }
```

- 浅色 `--color-text-info` = `--blue-500` `#0169cc`；与正文色 `#282828` 按 80/20 混合后约 `#085CAB`，所以看起来是偏沉的蓝，不是纯品牌蓝。深色 `--color-text-info` = `--blue-200` `#66b5ff`。
- `font-weight: 500`（medium）。issue 里猜的「加粗？」到这里可以定论：**是 medium，不是 bold**。
- 图标 16×16，与文字底部对齐、右侧留 3px；favicon 本身带 `rounded-2xs` 圆角。
- 另有 `[data-appearance=underlined-text]` 变体：`font-weight: inherit` + `text-decoration-line: underline`（常显实线），说明「常显实线」在系统里是另一种 appearance，而不是这条默认路径。

旧组件 `TextLink`（`es6-32b20093825a.js`）仍然存在，但走的是另一条外观：`data-underline="always"`、颜色继承正文、实线下划线。两者不要混为一谈。

对 Vega 的直接影响：**GPUI 0.6.1 的 `UnderlineStyle` 只有 `thickness / color / wavy` 三个字段，没有虚线样式**（`gpui-pre-0.3.4/src/style.rs:828-837`，`paint_underline` 只分 solid 与 wavy，见 `window.rs:4373-4404`）。所以 Vega 若要「下划虚线」，只能自绘 dash（沿字形底部画短段），或改用 wavy 之外的替代表达；不能只改一个样式字段。

对 Vega 的直接影响：**GPUI 0.6.1 的 `UnderlineStyle` 只有 `thickness / color / wavy` 三个字段，没有虚线样式**（`gpui-pre-0.3.4/src/style.rs:828-837`，`paint_underline` 只分 solid 与 wavy，见 `window.rs:4372-4404`）。所以 Vega 若要「下划虚线」，只能自绘 dash（沿字形底部画短段），或改用 wavy 之外的替代表达；不能只改一个样式字段。

## 三、GitHub logo 怎么展示

两处，形态不同：

1. **图标本身是内联 SVG**，不是 PNG/emoji。`Github` 图标来自 lucide（`github-BCbrDk0b-*.js` / `lucide-github-*.js`），路径为 `M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5...`，`stroke: currentColor`、`fill: none`、24 viewBox。连接器列表另有一套 20×20 的 GitHub mark（`app-initial-b21bd554b363.js` 内 `l7r`，`fill: currentColor`）。
2. **卡片站点行用 favicon 组件**（`response-f556de04e01e.js` 内 `Favicon`）：默认 12px（inline 时 `1em`），圆形容器 `border-radius: 100%`，图片来源 `https://www.google.com/s2/favicons?domain=${encodeURIComponent(origin)}&sz=32`；`loadRemote=false` 或取不到 origin 时降级为 globe 图标；`role="presentation"`，有 alt 才给 `role="img"`。

也就是说：**GitHub logo 在 ChatGPT 里是「图标组件 + 远程 favicon」两层，而不是给 GitHub 特判一张图**。域名来源统一，图标按 origin 自动取，失败有兜底。

Vega 对应能力：`gpui-kit-assets 0.6.1` **已经打包了** `icons/github.svg`，生成后可用 `IconName::Github`（已在 `target/debug/build/gpui-kit-assets-*/out/icon_name.rs` 确认）；但 `crates/vega_ui/src/icons.rs` 的 `Icon` 枚举目前没有 GitHub 变体，需要新增一条映射才能用。Vega 没有 favicon 抓取通道（也不应为 Markdown 链接引入联网取图），所以站点行只能用静态图标。

## 四、「codex 预览 block」的两条通路

截图 2 那张卡不是 Markdown 链接展开的，而是 GitHub 集成渲染的卡片；「预览 block」在 ChatGPT App 里对应两类不同组件：

**A. 代码围栏 → `CodeBlock`**

```css
._CodeBlock_xtyl9_1 {                     /* es6-a0739f375c3d.css */
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background-color: var(--codeblock-background-color);
  position: relative;
}
._CopyButtonContainer {                    /* markdown-de681c624464.css */
  border-radius: inherit;
  background-color: var(--codeblock-background-color);
  padding: 10px 10px 0 0;
  position: absolute; top: 0; right: 0;
}
._SyntaxHighlighter {
  color: var(--color-text);
  font-size: .875em; line-height: 1.5em;
  margin: 0; padding: 14px 40px 14px 16px;
  background: 0 0; display: block; overflow: auto;
}
```

配合 `RSa` 组件的渲染策略：`IntersectionObserver`（`rootMargin: 600px`）延迟渲染可视区外的代码块，先出 `<pre class="_CodeBlockPlaceholder">` 占位；`isCodeFenceOpen`（流式未闭合）时隐藏 Copy；`stickyHeader` 只对 `default` 变体生效。语法色走 `--codeblock-syntax-1..5` 五档 + 注释/标点用 `color-mix` 降透明度。

**B. 链接卡片 → GenUI `RecommendationLinkCard`**

`recommendation-link-card-ff3151dfb9de.js` 里有完整组件源码（GenUI DSL）：

```
card(size="full", padding=0)
└ LinkImpression(url)                        // 可见性埋点，不触发点击
  └ pressable(padding=4, radius="3xl", ariaLabel=state.title, onClick=GenUI.openUrl)
    └ row(gap=4, align=center)
      ├ image(src=state.image_url, 128×96, fit=contain, radius=xl, hideOnFailure)   // 可选
      └ col(gap=1)
        ├ text(size=md, weight=semibold, color=primary)   // state.title
        └ text(size=sm, color=secondary)                  // state.snippet，可选
```

外壳（`product-intervention-notice-12be15486477.css`）：

```css
._CardLinkItem { border: 1px solid var(--color-border); border-radius: var(--radius-xl);
  max-width: 100%; overflow: hidden; transition-property: box-shadow;
  transition-duration: .15s; transition-timing-function: ease; }
._CardLinkItem:hover { box-shadow: var(--shadow-200); }
._CardLinkItem[data-size=sm] { width: 200px; }   /* 240px 变体同选择器，按容器覆盖 */
```

所以卡片是「服务端/工具给结构化字段（title、snippet、image_url、domain）→ 前端按固定 DSL 画」，**不是前端解析 og:\* 生成的**。这点和截图一致：卡片里没有正文摘要，只有标题 + 仓库/评论数/时间这类结构化元信息，说明数据来自 GitHub 集成而不是网页抓取。

## 五、Vega 现状对照

| 项 | Vega（#357/#455 之后） | ChatGPT App（正文外链） |
|---|---|---|
| 链接色 | `SpanStyle::Link` → `colors.accent`（`#3478D8` / `#8FC7FF`） | `color-mix(text-info 80%, 正文色 20%)` ≈ `#085CAB` / 深色 `#66b5ff` |
| 字重 | 继承正文 | `font-weight: 500`（medium） |
| 下划线 | **常显** 1px 实线，颜色同 accent（`render_rows.rs:1104-1111`） | **默认无**，仅 hover 出现虚线 `.5px`、offset `2px` |
| hover | 只有指针变手形（`links.rs:99-101`），字形不变 | 虚线出现（`text-decoration-color` 在 transition 里，不重排） |
| 图标 | `Icon` 枚举无 GitHub；依赖里有 `icons/github.svg`，可新增映射 | 外链左侧固定 16px favicon（`s2/favicons`），失败降级 globe |
| 焦点 | 正文无 tab stop（#357 明确不做） | `:focus-visible` 2px ring |
| 虚线 | 不支持（GPUI `UnderlineStyle` 无 dash） | `underline dashed` `.5px`，正文外链 hover 默认 |
| 代码块 | `code_bg` + `TOOL_DETAIL_RADIUS` + `MARKDOWN_CODE_PADDING` + 横向滚动 + 边缘淡出；**已有语言头与固定复制按钮**（`code_header`，#477/#479），无边框、无延迟渲染 | 1px 边框 + `radius-md` + 独立底色 + 悬浮 Copy + 语言标签 + `IntersectionObserver` 延迟渲染 |
| 链接卡片 | 无 | GenUI 结构化卡片 |

## 六、如果要改，建议的最小步（未实施）

1. **下划线从常显改为仅 hover**：这是 ChatGPT 正文外链的实际行为，也是 issue 里「鼠标 hover 才展示下划线」的直接含义。现有 `LinkInteraction.hovered` 已在鼠标移动时 `window.refresh()`（`links.rs:172-178`），但 `block_text_runs` 在物化阶段一次性生成 runs，hover 状态没有进入 run 样式；要么把链接 run 的 underline 挪到绘制阶段按 hover 叠加，要么按 hover 重建该行 runs。**这会改动 #357 的「常显下划线」契约，需先改 spec。**
2. **虚线需要自绘**：GPUI 没有 dash 样式，`UnderlineStyle` 只能 solid/wavy。如果要做 `dashed .5px + offset 2px`，需在 `paint_underline` 之外自绘短段；也可先只做「hover 才显示实线」，把 dash 当作后续增量。
3. **链接色可沿用 accent**：ChatGPT 用的是 `text-info 80% + 正文色 20%` 的混合色（≈`#085CAB`），与 Vega 现有 `accent` `#3478D8` 是同一色系但更沉。是否精确对齐需视觉评审，不宜在无评审下直接改 token。
4. **字重**：ChatGPT 是 500（medium），Vega 目前继承正文。若要跟随，需在 `apply_span_style` 里设 `font_weight`，但正文链接加粗会改变行宽与折行，需一并验证布局。
5. **代码块不要重复开卡**：#477/#479 已在 master 落地语言头 + 固定复制按钮（`code_header`，`render_rows.rs:687-724`）。与 ChatGPT 的剩余差异只有外壳（1px 边框、`radius-md`）和 `IntersectionObserver` 延迟渲染；后者在 Vega 的虚拟化流里是否需要另说。

## 附：取证命令

```bash
node .scratch/asar-peek.js list /Applications/ChatGPT.app/Contents/Resources/app.asar 'webview/assets/(markdown|app-shared|es6|app-initial|response)'
node .scratch/asar-peek.js cat  /Applications/ChatGPT.app/Contents/Resources/app.asar <path> <out>
```

## 变更记录

- 2026-10-19：首版。基于 ChatGPT.app 26.1013.40934 产物取证，回答 #486 三个问题，未做代码改动。
- 2026-10-19：**修正**。用户指出截图里的虚线确属链接，复查找出 `CO` 组件默认 `appearance="inline-mention"`，即正文外链本就带 favicon + 蓝字 + 500 字重 + hover 虚线。首版「虚线只属于 `@` mention」的结论作废；同时补上颜色混合公式、图标尺寸与 `medium` 字重证据。
