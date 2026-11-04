# 即时折叠开发验证记录

关联 #876 / PR #881；基线 `9d3803dadcf30149a55fad116501c815651c3d07`。

## 实现与独审

- 普通点击不准备全任务拖拽排序，真正拖拽开始才冻结投影。
- 排序/分区和焦点集合缓存按任务、偏好、项目实际变化失效；置顶、归档、删除、重命名和拖拽结束均接入。折叠回投只改轻量字段，不复制/统计整个任务集合。
- UI 同步叠加最新本地意图；后台单批串行、每目标只留最新意图。失败只回退对应未被新点击取代的状态，错误保留到对应成功重试。刷新与通用请求串行并保留本地意图；切库/销毁取消旧保存，数据库和 generation 同时隔离旧回投。
- 最小事务只合并当前 collapsed 并更新 revision，验证项目/组存在，保留偏好/排序/成员关系。事务基版本与完整快照版本连续才推进完整快照版本；外部写入间隙触发协调刷新，不能绕过通用请求冲突检测。
- 非实现者独审在 `0e351ef4c322f3220dde34cbf0199c90bdfc5868` 发现轻量结果误认证整份元数据版本的 P2。生产修正 `8b2c3fb59a22a4d345d8810e0b2cba26ef02102d` 的增量独审确认关闭该路径；同次指出新增测试误用 SetPreferences 改手动顺序，按既有保序契约仅修测试。`cf18ebea25e0906708c666bcabcaf286291db523` 测试差异独审无发现。最终源码在此后只补测试类型导入，无生产变化。清理版本仍须增量独审及最终 required CI。

## 实际云端执行

所有行为执行仅 GitHub Actions；本地只固定 Rust 1.98 格式和 git diff 静态检查。

| 精确开发 HEAD | Actions run | 实际结果 |
|---|---|---|
| `0e351ef4c322f3220dde34cbf0199c90bdfc5868` | [41757148211](https://github.com/puzige/vega/actions/runs/41757148211) | Rust 28 执行/28 通过/0 跳过；Clippy 因未使用导入、可合并条件、多余借用失败，未绕过 |
| `8b2c3fb59a22a4d345d8810e0b2cba26ef02102d` | [41758944250](https://github.com/puzige/vega/actions/runs/41758944250) | Clippy 成功；Rust 29 执行/28 通过/1 失败/0 跳过，失败为测试误用保序偏好动作（`[] != [thread-b,thread-a]`） |
| `cf18ebea25e0906708c666bcabcaf286291db523` | [41759976627](https://github.com/puzige/vega/actions/runs/41759976627) | 新测试漏导入 SidebarProjectView 导致编译失败，不是行为通过 |
| `06d8b9bd0135cddfe840a0a494ebaa5c70a4738e` | [41760794307](https://github.com/puzige/vega/actions/runs/41760794307) | Nextest 29 执行/29 通过/0 跳过；Python 5+15+40=60 通过；fmt、全目标 Clippy、required check 全成功 |

成功 run 的 Nextest job `126347753343`，Clippy job `126347753654`，required 汇总 `126349467903`。命令由既有 PR Check 执行：三组 Python、`cargo nextest list --workspace --message-format json`、非空 `cargo nextest run --workspace`；fmt 与 `cargo clippy --workspace --all-targets -- -D warnings`。无本地编译、运行或测试。

## 可恢复开发测试与最终清理

成功来源完整 SHA：`06d8b9bd0135cddfe840a0a494ebaa5c70a4738e`，清理前已用 `git ls-remote origin refs/pull/881/head` 读回一致。原始两文件补丁：[instant-collapse-development-tests.patch](instant-collapse-development-tests.patch)，SHA-256 `b5c2fdee85d8881d10706804673b780af8f0edc920964893e3355be988ba5b7a`，1419 行原始测试源码，可在独立开发工作树用 `git apply` 恢复。开发时 UI 专用依赖为 workspace gpui_kit 的 `test-support` 与 tempfile；模块接线分别 `#[cfg(test)] mod tests;` 和 `#[cfg(test)] mod collapse_tests;`。本补丁仅留档，不在构建路径。

14 项存储回归包括最小写入保持其他字段、合并并发连接、无变化不增版本、重复意图、删除目标拒绝、SQL/commit 失败原子回滚、无关任务数据损坏不影响轻量保存、版本上限、取消与重入、旧保存取消、重开保持。15 项 UI/状态回归包括奇偶快速点击、多目标合并、同值不同代旧成功、旧失败、新意图优先、最新失败回退与重试、旧 revision、删除过滤、真实写许可占用下即时回调、刷新回投、切库/generation、真实 SQLite abort trigger、投影失效与外部偏好不丢失。

最终清理仅删除两个开发测试文件、两处 test 模块声明、UI dev-dependencies；不改变生产实现、安全断言、benchmark、Python/fixture 或通用 CI。清理版本本地静态检查通过；最终无测试 HEAD 的完整 required CI 尚待执行，不能把开发结果冒充最终门禁。

## 未验证与用户手测

无实际原生时延测量，不承诺毫秒预算。真实鼠标/Enter/Space/拖拽、窄窗/主题、屏幕阅读器与真实存储故障体验仍待更新后用户手测；GPUI 回调测试不等同于 rendered mouse-down 事件或真实桌面验收。未安装、启动或重启本机 Vega；任务保持开放，合并后 In review，不自动 Done。
