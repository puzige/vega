# #381 最终澄清：耗时紧跟命令

2026-10-15，按 `docs/vega-tool-block-codex-parity.md` 的“最新用户澄清”实施。

## 变更

- `tool_card.rs` 仅调整命令行时间槽：放在现有 inline_chevron 调用之前，显示 `· 耗时`，删除将时间推向整行右侧的弹性空白。
- 保留时间槽不收缩、长命令单行省略；短命令、时间与原箭头相邻。未更改箭头实现或显隐/hover/focus/键盘/点击逻辑。
- 现有计时来源、human_duration/elapsed 单位格式、整体 Worked for、详情右下角图标及 Success/Failure 均保持。

## 回归与验证

先更新相关 GPUI 几何断言，再调整实现。原固定右边界断言迁移为标题→时间→箭头各相隔共享间距；480/1000 宽下短行不被推右、长行省略、时间宽度与短行一致且足以显示、箭头不溢出。无时长时不渲染时间槽，原标题与箭头仍相邻。保留独立/实时/终态/历史计时、footer 滚动及图标位置和隐私回归。

本地 `cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 均 exit 0；未编译、运行测试或 Clippy。功能回归待云端 PR Check / CI，真实外观和交互等待用户手测。不提交或 push。
