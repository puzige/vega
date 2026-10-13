# Issue #296 · 信息流手测反馈修复交付

对应[展开详情对齐、单条问题提示与箭头状态规格](vega-issue-296-stream-polish.md)。本轮仅修复 PR #317 用户手测后的三项反馈，未安装、启动或操作真实应用。

## Freeze

- 验证时间：2026-10-12 23:51 UTC / 2026-10-13 07:51 Asia/Shanghai。
- 基线：`c6dfd9e`；分支：`feat/296-stream-polish`；提交与 PR 由主控集成。
- 环境：Darwin arm64；rustc 1.98.0 / cargo 1.98.0（Homebrew）；git 2.55.0。
- 原始日志目录：`~/Workspace/artifacts/vega-296-stream-polish/`。下文仅记录合成测试、bounded footer 与日志 hash，不附用户截图或会话原文。

## 实现与证据

Thinking 标题去掉旧8px内距与1px占位边框，采用16px图标槽、6px图文间距、25px高度和13.5字号，与单工具和工具组首行共轴；键盘焦点通过不占布局空间的背景表达。原始思考/摘要正文、选择、限高与内部滚动保留。两种 disclosure helper 均以启用且展开/hover/键盘focus之一成立为显隐条件，保留箭头槽位；阶段摘要仍没有Bot图标。

计时行移除异常badge并始终使用clock颜色，真实状态/耗时文案不变。新 `run_notice` 模块按既有message identity集中派生每轮唯一尾部提示：具体运行失败优先，其次中断/泛化失败，否则源顺序最后异常工具的安全摘要。后续成功不生成异常，工具详情输出不会进入尾提示。下一User是归属截止边界，迟到Summary不能把旧轮提示移到下一轮后；Final收起与重开不隐藏尾提示。

Assistant.failure、Summary和工具原状态均保留；主信息流只抑制重复的旧提示，无owner项仍使用原独立路径。渲染与锚点高度采用同一投影规则，提示迁移会使旧尾行与新尾行重新测量。具体typed运行诊断按message id留在当前stream的派生状态，Composer去重owner独立维护；保存/模型/前置错误清除去重owner，不会因文案相同被隐藏。原controller_error getter和状态保留。

App两个完成入口显式传已有 `terminal_message_id`，缺失id仍是独立Composer提示，不以最近轮次推测。第一个入口把typed错误写到同一个已应用历史刷新的display_stream，与第二个入口保持结构一致；此为展示归属修正，不宣称已复现实机ABA问题。没有后端、持久化、权限决策或错误分类变更。

| ID | 证据 | 结果 |
|---|---|---|
| P01 | 真实GPUI Final展开后Thinking/单工具/工具组icon与label左轴相同，label距行左缘22；hover、键盘focus和展开不移动左轴 | PASS，进程内 |
| P02 | 五个生产入口在浅深主题：hover、展开后离开、再次收起、Enter/Space、focus/blur；两helper在disabled且expanded false/true时hover/focus均不绘箭头 | PASS，实际paint与交互 |
| P03 | 原失败/拒绝工具保留，尾提示实际文本选较新的rejected，再由cancelled更新，后续Success不新增异常；Final折叠/重开仍在外，无输出正文泄露 | PASS，投影与GPUI bounds |
| P04 | 多片段失败历史、两轮、前插、迟到/重复Summary、空正文运行失败、具体诊断与同文案独立Composer错误 | PASS，原数据/字节不变 |
| P05 | 真实帧测量后提示从工具迁到正文再Summary；40段正文锚点恢复到原entry/0偏移且保持非tail阅读；旧矩阵继续覆盖选择、回看、权限/问题/产物及工具定位 | PASS，进程内 |
| App | 首次提交Quota、HTTP安全降级和外来错误不污染当前轮三个生产root入口；controller API留存且已有尾提示时Composer不重复 | PASS，provider边界替身 |

## 测试命令与原始结果

