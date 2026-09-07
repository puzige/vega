# R12 — 主 Agent 验收与交付状态

## 当前结论

R12 任务菜单、窗口导航和实时分支已实现并通过实际客户端检查。最终应用代码 `f2340d2`，交付在 `codex/vega-review-integration`；可运行签名副本在外部验收目录 `native-r12/Vega.app`。当前单实例显示浅色 1280×750 三栏，右栏为真实 README 只读预览。

**最新完整 workspace：948 passed / 1 failed / 0 ignored，exit 101。整体门禁未全绿。** 唯一失败位于未改动的 Git fault fixture，详见下文；新增菜单指针回归已在该次全量中通过。上一版 `c53752e` 完整 workspace 为 948/0/0，不能替代最终结果。没有 push、master 合并或安装版替换。

## 规格与实现

- [总规格](vega-r12-task-navigation.md)、[任务动作](vega-r12-task-actions.md)、[导航交付](vega-r12-navigation-delivery.md)、[实时分支交付](vega-r12-live-branch-delivery.md)。三个 Astra medium executor 使用独立 sibling worktree；主 Agent 负责规格裁决、交叉审查、集成和 native CUA。
- 更多菜单：置顶、重命名、归档/恢复、标记未读/已读、Finder、复制项目路径、复制会话 ID、设置和删除确认。不存在独立任务/日志目录的能力仍不伪造。新写入/路径解析使用后台服务；确认只合并对应字段。
- 导航：后退/前进、Cmd+[ / Cmd+]、设置返回、删除/归档条目跳过；最多100条窗口历史。草稿仅保留字符串，最多100个缓存任务/总1MiB；容量拒绝有提示，不能静默丢文字。纯设置/空页返回不依赖数据库可用。
- 路由先只读解析，再由前台最终确认草稿容量和 mutation epoch；接受后异步记录真正访问，清未读/更新时间，不重新安装旧 Thread 快照。输入编辑和 IME 有局部保护，连续导航不依赖已销毁编辑器的焦点。
- 实时分支：后台单 worker、合并请求、身份/路径/generation守卫；普通仓库和固定拓扑 linked worktree、detached、非Git/错误状态。4KiB元数据、128项目一批、250ms协作预算、1秒UI过期、约2秒目标刷新；符号链接/非标准Git布局返回Unknown。无新的Git写入、runner超时调整或依赖。

## Freeze 与统一检查

- Git source HEAD: `f2340d207e8f73b3bdc87b97a882c2941b1f4773`。验证时 tracked diff 为空；交付文档随后单独提交。
- 验证开始 UTC: `2026-09-06T04:51:28.893428+00:00`；Darwin arm64，rustc/cargo1.98.0，git2.55.0。
- 每轮 native 启动前先退出旧 Vega 并用进程清单确认0实例；统一检查时 native 已退出。配置只复制自有 config.toml/reasoning.toml，无凭据复制、Keychain访问或真实供应商调用。

