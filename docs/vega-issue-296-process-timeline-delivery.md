# Issue #296 · 信息流行为与 Codex 截图对齐交付记录

状态：行为与视觉切片均已实现，最终定向回归170项通过，独立审查无剩余生产阻塞项。已提交 [PR #317](https://github.com/puzige/vega/pull/317)，合并状态及全量门禁以PR为准；原生像素、真实系统集成与安装仍未验证，等待合并后用户手测。

## Freeze

- 验证时间：2026-10-12 15:53 UTC / 2026-10-12 23:53 Asia/Shanghai。
- 基线：`origin/master@7fee79e`；分支：`feat/296-process-timeline`。
- 契约：[过程时间线规格](vega-issue-296-process-timeline.md)，替代旧 single-activity §2 的运行时顶部工具聚合规则。
- 环境：Darwin arm64；Rust / Cargo 1.98.0；Git 2.55.0。
- 行为测试时源码摘要：tracked conversation-stream diff SHA-256 `203b42b3b9f299bf5d91be368a73477b2087f436ac0ec6830e20d55a392cbb86`；新增测试文件 SHA-256 `166c0b777281b55b75e3258d9124bcd0cba9d701419b911d4b9a2807e40a132f`。
- 证据级别：GPUI 生产组件、真实进程内事件投影、合成历史、实际 GPUI 选字/键盘/滚轮事件；没有启动或操作真实应用，没有真实 Provider 验收。

## 根因与实现

1. `visible_segment_children` 只留下产物，而 `run_details` 把所有操作渲染在头部。运行时恢复 source segment 的思考/工具投影，头部仅在已收起过程时承载统一详情。每个操作保持原实体，工具详情仍默认折叠。
2. `settle_final(reading=true)` 原先直接清掉待收起状态。现在阅读保护期间保留 pending，且不发送无变化通知。选区、实际 inline disclosure 焦点、手动展开、上滚均保护阅读；滚轮回尾在 GPUI list layout 之后再次检查，避免“已经回尾却要等下一次输入才收起”。
3. response 边界原先可能合并多个 Commentary 或 Final 到同一 copy。实时有效 Commentary 完成时关闭当前 parser；hydration 先完整验证 metadata，再只在跨原 copy 的有效边界拆分。连续 Commentary 的覆盖可联合确认；未知、冲突、越界、UTF-8 非边界仍保守显示原正文。
4. 新派生片段使用原正文 ordinal 加 `response-{起始字节}` 后缀；不重编号后续原正文或工具 key。live 和历史中的 A/B/tool/Final、A/Thinking/Final 均按 key 恢复到相同正文。重复 metadata 不关闭新 Final parser。原 copy 首段 key 与普通未跨边界历史保持不变。
5. inline 工具高度失效定位到其 segment；收起后定位头部统一详情。产物保留外显与原 segment 锚点，失败/取消会撤销已展示的 Final 收起。保留全部原文字节、工具实体、顺序、选字与复制路径。

## 先行矩阵与结果

| ID | 实际操作 | 预期与证据 | 状态 |
|---|---|---|---|
| T01 | 实时 A→工具1→B→工具2 | GPUI 实际正文/segment bounds 按序；头部没有重复详情 | PASS |
| T02 | Final 确认→自动收起→点击展开 | 只隐藏可信过程；展开得到 A/工具1/B/工具2；Final 不重复；拼接原文不变 | PASS |
| T03 | 连续 Unicode response、逐字符分块、重复元信息 | 全部 Commentary 可收起，Final 在外；重复旧 Commentary 不终结 Final parser | PASS |
| T04 | 已知/未知 Final hydration、两轮并存、原 key 重载、工具/思考边界 | 归属与正文对应一致；无时长仍显示“已完成”；其他轮原文和手动展开不变 | PASS |
| T05 | 元信息缺失/UTF-8/越界/冲突/间隙；Final 已渲染后失败或取消 | 未分类正文保留、损坏元信息不拆；失败/取消恢复正文与工具；既有拒绝/权限断言保留 | PASS |
| T06 | 真正拖拽选字、Tab 聚焦 inline 工具、展开详情、上滚后滚轮回尾 | 阅读保护不清 pending；clear+composer focus 或回尾独立触发收起；手动展开保留 | PASS |
| T07 | 工具/正文/产物锚点，长详情滚动，浅深 hover/focus 几何 | 收起前就地、收起后统一详情；产物不搬入详情；原长详情上限/滚动/复制断言保留 | PASS |

旧展示测试仅更新本规格取代的断言，或先注入可信 Final 后再验证统一详情的 hover/focus/限高滚动；原业务、安全、选择、原文字节、产物与几何断言继续保留。旧 backscroll 测试改为真实存在溢出的上滚前置，旧选区状态测试补真实选区应有的 selection focus；另新增实际拖拽与清除测试覆盖生产路径。

## 命令与原始日志

日志根目录：`~/Workspace/artifacts/vega-296-process-timeline/`。全部本地命令使用该 worktree 的默认 target，无共享 target/锁脚本，无 workspace 全量测试。

先红命令：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_timeline_/)'
```

`red-tests.log` 原始结尾：

```text
Summary [   0.046s] 4 tests run: 0 passed, 4 failed, 661 skipped
```

首次冷构建日志 `red.log` 保留测试 fixture 的字段名编译错误；修正 fixture 后 `red-tests.log` 为实现前四项实际失败。后续新增真实滚轮回尾用例也先失败于 pending 未清，见 `expanded-matrix-tests.log`；中间编译/fixture/旧契约回归失败全部保留，不删除失败证据。

最终覆盖受影响功能的定向命令：

```sh
cargo nextest run -p vega_ui -E 'test(/conversation_stream::tests::issue296_/) | test(/issue147_/) | test(/issue146_/) | test(/issue151_/) | test(/issue72_/) | test(/thinking::/) | test(/timeline::/) | test(/issue98_/)'
```

最终原始输出为 `green.log`。`reading-fixtures-fixed.log` 单独记录 15 项新矩阵与阅读保护回归全部通过。格式化使用 `cargo fmt -p vega_ui`；`git diff --check` 通过。全量 Clippy / nextest 由云端 PR 门禁承担。

```text
Summary [   0.839s] 118 tests run: 118 passed, 557 skipped
```

- `red-tests.log` SHA-256：`4b0147f35076b47ab6722a1cf11873167c7f444f3b28fffd86ff828b9a64080d`。
- `green.log` SHA-256：`e4a58da9dd0e661894ae24bb91264683dad285eb2827501112e57138be692123`。
- 独立只读审查已检查 production diff、身份恢复、重复元信息、末帧回尾、inline 焦点、旧断言调整，没有剩余阻塞 finding；审查方未重复运行测试。

## 限制与用户手测

- NOT RUN：真实模型、真实 ACP、真实应用像素验收与安装；这些留给合并后的用户手测。ACP 缺失可信 Final 时保留时间线，不推断末段就是答案。
- LIMIT：只有已收到有效 metadata 的新增 response 边界拥有精确阶段归属；历史没有持久化的 Thinking 无法凭空恢复。跨 copy 拆分保留原 key 首段，但旧像素 offset 超过首段高度时按现有规则 clamp，不承诺异常旧历史逐像素相同。
- 视觉切片实现与进程内验收见下文；原生字形墨迹和真实应用仍待用户手测。
- 手测：发送会触发多次工具的请求，确认说明后就地出现操作；Final 后过程收起且答案外显；展开可读原说明与工具；运行中选字/展开/上滚应保持阅读，退出选区与焦点或回尾后才收起；重开历史与跳转锚点应到达同一正文/工具，产物与权限提示继续可见。


## 追加视觉切片：Codex 信息流截图对齐

依据 [Codex 信息流截图对齐规格](vega-issue-296-codex-stream-parity.md)，在行为基线 `1868586` 上实现。会话正文使用独立736宽度、13.5字号、23行高、15.5段距；Composer768与正文保持同中轴。纯计时行23高、4.5分隔线前距与1px线替代顶部图标/动态操作名，不再生成 Vega 角色行或20px角色占位。用户气泡最大70%、左右16内距、16圆角，浅色使用会话独立颜色，暗色沿用原语义色，不改变 Sidebar/Settings 共用字体和颜色。

阶段复用原 `RunActivitySegment` 所有权与焦点，已有摘要/思考标题默认折叠，12px箭头在文字后6px预留槽内仅hover/focus出现，hover不画整行灰底。展开直接投影原工具实体，flatten原相邻工具组；无摘要单工具与真实多工具聚合仍保留。工具行25高、16px图标槽、6px文字间距，详情容器/输出/时长/复制不删除。

同一阶段多个摘要时，只有用于阶段标题的简单单行摘要免去重复正文；其余摘要源和独立原始思考始终可达，完整多行摘要不截断存储或复制内容。思考限高与滚动继续生效，已经展开的阶段追加新思考会继承展开状态并在读者仍位于底部时继续跟随。收起阶段后隐藏工具内部的展开状态不再保护阅读；内部状态仍保留，Final统一收起后重新展开仍可访问。

工具锚点在Final收起前先揭示自己的阶段并就地定位，Final收起后才进入统一详情。正文去角色后的历史前插不再减20px；正文、选择、Markdown/table wrap与估算同时使用新宽度/字号/段距。左侧真实消息短刻度使用6×2矩形、10节距，宽窗口中心位于正文左轴前54，窄窗口夹在安全边缘；预览移动到rail右侧，原鼠标/键盘/滚轮导航保持。

工具glyph校准只读取本项目依赖的 Lucide `square-terminal.svg`：24 viewBox、18宽矩形、2px stroke，外轮廓20/24；使用16px SVG盒在16px槽内产生约13.33px可见轮廓，避免再次缩小到13px盒。共享 `icon()` 默认尺寸保持16，仅新增可选尺寸helper供12px箭头使用。未读取Codex私有源码或资产。

| ID | 实际证据 | 结果 |
|---|---|---|
| V01 | GPUI实际bounds验证736列宽、23计时行、4.5间距、1线及共左轴，Vega角色与leading图标不存在；旧宽窄列/导航测试同步新中轴 | PASS |
| V02 | 合成CJK实际selection layout验证13.5/23、两段首行间38.5；70%气泡/16内距/圆角及paintedquad颜色；真实拖选Cmd+C、inline-code选区和原文回归 | PASS |
| V03 | 默认阶段隐藏工具，12箭头+6相邻间距；展开直接两工具行、25起点差，无工具组/思考重复标题 | PASS |
| V04 | 同段多个summary及raw+多行summary实际source节点按原顺序可达；单工具、多工具安全标签原回归保留 | PASS |
| V05 | A→阶段1→B→阶段2真实bounds；Final后阶段收起；重新展开源顺序为A/摘要1/工具1/B/摘要2/工具2，Final不重复 | PASS |
| V06 | 阶段焦点退出、手动展开、工具锚点、隐藏expanded工具不阻塞Final、新思考跨首次overflow；原选区/回看/历史/多轮行为适用断言 | PASS |
| V07 | 浅深色stage shimmer实际paint probe、Reduce Motion、hover无灰底且箭头不移动；主题独立颜色断言；原权限/错误/产物外显与业务回归 | PASS |
| V08 | 1400/480宽窗口实际rail轴线、6×2 paintedquads、10节距；原真实鼠标/键盘预览与消息跳转断言 | PASS |

这些PASS均指进程内投影、行盒、paintedquad、实际交互/文本布局或静态主题契约，不代表原生字形逐像素截图验收。截图颜色/1472原始像素宽度是测量事实；2:1密度、字体family/weight与13.5初始字号是规格推定。助手正文到阶段仍采用已移除旧pb16/role20后的24过渡盒初值；原生上一行ink底到阶段ink顶25.5–26、抗锯齿、inline-code约24行盒与字体fallback最终外观仍待用户手测校准。本轮没有启动、安装、截图或操作真实应用。

视觉红测命令及原始日志：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_visual_/)'
```

