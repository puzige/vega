# 左侧列表与按钮写操作审计

关联 #864 / PR #866。2026-11-03 用户授权扩大为左侧入口写操作审计；本轮只审计和记录，不批量修复，不扩展为全应用审计。

## 版本与方法

- 远端待合并修复：`8770a2f6978073f0f0e2a0b916943bbec1200356`。本地 fetch 后同步 master `ce9c89e478715fa779ff290f058afe7cd0258cb7`，审计生产版本 `b9c5461487061ff8a80243e9542dcccd96a4c6b7`。四个任务提交的 range-diff 均为 `=`，新增基线为文本选择修复。
- 静态逐项追踪 `crates/vega_ui/src/sidebar/` 所有生产文件，覆盖两种组织投影、旧项目/会话块、行尾按钮、菜单、键盘与拖拽入口，以及左上角导航和显隐/宽度控件的直接处理链。
- 追到 conversation/store 层确认写许可和事务；不是仅搜索 SQL 或方法名称。没有运行应用、读取用户数据库、采样现场、编译或测试。此为实现风险审计，不证明每个入口已经发生现场转圈，也不替代非实现者对 PR 生产变更的独立审查。

## 结论与机制

仍有 **5 类数据库同步写入口**和 **1 组同步配置落盘入口**。项目访问与置顶已在 PR 后台化，但不等于整个侧栏已经没有同步持久化。

数据库共享写队列按数据库身份协调，`write_coordination::acquire` 同步等待条件变量，队列超时 10 秒（`crates/vega_store/src/write_coordination.rs:12,252–365`）；默认 SQLite busy timeout 2 秒（`crates/vega_store/src/lib.rs:128–175`）。这是不同等待层，不是固定耗时或整个操作的总上限。UI 回调一旦走入它们就可能冻结事件循环。

`TaskMutationState` 是 UI 层 pending/epoch 围栏，不是后台线程，也不覆盖所有 runtime 持久化：`crates/vega_ui/src/navigation.rs:297–328`。因此“先检查 busy，再 begin_task_mutation”不能消除数据库共享队列竞争。

## 仍在 UI 主线程写入：优先修复

| 入口 | 可验证调用链 | 风险与最小方向 |
|---|---|---|
| 归档 / 恢复（行尾与菜单） | `threads_block.rs:465–500`：`set_thread_status → mutate_thread → with_store → conversation::set_thread_status`；`vega_conversation/src/threads.rs:398–414` 使用 immediate transaction，归档还会暂停 automation | 写许可、事务、自动化状态提交和随后列表读取均可能阻塞。整个服务调用后台执行；提交后才更新真实状态，保留运行任务/权限约束、失败恢复和围栏回收 |
| 标为已读 / 未读 | `threads_block.rs:820–865` 的 unread-only 快路径直接 `conversation::update_thread`；`vega_conversation/src/threads.rs:597–638` 获取 immediate transaction | 重命名已后台化，但此特殊分支仍同步。统一后台提交并更新目标行，保留“不导航、不乱改时间”的业务语义 |
| 确认删除会话 | `sidebar/mod.rs:455–500 → conversation::delete_thread`；`vega_conversation/src/threads.rs:552–590` 检查子任务与预计算清理后进入 `vega_store/src/threads.rs:334–348` 删除事务 | 共享队列和消息/工具调用删除开销都在 UI；安全检查及清理判定必须整体留在服务层、移到后台，不能为了响应性绕过。成功后才清路由和发删除事件，失败允许重试 |
| 移除项目（两种入口共用） | `projects_block.rs:424–468 → projects::remove`；`vega_store/src/projects.rs:154–171` 先把相关 threads 的 project_id 清空，再删除项目 | 不只是删一行；批量解绑、写锁等待与刷新都同步。保持“移除项目不删除会话”契约，事务后台执行，真实成功后才更改选择 |
| 左上后退 / 前进进入会话 | `vega/src/window/navigation.rs:345–505` 后台解析结束后，在 UI 回投 `record_navigation_visit`；同文件 `170–193 → conversation::visit_thread → store::visit_thread`（`vega_store/src/threads.rs:485–511`） | 后台解析不等于整条链后台化。该访问写入同时更新会话时间、清未读、更新项目访问时间。应后台记录并防旧路由结果覆盖；同服务也被搜索接受与通知入口复用，它们是受影响调用方，不代表本轮已审全搜索/通知功能 |

上述“优先”表示响应性缺陷优先级，不表示已发现数据损坏或死锁。删除/移除虽然低频，不能留在主线程。

## 配置落盘：不是数据库队列，但同样可能卡 UI

`sidebar/mod.rs:80–105,276–328`：宽度拖动结束、整体显隐、旧项目/会话块折叠、显式显示侧栏，经 `persist_ui → config::update` 同步落盘。

