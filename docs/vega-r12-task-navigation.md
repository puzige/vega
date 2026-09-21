# R12 — 日常任务操作、导航与当前分支

2026-09-06，用户在 R11 交付后要求继续推进。主 Agent 复查本机 ZCode「更多」与设置返回入口，按 R11 清单 N02/N16/N17/N18/N20/N21 承接本轮。功能与 UI 优先，Astra medium 实现，主 Agent 审查/集成/真实单实例验收。继续无 Keychain、去对话指标、性能延期的既有裁决。

## 参考与现状

ZCode 更多菜单实际展示：置顶任务、重命名任务、归档任务、标记为未读、在 Finder 中打开、复制路径、复制任务路径、复制日志路径、复制会话 ID、前往配置、查看调用轨迹、反馈问题。主界面左上有后退/前进；设置使用独立返回入口。只查看菜单和页面，不修改参考端任务/账户/配置。

Vega 基线 4e6e026：侧栏已有 Pin/Archive/Restore/Delete/双击重命名；Thread 和 store 已有 unread 字段，UI 能画未读点，但没有人工设置入口。当前项目路径可从已注册项目解析，任务/日志没有独立目录，不伪造这两类路径。后退/前进缺少。项目后缀 reads git_default_branch，该字段实际为注册时 HEAD 缓存，切换后可能错误。

## A：任务更多菜单

- 同一个侧栏「…」菜单提供置顶/取消置顶、重命名、归档/恢复、标记未读/已读、Finder 打开所属项目、复制项目路径、复制会话 ID、前往设置、现有删除确认。保留双击重命名，不让双击第一下丢失其他任务草稿。菜单当前任务无需先打开才能操作；必须针对捕获的任务身份，不因项目/任务切换误作用到新目标。
- 复用已有 ThreadUpdate/unread 与持久层，不新增迁移。现有 open_thread 只触碰时间戳，并不清未读；真正用户访问要增加专用 visit_thread(&Store, &str) -> Result<Thread, ConversationError>，由 A 提供并由侧栏/命令面板/导航使用，内部保留打开时间戳并原子清未读。模型/模式/历史刷新等非访问读路径继续使用原 open_thread，不暗中清未读。手动把当前任务标未读后不因同任务属性刷新立即清除，下一次真正打开时清除。只合并自身已提交字段，不能把旧快照覆盖后续模型/模式/标题修改。
- 使用菜单即可重命名，Enter 提交、Esc 取消；失败保留可重试输入与明确提示。复制内容完全来自可信的已注册项目/实际任务；空路径、已移除项目和失败不能假装复制成功。Finder 使用参数化 OS API，不拼 shell 命令。复制会话 ID 不读模型/凭据。
- 新文件/数据库 IO 放在 conversation/service 或后台 worker，UI 不直接 SQL、不增加关键路径同步 IO。现有动作路径可复用，若需要将新组合动作改为异步，保留 durable ack 和身份守卫，不做全应用存储重构。
- 不提供假的「任务路径/日志路径/调用轨迹/反馈」入口；这些在差距表继续缺少。菜单宽高随条目/窗口收敛，键盘箭头/Enter/Escape/Tab 和鼠标点击真实可达。
- 归属：`vega_ui/src/sidebar/threads_block.rs`、相关 row helper 与新 task-action 子模块；允许 sidebar/mod.rs 的最小事件/刷新接线。新 conversation/store helper/types 由此卡提供，但不得改导航或 projects_block。UI lib/types module 的导出只增加本卡名称，主 Agent 集成。

## N：后退 / 前进

- 窗口内最多 100 条逻辑路由历史，覆盖项目空态、具体任务、设置。新建任务、侧栏打开、命令面板打开和项目选择都可被记录。属性刷新/相同路由不能增加历史；一次项目+任务原子导航不能插入中间空态；后退后新导航丢弃前进分支。重启不恢复本轮历史。
- 左上提供可访问的后退/前进按钮，禁用状态准确，并提供 Cmd+[ / Cmd+]。侧栏隐藏时仍可操作。不得拦截正在编辑的文字/IME 或与已有局部菜单 Escape 等冲突；遵循已有 deferred global shortcut 模式，禁止 global key dispatch 内同步重入 window。
- 返回必须重新验证实际任务/项目并走生产路由；删除/归档/移除后的失效条目安全跳过，不复活数据，不载入旧快照。异步完成必须有当前导航 generation 守卫，旧响应不能改变新路由。失败保留当前页面与可解释状态，不能丢失前进/后退能力。
- 草稿按任务在当前窗口内有限保存，来回导航恢复未发送文字，设置进入/返回不丢草稿。仅保留草稿字符串/必要输入状态，不缓存整个带 worker 的 ConversationStream；不持久化正文、不绕过已有取消/权限/lease 行为，不复活旧 worker。容量上限明确（100 个任务、总 1 MiB；已触及上限时优先拒绝会丢失非空草稿的导航并给提示，不能静默删草稿）。
- 设置内部切换分类不增加页面历史；设置返回与历史状态一致。现有 SettingsOpen/OpenedThread/SelectedProject 和原生测试路由必须继续工作，避免为本卡重构全部应用路由。
- 归属：新 app/window navigation 模块、`vega/src/window/mod.rs`/render 的必要接线、app_palette 的必要适配、独立 UI navigation 模块与全局键绑定。sidebar/mod.rs 仅允许与 A 明确约定的 controls 挂载 hunk，A 保留数据动作归属。不得改 branch/projects_block 或 A 的菜单/持久化模块。

