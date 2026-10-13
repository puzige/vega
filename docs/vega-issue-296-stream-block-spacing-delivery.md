# Issue #296 · 信息流工具块间距交付记录

2026-10-13，规格：[信息流工具块间距一致性](vega-issue-296-stream-block-spacing.md)。实现切片基于 `origin/master@ff5db04` 复核；实现提交以本切片的 Git 历史为准。本记录不包含用户原始截图、会话文字或工具参数。

## 根因与结果

`stage_top_insets` 已为有摘要的阶段计算时钟后16、说明后24逻辑像素顶部间距，但无摘要的纯工具 segment 在实际渲染和估高时均忽略了 `top`。独立 `Tool`/`ToolGroup` 和可信 Final 手动展开后的过程详情也没有相同的说明→工具块外距。过程收起时工具行被隐藏，剩余说明段自带顶部间距，所以用户看到收起后间距似乎恢复。

修复把同一顶部间距应用到无摘要的工具 segment，并同步 segment 估高；独立工具块按可见前项应用一次间距，锚点估高使用同一结果；Final 展开的过程详情按说明→首操作边界应用一次间距并同步有界详情估高。同组后续工具保持既有内部行距。只有产物的 segment 保持原高度与外显位置。没有改源事件、原文、权限或存储。

## 验收矩阵与证据

| ID | 结果 | 证据 |
|---|---|---|
| B01 | PASS | GPUI 行盒：时钟分隔线→无摘要工具从原0变为16；单工具行仍25。 |
| B02 | PASS | GPUI 行盒：说明→无摘要工具组从原0变为24；组内不重复加外距；下一说明→有摘要阶段仍24。 |
| B03 | PASS | 可信 Final 自动收起后手动展开，说明→第一操作24，同组下一操作25，后续说明→操作24；再次收起无详情残留，原文 `ABOK` 不变。 |
| B04 | PASS | 历史恢复、独立单工具/工具组相邻说明、仅产物 segment 的渲染及估高定向测试通过；关联阶段、锚点和阅读回归通过。 |

缺陷 RED：

```text
cargo nextest run -p vega_ui -E 'test(/issue296_spacing_b0[12]/)'
exit 100; 2 tests run: 0 passed, 2 failed, 731 skipped
B01 actual 0px, expected 16px; B02 actual 0px, expected 24px
raw log SHA-256: 3a3fe497f832051132279c76d9a197e30f8d3ae2fd97c44368b6a3336a15b7aa
```

基线更新至 `ff5db04` 后的定向 GREEN：

```text
cargo nextest run -p vega_ui -E 'test(/issue296_spacing/) | test(/issue296_expansion_p06/) | test(/issue296_expansion_p07/) | test(/issue296_v2_process_spacing/) | test(/issue296_codex_stream/)'
exit 0; 28 tests run: 28 passed, 716 skipped
raw log SHA-256: d69822daf459a509ed913aed42836c8eb131a0b9d22002d6abefddae87a5734a
cargo fmt --all -- --check
exit 0
git diff --check
exit 0
```

规格偏离：无。自动证据是进程内 GPUI 布局、顺序与估高断言；未做真实应用像素验收，也未安装、启动或中断用户当前 Vega。用户更新后手测 M01：运行中观察说明后新工具的块距、展开多条工具的内部行距、可信 Final 后收起与重新展开、窄窗口和长详情滚动。无数据迁移；如需回滚，revert 本切片的 squash commit。
