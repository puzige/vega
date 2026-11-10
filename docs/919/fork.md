# 在线 fork 的锁外准备与具名安装（#919）

## 范围与验收

本阶段仅迁移 `fork_thread` / `fork_latest_thread`；不修改其他线程服务、共享 writer 或来源 fence 实现。基于 HEAD `555b0c0b3c00031ad22b69c7a5622e3c1dc4116b` 与当前未提交集成；不提交、不操作 index、不运行 Cargo，由主 Agent 中央验证。

- 生产 fork 通过 owned、sealed、具名 `ForkCommand` 提交同库 actor；不传 Connection、闭包或任意 SQL。
- 捕获只读取当前 fork 来源及必需的 mutation ancestry；隔离内存投影不复制其他生产库数据。读取事务结束后完成历史复制、引用映射、图片校验、历史重建及工具终态解析。
- actor 复核物理数据库/schema/按来源 revision、来源线程、cutoff 与 project 存在性。未纳入来源 revision 的 fork 元数据另作精确比较，不能省略完整性检查。
- 安装只有固定表的参数化 SQL，原子提交；失败不留下线程或部分历史。标题序号在提交事务内按当前同项目线程确定，ID 仍用 ULID。
- 保留 Native/project/top-level、无活跃工作、可信 Final、消息/工具/图片/截图/历史引用及 mutation origin 的全部验证。
- 慢准备不持生产写事务，不阻止父任务和两个子任务提交真实消息及完整工具终态；准备后的来源变化拒绝，不重放准备或事务体。
- 开发回归覆盖正常历史/工具/图片、排队来源变化与无半成品、坏历史拒绝和准备期间其他写入推进。Rust 测试仅开发使用，由主 Agent 统一运行与合并前留痕清理；本阶段不宣称已编译或已测试。

## 设计

`ForkProjection` 捕获只读来源快照与 SourceFence，固定白名单表在隔离内存库重用既有 `clone_prefix` 验证和映射。`PreparedForkInstall` 只携带固定表标识与 owned 参数行，不携带数据库连接、动态 SQL、用户回调或投影逻辑。生产 actor 校验后执行固定安装语句；完整历史与工具恢复已经在命令构造前验证。

捕获保留旧的全来源 ContextSourceVersion 读取完整性检查；只读事务结束后才运行 `clone_prefix_with_origins`、图片解码、完整历史重建和 terminal recovery。mutation ancestry 在生产只读快照中解析为 owned origin，仅读取必要祖先并持对应 SourceFence，不把祖先历史复制进内存。隔离库只复制当前来源、其 project、delivered pending 和 fork 所需固定表，累计限制 400,000 行 / 192 MiB，原图片 32 MiB 和 ContextSource 自身界限继续生效。

提交 DTO 中表名/列名均由私有枚举固定选择；不接受 SQL 字符串。三组无 revision 触发器的 metadata（duration、builtin Skill、Skill audit）保留精确逐行比较，不重新物化全历史。标题在实际提交时重新分配，两个已经准备好的命令不会使用同一预计算名称。actor admission 与 BEGIN 合用原 10 秒余额，锁外计算不计入；开始执行后等待真实回执，不自动重试。原后台服务调用方式未变，未新增 UI 等待入口。

## 开发回归（已添加，尚未运行）

专属模块 `crates/vega_conversation/src/threads/fork_tests.rs` 的六项行为回归：

1. 正常消息/完整终态工具/图片及映射，两个预先准备的 fork 在提交时分配不同标题，外键完整。
2. 坏审批、坏图片、坏 response range 全部拒绝，零 fork。
3. actor 执行屏障后排入 fork，前序命令变更来源；fork 拒绝且不留部分消息。
4. 未版本化 duration 元数据变化拒绝，零 fork。
5. 其他物理数据库拒绝，以及安装中途 SQL 失败的事务回滚。故障触发器在准备前创建，避免只测到 schema 变化拒绝。
6. 暂停只读捕获后的锁外投影准备，父任务与两个子任务通过同库具名命令提交真实消息及完整终态工具；恢复前读回全部回执，恢复后 fork 成功。

