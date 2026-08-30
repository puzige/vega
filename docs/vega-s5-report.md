# ✦ Vega — S5 验收报告（写工具、权限门禁与三模式）

**日期** 2026-08-30 · **范围** T23-T29 · **结论** S5 功能 DoD 已在 mock/自动化边界达成；T29 squash merge 后闭环

## 1. 交付范围与 PR 台账

| 交付 | PR | master squash commit |
|---|---:|---|
| S5 SDD / C1-C7 | #23 | `d13a64d77101def644b09821e67e6620e14ed0af` |
| T23 write/edit、围栏与 preimage | #24 | `b7fa9b9a83b85a4581c29ba7096e7f0f58a54b26` |
| T24 Seatbelt bash 与危险命令规则 | #25 | `c9a091ff2bb674ccf06db053b9bc407dc2a605e1` |
| T25 权限引擎与 exact rules | #26 | `7dbe2bc03f7657dbdad5c192054183975e6de68a` |
| T26 runtime/conversation 全量审计接线 | #27 | `d2d831a05be74b2753c0936a1b086093546801b5` |
| T27 工具卡与权限卡 | #28 | `8642aa1a677a47243d9a9b8ce53ad95b8e2cffc5` |
| T28 Ask/Plan/Execute 与 Plan 审批 | #29 | `65d83665b0683a5cd3b80a453145b4daf0483215` |
| T29 端到端验收、报告与 README | 本 PR | squash merge 后生成 |

T29 只有一条窄的 headless 集成测试、本文和 README 状态更新；没有改生产行为、依赖、schema 或 migration。

## 2. T29 端到端场景

`vega_conversation/tests/s5_acceptance.rs` 的
`confirm_mutations_are_checkpointed_audited_and_content_free_end_to_end` 使用临时项目、临时数据根、`MockProvider` 和脚本化权限 hook，完整执行同一轮：

1. Confirm 模式 new-file write，用户 Once；组合测试验证最终 exact `created_new_file` metadata 布局，metadata-before-target 的故障注入顺序由 T23 owner test 证明。
2. existing-file edit，用户 Always；组合测试验证 preimage 精确等于修改前字节和最终 exact rule，rule/approval 同事务原子性由 T25/T26 owner tests 证明。
3. 同一 normalized path 的第二次 edit 命中 exact rule，不再请求权限，但仍经历完整 proposal/approval/output/terminal 审计；第二份 preimage 精确等于第一次 edit 后字节。
4. safe bash 被用户带 note 拒绝；零进程副作用，rejected 行没有 exit code、duration 或 `output_full_path`，provider 在观察拒绝结果后继续并收敛。

测试同时断言：

- 权限 hook 恰好收到 write、首次 edit、bash 三次请求；第二次 edit 的 approval source 为 `rule`。
- 四次调用的 `ConversationEvent` 顺序稳定；首事件为 `MessageStarted`、尾事件为 `MessageFinished`。`running` 是持久化内部状态，按共享事件契约不另发 UI event；其 barrier 由 T26 owner tests 覆盖。
- DB 的 valid write/edit audit、success output、approval JSON 均 strict decode；所有 `checkpoint_ref` 可 roundtrip，且不含 raw project/thread/call id 或绝对数据根。
- DB input/output、共享事件、event sink 和第二轮 provider request 均不含 write/edit 正文 sentinel 或绝对数据根；provider 收到四个按 call id 关联的安全 tool result。
- new-file call root 只有 exact metadata；两个 existing-file call root 无 metadata，preimage 位于 `files/existing.txt`。

聚焦命令原始摘要：

```text
$ cargo test -p vega_conversation --test s5_acceptance -- --nocapture
running 1 test
test confirm_mutations_are_checkpointed_audited_and_content_free_end_to_end ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

这条测试补跨边界证据，不替代各 owner suite 的路径、codec、并发、GPUI、sandbox 和取消矩阵。

## 3. 门禁与测试总数

```text
$ cargo fmt --all -- --check
exit 0

$ cargo clippy --workspace --all-targets --all-features -- -D warnings
Finished `dev` profile ...
exit 0

$ cargo test --workspace --all-features
439 unit/integration passed; 0 failed; 1 ignored
5 doctests passed; 0 failed
exit 0

$ cargo build --workspace --all-features
Finished `dev` profile ...
exit 0

