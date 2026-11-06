# 子任务准备写事务边界（#891，v1）

## 目标、基线和事实

初始分析基线 `6240623d72f04b1bde2fa17219f4358bf0ffd2e8`，实现前已同步至 `230285c8d553f6e766b216e94fa7b49d7e55b4ab`。本轮用户要求先列锁外候选、原子性和安全复核及验收，再修改。历史 `event_pipeline/database/5/5` 的实际持锁者仍未知；本轮消除已确认的长锁暴露，不宣称历史归因成立，不重放工具。

`prepare_run_with_images_and_reasoning` 最终 immediate 事务当前覆盖阶段 JSON 脱敏、两次完整历史加载/图片指纹、模型历史投影、checkpoint 目录创建、消息和权限 fence 安装、技能激活及安全末检。子任务准入已在父回执前提交，运行准备在父回执持久化后才开始；未发现该路径锁循环。

## 先定义验收（初始全部待验证）

| ID | 前置与操作 | 可观察结果与必要回归 | 证据 |
|---|---|---|---|
| C1 | 子任务首次执行、followup 或恢复；历史含旧工具/图片及已完成阶段 | 不再为子任务计算未使用的 preinsert 全量历史、冷前缀或 replay lease；最终投影仍只有用户消息/附件、最新合法完成阶段与恢复信息；replay prefix 仍为 1 | Actions 行为回归、静态分支核对 |
| C2 | 另一连接持 immediate writer，准备阶段 JSON 脱敏 | deferred 一致快照读取并完成 JSON 解析/遍历/重编码，不申请应用写许可、不等待 SQLite writer；无写入，准备结果不含可调回调 | Actions 合成 WAL 争用 |
| C3 | 准备后任一 checkpoint payload / recovery reconciliations 同 id 更新、增加、删除、迁移到其他 child | 新 immediate 事务内精确比较两表的完整行集合（child、id、raw）；stale 拒绝，未产生部分脱敏或新 assistant/fence；不在 writer 内重新解析/脱敏 | Actions 状态变化/回滚 |
| C4 | 安装阶段第二条 SQL 失败或最终权限/凭据检查失败 | 保存点/外层事务回滚，旧数据和新消息均保持原子性；每条更新核对影响行数；凭据/例外来自同一不可变 authority，安装前和提交前继续复核 | Actions 故障注入、非实现者审查 |
| C5 | Execute 准备以及 Ask/Plan 路径 | checkpoint 目录只在最终 writer 之前创建，事务内无 create_dir_all；原路径/错误传播不变，锁内 authority 仍核对 run mode。允许失败准备留下 Vega 创建的空目录，不产生已提交运行 | Actions/静态 |
| C6 | 普通会话、新消息、恢复、pending/credential continuation | 普通 preinsert history、checkpoint/replay 语义不变；最终上下文指纹、权限 reset/admission、delegation identity/instruction、终态工具、序号及技能预算/撤销校验保留 | Actions 风险回归、差异独审 |
| C7 | 交付 | 不改 timeout/重试/取消、schema/依赖/用户数据；本地只 fmt/diff/static；开发测试留存可恢复证据后清理，最终无测试 head required CI，用户手测前 Issue 不关闭 | PR/Actions/用户 |

## 操作处置与最小实现计划

| 操作 | 本轮处置 | 原子性与安全理由 |
|---|---|---|
| 子任务第一次 `load_source_in_transaction` / checkpoint / `delegated_history` | 直接消除，不改为缓存 | 唯一用途是 cold_prefix_len/replay lease；子任务 uses_existing_user=true，lease 不创建，最终 replay prefix 固定 1。普通会话分支原样保留 |
| `delegated_stages::redact_payloads` JSON 遍历/脱敏/编码 | 前置为 PreparedPayloadRedaction；deferred 快照释放后再开新 immediate | 准备保留两表完整原行集合和具体更新；writer 只读回比较原行及执行已准备 SQL，保存点回滚，禁止慢 fallback |
| checkpoint 目录创建 | 放在已验证 credential_authority 后、最终 writer 前；仅 Execute | 路径仍取 DB parent/checkpoints；不将文件系统操作伪装成 DB 原子性；authority 匹配仍在 writer 内 |
| 第二次完整 source 加载、图片指纹和最终 delegated_history | **保留必要原子边界**，仍是剩余长锁风险 | source_version 是最大 seq，不覆盖同 seq 正文/图片/历史投影变化；现有“version”函数也扫描正文和图片。仅比较 max seq 或 PRAGMA data_version 会丢安全性或因父事件写入而误拒绝。彻底前置需要覆盖全部源表的 thread 级 revision 协议/迁移及投影后指纹安装，不在本轮盲目引入 |
| legacy redaction 源/summary/offset 检查和 install | 保留；当前重复源指纹检查也记录为剩余风险 | 元数据变化及偏移重映射必须原子；本轮不改该已验收协议，以免扩大历史脱敏范围 |
| 权限、admission、owner、delegation/instruction、序号和消息/fence 安装 | 保留锁内 | 真实准入和多表状态必须同一新写事务重读和提交 |
| skill 最终源状态/consent/revocation、预算及凭据末检 | 保留锁内 | 文件身份和撤销不由 DB 快照冻结；不能为缩锁删末检或重用旧权限 |

