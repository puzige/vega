# Issue #296 · 工具操作语义图标交付

实现依据：[冻结规格](vega-issue-296-tool-icons.md)。基线为 `5947e042fd600f814ddefc091895671bbbed9675`，分支 `feat/296-stream-polish`。本记录使用合成测试数据，不包含真实会话或私有路径。

## 变更与验收证据

生产改动仅涉及 `icons.rs`、`tool_card.rs`、`tool_activity_group.rs`。Read、Find、Edit、MCP、Skill、Codex ACP 和 ComputerUse 使用各自语义图标；ComputerUse 仅匹配已有安全类型化 action。损坏投影优先降级为扳手。同图标组使用共同图标，混合和空组使用扳手，子项更新时按当前投影重新计算。

BookOpen 和 Globe 复用锁定 GPUI Kit 的本地资源，其他缺失图形沿用共享内联 SVG 渲染方式。保持 24 viewBox、2 源描边、圆端点/连接；原 16px 槽、25px 行高、6px 图文间距和中性色未改。没有新增依赖、协议、存储或权限逻辑。

| 规格 | 实际证据 |
|---|---|
| I01 | 扩展既有 `tool_activity_leading_visual_is_category_owned_and_neutral_for_every_state`，覆盖类别、真实 ComputerUse 动作及未知 action；经生产 ToolCall 投影、状态更新和历史恢复核对图标与可见文本 |
| I02 | 同一既有用例覆盖七种状态及浅深主题中性色；保留原状态与安全断言 |
| I03 | 既有混合工具组与历史恢复用例更新被替代的图标预期；仅新增 `issue296_computer_action_groups_use_current_icons`，覆盖空组、同为 observe、observe+screenshot、运行更新和损坏结果后的混合降级 |
| I04 | 审查共享 renderer 和本地图标资源，原几何代码未变；定向集合保留紧凑行对齐、焦点和 hover 回归。未新增重复 SVG 解析测试，真实轮廓观感仍属 M01 |
| I05 | 工具卡安全投影、历史恢复、详情、CUA、吸顶及相关信息流回归均纳入定向集合；原文和安全断言保留 |

## 定向检查

日志目录为 `~/Workspace/artifacts/vega-296-tool-icons/`，以下命令均在本卡 worktree 执行，使用默认 target。没有执行本地全量测试或 Clippy，没有安装、启动或操作原生 Vega。

先在旧生产实现上更新验收预期：

```sh
cargo nextest run -p vega_ui -E 'test(/tool_activity_leading_visual_is_category_owned_and_neutral_for_every_state/) | test(/issue70_t70_2_adjacent_mixed_tools_share_one_collapsed_item/)'
```

退出码 `100`，日志 `red.log`。两项分别检出旧类别图标映射及混合组取首项图标，没有 fixture 失败。

```text
Summary [   0.038s] 2 tests run: 0 passed, 2 failed, 714 skipped
SHA256 4d7e2a25d64883ade97366aaff33ee1d889578d73a08bc7c14623175e89ff24e
```

实现后的受影响集合：

```sh
cargo nextest run -p vega_ui -E 'test(/tool_card::/) | test(/tool_activity_group::/) | test(/issue70_tool_activity::/) | test(/issue296_single_activity::/) | test(/issue296_disclosure_focus_rings/) | test(/issue296_polish_p01/) | test(/issue296_sticky_/) | test(/cua_usability::/)'
```

退出码 `0`，日志 `green.log`。

```text
Summary [   3.182s] 66 tests run: 66 passed, 651 skipped
SHA256 baff6161fcae2af818aeafc2974d499ee77ffe0c2d0561e4120bf7a963d277f3
```

该次编译提示测试模块遗留的 `vega_theme::LIGHT` 未使用。仅删除此 import 后，重新编译并定向验证其所在用例：

```sh
cargo nextest run -p vega_ui -E 'test(/tool_activity_leading_visual_is_category_owned_and_neutral_for_every_state/)'
```

退出码 `0`，日志 `final.log`，未再出现本 crate 的 unused import 警告。三次日志均有既有依赖 `block v0.1.6` 的 future-incompatibility 提示，本轮未修改依赖。

```text
Summary [   0.017s] 1 test run: 1 passed, 716 skipped
SHA256 d8300606705b3a9d2d074984f14dab6de37b9808600734b5e99f9484a80e92b0
```

最终 `cargo fmt -p vega_ui -- --check` 与 `git diff --check` 均退出 `0`。

## 冻结与剩余边界

代码冻结时间：`2026-10-13 02:31:04 UTC` / `2026-10-13 10:31:04 +08:00`。环境为 macOS 15.8.1 / Darwin arm64；`rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`、`cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`、`git version 2.55.0`。

最终源码指纹：按以下相对路径排序，以 `sha256 + 两个空格 + path + 换行` 生成 manifest，再计算 SHA256 为 `7de90bae06333be05d45d039f77ff6fabd40860ab44fdb50e2c9cc05851c1bf9`。

```text
c4aa7a0cd6fe492b3c3246c9c8d43228ace204354a1965b19fd2f367a6782bef  crates/vega_ui/src/conversation_stream/tests/issue296_single_activity.rs
2af7cfcc987883687b12b72361db72dc5edb4a67aaa3f4953fd7dfbe76c5763e  crates/vega_ui/src/conversation_stream/tests/issue70_tool_activity.rs
9ade7c950e676d9851a582400b6bb28d12ac3d688a56b39aad04519c4f45fd98  crates/vega_ui/src/icons.rs
29e3891ea96dbfffc634083467fe8912100e731eaf0ce613bbd77e79645c5467  crates/vega_ui/src/tool_activity_group.rs
5ab1645e19b2bc5831a59b43e94588c718ea23c14f9c4ed8c35ae24f66b62d50  crates/vega_ui/src/tool_card.rs
```

M01 未进行：用户需在新版真实会话的浅深主题中确认书本、文件夹、铅笔、插头和各电脑动作轮廓可辨，图标重量及对齐一致。自动化与静态审查不替代原生像素观感验收。云端全量门禁由主控后续 PR 执行。本次无数据迁移，回滚为撤回本轮 UI 改动。
