# #381 手测修订：标题时间与结果 footer

契约：`docs/vega-tool-block-codex-parity.md` 的用户手测修订，2026-10-15。

## 实现

- `crates/vega_ui/src/tool_card.rs`：时间取自原运行时钟/结果 duration_ms，放在命令整行最右的不收缩槽位；命令仍为单行省略，箭头与键盘入口保留。批准未运行、缺失历史时长不造数值。
- 展开详情的最后一行在原滚动容器内右对齐，使用 intrinsic-width 图标与文字组，修正原全宽子项导致 justify_end 无效的问题。成功 `Check + Success` 使用低强调文字色；失败 `Close + Failure` 使用 danger；两者都不再重复 exit/耗时。拒绝、取消、待授权、运行中保留准确文字。
- footer 保留 ROW_HEIGHT 下限与 flex_shrink_0；真实输出和完整命令未改写。原截断/复用提示仅在真实标记存在时显示于 Shell 类型行。底层 metadata、权限投影、回合 Worked for 均未修改。

## 回归

先新增 GPUI 几何回归后实施。覆盖 480/1000 宽下短长命令时间边缘一致、时间不收缩、长标题单行限宽、箭头与时间无重叠；终态时间及 footer intrinsic 右对齐、图标、滚动内容内位置。原 0/1/2/65 秒、独立工具 5/3 秒、终态停止计时覆盖迁到标题时间；历史运行无起点与终态无时长不造时间。保留实体、失败颜色、拒绝/取消、完整命令、输出、折叠、滚动及隐私回归，旧 exit 展示断言按新契约替换为 Failure 与底层 metadata 断言。

## 验证

- `cargo fmt --all`：exit 0。
- `cargo fmt --all -- --check`：exit 0，无输出。
- `git diff --check`：exit 0，无输出。
- 未本地执行编译、cargo check、Clippy 或测试；新增及迁移测试待 PR Check / CI，不标记为通过。

真实浅深主题、窄窗口长命令、键盘展开/收起与长输出滚到底部由用户在合并后手测。无新增依赖、token、数据迁移或应用重启。