不引入全局 revision、迁移或索引，不改普通历史输出或 stage 协议。读事务绝不升级为写事务；先释放准备快照，再取写许可。Prepared 数据不写日志；只复用现有 Preparation 运行身份和锁诊断。真实模型/父子 UI 并发及历史事件归因仍由新版现场证据和用户手测判断。

## 验证与交付记录

首轮开发候选 `07758e1d10b11ca5a1b14150935dd4021092dcb3`，PR #901，Actions [42151313567](https://github.com/puzige/vega/actions/runs/42151313567)：Clippy 成功，Nextest 8 发现/8 执行/7 通过/1 失败/0 跳过。失败发生在测试夹具 `capture_delegated_fence`：父/子 message 使用非 ULID，真实身份校验拒绝；修正合法 ID，不改生产校验。独立静态审查无生产缺陷，要求补验安全末检回滚和 followup/恢复投影；增量加入真实 credential reader + epoch 失效正反对照（安装后触发器）、准备期间权限变化、图片/旧工具/合法与未完成阶段/恢复声明投影。投影复杂样例直接调用生产 `delegated_history`，完整准备的 prefix/lease/fence 仍由独立整链测试覆盖，不冒充真实 App 或模型集成。同步远端 `dc4c077d405b0047a434c3df5fcd9bea510ec2ec`（#900 侧栏/文档，无交叠）。

生产改动已按上述边界实现。开发回归包含 store 的真实 WAL writer 占用、完整原行集合失效、保存点/外层回滚、跨 child 隔离，以及整链路 Ask/Plan/Execute、委派成功/指令替换拒绝、目录失败不提交；并补齐凭据末检失效与成功对照、准备期间 authority 改变、followup/恢复历史投影。准备范围、拒绝状态、安装原子性优先用稳定 store 边界验证；不以源码字符串测试代替行为，不恢复旧测试集合。

开发验证版本 `265eb5ec532ea4b44a854e0843f80e1238705d02` 的 [Actions 42153950723](https://github.com/puzige/vega/actions/runs/42153950723) 全部成功：Nextest job `127683518031` 使用 `cargo nextest list --workspace --message-format json` 与 `cargo nextest run --workspace`，11 发现/11 执行/11 通过/0 跳过；三组 Python 5/15/40 全部 OK。Clippy job `127683517766` 的 fmt 与全目标 Clippy 成功；required job `127684858430` 成功。前轮 `4d0d3443b651e33253b4ca9ddcf080825d87c8b8` 的编译失败来自测试误用未直接依赖的参数宏；最终仅修正两处字符串参数数组和 followup 请求 ID，未新增依赖、改变生产约束或放宽断言。

原始源码保留于远端 tag [`evidence/891-child-preparation-tests-265eb5ec`](https://github.com/puzige/vega/tree/evidence/891-child-preparation-tests-265eb5ec)，已读回完整 SHA。pipeline 测试 blob `3fa4a3beec1885a4f8a0887d18dfcaa7acc5fa74`，store 测试 blob `2ee74324b31d88cf1629f48d6af2968f7399c746`。生产初审与补验增量审查已完成；两次异常审查未产生结论，不计通过。用户恢复后，`4d0d3443…` → `265eb5ec…` 三行增量由非实现者独审通过，未发现阻塞。

留痕和增量独审通过后，仅删除两份开发测试文件（6 pipeline + 5 store）及各自模块接线；没有新增独占依赖/feature 需要清理，不改生产逻辑、安全断言、Python 测试或 CI。清理差异独审与最终无 Rust 测试 head 的完整 required CI 待取得，开发版本绿色不替代最终门禁。本地只做格式、差异和静态清点，不运行 Vega/编译/测试。

最终仍保留一次全量 source 工作，不宣称“全部历史计算已移出 writer”或 wall-clock 保证。最终门禁全满足后 squash merge；Issue 保持开放、In review 等用户手测，不把 CI/合并当作用户验收。