先行红测（当时只有P01/P02/P03三项）：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_polish_/)'
```

`red.log`，退出码 `100`：

```text
Summary [   0.040s] 3 tests run: 0 passed, 3 failed, 685 skipped
```

SHA-256：`cdee01e445d2edb21e531e41a8d3ad57a095f9fa8e1c7c65042ef62ba419bf3f`。分别检出Thinking额外9px、展开箭头离开消失和顶栏异常badge仍存在。

受影响回归命令：

```sh
cargo nextest run -p vega_ui -p vega -E 'test(/issue296_/) | test(/issue146_/) | test(/issue147_/) | test(/issue151_/) | test(/thinking::/) | test(/issue72_/) | test(/issue98_/) | test(/issue70_/) | test(/issue103_/) | test(/a7_first_submit_provider_quota/) | test(/a7_provider_http_failure/) | test(/a7_foreign_runtime/)'
```

`regression-second.log`，退出码 `100`：

```text
Summary [  35.852s] 216 tests run: 214 passed, 2 failed, 733 skipped
```

SHA-256：`86beaad83bc8e7a3f38630c1fe132dce98c916d543002b12e104b0860b3c7987`。214项中包含全部既有受影响断言和三项App入口；两个失败属于新增测试的Bash合成fixture，尚未满足Approved生命周期及各终态metadata合同，安全投影正确退化为损坏结果。修正测试输入后未修改生产逻辑。该命令的宽 `issue296_` 正则也选中了先前Settings等同卡用例；保留真实结果，没有以此声称workspace全量通过，也没有重复执行已通过集合。

最终仅重跑新增矩阵：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_polish_/)'
cargo fmt -p vega -p vega_ui --check
git diff --check
```

`green-final.log`，nextest退出码 `0`：

```text
Summary [   0.671s] 8 tests run: 8 passed, 685 skipped
```

SHA-256：`217cd53578c6e4e602d5f90f6adb0316b6b0576c893175af363ec6734cab5a7f`。此结果与前述214项通过证据共同覆盖最终改动，不能把原216项日志写成全绿。fmt/diff检查均退出 `0`；仅已有 `block v0.1.6` future-incompatibility 警告。没有本地运行workspace全量或Clippy。

中间 `first.log`、`expanded.log`、`regression.log`、`green.log` 均保留：含原三项首次修复后通过、测试中误用GPUI keyed-state阶段API、测试借用编译错误及不完整Bash fixture。测试已改用真实focus_next与paint证据，未放松disabled、权限、错误降级、复制或导航断言。旧测试只迁移展开箭头常驻、Thinking无整行hover底色及尾提示选择器规则。

## Residuals

- LIMIT：最后异常按源时间线顺序，历史没有完成事件时序；具体runtime诊断只保留于当前stream，重开且无详细诊断仍诚实使用Persisted。
- NOT RUN：原生字形像素、真实provider/ACP与真实应用手测；进程内bounds/paint不等同于原生截图验收。完整云端门禁由新PR承担。
- 用户手测：展开完成轮，确认Thinking与工具图标及文字共轴；移开鼠标后展开箭头仍在，收起后仅hover/键盘focus可见；计时旁无红色异常汇总，每轮尾部最多一条问题，展开原工具仍能查看真实失败/拒绝详情；再提交一轮、重开历史并尝试选字/上滚，确认提示归属和阅读位置不变。

## 追加 P06–P08：紧凑展开与旋转（2026-10-13）

增量基于PR #318首个提交 `c67cc72`，本地验证时间2026-10-13 00:17 UTC / 08:17 Asia/Shanghai。原P01–P05及其失败记录仍有效，下面的结果不覆盖或改写前一轮日志。

首阶段的顶部间距由stream层一次投影，按同run前一个可见内容选择clock后的16px或正文后的24px；空阶段与无可见Markdown行的非失败正文不制造幽灵间距，原copy、identity保留，失败空正文仍保留安全提示。render与未测量高度/锚点使用同一计算。

阶段展开现在按源顺序显示25px紧凑Thinking行和原ToolCard，工具组只在此展示投影中扁平化，同级gap为0。Thinking全文与阶段开关独立，默认关闭；再次点击才在240px上限内显示Markdown。MarkdownStream/StreamModel在源变动时更新缓存，不在render重解析；可选择文本与Cmd-C继续复制可见文本，右键“复制原文”使用完整原始字节，链接没有增加外部打开行为。Final统一过程使用同一扁平子项投影与detail索引，正文穿插和身份保持。

