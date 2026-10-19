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
| 链接默认样式 | 两代组件并存：旧版正文链接用 `TextLink`，默认 `data-underline="always"`（常显下划线）+ **继承正文色**；新版正文 `a` 走 inline mention 组件（`aka`），默认蓝 + hover 才加下划线。**没有**「常显下划线 + 品牌蓝 + 加粗」这个组合 |
| hover | 旧版：`underline` + `color-mix(in oklab, currentcolor 70%, transparent)` + `text-underline-offset: .1em`；`data-primary` 才把颜色从 blue-500 提到 blue-800。新版 mention：`underline dashed` + `thickness: .5px` + `offset: 2px` |
| 下划虚线 | 只在 **inline mention**（`@` 引用/路径引用）hover 时出现；`TextLink` 本身没有 dash 分支 |
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

## 二、下划虚线：它其实是 inline mention

issue 里问的「hover 才展示下划线或者下划虚线」，在 ChatGPT App 里是两个不同组件：

- **正文 Markdown 链接**：旧版 `TextLink` 只有 `solid` 下划线（`always` / `hover` 二选一，无 dash 分支）；新版正文 `a` 走 mention 组件，即下面那条。
- **`@` mention**（`_Mention_exp8z_2`）：hover 时 `text-decoration: underline dashed`、`text-decoration-thickness: .5px`、`text-underline-offset: 2px`，通过 `data-underline-on-hover` 开关（`app-initial-4d5df9210cbc.css`）。

```css
._Mention[data-inline-mention-interactive][data-underline-on-hover]:hover,
[data-inline-mention-interactive]:hover ._Mention[data-underline-on-hover] {
  text-underline-offset: 2px;
  text-decoration: underline dashed;
  text-decoration-thickness: .5px;
}
```

对应组件 `Mention`（`app-initial-b21bd554b363.js`）的默认值：`underlineOnHover` 默认 `= interactive`，即「可交互才给 hover 下划线」。这是「hover 才出现」这一交互在 ChatGPT 里的原始出处。

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

| 项 | Vega（#357/#455 之后） | ChatGPT App |
|---|---|---|
| 链接色 | `SpanStyle::Link` → `colors.accent`（`#3478D8` / `#8FC7FF`） | 旧版 `TextLink` 继承正文色；新版正文 `a` 走 mention（蓝）；`data-primary` 才用 `--link-primary-text-color` |
| 下划线 | 常显 1px 实线，颜色同 accent（`render_rows.rs:1104-1111`） | 旧版常显（文字色 70%）；新版 hover 才出（虚线 .5px） |
| hover | 只有指针变手形（`links.rs:99-101`），字形不变 | 下划线出现/颜色加深；`pointer:fine` 才启用 |
| 焦点 | 正文无 tab stop（#357 明确不做） | `:focus-visible` 2px ring |
| 虚线 | 不支持（GPUI `UnderlineStyle` 无 dash） | mention 用 `underline dashed` .5px |
| 图标 | `Icon` 枚举无 GitHub；依赖里有 `icons/github.svg`，可新增映射 | 图标组件 + `s2/favicons` 远程取图 |
| 代码块 | `code_bg` + `TOOL_DETAIL_RADIUS` + `MARKDOWN_CODE_PADDING` + 横向滚动 + 边缘淡出；**已有语言头与固定复制按钮**（`code_header`，#477/#479），无边框、无延迟渲染 | 1px 边框 + `radius-md` + 独立底色 + 悬浮 Copy + 语言标签 + `IntersectionObserver` 延迟渲染 |
| 链接卡片 | 无 | GenUI 结构化卡片 |

## 六、如果要改，建议的最小步（未实施）

1. **链接色与下划线分离**：正文链接保持 accent 文字色，下划线改 `accent` 的 70% 透明度版本，降低正文视觉噪声；这是纯 token 改动，不动交互。
2. **hover 才显示下划线**：需要把 `apply_span_style` 的静态 `underline` 改为「按下划线由 hover 状态决定」。现有 `LinkInteraction.hovered` 已存在并在移动时 `window.refresh()`（`links.rs:172-178`），但 `block_text_runs` 在物化阶段一次性生成 runs，hover 状态没有进入 run 样式；要么把链接 run 的 underline 挪到绘制阶段按 hover 叠加，要么按 hover 重建该行 runs。**这会改动 #357 的「常显下划线」契约，需先改 spec。**
3. **不要照搬虚线**：GPUI 无 dash 支持，自绘 dash 需要新绘制路径；收益（区分 mention）在 Vega 当前没有 mention 语义，建议先不做。
4. **代码块不要重复开卡**：#477/#479 已在 master 落地语言头 + 固定复制按钮（`code_header`，`render_rows.rs:687-724`）。与 ChatGPT 的剩余差异只有外壳（1px 边框、`radius-md`）和 `IntersectionObserver` 延迟渲染；后者在 Vega 的虚拟化流里是否需要另说。

## 附：取证命令

```bash
node .scratch/asar-peek.js list /Applications/ChatGPT.app/Contents/Resources/app.asar 'webview/assets/(markdown|app-shared|es6|app-initial|response)'
node .scratch/asar-peek.js cat  /Applications/ChatGPT.app/Contents/Resources/app.asar <path> <out>
```

## 变更记录

- 2026-10-19：首版。基于 ChatGPT.app 26.1013.40934 产物取证，回答 #486 三个问题，未做代码改动。
