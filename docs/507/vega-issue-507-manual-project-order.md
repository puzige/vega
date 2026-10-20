# #507 — 项目文件夹仅允许手动改序

版本 v1.1，2026-10-20。Issue：https://github.com/puzige/vega/issues/507

v1.1：生产未改、RED 尚未启动时，根据独立审查补清多连接、注册后置失败与真正只读快照，明确迁移／排序保存失败回滚。产品需求和最小方案不变。

原始排查基线：`e93518cf76a24df921a1af2c2663ebce75f614fd`。
先行测试／实现基线：`3a2ef8e9795d609f022b7b81011360db102e3de6`。测试尚未提交／执行、生产未改时 fetch 并 fast-forward 同步 #510；该提交仅将 thinking 档位 Light 改为 Low 及新增 docs/506，不涉及本卡存储／服务范围。验收与测试目标不变。
任务分支：`feat/507-manual-project-order`，独立 worktree。

用户在确认产品规则后明确要求当前 Vega 会话直接接手，本次由主会话承担方案、独立审查、PR/CI 与集成职责，专职子 agent 单独负责代码。默认 Codex/Vega 分段交接不再阻塞本卡，但不改变 TEST-FIRST、本地执行限制、云端门禁和用户手测要求。

## 需求与非目标

- 已有项目的相对次序只由拖动或菜单“上移／下移”改变。
- 点击、切换、展开／收起、打开会话、消息活动、刷新和重启不改变项目顺序。
- 新项目固定追加到末尾；重复打开已有文件夹只选择／展开，不移动它。
- 保留已有手动顺序；历史未记录项目一次性固定在手排前缀之后，兼容升级前的实际可见顺序。
- `last_opened_at` 继续记录真实访问；它仅可在升级时还原旧顺序，之后不得决定侧栏项目位置。
- 不改变会话排序、置顶、分组、内容、归属、折叠或当前导航；不改项目推荐选择器、旧隐藏视图、拖动手势、UI 文案、发布和 CI。

## 验收矩阵（先于实现计划冻结）

| ID | 前置与操作 | 可观察结果与不变量 | 验证边界 |
|---|---|---|---|
| A1 | 旧版本文件数据库有三个项目、无手动顺序；访问时间有并列且名称与 ID 顺序不同；执行正式迁移 | 顺序一次性固定为旧侧栏回退 `last_opened_at DESC, id ASC`，包含全部项目；迁移再次执行不改变顺序、访问时间或导航元数据 | Store 真实迁移、存储读回 |
| A2 | 旧库有部分手排，含位置间隙／并列，另有未记录项目；迁移后反转访问时间与名称 | 原手排前缀按 `position, project_id` 保留，历史遗漏仅在其后按旧回退补齐；补齐后改时间／名称不再改序，重开文件库读回一致 | Store 迁移、持久化读回 |
| A3 | 最新库无项目，依次通过生产创建入口创建项目；已有手排后再创建 | 顺序完整、无重复，新项严格追加末尾且已有相对次序不变；创建发生在外层事务中也同样成立 | Store 创建、组织读取 |
| A4 | 创建失败、排序行插入被拒绝或外层事务回滚；移除中间项目后再注册新项 | 失败／回滚不遗留项目或排序行；移除仅删除该项，保留其他次序；后续新项仍追加末尾；迁移补齐中途被测试自有约束拒绝时，排序和 user_version 回滚，解除故障后可重试 | Store 原子性、外键级联、迁移失败 |
| A5 | 从注册服务依次打开至少三个 owned 临时目录，再重复打开首项；最后新加一项，刷新并重开 DB | 返回 snapshot 和后续读取顺序一致；重复打开不移动，新项在末尾；无效目录失败无副作用；有效目录在项目插入后因 revision 耗尽而失败时，项目、排序行及元数据整体回滚 | Conversation 生产注册／快照、真实文件 DB |
| A6 | 快照含已注册项目；依次 touch 末项、打开／访问其会话、创建／更新消息相关活动，执行折叠／展开和偏好动作；用真正只读连接读取迁移后的文件库 | 完整项目 ID 序列与 `project_order` 始终不变；真实访问时间仍可更新，折叠与未读等原有语义保留；`Store::open_read_only()` 上 snapshot 成功且不写任何元数据。消息可按旧语义推进会话排序／revision，只有纯快照读取不写 revision | Conversation 快照与生产访问／组织动作、只读连接 |
| A7 | 三个项目；MoveProject 末项移到首项之前，再把首项移到末尾；其后访问项目并重开 DB | 手动动作精确生效并持久化，其余相对次序、折叠与会话数据不变；后续活动不覆盖手排 | Conversation 公开移动、文件重开 |
| A8 | 已有顺序；同一文件库两连接 A/B，B 注册／移动提交后 A 用旧 revision 移动，再刷新重试；另提交源缺失、锚点缺失、自移动或在排序重建阶段注入插入失败 | 相对 B 已提交状态原子拒绝，完整快照／排序与 revision 不变；刷新重试可成功且不丢 B 新项目；重建插入失败时此前 DELETE 和部分 INSERT 同样回滚 | Conversation 多连接、原子失败与恢复 |
| A9 | 用户在合并版本点击首中末项目、折叠展开、打开各项目会话、收发消息、展开更多、重启；使用拖动和上移／下移 | 自动行为不改序，手动行为改序；分页、菜单和键盘操作仍可用，无新增常驻提示 | 用户纯手动验收，未执行不标通过 |

