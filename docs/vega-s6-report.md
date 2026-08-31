# ✦ Vega — S6 验收报告（Diff 审阅、产物、分支与 Commit）

**日期** 2026-08-31 · **范围** T30-T35 · **结论** S6 自动化/Mock 边界已闭合；T35 PR 与 squash merge 待完成

## 1. 证据冻结与交付台账

- evidence cutoff：`2026-08-31T07:54:54+08:00`（`2026-08-30T23:54:54Z`）
- fetched `origin/master`：`0f8ce5e4369a861a9cdca5833082ab0f61ad3bc0`
- tested implementation HEAD：`1a08433affac68c48fde1910c8af7810e44f614a`
- branch：`feat/s6-t35-acceptance`，相对 `origin/master` 为 `+2/-0`，测试时 clean
- T35 PR：`NOT CREATED`；T35 squash：`PENDING`
- report/README finalization commit：cutoff 时 `PENDING`；本文不能自包含自身 commit hash
- 环境：macOS 27.0（26A5421a）/ arm64；Rust/Cargo 1.98.0；Apple Git 2.54.0
- 真实 provider、API key、外网费用、真实 `/usr/bin/open`：`NOT USED`

| 交付 | merged PR | master squash commit |
|---|---:|---|
| S6 SDD / C1-C8 | #31 | `f92c5267f69983b1bf0667e29c2ec621c1fc7982` |
| T30 bounded Git workspace snapshot | #32 | `22e94546ac745916ed15691e8ac16af94a162d58` |
| T31 Diff UI/controller | #33 | `4d528411cd51dbab4dc16f3325f1a7d96a245981` |
| T32 artifact/Open in | #34 | `7e57e6328487b145330b80d2b690498c05d05827` |
| T33 branch selector | #37 | `924a9f51ff1082c9ee13f43db83c219674cfad8a` |
| T34 two-stage commit assistance | #38 | `0f8ce5e4369a861a9cdca5833082ab0f61ad3bc0` |
| T35 acceptance tests | pending | `2a8f133ad6ef1b84aa6030279637bcab139aeb49`（branch commit） |
| T35 discovered T34 fix | pending | `1a08433affac68c48fde1910c8af7810e44f614a`（branch commit） |

T30 的 PR #32 由 merged PR metadata 与 merge commit exact 交叉核对，不由缺少 `(#32)` 的本地 subject 推断。S7 的 #35/#36 虽在 first-parent 历史中穿插，不属于 S6 台账。

## 2. T35 production E2E

`crates/vega_conversation/tests/s6_acceptance.rs` 使用 owned temporary repositories、repo-local identity、临时 Store、`MockProvider`、production `Tools`/conversation event stream，以及 production Git/artifact/branch/commit services；fixture Git 子进程清除全部 inherited `GIT_*`，无 remote。

主场景 `agent_diff_artifact_dirty_reject_and_two_stage_commit` 在同一仓库完成：

1. agent 经真实 edit + Once permission 修改原文件和 README；每次 matching Proposed、Approved(Once)、Finished 均 exact once 且严格有序。
2. agent 经真实 permission-gated bash `mv` 与 write 生成 Deleted + Untracked rename 两侧及 `artifact.md`。T30 production snapshot 对纯 worktree `mv` 的真实表示是 Deleted + Untracked；本测试不伪造 `previous_label`。exact rename codec/topology 继续由 T30 owner evidence 覆盖。
3. 原路径 edit artifact 在 rename 后按 production 语义永久降级为 `WorkspaceChange`、清空 current id 并禁用 Preview/Open；README card 保持 `AgentArtifact`。新 artifact 的 bounded preview 字节精确。
4. 每个 rename/untracked row 的 lazy diff projection 可用；dirty repository 的 branch refresh 返回 typed `BranchDirty`，HEAD 未动。
5. 同仓 Checklist 精确为零 staged、四个非 forced optional rows：README Modified、artifact Added、original Deleted、renamed Added。选择全部 rows 后 Prepare 将合法 D+? 归一为 staged R；Mock draft 请求一次、tools 为空；用户编辑 message 后 Commit 一次。
6. terminal 断言 exact single parent=base、current ref=`main`、porcelain-v2 clean、exact commit message、exact tree name set、三份文件 exact bytes、无 remote。

