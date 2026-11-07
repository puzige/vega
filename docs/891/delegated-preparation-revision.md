# 子任务 preparation revision 缩锁（v1，#891）

初始基线 `d4beacac2f01d4c09af7545163393e9d43753fe9`，本轮 fetch 后快进至 `922cabfe5c1dbf2b19bff9ee75e8afc3c7ac9d75`（仅不重叠的 Skill UI 差异）。用户已确认实施；不延长超时、不重放工具、不新建子任务。仅改 delegated 最终准备路径，普通会话历史/replay 不变。本地只做格式、差异与静态检查；行为和性能对照仅现有 Actions。

## 实施与一致性

同一 deferred 快照取得 child revision、完整依赖数据及终态 mutation origin。复制当前 child 必需表到私有内存 SQLite 投影，不复制用户配置、凭据或其他会话正文。复用现有脱敏安装、source/指纹、阶段历史和工具审批验证实现，在私有投影中先安装脱敏和冻结的 assistant 行，再生成预计提交后结果。恢复投影复用同一 source，不重复加载。私有投影不持有真实数据库 writer；不使用全库 backup，不把整个用户数据库复制到磁盘。保留 rowid，避免 latest-by-rowid 阶段语义变化；投影 assistant 仅写必需消息列，不调用侧栏推广。捕获允许已登记且尚未插入的本次 assistant 映射，其他缺失/跨会话映射拒绝。组合对象绑定原 Connection 与同 child 准备产物，不允许相同 revision 的其他库误安装。

真实 immediate 事务在任何安装前核对 revision；新 assistant ID、created_at、seq 冻结并核对。实际脱敏 SQL、assistant、fence、技能审计与原一次性操作仍同事务提交。内部安装接口仅由已验 revision 的组合准备对象使用，不向 conversation 暴露跳过验新入口。相关依赖变化即拒绝，不重试、不慢扫描兜底。事务自身写入引起 revision 变化不算外部冲突。

新增迁移 0060：thread preparation revision，整数且拒绝溢出；保留删除后的 tombstone 防止删除重建 ABA。相关表所有 INSERT/UPDATE/DELETE 均推进受影响归属；关联表通过 message/call 取归属，UPDATE 同时覆盖 OLD/NEW。BEFORE INSERT/UPDATE 还覆盖所有唯一键冲突的旧行归属，以处理 recursive_triggers 默认关闭时 REPLACE 隐式删除不触发 delete trigger 的情况。实际删除父 message/thread 由父表触发失效，不能依赖级联之后还能查到父行。revision 缺失/异常拒绝。

## 依赖闭包与保留边界

| 实际读取 | revision / 验证边界 |
|---|---|
| source 消息/工具/图片 | messages、tool_calls、image_attachments；全部字段与归属变更 |
| source 间接读取 | pending_messages、historical_message_delivery、historical_tool_receipts、tool_call_rounds、context_micro_compactions、computer_screenshot_artifacts |
| legacy 脱敏与偏移 | context_checkpoints、assistant_response_starts、message_timestamp_boundaries、generated_notices，以及 source 依赖 |
| completed stage/final response | delegated_executions、delegated_execution_messages、delegated_execution_recovery、delegated_stage_checkpoints、assistant_responses、messages |
| recovery 投影 | 同一 child source、execution-message 映射、checkpoint 指针与 payload；跨 child 指针/归属不合法时拒绝，不静默漏拷贝 |
| mutation origin | child fork_mutation_origins、thread_timestamp_fork_prefixes、threads；祖先查询锁外取值、锁内重新调用原 origin 校验并比较结果，不缓存旧 origin 批准运行 |
| context policy / authority / skill consent | 最新状态留在真实事务。实际 `prepare_skill_run_with_config_dir` 对非 direct_user 返回 None，delegated 强制 uses_existing_user，因此没有显式 Skill 候选/预算工作需要移动；普通路径的准备、consent/audit/文件末检不改 |
| foreign_call_ids / next seq | 留真实事务内，残余查询成本明确保留；不使用全库 revision |

真实 writer 内仍保留 authority/admission/reset、父子身份与指令未消费、权限 fence、技能 consent/revocation、Skill 文件与目录身份末检、取消和凭据 authority 末检。文件系统不由 SQLite 冻结。所有失败原子回滚，安全末检不移至提交后。权限协调锁不在本轮改造范围。

## 验收（初始待验证）

A1 首次/followup/恢复历史、图片、合法完成阶段和恢复声明语义不变；无工具重放。
A2 同 seq 正文、工具结果/审批、图片字节更新使旧准备失效；无半个 assistant/fence/脱敏提交。
A3 增删、OLD/NEW 归属迁移、REPLACE 唯一键冲突与删除重建均失效。
A4 response/stage/checkpoint/recovery/历史投影等依赖变化失效；非法跨 child 指针拒绝。
A5 父会话无关追加不使 child 快照过期。
A6 权限/凭据失效、技能撤销/文件变更、取消仍拒绝并整体回滚。
A7 安装中途 SQL/行数异常整体回滚。
A8 成功后 canonical source 与准备的 source（含原 fingerprint）完全相同。
A9 准备计算暂停时，另一连接可获得真实 writer 并提交。
A10 普通会话既有 source/preinsert/replay 路径不变。
A11 迁移到 0060、触发器完整性、级联/REPLACE/异常 revision 均验证。

性能分开记录真实写许可持有时长、等待时长和 preparation 总耗时；同 fixture、同执行环境的旧/新路径对照，70% 持锁 p95 下降仅目标，未测不称实现收益。大量真实脱敏更新、foreign ID 查询、origin 复核、文件末检和 commit 的残余成本单列。不得把内存投影计算时间当作真实 writer 持锁时间。

开发对照先提供存储投影的受控微基准（同 fixture、12 样本、p95 nearest-rank）。这不是完整 conversation preparation 实测，不包含 authority/history 转换/终态审批/文件末检/竞争等待；完整持锁、等待、总耗时尚待云端独立用例，不以微基准替代或宣称达成 70%。

## 开发验证记录

- 首个开发 head：`78c33cdfd01dee331fa4e3b38004af270f3140fc`，draft PR #912。
- 现有 Actions run `42332217116`：Clippy/Nextest 编译均失败；`preparation.rs:163` E0277，当前 rusqlite 不支持 `Value::from(ValueRef)`。未执行 Rust 回归，性能未取得；修正使用 `row.get::<_, Value>(index)`，不改变断言/工作流/依赖，待新 head 验证。
- 第二个开发 head `e1b7ed8b376a8ed0d961d8f3157aba1bf17165d0`，Actions `42333155591`：Clippy/Nextest 均在迁移 fixture 编译 E0308 失败，`permission_contracts::initialize` 需要 Transaction 而不是 Connection。测试接线已按生产迁移契约修正；未执行行为回归，不重试旧 head。
- 后续增量：完整 preparation 成功 source/fence、canonical 历史等价、最终取消回滚和过期权限拒绝用例；均仅临时 fixture，不执行模型/工具。新增 cfg(test) 的旧/新准备位置对照、现有 ObservationScope 的 runtime_event permit 持有时间和总耗时；另以锁外暂停时竞争连接提交作并发边界证明，不将人为暂停作为自然延迟收益。所有新增回归仍待 CI。
- 本地已执行 `cargo fmt --all -- --check` 与 `git diff --check`；不进行本地运行/编译/测试。

独立审查仍是合并门禁；本轮禁止新建子任务，自审不能替代独审。没有非实现者精确版本结论时保留未合并候选，不绕过。
