# Steering 后的委派准入

## 目标与基线

Issue #760；基线 `50106310837ebbd5cc29902a24c473eabdc93a90`。修复一次及连续多次 steering 后首次 spawn 与合法 followup 被拒绝。历史 9 次失败与固定代码证据见 [Issue](https://github.com/puzige/vega/issues/760)。用户已授权实现；本地不运行 Vega、不编译、不执行测试。

## 契约与最小实现

保留起始 assistant 消息作为稳定父执行 ID：ChildLaunch、delegated executions、父 fence、usage、结果/回执归属不随可见回复分段迁移。新增专用的 delegated run fence 读取，不改变普通 `load_run_fence` 的 streaming 限制。

仅允许从精确起始消息沿持久化 steering 接续到 streaming 回复：每跳前段必须 done，pending 输入必须为同线程已 delivered 且未权限失效的 steer，accepted_after_assistant_id 必须匹配前段，输入消息与新回复必须在该线程中按 seq 紧邻，角色分别为 user / assistant。接续目标不得有独立 run fence，防止跨到新执行。seq 严格递增，任意多跳不循环；没有仍 streaming 的合法接续即拒绝。没有 steering 的 streaming 起点保持正常。

capture_delegated_fence 在既有准入事务中读取上述来源，并将父线程当前 capture_fence 与已接受快照完整比对，拒绝 epoch/reset、模式/revision、规则、项目/工作区、数据库身份变化。不会刷新或扩大旧权限。原始快照仍须精确相等、有效，保留父子归属及 previous delegated fence 检查。停止使用既有 cancellation token；准入等待事务后及提交前检查，提交前取消拒绝并回滚，提交后取消走既有生命周期收敛。取消检查为准入线性化点；不声称 SQLite commit 与 token.cancel 可原子化。

禁止接受任意 done 消息、最近 fence 兜底、复制当前权限覆盖旧快照；不改并行上限、嵌套委派、续派终态与所有权要求。不加 schema、UI 文案或无关重构。

## 验证

开发行为测试调用真实 SteeringAccepted 持久化及 create_child 准入路径；仅外部模型、App host 不参与此边界验证。覆盖 0/1/2/3 次 steering、交错新建/完成/续派、精确父执行归属、权限模式/revision/epoch/reset/规则等变化、结束与取消、错误来源与原子回滚。测试保留必要断言，不用源码字符串替代。

自动测试只在现有 PR Actions 执行，记录开发完整 SHA、可恢复测试源码、run/job 与实际计数；通过后按仓库政策独立移除临时 Rust 测试及独占支持，再验证最终 head。实现由非写入者独立审查，最终 required CI 通过后合并，保持 Issue 开放、In review 等用户 App 手测。0 Rust 执行不是行为回归通过。

用户手测：运行中连续至少三次插话，在插话前后 spawn 两个独立 child，完成释放后精确 followup，再插话再 followup；检查结果/回执和停止后无后台遗留。真实权限变更后旧执行不得派发，新请求按新授权正常。

## 当前状态

实现与云端验证进行中；未取得测试或用户手测结果。