`~/Workspace/artifacts/vega-296-process-timeline/visual-red.log`：

```text
Summary [   0.043s] 2 tests run: 0 passed, 2 failed, 675 skipped
```

V01缺少divider与V03缺少stage在实现前真实失败。SHA-256：`08b447b6de73ae86565099c18fda5e64ad8f44c8c35919433a83c86313bf9984`。中间日志 `visual-first.log`、`visual-regression-first.log`、`visual-regression-second.log`、`visual-expanded.log`、`visual-matrix.log`、`visual-matrix-second.log`保留，不覆盖失败证据。

最终视觉及受影响回归命令：

```sh
cargo nextest run -p vega_ui -p vega_theme -E 'package(vega_theme) | test(/conversation_stream::tests::issue296_/) | test(/issue147_/) | test(/issue146_/) | test(/issue151_/) | test(/issue72_/) | test(/thinking::/) | test(/timeline::/) | test(/issue98_/) | test(/issue78_/) | test(/issue103_/)'
cargo fmt -p vega_ui -p vega_theme --check
git diff --check
```

`~/Workspace/artifacts/vega-296-process-timeline/visual-green.log`：

```text
Summary [   1.039s] 170 tests run: 170 passed, 543 skipped
```

其中28项来自 `vega_theme`，142项来自 `vega_ui`（包含10项新视觉矩阵测试）。SHA-256：`765b620ffade3ae50306be47b268e944e283f0bc7869cbf72ea4eb52f57b49b8`。fmt与diff检查通过。唯一警告为已有依赖 `block v0.1.6` 的 future-incompatibility，无新增编译warning。没有运行workspace全量测试或Clippy；云端门禁继续承担全量校验。

适用旧测试按新规格更新角色/宽度/行高/箭头/颜色与阶段选择器；保留逐字符layout、内部空行、工具详情限高滚动、字节复制、真实焦点/hover、权限和导航业务断言。历史统一详情锚点fixture改选Step5，以确保缩小字体后的目标后方仍有足够滚动空间，目标索引/像素偏移的原精确断言保留。

用户手测补充：与六张参考图并排检查纯计时横线、正文列/气泡/左刻度、摘要文字后箭头、阶段展开直接工具行；确认hover不出现整行灰底。分别体验阶段展开→工具展开→阶段收起→回Composer→Final，以及选字/回看/阶段键盘焦点→Final→退出阅读。阶段summary/raw多行源、长思考尾随与手动回看、最终重开工具输出、历史锚点与权限/产物都应可达。原生ink间距与字形抗锯齿作为待验证项记录，不声称全部像素已通过。
