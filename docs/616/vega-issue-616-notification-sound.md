# Vega 系统通知声音

## 范围与基线

- Issue：[#616](https://github.com/puzige/vega/issues/616)，标题为「Vega 通知没有声音」，正文为空。
- 规格 v1.0；代码基线 `76c164876542c679abe1bfb5f525f1ade2fcd2e4`；分支 `feat/616-notification-sound`。
- 本卡将 [#481 通知规格](../481/vega-issue-481-session-completion-notifications.md) 的「不新增声音」范围更新为：已有合格系统通知使用 macOS 默认通知声音，声音是否实际播放由系统授权、通知声音设置、专注模式及音量等系统策略决定。
- 保留既有触发条件、内容、本地化、tag、去重、未读和点击路由；不新增声音设置页面、自定义音效、独立音频播放或系统设置修改。
- 本地不运行 Vega、不构建或测试；自动检查仅交现有 Actions，真实 macOS 通知与声音由用户手测。

## 原始证据与根因

静态事实：

1. `crates/vega/src/window/notifications.rs` 在后台终态或待回答问题满足原有条件时，分别调用 `app_notifications::show` / `show_question`。
2. `crates/vega/src/app_notifications.rs` 的两个入口使用 GPUI Kit 的 `App::show_system_notification(SystemNotification)`；通知 tag 与路由存入同一全局映射，响应通过 `on_system_notification_response` 回到原会话。
3. 工作区固定 `gpui-kit =0.6.1`，`Cargo.lock` 中实际平台依赖为 `gpui-pre =0.3.4` / `gpui-pre-macos =0.3.4`。
4. 该 GPUI 的 `SystemNotification` 只有 `tag`、`title`、`body`、`actions`，没有声音选项。
5. macOS 平台 `system_notifications.rs` 虽请求 `Alert | Sound` 授权，但创建 `UNMutableNotificationContent` 时不设置 `sound`；前台 delegate 只返回 `Banner | List`，不包含 `Sound`。仅申请声音权限不会给通知附加声音。
6. 额外只读核对发布包 `gpui-pre 0.3.9` / `gpui-pre-macos 0.3.9`：通知结构仍无声音字段，macOS 投递与前台展示仍缺同样的声音设置。因此仅升级到此快照不能修复。

根因位于通知平台适配层，而非 Vega 未读、触发或输出摘要逻辑。用户当前安装构建及系统设置尚未核验；本次不将静态根因冒充真实音频复现。

## 验收矩阵

| 场景 | 可观察结果 | 验证方式 |
|---|---|---|
| Vega 在后台，有合格输出的完成、失败或非用户中断 | 各通知保留原标题/摘要/路由，并请求系统默认通知声音 | 静态适配层核对；用户在正式 bundle 手测 |
| Vega 前台显示其他会话或 Settings | 原本应发送的通知仍发送，系统允许时具有声音 | 静态触发条件与前台 presentation options 核对；用户手测 |
| 当前会话真实可见 | 保持不通知，不额外响铃 | 原触发链路不变；用户手测 |
| 后台待回答问题 | 发送一次原问题通知并请求默认声音；解决/终止仍按原规则撤回 | 静态问题去重/生命周期核对；用户手测 |
| 重复终态、旧 generation、重复 pending 问题 | 不新增通知或独立声音副作用 | 原 ingress 与去重链路不变 |
| 用户取消、内部 handoff、无合格输出 | 保持原通知抑制；不单独播放音频 | 原触发链路不变 |
| 通知权限拒绝、声音关闭、专注模式或静音 | 尊重系统策略，不用独立播放器绕过；任务收尾和未读不受影响 | 原生 API 契约审查；用户手测 |
| 点击通知 | 激活 Vega，进入原会话，不启动新 run；已有错误恢复保留 | 适配层响应与路由接线审查；用户手测 |
| 非 app bundle / 非 macOS | 不引入崩溃或新的平台编译依赖问题 | 静态平台边界；云端现有门禁 |

## 实现前提与路线

当前已发布且核对过的 GPUI Kit API 无法表达本卡的声音需求。不得通过修改本机 Cargo 缓存、运行脚本播放声音、替换未知版本依赖或运行时方法交换绕过。

可行路线需明确授权后才能实施：

1. 推荐在获授权的 GPUI 上游/独立修补来源中只修 macOS 后端，再以可获取的固定版本或不可变 commit 接入 Vega。该后端已有 `objc2-user-notifications` 及默认声音类型，无需新增后端 crate；需要设置普通 `UNNotificationSound::defaultSound()` 并加入前台 `Sound` 展示选项，禁止 critical sound。现有 tag、delegate、授权和点击路线可以全部保留。当前外部上游仅有只读权限，也没有核验到包含修复的发布版本；维护独立修补来源及依赖来源覆盖必须先批准。若选择后端默认声音，会作用于 Vega 经该后端发出的所有已有系统通知；上游是否要求显式 opt-in 字段需另外核对。
2. 为 Vega 增加受控的 macOS 原生通知适配层，使用与当前依赖图兼容的 Objective-C 类型绑定，设置普通默认声音及前台 `Sound` 展示选项，保持相同 tag/点击路由/惰性授权。新增直接依赖必须先批准；新适配层不能与旧 GPUI 通知重复投递或争用 delegate。该路线需承担更多平台代码，不是首选最小改动。

在路线批准前只记录分析与规格，不编写生产代码。独立只读核查已确认不存在遗漏的 Vega 侧声音开关或原生通知内容 hook；组件层通知也构造同一无声音字段的请求，不是可用替代路线。

## 验证与交付边界

- 改动保持最小，不恢复历史 Rust 测试、不修改 Python 检查或 CI。
- 本地仅快速差异、格式和静态检查；实现独立审查与最新 head 的 required check `check (fmt, clippy, test)` 通过后才能合并。
- 最终 Rust 空集如由云端确认，记为 `no Rust tests`（0 发现 / 0 执行），不称测试通过。
- 当前仅完成静态诊断与规格；未运行 Vega、未构建、未测试、未取得本卡 CI 或实际音频结果。
- 合并后 Issue 保持开放、Project 为 In review，等待用户在正式发布构建上手测；不把 CI、合并或权限申请成功当作实际声音已播放。
- 回滚为撤销本卡实现并走现有 PR/发布流程；不涉及用户配置或数据库迁移。