$ git diff --check
exit 0
```

| crate / suite | passed | ignored |
|---|---:|---:|
| `vega` binary/controller | 9 | 0 |
| `vega_conversation` unit + T22 + T29 integration | 90 | 0 |
| `vega_markdown` | 32 | 0 |
| `vega_runtime` | 77 | 0 |
| `vega_store` | 69 | 1（真实 macOS Keychain roundtrip） |
| `vega_theme` | 6 | 0 |
| `vega_token` | 0 | 0 |
| `vega_tools` | 90 | 0 |
| `vega_ui` | 66 | 0 |
| doctest | 5 | 0 |
| **合计** | **444** | **1** |

聚焦 suite 另行通过：tools mutation 29、bash 26、danger 3；runtime permission 13、agent 28；store permissions 6、recovery 4；UI permission card 5、tool card 14、Plan card 2。Composer 验收不以 `composer` 单过滤器虚报：1-8 行由 `text_input::tests::visual_wrap_grows_shrinks_and_caps_with_cursor_follow`，历史由 `conversation_stream::tests::multiline_history_continues_and_is_thread_scoped` 与 conversation durable-history test 共同覆盖。

唯一额外提示来自上游 `block v0.1.6` future-incompatibility，不是当前 Clippy warning 或失败。

## 4. S5 DoD 对照

| DoD | 结果 | 证据 |
|---|---|---|
| SDD 先于代码，C1-C7 与危险卡键位入 spec | ✅ | #23 先于 #24-#29；tech/ui spec 已锁定 |
| T23-T29 squash merge，master 四门禁全绿 | ⏳ | #23-#29 已 squash；T29 本 PR 分支四门禁全绿，self merge 后闭环 |
| Confirm/no-rule 均确认；exact Always 后续免弹但全审计 | ✅ | T29 组合场景；runtime/conversation permission owner tests |
| once/always/deny/note/timeout 可审计 | ✅ | strict approval codec、permission engine、permission queue/UI first-wins tests |
| danger 先于 ReadOnly/rule/Auto，规则集中且未削弱 | ✅ | runtime 13 个 permission tests、danger 3 个 corpus tests；危险 UI 键位矩阵 |
| write/edit fence、唯一匹配、preimage、fingerprint recovery | ✅ | tools mutation 29；T26 recovery/call-id tests；T29 exact preimage |
| valid/invalid projection 与 output strict，正文/数据根不传播 | ✅ | tools codec、conversation strict recovery、UI fail-closed cards、T29 sentinel scan |
| startup pending recovery 只写 strict deny/recovery | ✅ | store recovery 4 与 conversation reopen tests |
| bash cwd/timeout/有界输出/Seatbelt/temp/hardlink/process-group | ✅ | tools bash 26；真实 `/usr/bin/sandbox-exec` 集成均通过 |
| 危险卡完整键盘与 disappear/timeout fail closed | ✅（自动化） | 5 个 permission-card tests + conversation-stream close/switch tests；未做人手视觉走查 |
| Ask/Plan 零 mutating tools；批准后才 Execute | ✅ | runtime capability tests；Plan completion/approval/approved-instruction tests |
| Plan supersede、重启、completion/review 竞态、single winner | ✅ | store messages 并发 tests、conversation plans/agent tests、controller tests |
| 六表、0002 add-only、headless、唯一事件流、UI 无 SQLite | ✅ | migration/依赖/源码 scans 与 fresh/v1 upgrade tests |
| UI Spec §6 与 P1-P8 逐项记录 | ✅（记录义务） | 见 §7；未测项和未达标项没有标为通过 |
| 报告/README/偏离与后置 | ✅ | 本文与 README |

Phase 1 plan 的 S5 验收原文已满足自动化部分：默认 Confirm 对每次无 exact rule 的写操作请求确认；危险拦截生效；Plan 只有批准后进入 Execute；工具与审批状态均落库可审计。

## 5. 安全与审计矩阵

| 领域 | 结果 |
|---|---|
| write/edit path fence | absolute、`..`、symlink、hardlink、`.git`、worktree gitdir/hooks、missing parent 均 fail closed |
| checkpoint | new-file metadata 在创建前 exact 落盘；existing target 仅保存 binary preimage；wire 只传 opaque ref |
| edit | old string 必须按字节唯一匹配；0、多匹配、空、non-UTF8 等拒绝且零修改 |
| strict codecs | missing/extra/wrong type、负/小数/overflow、常量/path/hash/ref 错误全部拒绝 |
| invalid mutations | deterministic `write_edit_invalid_v1` + deny/validation；零 permission/execution，不回显 raw JSON/path/body |
| permission order | capability → danger → ReadOnly → danger approval → exact rule → Auto → Confirm，不能旁路 |
| bash | 16 KiB chunk、64 KiB line、head/tail 各 2k 行与 4 MiB、≤8 MiB retained；无无界 bash output read |
| sandbox | exact project + per-call 0700 private temp；共享 `/private/tmp`、`.git`/gitdir 写拒绝；dual-root hardlink scan |
| cancellation | TERM → grace → KILL → wait/reap shell 与继承 PGID descendants；超时/取消均 fail closed |
| UI cards | valid 安全投影才渲染；corrupt/late/invalid shape 固定损坏或 rejected card，不能回退到 raw provider input |
| Plan | add-only columns；corrupt state fail closed；conditional update/Immediate transaction 保证 first-wins |

## 6. 红线复核

| 红线 | 结果 |
|---|---|
| 新白名单外依赖 | 0；T29 未改 `Cargo.toml`/`Cargo.lock`。T27 的 `gpui/test-support` 及其 pinned dev-only transitive 依赖已获人类批准，production tree 不含该测试依赖 |
| 真实 API key / 网络 / 费用 | 0；T29 只用 `MockProvider`、tempfile 与 mock permission hook |
| runtime/tools UI/GPUI 依赖 | `cargo tree` 两个 headless scan 均 0 命中 |
| UI 直接使用 `RuntimeEvent` 或 SQLite | 两项 scan 均 0 命中；UI 只消费 `ConversationEvent`/typed conversation API |
| UI 六位硬编码色值 | 0 命中；视觉值来自 theme token |
| 非测试 `unwrap`/`expect`/panic/unsafe | 扫描逐项分类后 0 个生产违规；命中均位于 `#[cfg(test)]`、doc example 或受控测试 fixture |
| schema | 仍恰好六表；`0001_init.sql` 对 #23 基线 diff 为 0；`0002_plan_review.sql` 仅三条 `ALTER TABLE messages ADD COLUMN` |
| migration 删除/重建 | 0；无 `DROP TABLE`、第七表或生产 runtime DDL |
| key | config 只存 `key_ref`，密钥实现只走 Keychain；真实 Keychain test 保持 ignored，未执行 |
| write/edit 正文 | T29 DB/event/provider sentinel 断言通过；UI owner tests 隐藏 fingerprint/checkpoint ref/raw input |
| bash unbounded output / broad temp allow | bash 使用固定 chunk/ring；`read_to_end` 命中仅 read/grep 的有界探针，不在 bash；`/private/tmp` 生产命中仅 private temp base，不 broad-allow |