独立 clean fixture 的 `clean_fixture_branch_switches_authoritatively` 验证 prepare 不变更 HEAD，execute 后 authoritative snapshot 只标 `topic` current，工作树 exact materialize 且 clean。dirty fixture 从未被清理后伪装为 branch success。

聚焦原始摘要：

```text
$ cargo test -p vega_conversation --test s6_acceptance
running 2 tests
test clean_fixture_branch_switches_authoritatively ... ok
test agent_diff_artifact_dirty_reject_and_two_stage_commit ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p vega --bin vega s6_controller -- --list | rg -c s6_controller
2
$ cargo test -p vega --bin vega s6_controller
test branch_controller_s6_controller_owner_success_applies_authority_then_releases ... ok
test commit_app_production_handlers_reconcile_before_release_across_close_and_routes_s6_controller ... ok
test result: ok. 2 passed; 0 failed; 32 filtered out
```

App 证据复用 T33/T34 已提交的 production handler E2E：真实 handler、real repo mutation、duplicate/close/disconnect recovery、authoritative consumers 后 exact lease release；不是本地伪造 success。过滤前先硬断言列表数 `2 >= 1`，避免零测试假绿。

Open 边界没有在跨链测试调用真实 `/usr/bin/open`。T35 只断言 artifact 的 current-id/Preview/Open eligibility；exact 六套 raw argv 与 0/1 fake attempt 复用 T32 owner test `open_in_uses_six_exact_raw_argv_forms`（1/1 pass）。PermissionRequest exact 字段复用 S5 permission owner evidence；T35 新增的是 production event cardinality/order 与 Once 结果，不扩大声明。

## 3. 验收发现并闭合的生产缺陷

E2E-first 首轮在同仓 Prepare 暴露了 T34 缺陷：A 的真实状态为 `.M README.md`、`.D src/original.rs`、`? artifact.md`、`? src/renamed.rs`，但 untracked `x='?'` 被错误投影进 forced staged；即使修正该投影，`git add` 又会把所选 D+? 合法归一为 staged `R.`，原 per-row transition ownership 会返回 `ChangedDuringRead`。

`1a08433…` 做了窄修复：

- untracked 只进入 optional，不进入 forced staged；
- 只允许两条均被选择的 ordinary `.D` source + untracked `??` destination 归一为一个 exact `R.`；destination mode/stage exact、source stage absent、Y 全 clean；
- B 中任何 path/previous 与 source/destination 相交的 record 集合必须恰好只有该一个 R，额外 touching record 一律拒绝；原 outside-selection freeze 不变。

两个 real-Git positive owner tests 与一个 synthetic extra-touching negative 均通过；trusted-git owner suite 最终 `66/66`，独立终审结论 P0=0/P1=0。此项是已修复的验收发现，不列为未解决 residual。

## 4. 四门禁、总数与性能

| gate | time (Asia/Shanghai) | result |
|---|---|---|
| `cargo fmt --all -- --check` | 07:49:26–07:49:27 | PASS, exit 0 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 07:49:27–07:49:29 | PASS, exit 0 |
| `cargo test --workspace --all-features` | 07:51:16–07:52:56 | PASS, exit 0 |
| `cargo build --workspace --all-targets --all-features` | 07:53:01–07:53:03 | PASS, exit 0 |
| `cargo xtask bench` | 07:53:08–07:53:45 | PASS, exit 0 |
| `git diff --check` + supplemental `origin/master...HEAD` | 07:54 | PASS, empty output |

全量 count 只由最终 workspace log 的每条 `test result:` 机器求和，focused 子集未重复累加：

| crate / suite | passed | ignored |
|---|---:|---:|
| `vega` controller | 34 | 0 |
| `vega_conversation` unit + S5/S6/TODO integrations | 243 | 0 |
| `vega_markdown` | 32 | 0 |
| `vega_runtime` | 84 | 0 |
| `vega_store` | 69 | 1（真实 macOS Keychain roundtrip） |
| `vega_theme` | 6 | 0 |
| `vega_token` | 23 | 0 |
| `vega_tools` | 90 | 0 |
| `vega_ui` | 101 | 0 |
| doctests | 5 | 0 |
| **合计** | **687** | **1** |