## B：侧栏当前分支

- 侧栏后缀来自当前文件系统/Git 状态，不把注册时 git_default_branch 值当作实时分支。支持普通 repo、.git 文件链接 worktree、detached、非 Git/已不存在目录；unknown/读取失败不能保留旧分支造成假状态，不修改真实 repo 或 git_default_branch 语义。
- 初始化/重新加载/选择项目后异步刷新；可见项目使用有界 coalesced 后台刷新或受控 activation 触发，确保从应用或自带终端/外部切换分支后能自动更新（目标 2 秒级，明确 cadence，并非性能承诺）。不在 render 中读文件/执行 Git，不无限启动并发 worker，不对 inactive/移除项目应用迟到结果。
- 当前状态与项目 ID、已注册路径和请求 generation 绑定；untrusted .git 路径不得产生任意文件遍历；优先复用现有可信 Git/service 或等价有界只读解析。每次读取与输出有明确预算；不引入新依赖或放宽现有 Git runner 超时。
- 归属：`vega_ui/src/sidebar/projects_block.rs` 与新的 conversation 只读分支查询模块、必要 shared types/exports；不改 sidebar/mod.rs、window/*、Git mutation runner 或导航。生命周期由 ProjectsBlock entity 及 weak subscription/task 控制。如已有 repo authority 要求使此实现边界不可达，先报主 Agent 评审。

## 统一验收与边界

各卡规格先于实现，独立 sibling worktree，fetch/update 后开始，no pi、no push/master 合并。新增依赖不批准；scope 中 routine 选择由 executor 落本卡具体规格补充，涉及语义/文件归属冲突先交主 Agent。交付需代码提交、spec/delivery 文档、原始日志/时间/来源与未运行项。主 Agent 不写功能实现。

优先 production 根视图与真实 controller + owned migrated DB/repo；新异步动作测试正常提交、失败保留、迟到结果/路由切换、键盘可达与草稿。分支使用真实文件/Git 切换和实际侧栏展示，不以 injected label 代替。最后主 Agent 统一 fmt、strict all-target clippy、workspace tests/build 与单实例 native；先退出旧实例，使用自有项目做全部会改变状态的操作。性能不跑；不读取真实 key，不将 MockProvider 当真实网络验收。

R11 最后全量 925/3/0 的三项 Git 测试保留为已有未解决 gate。不得删/放宽断言、忽略测试或靠反复重跑清绿；若复现，保存首次日志，做受影响范围与回归来源分析，单独跟踪已有生命周期问题。本轮不扩大为 Git runner 重写，也不虚称全量通过。

### N implementation contract (2026-09-06)

The window reconciles the final SettingsOpen/OpenedThread/SelectedProject identity before rendering or history commands. One synchronous multi-global update produces one logical entry; title/model/unread changes only refresh the current projection. History holds at most 100 identities, including the current entry. Settings is one identity independent of category; closing Settings traverses back when possible. New routes truncate the forward branch.

A file-backed conversation navigation service validates registered projects and active tasks in a background worker. Traversal scans at most 100 candidates, skips absent/archived routes, and commits the cursor only with a successful current-generation result. Service failure leaves route and cursor intact. Repeated traversal while pending is inert; external navigation invalidates pending work.

Before replacing a stream, its nonempty composer text is retained in a window-only map. Cached text plus a departing draft must fit 100 task IDs and 1 MiB UTF-8; otherwise the route is restored and an inline capacity message explains the refused navigation. Restoring a draft removes its cache entry, so the live editor remains its single owner. Empty drafts are removed. Draft restoration uses the existing composer input API. **Issue #67 supersedes the old route-cancellation clause:** new/existing task navigation and Settings retain only the exact active agent's `ConversationStream` until its terminal handshake, without cancelling the run; all non-active task streams remain subject to this bounded draft-only policy. See [`vega-issue-67-background-run.md`](vega-issue-67-background-run.md).

Navigation controls are mounted in the sidebar brand and a content header when the sidebar is absent. Cmd+[ and Cmd+] use deferred global handlers: ordinary composer focus permits navigation; active composer IME composition and other text editors retain their input behavior. Both controls have named shortcut tooltips and Tab/Enter/Space activation. Mouse controls remain available while editing. Root controller tests use owned file-backed databases and actual controls/shortcuts; bounded-state tests supplement capacity and stale-result invariants.


N race closure: route resolution (including palette tasks) is read-only. Before applying a result, the root checks the task-mutation epoch/pending count, the route generation, and the current draft budget. Only an accepted task route starts the genuine-visit write. That short write holds the same task mutation guard used by menu writes; later task writes report busy while typing remains enabled. Its acknowledgement merges only unread/timestamp fields into the still-current task and never reinstalls a full Thread. A dropped window still releases the global pending count at App acknowledgement. Settings and the unselected landing page resolve without database availability. A real sidebar delete during resolution and a draft growing beyond retained-byte capacity before acceptance are required root regressions.

N native focus closure: Cmd+[ / Cmd+] have focus-independent app bindings so consecutive navigation works during the interval after the old composer focus node is dropped. TextInput-specific bindings remain more local and continue to protect active IME composition and other editor-owned contexts. The root regression must navigate backward then forward without refocusing the destination.
