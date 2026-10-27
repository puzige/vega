# 连续 assistant owner 的压缩历史边界

## 范围与证据

对应 #678。基线 `0b449ba9435f15445979006ef9c5d15b50c13e55`。普通历史投影保留空终态 assistant，旧归一化却按 User/System 区间选择唯一工具 owner，重建时会丢失同一区间的其他 assistant。真实快照支持该缺陷路径，但没有原运行内存快照或精确失败分支，不能声称回放了现场。私有证据不进入仓库或 CI。

## 验收

| 场景 | 预期 | 验证层级 |
| --- | --- | --- |
| 空 interrupted → 带工具 done；带工具 interrupted → 空 failed | 保留所有持久化消息及顺序，允许合法压缩 | 合成历史、生产归一化 / hook，Actions |
| 同一 user 后多个非空 owner、不同 API 轮次 | 按源 owner 顺序核对正文、参数、工具结果及位置，不合并 owner | 生产归一化，Actions |
| cold 历史与合法 warm 多轮表示 | 都归一化为相同的 canonical 历史 | Actions |
| 部分 checkpoint，非 ASCII 文本、相同位置不同 response | 只验证未覆盖后缀，游标不丢失、不重复 | 生产裁剪与归一化，Actions |
| 正文/参数/结果/顺序/owner/位置变化、不完整工具配对 | 拒绝，不安装不可信 checkpoint | Actions |
| source/model 围栏、最终 CAS | 保持既有行为，不修改本卡之外的机制 | 差异审查及云端门禁 |

真实应用及原现场由用户在发布后手测，不能用合成验证冒充。没有新增 UI 文案、迁移、依赖或用户数据修改，不实施 #679。

## 最小实现计划

1. `compact_impl` 向归一化传入 checkpoint 已覆盖的消息序号；继续使用 `source_after_checkpoint` 的 UTF-8/工具游标裁剪。
2. `normalize_terminal_history_groups` 按源中未覆盖的、非当前、非 streaming assistant 行顺序处理。User/System 前缀保持原样，每个 owner 独立核验；无工具行要求 canonical 单消息逐字段一致，包括空行。
3. 对带工具行，按该 owner 的正文长度、调用数和结果数确定最短完整候选，复用原有精确正文、参数、位置、结果、顺序和完整配对校验后重建该 owner。候选中的每条 assistant 必须贡献正文或工具调用；不贡献两者的空消息只能作为独立无工具 owner 消费，不能吞入工具 owner（包括开头、轮次之间和正文尾部）。未知归属、缺少或多余消息拒绝，不猜测或跳过空消息。仍由最终完整历史比较检查 user、位置和来源一致性。
4. 使用合成 SQLite 数据加载真实 ContextSource（不扩大生产 API），开发回归交现有 Actions。保留原始测试提交和结果，按仓库政策在合并前清除开发 Rust 测试，最终 head 再跑完整 required CI。

## 风险与非目标

owner 边界不能仅凭文本相同推断，必须同时保持源顺序和完整调用身份。无工具空行必须消耗一条真实空消息；不能通过 blanket-ignore 空行弱化检查。最短完整候选的每个消息必须有合法角色，跨 owner 工具由既有精确校验拒绝。任意多余尾部仍拒绝。当前 live owner 围栏、摘要 provider、source fingerprint、checkpoint CAS 不变。

2026-10-27 续接：基线快进到 `7281c4ad5d7ad2416c8e24d886aae2fd8d9c8199`，原未提交产物保留。静态核对发现候选扫描可吞并额外空 assistant，按原“多余消息拒绝”验收补充上述逐消息准入；未改变验收含义。warm 合成表示拆分非空正文与工具调用，验证既有归一化允许的表示，不声称录制了真实 warm 请求。