最终 bench 原始值：

```text
cold_start     p50=2025ms p99=2029ms      spawn-to-exit placeholder
memory_idle    108.7 MB                   RSS after 5 s
render_frame   fps=60                     60Hz vsync cap
stream_phase   fps=60 build p50=88µs p99=285µs ~500 delta/s; frozen_remat=0
```

`block v0.1.6` future-incompatibility 是上游提示，不是 Clippy warning/failure。

原始日志仅留本机 `/tmp`，未进仓：`fmt/clippy/test/build/bench/static/focused` SHA-256 分别为 `806d28d…`、`f14557d5…`、`4032c74d…`、`f05357fe…`、`325e7d31…`、`6b0ea1b1…`、`ab0c2e86…`。

## 5. S6 DoD 对照

| DoD | 结果与证据 |
|---|---|
| bounded ephemeral projection 是唯一 UI 正文通道 | ✅ T30 projection/cap/redaction owner tests；focused projection redaction 1/1 |
| metadata + lazy patch 覆盖 staged/unstaged/untracked/binary/rename/non-UTF8；Diff UI 完整 | ✅ T30/T31 owner suites；T35 current opaque IDs/lazy projection |
| provenance immediate identity/hash、later downgrade、bash-only workspace change | ✅ T32 owner suites；T35 old edit card真实 stale/downgrade |
| Open 六 fixed targets、path fences、0/1 attempt、stale drop | ✅ T32 fake-launcher/security owners；T35 不真实 Open |
| branch clean/active/operation/filter guards与 authoritative refresh | ✅ T33 owners；T35 dirty reject + independent clean switch |
| three-source IndexSnapshot、component ledger、one-add/zero-add、A→B authority | ✅ T34 owners；T35 同仓四 row journey及 D+?→R 窄修复 |
| 32 KiB stdin、strict draft/summary/proof、Retiring lifecycle、无 retry/rollback/push/temp | ✅ T34 headless/app E2E与安全回归；T35 Mock draft/edit/commit/post-tree |
| process group/caps/filter/attrs/codec/ABA 安全内核 | ✅ T30-T34 owner suites与最终 full workspace gate；不由单一 T35 E2E 过度声明 |
| fresh temp/local identity/no remote；零新依赖/DDL；headless/UI boundary | ✅ T35 fixtures与静态门禁，仍六表 |
| report/README 与 ui-spec §6 分层 | ✅ 本文；人工/硬件未测不标通过 |

## 6. 红线扫描分类

| redline | 结果 |
|---|---|
| headless GPUI dependency | runtime/tools/conversation 三个 `cargo tree` 均零命中 |
| hardcoded color | 四个 S6 UI 文件六位色/rgba scan 零命中 |
| UI SQLite | `vega_ui` 与 app main 零命中 |
| schema/dependency | 恰好六个 `CREATE TABLE`；migration/root/crate manifests/lock 相对 origin/master 零 diff |
| event enums | 仅既有 `ConversationEvent` 与 `RuntimeEvent` 两处定义；T35 零 variant change |
| unwrap/expect | frozen scan 2493 hits；T35 新增 6 个均位于 `trusted_git #[cfg(test)]` owner fixtures；production fix 零新增；其余为此前各卡已审 test/doc/受控 fixture baseline |
| forbidden Git terms | frozen scan 237 hits；T35 唯一新增文本命中是 integration event vector 的 `.push(event.clone())`，不是 Git push；production fix 不新增危险 argv；其余为既有 test/comment/error vocabulary及 T30-T34 固定 trusted verbs |
| redaction | `projection_redaction` 1/1、`commit_redaction` 1/1；raw path/OID/stderr/key/body 未写入报告 |

## 7. UI Spec §6 / P1-P8