测试-only recovery trigger、secret-like fixtures、panic/unwrap 断言均保留在测试模块，没有成为生产路径。

## 7. UI Spec §6 与 P1-P8

本轮执行了自动化 GPUI tests 与一次 `cargo xtask bench` 实窗探针；没有执行完整人工截图/视觉走查。

```text
cold_start     p50=2022ms p99=2026ms      spawn-to-exit placeholder
memory_idle    107.3 MB                   RSS after 5 s
render_frame   fps=61                     60Hz vsync cap
stream_phase   fps=60 build p50=82µs p99=289µs ~500 delta/s; frozen_remat=0
```

| Checklist / 性能项 | S5 结果 |
|---|---|
| 颜色/字体来自 token | ✅ 自动扫描与 6 个 theme tests；未发现硬编码六位色值 |
| Light/Dark 无闪烁/遗漏 | ⚠️ theme 状态自动化通过；未进行 tool/permission/Plan/Composer 全状态人工切换 |
| CJK 混排 | ⚠️ CJK width、UTF-8 delta、Composer CJK wrap 自动化通过；真实窗口字体与豆腐块未人工走查，不声明整体通过 |
| 键盘全流程 | ⚠️ 普通/危险权限卡、PlanCard、模式/权限控件已有 GPUI 自动化；S6 diff/commit 尚未实现，完整「建会话→提交」流程不可走完 |
| 960×600 | ⚠️ 未执行人工最小窗口走查，不声明通过 |
| Codex/ZCode 并排截图 | ⚠️ 未执行，不声明通过 |
| P1 120fps | ⚠️ 目标机当前显示器 60Hz；实测 61fps，字面 120fps 留 S8 ProMotion 复测 |
| P2 token 上屏 `<16ms` | ⚠️ `lone_text_delta_flushes_during_provider_stall_within_sixteen_ms` 通过，controller 使用有界通道；bench 尚未给出完整 received→render 延迟分布 |
| P3 frozen 区零重排 | ✅ probe `frozen_remat=0`，冻结缓存自动化 tests 通过 |
| P4 滚动锚定 | ✅ 状态机自动化覆盖贴底/上翻/恢复；未做人工滚动观感走查 |
| P5 交互 `<100ms` | ⚠️ 未做人工/端到端延迟测量 |
| P6 仅允许两类动效 | ⚠️ 无新增装饰动画；未做人工视觉走查 |
| P7 首屏 `<50ms` | ⚠️ 当前 2022/2026ms 是 spawn-to-exit 占位，不能代表首帧；S8 补真实埋点 |
| P8 idle `<100MB` | ❌ 107.3MB，高于目标；S8 调优项 |

