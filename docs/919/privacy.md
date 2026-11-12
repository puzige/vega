# #919 隐私剩余迁移

## 授权与基线

用户指定共享基线 `7c3ee7c97db83fbec4ce099313661f9cb7325cb3`，主 Agent 正合并 `2a29adf9b2ea78eaf59d591838531b45adc7373e`。本执行不运行 Git/index/fetch/merge/commit 或 Cargo，不修改 migration、依赖、用户数据库、配置或 NSH。仅写 privacy.rs/子模块、credential_privacy.rs/子模块、pipeline/install.rs 与 pipeline.rs 的 redaction 最小接线及本文档。

## 实现前验收

1. `clean(FnMut)` 的历史扫描、JSON 解析/键碰撞处理、脱敏回调、委派名称消歧、response/tool/timestamp/generated notice offset 重映射全部在 actor 外执行。最终命令只携带封闭 owned 数据，不能携带 SQL 字符串、Connection、Transaction 或回调。
2. 使用有界、仅内存的准备副本复用原清理算法，保持 legacy、historical delivery/receipts、stage、pending、委派及关联 compaction/precompute/continuation 处理。准备完成后析构内存连接，仅保留固定表枚举与 owned 行变更；安装端只能执行这些固定表的具名变更。
3. 准备读事务绑定物理数据库身份、schema 与可信 `SourceFence`，保存全部现有线程的 revision 和线程集合，以保守 CAS 拒绝任何线程来源变化及新增/删除。单独绑定 permission epoch/reset、credential epoch/例外；actor 不重新扫描历史或解析 JSON，不硬编码 migration 编号。
4. 永久清理拒绝 streaming/running/cancelling，数据库 admission pause 与 precompute pause/drain 保留至回执；调用端释放 permission Mutex 后才能等待 actor，执行端按 permission → 短写事务顺序复核暂停与权限/CAS。任一失败完整回滚，不自动重试未知结果。
5. `PreparedRedactionAuthority` 保留 actor 外 policy；另提供无回调 commit token。保留数据库身份、路径、schema、规则版本、credential generation、exception generation、authority epoch、credential storage identity/version 与 SQL epoch/例外校验，preparation install 前后均复核。
6. 不改变 UI 文案或缩小全库迁移验收。最终静态格式检查只能称解析/格式通过；编译、Clippy、行为/并发测试与 CI 由主 Agent 统一执行并记录。

## 风险与验证计划

- 开发回归需覆盖：跨物理库/替换文件拒绝；来源/权限 epoch/例外变化拒绝；运行状态拒绝；invalid UTF-8 boundary/offset 数量拒绝；legacy/tool/timestamp/response/generated notice 一致性；JSON 键碰撞与 historical identity；委派名称消歧；关联 compaction 清理；后段 SQL 失败的原子回滚；callback 仅准备线程调用；token 不含 policy。
- `SourceFence` 为现有 API，migration 编号由主线统一处理，本范围不复制其编号。
- 现有 `PreparedLegacyRedaction` 内含 SQL 且自身 validate 扫描来源，不能直接用作本次永久清理 payload。

## 实际验证与交接

### 已实现 API 与边界

- `vega_store::credential_privacy::{prepare_clean, PreparedCleanup}`：准备阶段用目标线程与关联清理范围的内存副本复用私有 `clean_snapshot`，复制上限 400,000 行 / 192 MiB（源数据计数，不代表进程峰值内存）；不复制触发器、不创建磁盘准备库。历史扫描、JSON、回调和 offset 重映射只发生在调用线程。保留公开 `clean` 兼容入口，但它也先准备、再短事务安装；本轮生产 conversation 入口不再调用该兼容入口。
- `PreparedCleanup::{validate, install, install_in_transaction}`：`SourceFence`、全部现有 thread/revision 集合、permission state、credential epoch/例外保守 CAS；固定私有 `CleanupTable` 枚举 + owned old/new values，不携 SQL、连接或回调。SQL 仅执行端按枚举及实际 schema 生成。逐行 old-value CAS，影响行数必须为 1。`install_in_transaction(&mut Transaction)` 内部 savepoint 防止调用方捕获错误后提交部分变更；command 的 token 后置失败由外层事务回滚。
- `PreparedRedactionAuthority::commit_token` / `RedactionCommitToken::validate`：policy 只留准备端，token 保留既有 generation/exception/epoch/storage 校验并增加物理库/schema 校验。`InstallPreparation.redaction_authority` 改为 `Option<RedactionCommitToken>`，pipeline 仅作 token 最小接线。token 仍保留既有 storage metadata capture（无凭据读取/JSON/用户回调），不宣称 actor 已无全部 metadata 系统调用。
- `PermanentlyCleanPrivacy`：permission 锁在执行端取得，调用端不持该锁等待 actor；admission pause 和 precompute pause 随 owned 命令存活到执行结束，包括调用端失去回执的情况。不重试结果未知；replay invalidate 留准备端。
- 源 CAS 对任一现有线程变化都保守拒绝，因此无关线程写入也可能需要重新 review；未引入更弱的 scoped revision 断言。

