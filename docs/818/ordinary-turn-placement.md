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

用户明确要求先审阅 diff 与测试结果：仅在独立 worktree/开发分支及 draft PR 验证，不合并、不发布、不安装、不关闭 #818。自动测试仅由现有 Actions 执行，本地仅格式/差异/静态检查。开发 Rust 回归及 test-support 接线保留在开发分支供审阅；未来合并前必须另按仓库规则固定证据并清理，不能进入 master。