通常阶段使用一个320px上限的滚动区；存在产物时按源位置拆为连续操作区，每区最多320px，产物保持原位外显。实测合成“Thinking→工具→产物→工具”顺序、自然高度与未测量估算相同，关闭阶段后产物仍存在。旧ToolGroup的hover/focus覆盖移到仍受支持的独立ToolGroup生产渲染路径，未删掉第五种disclosure入口。

侧栏仅在既有Spinner上设置 `ease(gpui_kit::linear)`，保留0.8秒周期、16px尺寸、颜色、稳定id及RunningThreads来源。生产侧栏liveness和槽位几何测试通过。当前公开GPUI接口仅能读取quads，不能读取Spinner的SVG旋转变换；默认repeat使用真实Instant，TestClock不能模拟精确0.2秒角度。四段各90°是参数验算，并非实际角度采样。没有为测试新增动画循环、修改依赖或改变Reduce Motion机制，实际匀速观感由用户手测确认。

### 追加红绿记录

P06首间距、P07阶段紧凑来源行与Final工具扁平化先行红测：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_expansion_/)'
```

`expansion-red.log`，退出码 `100`：

```text
Summary [   0.049s] 3 tests run: 0 passed, 3 failed, 693 skipped
```

SHA-256：`2959a67694cf8a048978d47419d1e25161b3327db9225da4044896ac411f6d2b`。

受影响会话/Thinking选择与阅读、P01–P07及侧栏状态回归：

```sh
cargo nextest run -p vega_ui -E 'test(/conversation_stream::tests::issue296_/) | test(/thinking::/) | test(/issue147_/) | test(/issue146_/) | test(/transcript_hides_completed_statistics_but_keeps_failure_status/) | test(/issue150_/)'
```

`expansion-green.log`，退出码 `0`：

```text
Summary [   1.628s] 115 tests run: 115 passed, 585 skipped
```

SHA-256：`110efe6f38e52c4972f67caefc0b1a3303d331b3a70bbc9c57825a01d3c649b0`。

随后发现并去除空Assistant残留的16px占位，新增空TextDelta与已加载历史组合的真实bounds断言；只复跑该增量及受影响边界：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_expansion_/) | test(/issue103_thinking_scroll_survives_streaming_and_reopen/) | test(/failed_empty_assistant_hydrates_a_visible_safe_reason/)'
cargo fmt -p vega_ui --check
git diff --check
```

`expansion-edge.log`，nextest退出码 `0`，fmt/diff均退出 `0`：

```text
Summary [   0.122s] 10 tests run: 10 passed, 691 skipped
```

SHA-256：`23134adc2987a9b027d0035c1ab3dfda4226538c8b7e920259c7b2589170daf4`。

### 收尾审查的两项边界

空正文隐藏会依赖已同步的Markdown模型，因此把Final/尾提示/阶段间距投影移到mutable tail同步之后，避免首个TextDelta已绘出而尾提示仍在先前工具行。共享Markdown估高对零行返回0，避免空失败正文被重复计入一行；原failure与唯一安全提示保留。

审查先行红测：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_expansion_p07_first_render/) | test(/issue296_expansion_p07_empty_failure/)'
```

`expansion-review-red.log`，退出码 `100`：

```text
Summary [   0.046s] 2 tests run: 0 passed, 2 failed, 701 skipped
```

SHA-256：`19d8ab8a6d8f19060452562762784f8950acc0b6ae610304ba29e94f320b24af`。分别检出notice仍属于旧index、空失败估高85.15而实际正文与提示合计62。

两处生产修复后的窄回归：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_expansion_/) | test(/issue296_polish_p04_empty_runtime_failure/) | test(/issue296_polish_p05_notice_moves_tail/) | test(/failed_empty_assistant_hydrates_a_visible_safe_reason/) | test(/issue103_thinking_scroll_survives_streaming_and_reopen/)'
```

`expansion-review-green.log`，退出码 `100`：

```text
Summary [   0.125s] 14 tests run: 12 passed, 2 failed, 689 skipped
```