### 实际静态检查

- 工具：`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`，`rustc 1.98.0 (88d9e12ae 2026-08-18)`；Cargo 版本未查询，Cargo/编译/Clippy/测试/CI 均未执行（用户要求中央运行）。
- 初次 rustfmt check 返回 1，仅格式差异；随后只格式化授权文件。最终 `rustfmt --edition 2024 --config skip_children=true --check` 对下表 7 个 Rust 文件返回 0。它只证明解析/格式，不证明类型检查或行为。
- 未运行 Git（包括 diff/status），未修改 index/commit/fetch/merge；不验证或声称共享 HEAD 已完成合并。下表为本执行停写前实际源码 SHA-256，不能替代主 Agent 最终提交 SHA。

| 文件 | SHA-256 |
|---|---|
| `crates/vega_store/src/credential_privacy.rs` | `87b999e40e68bd61565dc3f7b2b9162732ec05d42508c3f4a3813197b06f14e9` |
| `crates/vega_store/src/credential_privacy/prepared.rs` | `f7955ffdc6d3e6d069841434014dcb91cab1b88fe27054d6eee684ee51cd6511` |
| `crates/vega_store/src/credential_privacy/tests.rs` | `9cf57198fa2fcb0521492a10e07d854c2d9d213cfd5f3dbf9c1da8a98e546d9c` |
| `crates/vega_conversation/src/privacy.rs` | `6ca04a268129b202e46a1f26c0aafcb6ad47338d4119fd58df96ab13e7e470fc` |
| `crates/vega_conversation/src/privacy/tests.rs` | `8379f65f9897b4f287f7adb30980e12111b6ffb2539ac647c0731b0a1cca176d` |
| `crates/vega_conversation/src/agent/pipeline/install.rs` | `259b8e8a9c179a2bdcc160518c5559a1dc925d05c083bf404934426209932958` |
| `crates/vega_conversation/src/agent/pipeline.rs` | `5501c0bbd93d11995be0b589bb3d73956b07f8095dfb8a7b9271447f26aa48c7` |

### 中央可运行开发测试（11 项，全部未运行）

仅临时目录/合成数据；测试源码及 cfg(test) 接线不得进入 master，中央验证留痕后由主 Agent 清理。

`cargo test -p vega_store credential_privacy::tests:: -- --test-threads=1`

- `cleanup_preparation_leaves_original_untouched_and_installs_on_another_connection`
- `cleanup_rejects_source_schema_permission_exception_and_database_changes`
- `cleanup_remaps_response_tool_timestamp_and_generated_notice_offsets`
- `cleanup_preserves_json_collision_and_historical_identity_redaction`
- `cleanup_rolls_back_earlier_changes_when_later_install_fails`
- `cleanup_retires_related_precompute_and_invalidates_continuations`
- `cleanup_rejects_invalid_projection_offsets_without_writing`

`cargo test -p vega_conversation privacy::tests:: -- --test-threads=1`

- `redaction_commit_token_validates_after_policy_is_dropped_on_another_connection`
- `redaction_commit_token_rejects_credential_and_exception_generations`
- `redaction_commit_token_rejects_database_schema_epoch_and_exception_changes`
- `redaction_commit_token_rejects_storage_replacement_without_generation_change`

### 未决与中央整合要求

1. 无新增依赖或本范围所需 API 缺口；依赖主线提供的 `SourceFence`、`WriteTransaction`、数据库 actor。主线 migration 编号冲突及 include 路径由主 Agent 统一解决，本范围没有硬编码 migration 编号。
2. 需要中央编译/Clippy、上述 11 项行为测试、preparation install 既有回归，以及权限/队列/退出并发验证；当前不能称任一行为通过。委派名称碰撞、畸形 UTF-8 边界、物理数据库文件替换和全 cleanup UI → actor 生命周期暂无本轮专门动态证据，需按风险补足，不能据静态复用原算法冒称覆盖完成。
3. `auxiliary.md` 中永久清理阻塞项已由本实现提供准备/安装 API，但本文不改他人文档；主 Agent 验证整合后更新总盘点。
4. 本范围完成代码写入并停写，未声明全卡验收、独立审查、CI、合并或用户手测完成。