这些是开发测试，不是已取得的 GREEN，也不能替代主 Agent 的 persistence actor 全事件/真实运行并发回归。按仓库政策合并前须固定源码与实际验证结果，再清理 Rust 开发测试及独占接线。

## 当前接口未决与中央接线

- `SourceFence::capture/validate` 当前来自协作者的 `context_compaction/source_version.rs`，复用了物理库身份、schema、来源 revision 和触发器定义验证；协作者已移除全局 epoch，本阶段不恢复它。
- **迁移接线已在共享树出现，尚待中央运行验证**：最后复核 `vega_store/src/lib.rs::MIGRATIONS` 已包含 `migrations/0065_source_validation.sql`。此文件和接线由来源 owner/主 Agent 完成，本阶段未编辑。静态 sqlite3 初始化确认该触发器已经存在。开发 fixture 仅在 trigger 不存在时安装 docs 对应 SQL；最终仍应由中央运行确认真实 `Store::migrate` 与 SourceFence 兼容。
- **内存库身份需协调修复**：`thread_admission.rs::DatabaseIdentity::for_connection` 对无路径连接每次生成新 ULID；SourceFence 捕获和验证分别调用，导致内存 fallback 必然不匹配。文件库 device/inode 路径不受影响。本阶段未编辑共享身份/fence 模块，也未通过跳过 fence 恢复内存成功；主 Agent 应协调稳定内存连接身份 API，并补同内存连接成功/不同连接拒绝回归。
- 以上不允许把阶段标作完整可合并交付。无独立审查、编译或行为运行结果；主 Agent 中央验证后继续处理具体失败，不削弱断言。

## 静态证据与精确内容身份

本轮未执行 Cargo、Git、index、提交、push、Issue、安装或用户数据库访问。使用 `rustfmt 1.8.0-stable (1159e78c47 2025-09-14)`，仅格式化本轮四个 Rust 文件，并以 `--edition 2024 --config skip_children=true --check` 验证通过。Python sqlite3 隔离内存 schema 检查了 16 组固定 SELECT/INSERT SQL 计划与 20 个受限捕获 SELECT 计划，均可编译；这不是 Rust 或业务行为测试。新模块无代码注释及 unwrap/expect 调用。最终重验期间来源 owner 新增了正式 0065 migration，原静态脚本再次安装 docs SQL 因 trigger 已存在退出；这是检查脚本与共享树更新的适配问题，不是产品测试失败。检查当前迁移接线后，使用当前完整 migration schema 重新验证 16 组固定 SQL 计划通过，未重复执行生产写入。

本轮起始内容 SHA256：

- threads.rs：`6250b305a658b4d60736e944ce36f8622ce9e62c3d6a534a3ab56c4a1299d55c`
- thread_fork.rs：`ff9dfe2ced071e85a28fccacc9d3b1a29be600787977b59d6e38eb503fdaa9bb`

最终四文件 SHA256（HEAD + 未提交集成的内容身份，不是提交 SHA）：

| 文件 | SHA256 |
| --- | --- |
| crates/vega_conversation/src/threads.rs | `d2ffc7883233b15412368dcb61f010c764828a940803a47cd494216787eb21d4` |
| crates/vega_conversation/src/threads/fork_tests.rs | `3bd897879da0d96c06c38ac970bfa3f9581d504b59368d0043e382461f85315a` |
| crates/vega_store/src/thread_fork.rs | `0edb7232bc7bb257cae6f3a7fca92735a49e7f1202084a1287793b12c52bd673` |
| crates/vega_store/src/thread_fork/prepared.rs | `0b4b555edcbb68d2061ca0beaadaeec6ff1a5c094a819128ad58857199df86f3` |

中央建议先编译 `vega_conversation`（含 tests）并运行 `threads::fork_tests` 六项，再跑已有共享 writer 和 preparation 并发回归。须复核本阶段最终 hash 后记录真实命令/源码/工具链/结果；旧轮测试证据不适用于此版本。未执行：Cargo check、Clippy、Rust test、全事件/实际运行回归、CI、独立审查、用户手测。

