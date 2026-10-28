# #687 Assistant Markdown 接入：P0 / P1

## 授权与基线

用户授权实施 P0、P1，不扩展 P2–P6。后续明确允许提交、推送开发分支和建立 draft PR，仅用于云端编译与必要回归；不合并、不发布应用。本地不运行、构建或测试 Vega。
用户明确允许生产 assistant 正文沿用已认可 demo 的提取样式排版参数，覆盖设计守则的来源限制，仅限此项；不复制字体/图标资产，不改全局主题、链接契约或预览服务。

独立验证分支基于 origin/master：c42148f69c9cb987901aeae56932e7ecf57a24ce。原实现工作基线为 c53441a3b5a6cdd5697228d34b67c6425dc566f4；六个生产文件移入验证分支时内容相同，不包含旧 demo 的提交历史及未提交改动。
已认可 demo assistant.rs SHA256：4767dc154896d460dec18deae2937eb73cca30276b4c0c9c294527bee2bddbc8。
原 demo 保留在原工作区。视觉参考为本会话用户认可的浅/深、窄/宽截图，不在公开材料附原始会话。

## P0 契约

仅 assistant 正文。用户、思考、工具、文件预览与更新说明不启用新策略。共享组件维持默认旧策略。
正文目标 F=16、行高=1.625F、行内代码=.92F、水平 padding=6、竖向 padding=1、圆角=6；本阶段不将这些参数全局替换，不搬入 demo 整窗外边距。
选文复制使用 generation 有效的 selected_text；消息复制保留 MessageCopy::display_text 的现有投影语义，不误称总是原始原文；代码复制保留现有代码来源。视觉折行、图标、padding 不新增文本字节。
流式 append/replace 继续使选区失效，不承诺生成中保留旧选区。
链接按 #357/#455/#486 与 #493：accent/500、默认无下划线、hover/focus 虚线、16+4 图标；owner/occurrence/starts_here/text_identity 完整保留；500ms/4px 点击仲裁、400ms/150ms 预览生命周期、键盘、焦点恢复、安全与资源上限不变。
#493 根层滚动采样顺序不动。MarkdownStream、StreamModel、虚拟列表与缓存不替换。

性能采用同机、同数据、同构建与窗口条件对比，记录绝对 p95 和内存及相对变化；10% 为诊断/停止扩展阈值，不代表已有基线或已通过。未取得测量不得默认切换。

## P1 实现路径

复用 inline_code::InlineFlow 的 FlowLayout/Fragment，而非移植 demo 的直接 ShapedLine paint。片段仍由既有 SelectableStyledText/InlineCodeText 生成 TextLayout，供选择、图标、虚线、命中与预览登记共同使用。
新策略明确限定在 assistant SelectionDocumentBuilder，默认路径不变。flow 中加入可选中文禁则边界：使用 shaped glyph 边界，保持正向推进；最小不可分标点组可溢出；重试只接受严格缩短的边界。
assistant 行内代码采用 .92 倍继承字号、6px 水平 padding、1px 垂直 padding 和 6px 圆角；默认共享策略保留旧值。P1 不提前替换各块的正文字号/行距，F=16/1.625 的全块视觉切换留 P2。

排版先按实际片段字号/图标/padding 计算可容纳字形范围，再以整行上下文恢复词边界并应用禁则；不可分最小组允许溢出，所有重试必须严格缩短。片段保留字号、行高和基线偏移；使用同样的 style refinement 创建并绘制片段 TextLayout，选择与链接继续消费该 TextLayout。行高至少容纳实际 ascent/descent 和背景，所有片段共享基线。背景只绘制，不注入复制字节。行盒按 Window::pixel_snap 与 TextLayout 同规则取整后计算基线。代码标签链接以片段元数据携带前置 padding，图标绘制、命中与预览登记统一读取该偏移，保持图标与代码底之间 4px 间隔；默认链接偏移为零。

代码块及非 assistant 入口不启用新行内策略。静态审查、云端与用户验证分别记录；源码完成不代表 P1 验收完成。

## 验收矩阵与停止条件