`vega_store/src/config.rs:391–411 → yaml_config::begin/save_fields/publish`：进程内 `EDIT_LOCK.lock()`、配置读取与 YAML 处理、临时文件写入、文件 `sync_all`、rename、父目录 `sync_all`（`yaml_config.rs:153–194,391–416`）。文件锁本身使用 `try_lock`，不能误写为阻塞文件锁；进程内 mutex 和文件系统操作仍会阻塞。

项目注册主体虽已后台，成功回投调用 `show_persisted`（`projects_block.rs:602`），仍有同步配置尾部。建议单独做串行/合并式偏好保存：保留 YAML 原样更新、冲突与文件权限校验，不为后台化降低原子发布或 fsync；禁止多个 worker 用旧快照覆盖其他配置字段。当前失败只记录日志，这点需与后台保存的用户可见恢复设计一起明确。

## 已后台化 / 无同类写队列风险的入口

| 入口 | 当前边界与限制 |
|---|---|
| 项目名称选择 / 最近打开时间 | `projects_block.rs:335–423` 与 `sidebar_organization/mod.rs:19–25`，后台单在途加最新待写合并；PR 修复，未合并，不表示当前安装版本已生效 |
| 置顶 / 取消置顶 | `threads_block/pin.rs` 与 `vega_conversation/src/threads.rs:416–439`，后台打开/提交/读取真实状态；PR 修复，置顶独立实现审查仍未完成 |
| 打开侧栏会话行 | `threads_block.rs:341–461`，访问写入在 background executor；不与导航历史完成回调中的同步写入混淆 |
| 重命名会话 | `commit_rename → apply_update` 非 unread-only 分支（`threads_block.rs:550–580,867–973`），写入与真实结果读取在后台 |
| 注册项目 | `projects_block.rs` 的注册 worker 包含目录检查、注册/reveal 和项目读取；成功后的同步 `show_persisted` 配置写入另列上表，不将整条操作判为全异步 |
| Fork 会话 | `threads_block.rs:1075–1137`，实际创建在后台；菜单可用性仍同步读 cutoff，是读取风险，不是写许可等待 |
| 分组创建/重命名/颜色/解散、任务移组、组/项目/任务排序（菜单和拖拽）、视图/排序偏好、分组/项目折叠与全部收起、分组内新建任务 | `SidebarOrganizationAction` 全量变体（`types/sidebar_organization.rs:105–157`）统一经 `submit_organization → organization_request → background_executor → service::apply`（`organization.rs:340–440`）；事务及 snapshot 在 worker，等待时可能显示局部 busy，不是 UI 线程等待共享队列 |
| 普通“新任务”与项目加号 | `threads_block.rs:204–249` 只创建内存草稿并做项目存在性读取，没有立即 INSERT；分组内新任务确实 INSERT，但走上述后台组织命令 |
| 展开菜单、显示更多、归档区显隐、取消重命名、打开设置入口 | 内存/路由状态；不把打开设置后的整个设置页面计入左侧入口审计 |
| Finder / 复制项目路径、复制会话 ID | 路径校验在后台；外部打开/剪贴板不使用数据库写许可，不能据此保证外部应用永不等待 |

当前侧栏菜单没有独立模型/权限修改按钮；`apply_update` 的通用后台分支不能当作“全应用模型/权限入口均已审查”的证据。

## 剩余边界与后续验收

- `ProjectsBlock::reload`、旧 `ThreadsBlock::reload`、Fork 可用性等仍含同步数据库读取。它们不是本次共享写许可等待的五类写入口，但迁移写入时必须避免完成回调再次同步重读；全量读取性能审计未在此展开。
- 全应用所有 Global observer、会话内容面板、设置页、编辑器、runtime 持久化的传递闭包不在本轮覆盖内。不能宣称“所有写操作安全”或“整个应用没有主线程 I/O”。
- 建议实现顺序：先把 unread 和归档/恢复的直接写入移出主线程；随后删除/移除及历史导航访问；配置偏好以独立、串行的保存边界处理。每批更新 spec 和必要回归，而不是把共享写锁直接改成无等待或移除协调器。
- 必须等待持久化成功的动作也应异步等待并禁用相关控件，不阻塞事件循环。保留事务、安全校验、实际结果、失败可重试、请求/数据库/路由围栏和视图释放后的清理。
- 必要回归：占用同库写许可期间 UI 处理立即返回；成功/失败/重试/重复提交；路由/数据库切换和视图销毁；删除/移除的真实事务语义；导航清未读与时间一致性；偏好连续修改与其他配置并发不丢字段。按仓库限制只在 Actions 运行；本轮未执行这些新回归。

本轮交付是审计清单，不含上述新增风险的生产修复或现场根因确认。PR #866 保持草稿；原置顶审查阻塞不因本审计而解除。