自动回归同时检查 `snapshot.projects` 的 ID 次序和 `project_order`，会话数据按稳定 ID 比较，不把按项目拼接的原始向量位置当成会话排序契约。多连接使用确定性交错，不引入并发压力测试；故障注入只用测试自有约束或现成 revision 耗尽分支。

自动回归直接调用真实 Store／Conversation 边界，不以源码字符串、mock 转发或镜像排序算法代替行为断言。用固定预期 ID 序列验证；持久化使用测试自建文件库，不访问用户数据库／配置／仓库。不恢复历史 Rust 测试集合。

## 根因与当前调用链

- `crates/vega_store/src/sidebar_organization.rs::projects()` 读取并按最近打开时间排序。
- `crates/vega_conversation/src/sidebar_organization/mod.rs::read_snapshot()` 同时返回原始 projects 与可能不完整的 `project_order`。
- 当前 `render_projects_pi()` 及菜单／拖动序列都采用已记录前缀，再追加未记录项目，因此旧项目或后续新项目仍可能按访问跳位。
- `ProjectsBlock::select_project()`、Store 的 `open_thread`／`visit_thread` 与注册服务会更新 `last_opened_at`，只取消点击更新时间不能修复其他路径。
- `mutations::MoveProject` 会补全当时顺序并事务写入；`register_and_reveal_project()` 的新建项目没有同步追加排序行。
- Store 当前迁移为 30。`sidebar_project_order.project_id` 主键并外键级联删除；写入动作已有 immediate transaction 与 revision 校验，快照只读。

## 实现计划与允许范围

先完成上表必要测试，生产逻辑保持基线不变，由主会话审查并建立 draft PR；Actions 确认目标行为缺失的有效 RED 后，才执行以下生产改动：

1. 新增 `crates/vega_store/migrations/0031_sidebar_project_order.sql`，在正式迁移事务内补齐全部历史项目顺序，保留已有手排前缀，未记录项目按旧回退一次性追加。不得改写旧迁移、项目时间或会话数据。
2. 同一迁移为项目 INSERT 接入尾部追加触发器，只追加排序行，不更新 revision、访问时间或其他组织字段；使用最大 position 而非项目数量计算尾部，不以 OR IGNORE 隐藏失败。让新建项目和排序行在同一 SQLite 语句／事务内生效；无论独立 create 或注册服务已有外层事务，失败一并回滚。不得为这一步开启嵌套事务。排序保存只重写排序表，不重新插入 projects，因此不触发追加。
3. `crates/vega_store/src/lib.rs` 仅增加新迁移接线；开发阶段允许测试模块接线，GREEN 后独立删除。
4. `crates/vega_store/src/sidebar_organization.rs::projects()` 改为依据持久化 position 的项目读取，不再按可变时间或名称排侧栏；共享行解码若需要可最小修改 `projects.rs`，不得改变其他显式排序消费者的现有行为。
5. 保留现有手动移动服务、快照一致性、UI 的顺序消费者和访问时间写入。迁移后全项目均有排序行，新注册在快照读取前已有尾部位置；不在只读 snapshot 隐式补写或提升 revision。删除沿用外键级联；后续注册保留存量次序并追加。
6. 允许本轮新建 Store／Conversation Rust 回归及其必要模块接线；临时依赖仅可使用现有已批准 workspace 依赖，优先无需新增。禁止修改 workflows、锁定依赖版本、runtime、工具、发布及无关 UI。

迁移只运行一次；补齐与 trigger 创建失败时由 migrate 原事务回滚且不推进 user_version。创建对排序行的修改与项目 INSERT 一体，不新增独立读写窗口；注册及 MoveProject 仍使用既有 immediate transaction。注册服务当前读取快照 revision 后提交 RevealProject，保留原 revision 行为，不额外改变访问时间导致的版本语义。

