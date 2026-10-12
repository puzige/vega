# Issue #296 · 信息流行为修复交付草稿

状态：行为切片已实现并完成进程内定向回归；2026-10-12 用户追加的 Codex 截图视觉切片待追加规格与实施。本记录不宣称已提交、合并、发布、安装或通过用户手测。

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
- PENDING：用户新提供的六张 Codex 截图视觉切片。本稿只记录行为基线；新视觉规格实现后补测并更新交付记录。
- 手测：发送会触发多次工具的请求，确认说明后就地出现操作；Final 后过程收起且答案外显；展开可读原说明与工具；运行中选字/展开/上滚应保持阅读，退出选区与焦点或回尾后才收起；重开历史与跳转锚点应到达同一正文/工具，产物与权限提示继续可见。