SHA-256：`1cfab36a7f5b4534d81bad16ec0321b852ffa5c6a00b5fad3cb6abf6341a884e`。12项通过；新增空失败断言残留的是既有提示估高与绘制取整的0.15px差，新首帧fixture直接丢弃Render结果则留下GPUI arena引用。仅修测试：正文自身估高严格等于实测16，总高允许小于0.25px舍入差；首帧改为正常Window.draw/clear生命周期，紧接绘制断言，之后才清理队列。

同一两项过滤命令的 `expansion-review-final.log`，退出码 `100`：

```text
Summary [   0.074s] 2 tests run: 1 passed, 1 failed, 701 skipped
```

SHA-256：`075c76b77c3a65ed29f7458bbeaaff947fbe12da5b6c877e619101d12f10a849`。空失败正文/唯一提示/原failure断言通过；首帧fixture清理队列仍不能代替正常绘制生命周期。随后 `expansion-review-frame.log` 的单例仍因typed WindowHandle更新时借住root而重入失败，退出码 `100`，没有再次修改生产代码。

最终首帧测试使用VisualTestContext更新入口，在同一回调内追加TextDelta、确认模型尚空、执行恰一次Window.draw/clear并立即核对模型与新尾部索引，避免后续通知掩盖时序：

```sh
cargo nextest run -p vega_ui -E 'test(/issue296_expansion_p07_first_render/)'
cargo fmt -p vega_ui --check
git diff --check
```

`expansion-review-frame-final.log`，nextest退出码 `0`：

```text
Summary [   0.043s] 1 test run: 1 passed, 702 skipped
```

SHA-256：`48453ded558a8a6f73d0e19a6996682984d5d577d0aba2e3e116be91cdfbf50d`。与前述12项及空失败单项通过证据共同覆盖最终改动，没有将14项失败日志写成全绿或重复已通过集合。收尾验证时间2026-10-13 00:25 UTC / 08:25 Asia/Shanghai；fmt/diff均退出 `0`。

中间日志保留于同一artifact目录：`expansion-first.log`/`expansion-second.log`记录新增Markdown helper的编译接线错误；`expansion-third.log`为首批10/10；`expansion-regression.log`为155/168，13项失败是被P06/P07明确替代的自动展开、旧selector和Final嵌套/间距断言，后续逐项保留原文、滚动、焦点和安全要求迁移。初次宽正则还选中同卡Settings测试；后续已收窄到conversation_stream，未运行workspace全量。`expansion-regression-second.log`记录迁移脚本语法错误未生效时的旧失败；`expansion-regression-third.log`为113/115，剩余一个新artifact fixture的checkpoint/call id不匹配、一个旧测试直接清除raw后未更新Markdown缓存/空body断言，均已修正。没有把这些失败日志改写为全绿。最终仅已有 `block v0.1.6` future-incompatibility警告。

### 首个PR head云端反馈

`c67cc72` 的CI run `38324687172` 未通过：Clippy报 `run_notice.rs` 的 `collapsible_match`；nextest为2321项中2320通过、1失败，退出码100，唯一失败为 `transcript_hides_completed_statistics_but_keeps_failure_status`。原始 `ci-first.log` SHA-256：`dfbf27606289b62078c6977f1a9e149ab6d25aa2f1f47add0be0da701983b7e7`。

本增量把Summary条件移到match guard，没有新增allow；恢复Completed summary的零高隐藏selector，Failed断言改为实际owned `run-notice-failed` 尾提示，原两份summary数据和可见错误断言保留。该case已包含于115/115。最新提交的云端Clippy/workspace全量门禁仍由主控推进，本地结果不代替新head CI。

### 追加手测范围

确认clock后的首摘要间距缩短；展开阶段只有紧凑同级来源/工具行，独立打开思考后Markdown正常、原文可复制；长工具区与思考全文分别有320/240上限，内部回看不会被流式文本抢走；产物出现在所属工具后且收起阶段仍可见；Final统一重开仍保序，尾提示只有一条；侧栏running圆环应匀速旋转。未进行原生App操作、真实provider调用、系统字形像素验收或实际动画角度采样。
