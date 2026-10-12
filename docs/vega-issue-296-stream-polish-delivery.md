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