## 验证与证据

本地仅静态阅读、`git diff --check` 和快速 rustfmt；不运行 Vega，不进行 cargo check／Clippy／test／nextest／build／打包，也不在本地执行迁移或行为脚本。

云端沿用 `.github/workflows/pr-check.yml`：fmt、全目标 Clippy、三组 Python 回归、Nextest 清点／执行与 required check `check (fmt, clippy, test)`。CI 启动后每 5 分钟查询一次，不短轮询或 watch。无关编译、依赖、网络和 runner 失败不算有效 RED；如测试接线错误，先留证审查并保持验收含义后修正。

交付顺序：测试＋规格提交 → Actions 有效 RED → 相同冻结测试的最小实现提交 → Actions GREEN → PR 固定原始源码／完整 SHA／RED-GREEN run-job-计数 → 独立删测提交 → 最终空集 head 完整门禁 → 转正式 PR并合并 → In review 等用户手测。最多三个任务提交；不 force-push 丢弃证据。

初始状态：所有自动测试待编写／Actions，用户手测待执行。Issue 保持开放，不使用自动关闭关键字。各阶段真实结果留存在同一 PR／Issue；空集是 0 发现／0 执行，不是测试通过。

### 先行测试与有效 RED

- Draft PR：[512](https://github.com/puzige/vega/pull/512)。独立审查通过后提交先行测试 [`1c4c29a129c9d04e96a2bbb9a42578be0ac6ac3a`](https://github.com/puzige/vega/commit/1c4c29a129c9d04e96a2bbb9a42578be0ac6ac3a)，该提交只有规格、两个新测试文件及两处 cfg(test) 接线，生产行为与基线一致。
- [Store 原始测试](https://github.com/puzige/vega/blob/1c4c29a129c9d04e96a2bbb9a42578be0ac6ac3a/crates/vega_store/src/project_order_tests.rs)：5 项，SHA-256 `8491e39745a94c53809c04f49b3cfa97f6e2c3dfd6a5babc3ac6d631c07ffdfe`。
- [Conversation 原始测试](https://github.com/puzige/vega/blob/1c4c29a129c9d04e96a2bbb9a42578be0ac6ac3a/crates/vega_conversation/src/sidebar_organization/project_order_tests.rs)：6 项，SHA-256 `4b0d53bee60075f39e4b13d188f0bf18fc3fd55c87ab77713facf022ac0031e0`。[原始提交补丁](https://github.com/puzige/vega/commit/1c4c29a129c9d04e96a2bbb9a42578be0ac6ac3a.patch)可从 PR 找回。
- [RED run 39394804823](https://github.com/puzige/vega/actions/runs/39394804823)／[Nextest job 118534766357](https://github.com/puzige/vega/actions/runs/39394804823/job/118534766357)：编译／发现成功，原始输出 `Starting 11 tests across 14 binaries`、`11 tests run: 0 passed, 11 failed, 0 skipped`，各测试 ignored 为 0；fmt／Clippy 与三组 Python 检查成功。
- 有效行为 RED：访问末项后 projects 实际 `[project-c,project-b,project-a]` 而非原顺序；新建和注册的 project_order 为空；历史迁移后未补齐排序；排序拒绝时 create 仍成功。不是缺接口、编译或 runner 故障。完整原始失败保存在上述 job，后续测试目标、数据与断言冻结不变。
- 仅在上述 RED 核实后委派生产最小实现；同一测试 GREEN、独立删测及最终空集门禁尚待后续云端验证。本地 fmt／diff 静态检查退出 0，未本地运行／编译／执行测试。

## 用户手测与回滚

1. 升级前记下至少三个项目顺序，包含已有手排和后来加入项目。
2. 升级后点击首中末项、折叠展开、打开各自会话并收发消息，确认不跳位；刷新／重启再确认。
3. 拖动及菜单上移／下移各执行一次，重启后仍保留；新加项目在末尾，重复添加已有文件夹不移动；移除中间项不影响其他相对位置。
4. 检查展开更多、键盘菜单与错误恢复无退化。由用户明确反馈通过后才关闭 Issue／Done。

应用回滚由后续 PR 撤销本卡生产读取改动并走云端发布；不删除用户项目或顺序数据，不回写 user_version、不自动降级或覆盖数据库。新增迁移／触发器属于持久化契约，需再次调整时新增递增迁移，而非修改已发布迁移。回滚前保留数据库备份，不借本卡访问或替换用户运行中数据库。
