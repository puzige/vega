# Issue #296 · 单一动态过程入口交付记录

2026-10-12。需求与先行矩阵见 [spec](vega-issue-296-single-activity.md)。基线 `19e4122`，保留 #304 输出边界及 #295 Composer 底栏规则。用户截图与真实会话不进入公开仓库。

## 实现

每个可靠 run 仅保留一个过程入口，标签随当前思考摘要或真实运行的工具集合更新；展开后按源顺序查看历史操作。正文在运行中保持可读，可信 final 到达后继续遵守原来的选择、焦点、手动展开与回看保护。产物、权限和消息级错误维持外显。

四类过程行共用固定右侧箭头槽，只有 hover 或键盘焦点时显示箭头。时长紧跟标签，图标按思考、工具、等待和终态表达语义，过程头不再使用刷新图标。聚合详情内的思考不重复播放 shimmer。

只修改 UI 展示及共享尺寸 token，无依赖、数据迁移、运行调度、模型或权限配置变更。与 spec 偏离：无。

## 本地定向证据

先行 S01/S02/S03 测试在旧实现上失败，保留原始输出：

```sh
cargo nextest run -p vega_ui issue296_single_activity
```

```text
Summary [   0.040s] 3 tests run: 0 passed, 3 failed, 647 skipped
SHA256 cc7b53b7ee28732329b7a8c06b7c13b8180b8ea77bf8fc7332c164f57f073856
```

实现及受影响回归的定向命令：

```sh
cargo nextest run -p vega_ui -E 'test(/conversation_stream::tests::(issue296|issue146|issue151|issue70|issue72|issue147|issue148|thinking|permissions_cards|timeline)/) | test(/tool_card::tests::/) | test(/tool_activity_group::/)'
```

退出码 0，原始 footer：

```text
Summary [   1.110s] 158 tests run: 158 passed, 500 skipped
SHA256 a654df8be8d84072181d38ca36325f9648571749fca38b6e936f9c079eea6169
```

| 矩阵 | 证据范围 |
|---|---|
| S01/S02 | 单一入口、外部正文、历史顺序、动态摘要/并行工具、待批准与真实运行区分 |
| S03/S04 | 四类箭头的右侧几何、实际绘制透明度、键盘焦点与激活、状态图标、唯一 shimmer |
| S05/S06 | final/未知历史、原始 copy identity、展开后真实拖选并复制 CJK/emoji、权限和产物外显 |
| S07/S08 | 长会话虚拟列表、隐藏详情锚点、滚动交接、窄列与主题绘制 |

箭头绘制证据来自同一透明度父元素下的 GPUI 测试探针；复制使用真实生产拖选和 Cmd-C 路径。它们属于进程内行为与绘制验证，不冒充安装应用的像素或真实服务验收。没有运行本地 workspace 全量门禁。

## 独立审查与交付

独立 reviewer 首轮指出：展开后两处 shimmer、未开始的工具被标成运行。已修复并加入回归。末轮补充了外显产物段锚点保护：当前仍显示产物的 source 段保持外层位置；同轮隐藏工具仍可进入详情。没有修改历史产物持久化结构，也不声称覆盖未持久化产物的重新挂载流程。

该补充通过生产 `ArtifactService.capture` / `apply_artifact_card` 构造测试，先红后绿：

```sh
cargo nextest run -p vega_ui issue296_single_activity_visible_artifact_anchor_stays_outside_details
cargo nextest run -p vega_ui -E 'test(/issue296_single_activity/) | test(/issue148_long_session_scroll/)'
```

```text
Summary [   0.049s] 1 test run: 0 passed, 1 failed, 658 skipped
SHA256 c20bb08445f7a3445badfc781fcfe8eb109ec4ac5cd63ea4484e55533d41eaba
Summary [   0.739s] 21 tests run: 21 passed, 638 skipped
SHA256 849917edc17924ff28046d05ae61ce41d57ecf8a40587eded63578583a062471
```

独立最终静态审查通过，无剩余 P1/P2；核证了不可展开行的 Tab 排除、隐藏正文的内部偏移、外显产物的外层偏移、原始复制身份和实际绘制显隐。

首轮 [PR #305 云端门禁](https://github.com/puzige/vega/actions/runs/38155758932) 的 fmt/Clippy 通过，nextest 原始结果为 `2214 tests run: 2212 passed, 2 failed, 5 skipped`。日志 SHA256 `72ceb06c77990e2a3ba2e871eac58f3bf305c345d17f04bf2fbe8f7dd876f6a5`。

两处失败经独立复核均来自本 spec 已替代的默认展开断言：纯思考回复改为先点击唯一过程入口再验证摘要与详情可达；压缩后失败用例只更正默认 collapsed，并保留失败状态、2345ms、终态、压缩记录、HTTP 503 错误及两类提示外显断言。不改生产逻辑，不删除用例或放宽业务断言。

云端门禁、PR/合并及实际安装身份以 Issue #296 后续交付评论为准。合并后卡片保留 In review，等待用户手测；不提前关闭、删除分支或工作树。回滚仅撤销本次 UI 投影，不改会话数据。
CI 展示断言修订后的定向命令（退出码 0）：

```sh
cargo nextest run -p vega -p vega_ui -E 'test(i61_v3_thinking_only_completion_keeps_summary_with_collapsed_details) | test(issue168_compaction_row_remains_visible_after_primary_provider_failure)'
```

```text
Summary [   0.344s] 2 tests run: 2 passed, 908 skipped
SHA256 a317cd0207274f891223f830def2adc2af1a4f34edcf2dff5f05fd17aebef694
```

增量独立审查通过；未修改生产逻辑。
