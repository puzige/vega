# #544 云端与独审证据

## F 原始基础层：63b6e44640318d855a0f7900571d0347e0980b63

- [Draft PR #611](https://github.com/puzige/vega/pull/611)，仍 In progress；只有基础层，未接 UI/app/pipeline，不代表 A1–A12 完成。
- [固定源码](https://github.com/puzige/vega/tree/63b6e44640318d855a0f7900571d0347e0980b63) / [原始 patch](https://github.com/puzige/vega/commit/63b6e44640318d855a0f7900571d0347e0980b63.patch)。开发测试源码随提交固定：admission 4、ordinary facts 10，合计 14 个源码定义；不等于发现或执行计数。
- 独立非写入者审查核对五份 SHA-256 与实现者 READY 一致；审查期间 parent 将同字节提交成此 SHA，审查者读回确认无内容变化。
- 本地 Rust 1.98.0 targeted rustfmt `--check --edition 2024 --config skip_children=true`、`git diff --check` 均 exit 0。未运行 Vega、编译/check/Clippy、构建、测试或 CUA。

### 独审实际结论：changes requested

1. P1：RawSubmission 强制要求完整 ResolvedModel；当前 UI 合法无 reasoning profile 时 model_snapshot_for_submit 返回 None，强制解析会把配置 I/O 放到本地 pending 之前。修正为先捕获 selected model/reasoning，真实解析后绑定 authority，不造假模型。
2. P1：receipt 校验把 accepted 和 persisted 正文等同，无法表达契约约定的不同载荷域。父复读现有历史脱敏代码，确认它主要处理 assistant/tool，不能笼统声称新 user 总会被脱敏；仍应以明确 persistence binding 校验实际回执，而不是放松 accepted 身份或伪造持久化正文。
3. P2：两个登记 worker，B 正在 commit 时 A 异常 drop 会将 commit 改 Unknown，随后 B 的真实成功 receipt 被拒绝。需保留精确 committer 归属并接受其当前 attempt 的真实回执，同时保持 abnormal/primary未知与禁重投。

裸 admission clone 只证明占用，不证明所有副作用 worker 已登记或收敛。后续接线必须使用 registered worker；不能仅凭 capability clone 或 UI释放生成恢复资格。以上修正不改变 A1–A12，不是删除断言或验收改小。

### Actions 首轮：编译失败，行为未执行

[Run 40158678667](https://github.com/puzige/vega/actions/runs/40158678667)，绑定 head `63b6e44640318d855a0f7900571d0347e0980b63`，completed / failure。

| job | ID | 结果 |
|---|---|---|
| Clippy | 121065405603 | failure；Clippy阶段编译错误 |
| Nextest | 121065405764 | failure；构建测试目标失败，发现/执行计数未取得 |
| required `check (fmt, clippy, test)` | 121066019182 | failure |

原始日志通过 `gh run view 40158678667 --log-failed` 读取，核心错误：

```text
error[E0599]: no associated function or constant named `new` found for struct `ulid::Ulid`
crates/vega_store/src/thread_admission.rs:32:44
    None => Identity::Memory(Ulid::new())
crates/vega_store/src/thread_admission.rs:108:22
    token: Ulid::new()
note: available functions include ulid::time::<impl ulid::Ulid>::generate
error: could not compile `vega_store` (lib) due to 2 previous errors
```

父核 `Cargo.toml` 实际锁定 `ulid = "=3.0.0"`，仓库既有代码使用 `Ulid::generate()`。相同不适配还存在新 conversation 模块，应一并修正；不改依赖/CI，不重跑未变化版本，不称功能 RED 或测试通过。当前原 writer 修独审发现，后续同 writer 纳入此精确编译错误，再对新固定 SHA 交原 PR Actions。

最终 head 尚未产生；没有功能通过、合并或用户手测结论。开发测试和独占支持最终须在实际验证留痕后独立清理，不能用零测试空集消解上述未解决错误。

## F 修复基础层：bc15fb9a240327ab679fc0dd9173a73bf3c762b9

- 同一 writer 已修上述三个发现，所有九处 ULID 调用使用锁定 API `Ulid::generate()`。原 14 个测试和 65 个断言保留，新增 8 个有区分度回归，合计 22 个源码定义。
- [固定源码](https://github.com/puzige/vega/tree/bc15fb9a240327ab679fc0dd9173a73bf3c762b9) / [修复 patch](https://github.com/puzige/vega/commit/bc15fb9a240327ab679fc0dd9173a73bf3c762b9.patch)。accepted、persisted 及原始快照分域；真实 committer 在 sibling 异常后仍能确认真实 commit，同时不清除异常与禁重试事实。
- 非写入者增量独审：READY-delta-review，无 actionable delta finding。审查前后复算三文件 SHA-256 一致：store/thread_admission `485d9a6cf24179aba90b20703c20e4d5bfedb68d5d837f77be96096e6d3d565a`、conversation/ordinary_submissions `ebb63614d498effcc5ae9c24f73fe89135f858a87b14c02c769b8472cd243fbe`、conversation/types/mod `1e828594e5f0017192c300c6542ddea885a66942a6f7541ba8c6e2387227e2f9`。
- 父独立执行 targeted Rust 1.98.0 rustfmt check、`git diff --check` exit 0；没有本地编译/测试或运行。

### Actions 修复轮：基础层 22/22，非完整功能验收

[Run 40160927551](https://github.com/puzige/vega/actions/runs/40160927551) 精确绑定 head `bc15fb9a240327ab679fc0dd9173a73bf3c762b9`，completed / success。

| job | ID | 实际结果 |
|---|---|---|
| Clippy（含 fmt） | 121072127169 | success |
| Nextest（含保留 Python 检查） | 121072127003 | success；22 发现 / 22 执行 / 22 通过 / 0 失败 / 0 跳过 |
| required `check (fmt, clippy, test)` | 121072751435 | success |

父读取原始 job 日志，Python 实际 5 + 15 + 40 = 60 项，均 OK；Nextest 原文：

```text
Summary [0.197s] 22 tests run: 22 passed, 0 skipped
Rust: nonempty suite completed successfully (22 discovered, including ignored/skipped)
```

此结果验证基础层能力/事实边界，不验证 UI、本卡完整 A1–A12、真实网络或 OS 呈现。基础层同字节独审/云端可复用；后续 I/G/U 集成增量和最终无 Rust 测试 head 必须分别验证，不能只拿此轮通过合并。当前单一 writer 接 UI/app/pipeline/冲突服务边界，仍 draft / In progress，无合并或用户手测。

## I/G/U 首版：5805080462bd77c4695ee1bb26a849a1b3cc3366

[固定源码](https://github.com/puzige/vega/tree/5805080462bd77c4695ee1bb26a849a1b3cc3366) / [原始 patch](https://github.com/puzige/vega/commit/5805080462bd77c4695ee1bb26a849a1b3cc3366.patch)，tree `aaac91c450767f3ad1eeb034ed1156210cb4c82f`。包含 app 后台准备、ordinary pipeline/事务、冲突服务准入和真实 UI pending/恢复接线；这是保存检查点，不是完整 READY。

- writer 停写后父复算 34 个 Rust 文件的 manifest，与 writer 最终 digest `e6237432dcf9b0a66e81c2e9d6c978fc452c2b3a704381c300ebc92a12a744d8` 一致。
- 本版仅取得 targeted Rust 1.98.0 rustfmt 和 `git diff --check` 静态结果；新增 7 个事务/生命周期测试定义，保留基础 22 个。集成发现/执行计数、编译和行为结果尚未取得。
- 2026-10-25T07:05:48Z 读回 PR head 为本 SHA、draft / OPEN / mergeStateStatus DIRTY，statusCheckRollup 为空；分支 Actions 列表仍只有基础层两轮。不能将旧基础层 green 当本版通过，也不能将未触发称为测试失败。
- fetch 得到 master `429c6a819fc5a30f8280afd2f6ed015d4682bf41`。不改变 index/worktree 的 `git merge-tree --write-tree HEAD origin/master` 识别 `history.rs` 和 store `lib.rs` 两处冲突。前者须同时保留新 fork 历史 guard 与本卡 repair admission；后者须同时导出 thread_fork/thread_admission。当前 writer 活跃且有未提交改动，暂不实际合并或覆盖；停写固定后集成并验证新 head。

### 非写入者独审：changes required，父已复读固定源码

审查使用 pinned `git show`，不包含审查期间开始的后续 working-tree 修改。四份未改基础文件 hash 匹配，固定实现契约 SHA256 `842703f62fe4cc2cb593b3bee42d3be4b60b2bb9ccddb89cefb09fdbde3b0c44` 匹配。审查者未写入、编译、测试、运行 Vega 或 CUA。

| 严重度 | 原始证据 / 缺口 | 修复边界 / 必要回归 |
|---|---|---|
| P1 | `agent/subagents.rs:1054–1065,1165–1196,1249–1270`：child 先 create/写入再 acquire，runtime 返回后先 drop，terminal/host release 尚未结束。底层 model/permission/context setters 只看 admission，delegated ownership 不能补此窗口 | child-specific capability 在 validation/create 前取得，贯穿 host admission、runtime、terminal、release；detached blocking work 持 clone；两个边界暂停时冲突 service mutation 均拒绝，真实结束才释放 |
| P2 | UI `ordinary_submissions.rs:101–108,118–149` 与 `content.rs:709–754`：history replacement 清 entries，却保留 ordinary tracking；重投影因已有记录提前返回，未提交/失败行和 Retry/Cancel 消失 | 区分记录与视觉行存在性；替换后按稳定 user ID 合并未决内容和控件，保留导航锚点，已提交不重复；text/mixed/image-only 均覆盖 |
| P2 | app `window/ordinary_send.rs:388–401`：仅校验旧提交的 attempt 后调用 thread-wide request_active_cancel，延迟 Cancel A 可取消新运行 B | 仅取消精确 submission/attempt token，或额外验证当前 controller 的对应归属；A 的迟到/重复 Cancel 不改变 B 的 token、computer-use controls 和运行态 |

这些是原 A7/A9/G 边界的产品缺陷，不改验收、不删断言。已知 title-drain/queue 唤醒、首次 effective provider/reasoning 冻结和真实 GPUI 回归另见契约有界补充，仍未完成。单一 writer 正处理该先前授权范围；本批独审修复在其停写后续交，不并发增加第二写入者。最终仍需新固定版本独审、真实云端验证、开发测试留痕清理、最终无 Rust 测试 head 的 required CI，以及合并后的用户手测。

## 有界补修：88f4011b47f7dd0ceade5b56438a11ed551a8fe8

[固定源码](https://github.com/puzige/vega/tree/88f4011b47f7dd0ceade5b56438a11ed551a8fe8) / [原始 patch](https://github.com/puzige/vega/commit/88f4011b47f7dd0ceade5b56438a11ed551a8fe8.patch)，tree `d4a66cea4e622ff919e566af7d2e80273b59e815`。父与非写入者各自复算 15 code/manifest 文件 digest `5a47c89446c8013d1423629972df11608b7745584c2cfb9d5ed706c11f2c2a03` 一致。父 targeted Rust 1.98 rustfmt、diff whitespace check exit 0；原 29 个完整测试定义字节保留，新增 10 个，共 39 个源码定义，不是云端发现/执行结果。已推送，仍未取得本版编译或行为结果。

独立 pinned delta 审查确认 title 持能力到实际 collection/title/usage 持久化结束、weak drain 不提前释放；effective authority 在 accepted payload 前绑定，重试复用且当前 provider/model/route/credentials 重验，未建立额外缺陷。真实列表测试含历史行 clipped bounds、key/MessageCopy/selection/clipboard/neighbor/scroll 观察，但尚未执行。另有两处 P2，父已复读固定源码，均为产品/真实入口缺口，不放宽验收：

1. `window/pending_messages.rs:625–633` 仅 controller 保存 queue wait；UI `composer_actions.rs:256–263` 在非 running/非 submit-pending 时拒绝 Stop，`render.rs:333–340` 也不显示 Stop。title-drain 时普通主运行已结束，因此用户不能取消等待。新测试 `window/ordinary_tests.rs:574–604` 直接调用 controller.stop_composer，绕过真实事件 guard；保留该层覆盖，补真实 Composer Stop 入口回归，并将可取消 wait 独立投影给 UI，不伪装 primary-running。
2. `window/pending_messages.rs:671–703` 超时移除 waiter 后调用 `reject_pending_queue_action`；该 helper 55–68 只写 error、不设 paused。UI `pending_messages.rs:618–636` 仅 paused 才显示 Continue，故提示稍后继续却无操作、释放 title 后也无 wake。超时须保留消息并转 recoverable paused/error；受控 clock 跨 30 秒、真实 Resume 入口验证恰好一次 launch，不提前释放能力或无限轮询。

以上独审不包含进行中的 master merge 与原三项修复；审查者已停写、无本地执行。父已在干净检查点发起合并 `429c6a819fc5a30f8280afd2f6ed015d4682bf41` 并将两冲突和原三缺陷交同 writer。后续新 delta 须另固定、独审和云端验证；不能把本节结论扩成完整通过。
