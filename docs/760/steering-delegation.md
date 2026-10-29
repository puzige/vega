# Steering 后的委派准入

## 目标与基线

Issue #760；基线 `50106310837ebbd5cc29902a24c473eabdc93a90`。修复一次及连续多次 steering 后首次 spawn 与合法 followup 被拒绝。历史 9 次失败与固定代码证据见 [Issue](https://github.com/puzige/vega/issues/760)。用户已授权实现；本地不运行 Vega、不编译、不执行测试。

## 契约与最小实现

保留起始 assistant 消息作为稳定父执行 ID：ChildLaunch、delegated executions、父 fence、usage、结果/回执归属不随可见回复分段迁移。新增专用的 delegated run fence 读取，不改变普通 `load_run_fence` 的 streaming 限制。

仅允许从精确起始消息沿持久化 steering 接续到 streaming 回复：每跳前段必须 done，pending 输入必须为同线程已 delivered 且未权限失效的 steer，accepted_after_assistant_id 必须匹配前段，输入消息与新回复必须在该线程中按 seq 紧邻，角色分别为 user / assistant。接续目标不得有独立 run fence，防止跨到新执行。seq 严格递增，任意多跳不循环；没有仍 streaming 的合法接续即拒绝。没有 steering 的 streaming 起点保持正常。

capture_delegated_fence 在既有准入事务中读取上述来源，并将父线程当前 capture_fence 与已接受快照完整比对，拒绝 epoch/reset、模式/revision、规则、项目/工作区、数据库身份变化。不会刷新或扩大旧权限。原始快照仍须精确相等、有效，保留父子归属及 previous delegated fence 检查。停止使用既有 cancellation token；准入等待事务后及提交前检查，提交前取消拒绝并回滚，提交后取消走既有生命周期收敛。取消检查为准入线性化点；不声称 SQLite commit 与 token.cancel 可原子化。

独立审查指出受限 Bash 的 command_scope 仍校验旧父回复。委派分支复用 capture_delegated_fence 完整证明，保留子执行自身 streaming、精确 delegated fence、held/running 和父子身份检查；普通分支不变。补充作用域请求/批准/消费及失效拒绝回归。

禁止接受任意 done 消息、最近 fence 兜底、复制当前权限覆盖旧快照；不改并行上限、嵌套委派、续派终态与所有权要求。不加 schema、UI 文案或无关重构。

## 验证

开发行为测试调用真实 SteeringAccepted 持久化及 create_child 准入路径；仅外部模型、App host 不参与此边界验证。覆盖 0/1/2/3 次 steering、交错新建/完成/续派、精确父执行归属、权限模式/revision/epoch/reset/规则等变化、结束与取消、错误来源与原子回滚。测试保留必要断言，不用源码字符串替代。

自动测试只在现有 PR Actions 执行，记录开发完整 SHA、可恢复测试源码、run/job 与实际计数；通过后按仓库政策独立移除临时 Rust 测试及独占支持，再验证最终 head。实现由非写入者独立审查，最终 required CI 通过后合并，保持 Issue 开放、In review 等用户 App 手测。0 Rust 执行不是行为回归通过。

用户手测：运行中连续至少三次插话，在插话前后 spawn 两个独立 child，完成释放后精确 followup，再插话再 followup；检查结果/回执和停止后无后台遗留。真实权限变更后旧执行不得派发，新请求按新授权正常。

## 当前状态

开发版本 `5cfad84033983249faea06e5a4487540a31c81f2` 的 [Actions 40860145280](https://github.com/puzige/vega/actions/runs/40860145280) required check 成功：Rust 6 发现/6 执行/6 通过/0 跳过，Python 5/15/40 通过，Clippy/fmt 通过。该版本独立审查发现上述 command_scope 缺口，已修订，不以首轮通过作为完整交付。

完整开发版本 `366811aa5344d69baf41d864bea56fc1bd99d4c4` 的 [Actions 40865308527](https://github.com/puzige/vega/actions/runs/40865308527) 成功：Rust 11 发现 / 11 执行 / 11 通过 / 0 失败 / 0 跳过；Python 三组 5/15/40 通过，fmt、全目标 Clippy、required check 成功。真实 Supervisor/hook 派发、回执确认、结果读取、usage、释放、多次 followup、作用域批准/消费和停止收敛均在该开发版本验证；外部模型与 App host 被隔离，不代表用户真实 App 验收。

### 可恢复回归源码

已通过 GitHub contents API 读回并与本地 Git blob 身份一致，均固定于上述完整开发 SHA：

- [准入与多跳回归](https://github.com/puzige/vega/blob/366811aa5344d69baf41d864bea56fc1bd99d4c4/crates/vega_conversation/src/agent/subagents/steering_tests.rs)，blob `368f775d65b43467449d27c05576ef21f656f081`。
- [受限作用域回归](https://github.com/puzige/vega/blob/366811aa5344d69baf41d864bea56fc1bd99d4c4/crates/vega_conversation/src/agent/subagents/steering_scope_tests.rs)，blob `ab76e2eff831e987b9f2414d3a4c3a484674b322`。
- [生命周期与确定性取消回归](https://github.com/puzige/vega/blob/366811aa5344d69baf41d864bea56fc1bd99d4c4/crates/vega_conversation/src/agent/subagents/steering_lifecycle_tests.rs)，blob `a2d5e240118e631ab3459bbd3ce890098c2c46d8`。

### 失败与修正留痕

- Actions 40862177599：Rust 11 执行，8 通过/3 失败；Clippy 报竞态测试持同步锁跨 await。测试项目路径未 canonicalize，与 Tools 的规范化路径不一致，导致作用域请求 StaleAuthority；修正夹具，不改断言。
- Actions 40863654437：Clippy 通过；Rust 10 通过/1 失败。剩余失败为测试误认为多目标 wait_agent 必须全部结束才返回，实际契约为任一新终态唤醒；改为逐个精确目标等待，仍要求全部完成/释放及完整结果、usage 断言。
- 独立审查要求取消回归证明已越过初始取消检查：临时 cfg(test) 握手在获取事务后通知，测试确认到达才取消；两种派发均断言 Cancelled 且无存储变化。最终开发 Actions 完整通过。审查对最终开发版本无发现，生产增量结论沿用，未以盲目重跑、跳过或放宽断言求通过。

### 最终生产版本

按仓库政策独立移除上述临时 Rust 测试、独占模块/依赖和握手；不改变已验证生产行为，不删除现有 Python、生产安全断言或 CI。最终 head 的 required CI 待结果，0 Rust 发现/执行只表示 no Rust tests，不代替上述 11 项行为验证。本地仅 fmt、metadata、diff 与静态清点；未编译、运行 Vega 或执行测试。未合并、未取得用户手测结果。