| Exact command | Result | Duration | Raw log / SHA256 |
|---|---|---|---|
| cargo fmt --all -- --check | PASS | 1.013 s | `/private/tmp/vega-r12-final-fmt.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| cargo clippy --workspace --all-targets --locked -- -D warnings | PASS | 2.337 s | `/private/tmp/vega-r12-final-clippy.log` / `1be0cc0c996fb0464a26b1360769a1b60c0e5d46dbe596af175be369b4e7ad2e` |
| cargo test --workspace --locked --no-fail-fast | FAIL: 948 passed / 1 failed / 0 ignored | 92.861 s | `/private/tmp/vega-r12-final-workspace-tests.log` / `f5b6f615d5498ac4793a9996010cba88e879c48f2f4e1a7e191e095291e0509e` |
| cargo build --workspace --locked | PASS | 3.365 s | `/private/tmp/vega-r12-final-build.log` / `87ef59b4bde9d5f490e2a84c41297a9834e49a213a013cb0c98d0de772c2d8c5` |

`cargo tree -p vega_runtime --edges normal --locked` 未出现 GPUI/UI/theme依赖；Cargo.toml/Cargo.lock、vega_runtime和git_workspace相对R12基线无改动，`git diff --check`通过。外部 `verification-manifest.json` 记录36个当前/早期检查和首次失败日志的哈希，未丢弃失败证据。

## 实际客户端验收

| 范围 | 实际观察与证据边界 |
|---|---|
| 菜单和未读 | 960×600九项菜单完整；键盘箭头/Enter标当前Alpha未读，置顶刷新后点仍保留，真正重访清除；只读DB复核。Tab与故障恢复由生产GPUI/file DB测试补充。 |
| 重命名和捕获目标 | 双击和菜单重命名都走真实UI；最终鼠标重命名非当前Beta为R12 Beta checked，Gamma页面和草稿保持。 |
| 复制/Finder | 会话ID与自有DB目标一致；最终f2340d2复制项目路径粘贴为真实注册路径，鼠标覆盖另一任务行也不切换页面；Finder实际选中对应项目。 |
| 导航和草稿 | c53752e连续Cmd[→Cmd]无需重新点击，Alpha/Beta各自草稿恢复；设置菜单→Esc保留草稿；后退后新建任务丢弃前进分支。f2340d2保留相同导航代码。 |
| 失效历史 | 真实Alpha→Beta→Gamma，然后归档非当前Beta，后退跳过Beta并恢复Alpha；恢复菜单返回活动列表。最终确认删除期间的异步竞态由真实生产根视图/owned DB测试验证。 |
| 实时分支 | linked worktree先注册、owner未注册由只读DB确认；外部checkout到另一分支及detached后，侧栏自动变化；最后恢复原分支。普通仓库实际checkout通过挂载ProjectsBlock生产测试，未另做native主仓库切换。 |
| 布局组合 | 960×600与1280×750、浅/深色、240px菜单、标题栏96px避让；隐藏侧栏后实际点击前进/后退有效；最终真实文件搜索→README预览组成三栏。 |
| 持久化 | 多次重启保留任务名、置顶和归档恢复状态；未读重启由真实文件DB reopen验证。草稿和历史不声明跨重启保留。3个自有任务、0条消息；测试文字均未发送。 |

Native原始图像保留在当前任务的CUA工具输出；外部 `native-checklist.json`、`native-preview-findings.json`、`owned-state-readback.json` 和应用 provenance 记录确切版本与边界。没有把程序化测试、模拟供应商或未运行项写成原生通过。

## 审查与 native 发现的修复

- 真实访问从 metadata open 中分离；后台结果不能清除被拒绝导航的目标未读，也不能重新打开等待期间被删除/归档的目标。
- 原生连续快捷键首败在根视图测试复现后修正；保留首次红灯及后续7项导航结果。
- 原生标题栏按钮覆盖系统控件，修正96px leading inset；菜单由侧栏宽度改为独立240px token。
- c53752e原生鼠标点击复制路径会穿透到背后Alpha，导致当前路由改变、后台复制被丢弃。挂载960×600指针测试先复现再修复popup occlusion和事件传播；f2340d2实际复制、Finder与非当前重命名再次通过。不能用先前键盘测试代替这条鼠标证据。

## 未关闭的完整门禁

最终失败：`git_workspace::trusted_git::tests::runner_mutation::service_reports_authoritative_state_after_add_and_commit_process_failures`，`stdout-overflow`分支预期`Failed(OutputTooLarge)`，实际`Failed(TimedOut)`。测试用3秒mutation期限，先真实tee/Git，再由Python写1MiB+1；现有输出不足以确认耗时发生在哪个阶段。

Git子树在c53752e、f2340d2和独立检查分支完全一致。精确测试仅复跑一次，1/0/0、26.81秒，日志 `/private/tmp/vega-r12-b-git-failure-once.log`，SHA256 `95e957f6af617c0005d7a3c9a100d0c70383b1d692769672442cb3d8d8bdf34f`。这支持时序敏感性，不能将最终全量标绿或宣称根因已修复。未放宽超时、断言、ignore或靠重复全量求绿。

R11的Diff retry、descendant readiness和InitialDrain三项历史失败仍保留在[R11验收](vega-r11-acceptance.md)，本轮没有重写Git runner。该时序诊断单独跟踪，功能/UI优先的用户裁决继续生效。

## 边界与后续

性能bench/soak按用户裁决延期；真实供应商请求、真实系统IME候选窗和异常文件系统硬超时未原生验收。没有读取旧Keychain或真实Key。未实现任务/日志独立目录、trace、反馈渠道或外部编辑器选择。

后续优先项目分组/筛选/排序/收起全部，再推进模型连接测试/发现、外观与通知设置；完整103项对照仍见[R11矩阵](vega-r11-zcode-button-matrix.md)。
