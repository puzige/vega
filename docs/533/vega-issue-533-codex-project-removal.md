# #533 — 移除含 Codex 历史的项目

## 基线、原因与范围（v2，2026-10-22）

- Issue：[#533](https://github.com/puzige/vega/issues/533)。用户已授权修复，不使用 CUA。
- 生产未改基线：`e4f18c656aa69d192d095a4338a2108e6c9a3c2c`；分支 `feat/533-codex-project-removal`。
- `ProjectsBlock::remove_project` → `projects::remove` 在事务中清空 `threads.project_id` 后删除项目。但 migration 0016 的 `codex_task_snapshots.selected_project_id` 使用 `ON DELETE RESTRICT`，阻止删除并使事务回滚。该引用与归档、完成、失败、停止状态无关。
- 快照不可变，身份加载还校验线程与快照项目一致，不能通过删快照、改快照、关闭外键或清空单边字段绕过。
- 延续 [A8 移除完整性](../vega-a8-project-removal-integrity.md)：只注销项目，保留本地文件、任务和历史；运行中 worker、任务 mutation fence 与草稿保护不变。
- 本卡不实现 Codex 历史继续发送/恢复、不自动建立新 session，不改变菜单排序、任务删除或权限策略。

## 验收（实现前冻结）

1. 真实 Store API 在外键启用的迁移数据库上移除含 Native/Codex 任务的项目成功；项目和 sidebar order 消失，所有目标线程成为 SQL NULL 项目绑定，其他项目不变。线程 ID、标题、归档/置顶、时间、消息/工具/用量/分组及 Codex 快照和 session state 原样保留，`foreign_key_check` 为空。覆盖 absent、intent、confirmed、uncertain、definitively_failed；空项目和未知 ID 保持幂等。
2. 从 schema 31 的已填充数据库增量迁移与新库初始化均成功，迁移幂等；快照/会话所有字段、唯一 session、状态机、快照不可变、任务删除级联仍有效。失败事务（删除项目被注入触发器拒绝）回滚全部线程解绑和登记变化。
3. 移除后 `codex_task_identity` 仍能读取原快照和会话历史。仅允许线程已解绑且快照原项目已不存在的历史差异；原项目仍存在时的错误解绑、绑定其他项目、Native 携带快照、Codex 缺失快照继续拒绝。相同路径重新登记获得新 ID，不重新挂接旧任务，不改变历史快照。
4. 历史可读不等于执行授权。已移除项目的 `begin_codex_session_creation` 不创建 intent，`codex_prompt_binding` 不返回可执行 binding；确认/uncertain/失败状态保留。正常项目的 absent→intent→confirmed→prompt-eligible 流程仍可用；不放宽 uncertain 重试或任何权限边界。
5. 通过挂载真实 Sidebar 的 GPUI 测试调用现有移除菜单动作，确认项目/路由清除、保留任务可发现且无重复；worker 活跃时拒绝，token 真正释放后成功（跨窗口 app-wide registry 语义不变）。保留本地文件。
6. 移除失败/运行保护的提示只出现一次，位于不随长任务列表滚出的 Sidebar 固定区域；在长列表和滚动状态下可见，失败不冒充成功，成功后原错误清除。无错误时不占额外空间，不新增确认、常驻帮助或成功说明。验证鼠标/键盘现有菜单入口，保留 action 命名与无障碍。

### v2：独立方案审查补齐（生产未改、未冻结测试前）

- 新快照的数据库 INSERT guard 必须保留当前完整性：Codex backend、NULL-safe 线程/快照项目一致、非 NULL 项目仍存在。迁移复制旧历史后才安装新 guard；通过生产绑定/物化 API 及直接 SQL 不变量探针验证不存在项目、其他项目、Native backend 均被拒绝，并且失败无部分线程/backend/快照/session 写入。
- 明确执行矩阵：当前线程与快照同一有效项目继续允许；已解绑且旧项目不存在可读历史但 begin/prompt 拒绝，快照同时有 worktree ID 也不能绕过；原有 `project_id=None/worktree_id=Some` 的合法 worktree-only API 行为保留。原项目仍存在时错误解绑、绑定其他项目继续拒绝。同路径新 ID 登记后旧历史仍不得建立 intent 或得到 prompt binding。
- 另加通过真实 `Store::migrate` 的 0032 中途故障注入，验证失败后 user_version 仍为31、原 schema对象/快照/session 全值不变、FK仍开启；去除故障后重试成功。成功迁移比较含 NULL、时间与原始 JSON 的完整行，不能只比较行数/解码状态。该用例与项目移除事务回滚分别覆盖。
- UI 首个操作后 frame 通过真实通知产生，不靠强制无关刷新；在最小窗口/窄侧栏下提示完全可见且仅一处。

## 最小实现计划

1. 先写齐上述存储、conversation 和 GPUI 行为回归，只增加开发阶段测试/最小测试接线；独立审查后 draft PR 由现有 Actions 取得有效 RED，禁止提前修改生产行为。
2. 新增递增 migration（当前下一版本 0032），将 Codex 的 `selected_project_id` 定义为创建时历史身份而非项目登记 FK。必要时事务重建快照与 session 表，逐字段复制且保留所有现有 CHECK、唯一性、索引、级联和触发器。不要编辑旧 migration，不切换外键开关；重建期间先处理依赖触发器，避免父表删除级联丢失会话。新快照仍通过生产 API/必要 insert 约束校验当前线程、项目和后端。
3. `codex_tasks::codex_task_identity` 区分仍绑定项目与合法已移除项目历史，保留不可变快照字段；在 session 创建和 prompt binding 边界重新验证当前有效项目，历史身份不得授权执行。
4. `sidebar/mod.rs::render` 在滚动任务区外显示 `ProjectsBlock.error`，从 organization 列表末尾移除重复展示。保持现有错误生命周期、menu 分发和 worker gate，不借此重构 Sidebar。
5. 同一冻结测试云端 GREEN 后固定原始源码/完整 SHA/patch 和 RED/GREEN run/job，再独立提交删除 Rust 测试与独占支持，独立审查差异并验证最终空集 required check，正式 PR 后合并。

## 测试接线与观测路线

- Store/conversation：owned 临时库或内存库，使用现有 `Store::open`/`migrate`、`projects::remove`、Codex materialize/bind/session/identity API；SQL 仅用于 fixture、旧版本迁移与故障注入/结果核验，不替代生产移除操作。
- UI：当前 pinned `gpui_kit 0.6.1` dev-only `test-support` 和白名单 `tempfile`，`TestAppContext` 挂载 `component::Root → Sidebar`，真实 menu/action、app globals 和 worker token。允许最小 cfg(test) 选择器/观察现有节点 bounds，不增加改变行为的生产 seam。不得恢复历史测试集合。
- 所有新增行为及失败边界须在生产实现前具备断言；接口缺失只可作为最小接口 RED，不能把无关接线编译错误当作行为失败。
- 不运行本地 Vega、编译、Clippy、Rust/Python 测试、打包或 CUA。本地仅快速静态检查（fmt、diff、metadata 无构建）与审查。自动测试仅由既有 `.github/workflows/pr-check.yml`，不增加 workflow/dispatch 或依赖升级。临时 GPUI feature 必需的测试传递包允许，须单独核对并于 GREEN 后清理。
- Actions 命令：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、现有 Python 三组检查及 `cargo nextest list/run --workspace`；CI 启动后每 5 分钟查询，不 watch。最终空集写 `no Rust tests (0 discovered, 0 executed)`，不替代带测试 GREEN。

## 手测与回滚

合并后等待用户手测，不主动删除现有项目。用户可选择一个自建可丢弃项目，分别包含 Native/Codex 历史，移除后核对登记消失、历史仍可读、本地文件保留；运行任务时应先停止并等待退出后重试，错误应无需滚动寻找。相同目录重新登记不自动收回旧任务。用户未确认前 Issue 开放、In review，不宣称应用已安装。

schema 迁移保留不可变身份，不做用户数据补丁或后台清理；回滚代码不得将数据库降级到恢复 RESTRICT 的旧 schema。发现迁移失败即保持原事务与数据，不自动删历史修复。

## 阶段记录

- 当前阶段（2026-10-22 11:36 UTC）：最小实现 `1f7df39960801f2dec1afb4db432a2fe306602a1` 已获非实现者独立审查、同一冻结套件云端 GREEN。主会话独立比较五份测试与RED提交字节一致；[Nextest39770395193](https://github.com/puzige/vega/actions/runs/39770395193/job/119796171665) 30发现/30执行/30通过/0失败/0跳过，4.397秒；Python5+15+40=60通过，Clippy和[required check](https://github.com/puzige/vega/actions/runs/39770395193/job/119800114561)成功。证据在[PR固定记录](https://github.com/puzige/vega/pull/540#issuecomment-6339769397)已读回，GitHub原始测试源码可访问；现在仅清理开发期Rust测试/独占支持，最终空集门禁仍待验证。
- 先行包 `9e57b7889f488d75fd891ff8dd075c9eb1bca9ab` 已获完整独立静态审查并建立 [draft PR #540](https://github.com/puzige/vega/pull/540)。首轮 [39765008946](https://github.com/puzige/vega/actions/runs/39765008946) 在 `usize` SQL 读写编译失败，无行为断言，不计RED；经独立方案审查，以 `c6712e0294fb6dacb89228102e2bda12a96b16bf` 仅修正检查型数字转换。第二轮 [39766103277](https://github.com/puzige/vega/actions/runs/39766103277) 30执行/13通过/17失败/0跳过，Clippy成功；6个旧Store fixture错误列名挡住行为，其余目标失败已有有效局部证据。独立审查后 `fc4926cb0fab0c111e532266b9abf84d4e110b45` 仅将fixture `text_offset` 对齐实际 `text_offset_bytes`、值4不变。原始失败和补丁固定在PR，无重试换绿。
- 完整有效 RED：[run39767902989 / Nextest](https://github.com/puzige/vega/actions/runs/39767902989/job/119787831314)，head `fc4926cb0fab0c111e532266b9abf84d4e110b45`，30执行/15通过/15失败/0跳过，Clippy成功；失败为6个conversation移除FK、4个Store移除FK、缺少新INSERT guard、缺少0032、Codex菜单移除后路由未清、两处错误提示不可见。实际提示y608/668超过600高视口，非观测故障；原有worktree-only/liveflow与键盘跨窗口worker重试等阳性对照已通过。无fixture/编译阻塞。
- 冻结Store原套件更新后SHA-256 `5f30d6a5e910bc0c20ca16f5fd17962f61c4c0ce1501a3b7628ce1e664bebaa1`；其余四份hash与下列READY检查点一致。实现阶段未追改测试；带相同测试GREEN已取得，最终删测门禁仍待取得。

- 2026-10-22 10:26 UTC 集成检查点：UI 恢复写入者经取消确认 terminal interrupted，已停止；保留其完整工作包，未重做。tracked diff SHA-256 `d3ca050082d12829d4a3846b6dd725870f16371670ce83257976424de09c5fc2`；Store 原套件 `48f9e8d2491eb6efbf97b4814d91d52364d89803a14fc407f110b22346d0da1b`，conversation 原套件 `4776e2403a2bccff06552fbcd86bdb88fdb38a77ccd87ed9549ac380a0d2d969`，UI 套件 `4d885d2ce85b2abbbfaa729bb6cec20aa04359299b3eb107e36fc2cbb8f34b03`。五份测试文件共30个目标；还需静态集成/接线确认，不声称有效RED。
- v2 独立补充测试写入者已 READY 停写；Store 边界套件 `a375d5bd24e96f2f307d970f5c84f0fb030298ba956dabadc946ef0b053dc438`、conversation 边界套件 `1c75f15baca96c43241277692341ad4241c0a749d3cd082a55954a84f1b99236`（5+4目标），文件级 rustfmt --check成功且独立静态审查无阻塞。可复用该内容审查，仅后续集成delta需再审；未运行行为测试。下一步以已完成补丁收尾并冻结，不扩充验收。

- 历史检查点（完整RED前）：规格 v2 / 先行测试完整包 READY，所有写入者停止；静态定义计数30（Store12/conversation14/UI4），实际发现/执行待 Actions。集成 Agent 对模块接线、GPUI实际 API 及通知驱动首帧静态核实无阻塞。`cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --format-version 1 --no-deps --offline --locked` 均退出0；主会话独立核对锁文件914→924包，已有身份/版本/source/checksum不变，只增加10个测试支持传递包。生产未改；完整包独立测试审查、RED、GREEN、最终门禁尚未取得。v1 独立方案审查无架构阻碍，提出3项边界精确化，已在上述 v2 接纳；最终测试冻结前补齐，不沿用旧测试通过结论。
- v2 测试草稿审查（冻结前）：保留已有17个目标，但 Store argv fixture 必须采用持久化 kind tag（`flag`/`option`）；`--fixture`、`readonly` 本身有效。新增直接 INSERT guard 与 API 预检查之后的原子回滚探针；worktree-only、project+worktree、移除后 absent 会话及重新登记后禁止执行的矩阵。terminal→intent 非法转换探针须先清空 uncertainty/failure code，使其只能被转换约束拒绝；追加 intent 唯一性、代表 INSERT CHECK 与索引保留。全行观察以稳定主键排序，不以物理 rowid 决定等价。v31 fixture 补 NULL 变体、非 NULL worktree 和合法带空白原始 JSON。
- 迁移中途回滚注入方案：在 owned v31 fixture 添加含记录的 `migration_drop_guard(thread_id REFERENCES codex_session_creations(thread_id) ON DELETE RESTRICT)`，真实 migrate 在退休旧 session 表时失败；比较注入后的所有原始行和 sqlite_master 对象，确认 user_version31/autocommit/FK/无重建残留，移除 guard 后同一路径重试到32。DROP TABLE 隐式删除不执行 DELETE trigger，不能用删除 trigger 假装覆盖此故障。
- 2026-10-22 09:19 UTC 恢复检查点：原测试写入者执行失败，已终止；主会话核实 tracked tree 无差异，保留两份尚未接线/未格式化的测试草稿：`crates/vega_store/src/project_removal_tests.rs`（369 行，SHA-256 `453500ea5c88b759f88b6fda6bff596b0eb69176ff7db731112948179d985d0a`）和 `crates/vega_conversation/src/codex_tasks/project_removal_tests.rs`（238 行，SHA-256 `c305257834359d42bbefa20825eb42593a1e39126f3a0b7769d350b09579be30`）。已有 7 个 store、10 个 conversation 测试目标；未执行，不声称 READY/已审查。下一步复用这些草稿补齐 mounted UI 与最小测试接线，再冻结独立审查；不重建已完成产物，不改变 v1 验收。