| 项目 | 观察方法/执行者 | 停止条件 |
|---|---|---|
| 基线与范围 | 主 Agent 静态核对路径、哈希及差异 | demo 覆盖、共享入口意外启用 |
| CJK/Latin/emoji/组合字符、极窄标点 | Actions 字形布局回归；用户真实窗口核对 | 非字符边界、无进展、标点拆组、漏字 |
| 强调/代码混排与高度 | Actions 布局与字符范围；用户浅深窄宽 | 测量/绘制偏离、基线错位 |
| 正反跨行拖选与复制 | 用户；Actions 可观察范围/文本 | padding/图标入文、重复遗漏、旧选区 |
| 同 URL 多实例、跨行代码标签链接 | Actions 交互及用户预览 | 身份丢失、误打开、重复图标/请求 |
| hover/focus/取消/键盘 | 用户；Actions 生命周期 | hover 重排、陈旧目标、焦点错误 |
| 虚拟列表与资源 | 后续 P4/P5 云端测量；本阶段保持接线 | #493 回退、全历史重排、无界对象 |

源码检查不能代替运行验证。P1 结束必须有非实现者独立审查、云端编译/必要回归和用户交互证据。安全、内容丢失、错误打开或崩溃均硬停止。P2 全块样式、P3 流式优化、P4 滚动、P5 全量链接/性能与 P6 默认切换仍未授权。

## 当前验证阶段

六个生产文件已通过格式与 diff 静态检查。非实现者已复核首个接缝、混合字号增量及图标偏移/像素取整修正；最终两项生产发现经增量静态检查已解决。

首轮云端版本 0986c5a80cbf6fdffc84c864e7a49a713ebdd9d2：[Actions 40627435365](https://github.com/puzige/vega/actions/runs/40627435365)。格式、Clippy 和测试编译成功；Nextest 发现 3 项、执行 3 项，0 通过、3 失败、0 跳过。两个几何测试在非绘制阶段调用 Window::with_text_style 而 panic；组合字符测试在 NoopTextSystem 后端断言失败。默认 TestAppContext 使用逐 Unicode scalar 的替身后端，不能作为真实字体/字形簇验证证据。用户交互及性能数据未取得。

开发分支临时 Rust 回归仅用于 Actions。修正测试接线时，几何断言经真实元素 prepaint 阶段执行，并显式确认回调已运行；片段原始 advance 对比 TextLayout 的 shaped line advance，另核对向上取整后的布局盒宽度及字形包含关系，不把原始 advance 与取整盒宽混为一谈。组合字符与 ZWJ 不拆分断言保持不变，不删除、ignore 或放宽。

用户后续明确允许临时云端主线程入口及专用 Actions 执行接线。采用独立 harness=false 开发测试目标，在 main 获取 current_platform(true).text_system() 并注入 HeadlessAppContext::new；使用系统字体、无 GPU 截图、无完整应用启动、无外部进程/网络 E2E。目标通过临时 feature 和 cfg(test) 隔离，复用本 crate 同一份生产源码，不扩大生产公共 API、不新增依赖。原三项断言迁移到该入口而非删除；主线程 runner 显式输出每项开始/通过和总计，断言 panic 使命令失败。默认 nextest 不发现此自定义目标，因此独立记录默认套件计数与原生入口 3 项的实际结果，绝不把空集标成回归通过。专用 CI 步骤先显式构建该目标，再直接执行开发测试程序以保留进程主线程；所有测试/feature/专用接线在验证留痕后清理，通用门禁及 Python 覆盖不降低。

目标覆盖仍为 UTF-8/标点分片、混合字号/行盒与 TextLayout 几何、代码链接图标间隔、跨行与同 URL 实例身份、共享默认路径。helper 几何结果不证明生产完整 prepaint/paint 接线、真实点击、拖选、联网或原生像素验收。仅启用已有锁定 GPUI Kit 的 test-support，不升级依赖；测试及独占接线/依赖需在将来合并前留存可恢复证据并清理。本轮不合并，因此不为取得空集绿色提前删除必要回归。

验证分支在修正前已合并 origin/master 9cf826730bc3f6951eb165c1b8e66f75fba3e63d，集成提交为 5d711ba58e98cb356ff73651b1c564667f14e484。原 demo 工作区未改动。