因此不能声称 UI Spec §6 全部达标；S5 功能 DoD 不以未实现的 S6 diff/commit 或 S8 性能调优冒充通过。

## 8. 偏离、后置与残余风险

**除下列已明确记录项外，无未披露偏离。** 以下包含明示后置、已知 spec 偏离与已接受残余：

1. ReadOnly 的 bash 只读白名单为空；只读能力仅由 read/glob/grep 提供。
2. permissions Phase 1 只做 project/tool/pattern 字节级 exact signature；wildcard 规则后置。
3. S5 checkpoint 仅是单文件修改前 preimage；浏览、工作区快照和回退属于 Phase 2。
4. Composer 的 @引用、slash 命令、模型选择器后置；分支选择器归 S6。
5. 危险卡 bare Enter 在任意焦点都 Deny，是 2026-08-30 人类安全裁决；覆盖普通卡 Enter=Once。
6. PlanCard 当前承载审批状态与逐行计划文本；更完整 markdown/产物/diff 联动随 S6 产物审阅完善。
7. Plan approval 已原子落库；provider 启动失败会显示 `ApprovedNotStarted` 并阻止新 draft。显式 Resume 操作尚未提供，后续卡补齐，不能把自动隐式重试当恢复。
8. bash process-group 收拢 shell 与仍继承 PGID 的 descendants；主动 `setsid` 逃逸是已知残余，不声称 100% process-tree containment。
9. Seatbelt 是 path-based；dual-root scan 后到打开文件前的并发 hardlink scan-to-open TOCTOU 仍存在。cleanup 根身份不可信或 reap 未确认时保留 private temp 供安全 GC，不扩大权限或冒险递归删除。
10. `fingerprint_v1` 使用仓内 safe Rust SHA-256 和公开向量，因为依赖白名单不含额外 crypto crate；未来替换仍需批准。
11. T26 保留两个不阻塞安全性的观察：极窄取消窗口可多计 `AgentOutcome.executed_tool_call_count` 但不启动副作用；跨 SQLite connection 的 call-id 主键竞态 fail closed，未必返回 provider-visible conflict。
12. **ui-spec §4.4 已知偏离**：Composer 已达到 1-8 visual rows 与 caret-follow 的 8-row painted viewport；超过 8 行尚不是独立 wheel/vertical inner-scroll，“超出内滚”只部分满足，S8 补齐。
13. 工具卡已对长命令做 width-bounded virtual rows；展开态单条超长物理输出行仍可能视觉裁切，持久化的有界输出保持完整，留 UI 打磨处理。

## 9. 真实 key、费用与 dogfood 边界

- 未读取、创建或使用真实 API key；真实 Keychain roundtrip test 保持 ignored。
- 未调用真实 LLM/API、未产生费用、未做 API 账单对比。
- S5 仍沿用 `cost_microcents=0` 占位；S7 才接 pricing engine。
- 真实「改代码 → diff → commit」和一周 dogfood 是人类活动：当前只把 mock runtime、权限/Plan、审计与失败恢复路径准备到位；S6 diff/commit 与 S7 成本可见完成后才具备完整 Phase 1 dogfood 路径。

## 10. 收尾结论

S5 的写工具、macOS sandbox、权限决策/卡片、Ask/Plan/Execute、Plan 审批与六表审计已通过 444 个自动化测试及静态红线检查。T29 PR squash merge 后，S5 代码/文档闭环；未完成的人工 UI、真实 provider、S6 交接和 S8 性能项均在本文显式保留。
