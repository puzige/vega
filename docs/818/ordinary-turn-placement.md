# 普通发送的新 turn 置顶接线修复

## 根因与范围

v0.1.350 已包含 #809 的 TurnPlacement，但普通发送从 `begin_ordinary_pending` 调用 `project_ordinary_submission`，没有开始放置。`pending_messages` 的 begin 不覆盖该路径，消息已投影时后续落地还会直接返回。

只在 `begin_ordinary_pending` 本次投影确实新增条目后，用投影前长度定位 turn 第一项。共享 `project_ordinary_submission` 保持无置顶副作用；不改布局、prepaint、其他发送后端、存储、用户文案或依赖版本。

## 验收与回归

| 场景 | 可观察契约 | 自动回归边界 |
|---|---|---|
| 本次普通发送短文本/图片 | 新 turn 第一项成为逻辑滚动顶部，开启已有置顶/留白状态，关闭 Tail | 调用真实 begin_ordinary_pending，断言新增条目及 ListState 滚动位置/模式 |
| 用户已滚到历史位置，重复投影同一提交 | 不重复追加，不改位置/模式，不重新开始置顶 | 调用真实 project_ordinary_submission；再投递相同 begin 也不抢滚动 |
| 重新打开会话恢复普通提交 | 可补齐消息但不置顶，不启动临时留白，不改变历史位置/模式 | 调用真实恢复入口 restore_ordinary_history_entries，验证其共用投影无置顶副作用 |

测试使用隔离的内存 Store、真实 admission/submission 与 GPUI TestAppContext，不访问用户数据库、模型、网络或原生桌面。首帧像素及布局留白仍属于既有用户手测边界，逻辑状态测试不冒充像素验收。

## 本轮交付边界

初轮按用户要求停在 draft PR，2026-11-01 用户进一步授权合并 PR #822。自动测试仅由现有 Actions 执行，本地仅格式/差异/静态检查。合并前清理开发 Rust 回归及 test-support 专用接线，生产修复保持不变；最终 head 必须经完整 required CI 确认后合并。合并会触发既有 master 云端发布，不额外安装或重启应用；#818 保持开放，合并后 In review 等待用户手测。

## 开发回归留痕

- 测试及修复版本：`80cd4eda733f94baa38d1c3fe2d8ce4b1ceeb57b`，基线 `632a5a2e554cecadb06d2a9bfa53e5fc3d6730a2`。
- [原始测试源码](https://github.com/puzige/vega/blob/80cd4eda733f94baa38d1c3fe2d8ce4b1ceeb57b/crates/vega_ui/src/conversation_stream/ordinary_submissions/placement_tests.rs)，Git blob `9eb0faee0f26168954c2b2d1c6ed1e648335bdd6`；SHA-256 `495e5a48f032e8cdf1f7fa9dad1dd1fe55edb5914483850c59d5156c573a18eb`。合并前已通过 GitHub contents API 读回核实可访问。
- [原始提交 patch](https://github.com/puzige/vega/commit/80cd4eda733f94baa38d1c3fe2d8ce4b1ceeb57b.patch) 包含测试、专用依赖和模块接线，清理后仍可恢复。
- [Actions 41277413675](https://github.com/puzige/vega/actions/runs/41277413675)：`cargo nextest list --workspace --message-format json` 发现 3；`cargo nextest run --workspace` 执行 3、通过 3、跳过 0。三项分别为普通发送（文本/纯图片/图文）、重复投影保留用户位置、历史恢复补齐但不置顶。
- 同版本格式、全目标 Clippy、required check 和既有 Python 5 / 15 / 40 项均通过；非实现者静态审查无阻断发现。
- 最终删测 head 的空集 CI 与上述行为回归结果分开记录；`no Rust tests` 不等于三项行为重新执行或原生 UI 手测通过。