| 项 | 自动化证据 | 人工/硬件边界 |
|---|---|---|
| token | ✅ theme/component tests；S6 hardcoded-color zero scan | 真实字体观感未走查 ⚠️ |
| Light/Dark | ✅ palette/theme state tests | 真实窗口切换无闪烁未走查 ⚠️ |
| CJK | ✅ CJK width、UTF-8/non-UTF8/emoji/escaping tests | fallback/豆腐块真实窗口未走查 ⚠️ |
| keyboard | ✅ diff/artifact/branch/commit GPUI owner tests与两个 production handler E2E | 完整真实窗口链未人工走查 ⚠️ |
| 960×600 | layout/geometry constants自动化 | 像素截图未做 ⚠️ |
| P1 120fps | bench 实测 60fps | 60Hz 机器不能证明 120fps；ProMotion 留 S8 ⚠️ |
| P2 `<16ms` | lone-delta/有界 channel owner tests | received→paint 真机分布未测 ⚠️ |
| P3 frozen zero-remat | `frozen_remat=0` ✅ | — |
| P4 anchor | 状态机 tests ✅ | 人工滚动观感未测 ⚠️ |
| P5 `<100ms` | controller first-wins/worker tests | 人工端到端延迟未测 ⚠️ |
| P6 motion | 无新增装饰动画 | 人工视觉未测 ⚠️ |
| P7 first frame `<50ms` | 当前 2025/2029ms 明示为 spawn-to-exit placeholder | 不能冒充首帧；S8 补 instrumentation ⚠️ |
| P8 idle `<100MB` | 108.7MB | 当前未达目标 ❌；S8 调优 |
| competitor | 无自动化替代 | Codex/ZCode 对照截图未做 ⚠️ |

## 8. 失败历史、限制与 residual

最终门禁为绿，但保留三类 pre-final 并发时序失败：trusted-git owner 首轮的既有 PGID fixture 读 attempt 文件得到 `ENOENT`，单跑与整组复跑通过；post-amend workspace 首轮的既有 deferred-overflow fixture 未在断言前创建 marker，单跑与 exact full rerun 通过；docs-only commit 后的 exact workspace gate 两次在 after-Git timeout fixture 读取 attempt marker 前得到 `ENOENT`，同一测试单跑通过。最后一项确认是 test-only 调度竞态：保留 production timeout/kill/authoritative assertions 与 30 秒 child sleep，仅把该 fixture 的有界 timeout window 从 500 ms 提高到 3 秒，确保 parallel load 下 child 先执行真实 Git 并写 marker；修复后重新运行全部 exact gates。这些失败未被改写成初次成功，也未被误报为 production green。

冻结 residual 原样保留：

1. filter driver repository：relevant `check-attr --all` 只要出现 `filter` attribute name 即拒绝；same-user preflight 后修改 attrs/config 是 path-based TOCTOU residual，不宣称原子隔离。
2. Git child 只收拢 inherited PGID descendants；主动 `setsid` 逃逸是 residual。
3. hooks 与 signing 固定关闭；依赖它们的 repo 需在终端提交。
4. Phase 1 image metadata-only；Open in 仅六个 fixed targets；custom handoff、PR assistance、Diff v2 留 Phase 2。
5. Composer @引用、/命令、模型选择器与 >8 行独立 inner-scroll 后置。
6. T35 报告无法包含自身尚未产生的 PR/squash hash；本报告只列既存 branch commits，Phase 1 最终报告补 squash。
7. fake launcher/MockProvider 不等于真实 app/LLM/key/费用；真实 UI/CJK/960×600/竞品截图/ProMotion/P1-P8 留人类/S8。
8. T34 path-based Git mutation接受同一用户在复验与 Git 实际读取/更新间替换 selected content/type/path、attrs/config/ref 的 TOCTOU；three-source/identity/hash/ref 仅缩窗，不宣称 byte-atomic/race-free。

## 9. 结论

S6 在 owned-temp/Mock/fake-launcher 自动化边界完成 Diff、产物、Open eligibility、分支与 two-stage commit 的生产入口闭环；E2E 还发现并修复了一个真实 T34 rename normalization 缺陷。最终 687 tests passed、1 个真实 Keychain test ignored，四门禁、bench 与静态红线全绿。T35 PR/squash、真实 provider/key/费用、真实 `/usr/bin/open`、人工 UI/硬件与 S8 性能调优仍明确 pending，不能把 mock 验收写成 dogfood。
