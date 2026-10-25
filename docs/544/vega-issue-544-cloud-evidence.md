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

## 修复及上游集成：93c25ac4e593677ff80e70b6e4e40b38ddb12db7

[固定源码](https://github.com/puzige/vega/tree/93c25ac4e593677ff80e70b6e4e40b38ddb12db7) / [原始 patch](https://github.com/puzige/vega/commit/93c25ac4e593677ff80e70b6e4e40b38ddb12db7.patch)，tree `5f8936ccdca9ad282c54c4d3050d8f455e26135a`，parents `88f4011b47f7dd0ceade5b56438a11ed551a8fe8` / `429c6a819fc5a30f8280afd2f6ed015d4682bf41`。writer 停写后父复算完整 35 文件 manifest `8d280cd6a29dbe49334a6c091582ce1faadb0ac41ce50646104a6b68e4b9729e` 一致，29 改动 Rust 文件定向 1.98 rustfmt、`git diff HEAD --check` 均 exit 0；实际 stage 两冲突后完成合并，08:42Z 推送。

- child 能力改为 reservation 前取、生命周期/阻塞写/terminal/release 贯穿，quarantine 保持占用；UI replacement 保留原 copy/selection/image 对象并恢复未决行；Cancel 校验精确 active submission/attempt。
- 原 39 个测试定义保留，新增 5 个；父完整属性清点 44（ordinary facts 20、store admission 4、事务 7、child lifecycle 2、app/controller 6、UI 5）。最初仅匹配 `#[test]`/GPUI 的 42 是遗漏两处 `#[tokio::test]` 的静态脚本口径，已按完整属性更正，不是发现数或删测。
- GitHub 源码读回验证 app 测试 blob `3bba659268449f38abd22f125409701eeef49562` 可访问。先前 zsh 未引用带问号 API path 导致 glob 错误，修正引用后只读成功；不是 GitHub 不可达，也未重放写入。

### Actions 40177662011：编译/测试接线失败，非行为结果

[Run](https://github.com/puzige/vega/actions/runs/40177662011) 精确 head 如上，08:49Z 按五分钟节奏查询得到 completed / failure。Clippy `121121753117`、Nextest `121121752775`、required `121122619478` 均 failure。fmt 实际 success；Python 原始日志 5 + 15 + 40 = 60 项 OK。Rust 构建失败，发现/执行计数未取得，不能称 44 项通过或失败。

父读原始日志确认三项，均非外部 runner 故障：

1. **生产编译缺陷 E0382**，`window/ordinary_send.rs:461–489`：CheckStatus 的 `async move` 捕获 `stream`，match 后又调用 `stream.update`。须保留外层 handle、为异步观察独立 clone，保持精确 thread/attempt 检查。
2. **测试接线 unused import**，UI `ordinary_tests.rs:2` 的 `use gpui_kit::gpui` 被 `-D warnings` 拒绝。删除确实未用导入，不添加 allow 或降低 lint。
3. **测试宏作用域递归**，app `ordinary_tests.rs:131` 的 `#[gpui_kit::test]` 展开 `#[test]` 时递归。父静态读取实际锁定 gpui-kit 0.6.1 facade `lib.rs:82–94`：test-support 的 glob 导出包含同名宏，要求测试显式导入类型，避免覆盖 Rust 内建 test；gpui-pre-macros 0.3.4 `test.rs:278–283` 生成未限定 `#[test]`。本 app main/window glob 加测试 `use super::*` 将宏带入作用域，须修显式 import 边界，不按编译器泛用建议提高递归上限。

云端随既有 test-support feature 机械解析 11 个测试闭包包（含 proptest/辅助宏），当前 Cargo.lock 未更新；这不是本轮新增产品依赖或 GPUI 升级。没有重跑、删除测试、削弱断言、放宽 lint 或改 CI。原 writer 尚在处理两项队列入口修复；本批确切编译证据在其停写后续交。修复后新 SHA 再用同一 required Actions 验证，首次失败永久保留。

### 同版独审增量：history 展示权威与分页顺序仍需修复

独立非写入者对 pinned 93 相对 88 的 35 文件 digest 复算一致；原 39 完整测试定义不变、5 个新增成立。确认 child admission、精确 Cancel、两 merge 冲突和上游隐私/storage/artifact 保留没有新增 actionable finding，但新 history restoration 发现两项，父已复读原始路径：

- **P1 隐私回退**：`ordinary_submissions.rs:165–195` 保留所有 tracked user（含 retired），再以匹配 key 无条件 `self.entries[index] = entry` 覆盖刚 hydrate 的 authoritative history。`window/privacy.rs:91–108` 的生产 privacy review/clean refresh 正是 replace_history_window 调用者。若普通旧正文后来被识别为凭据，新脱敏 page 会被旧正文/MessageCopy 覆盖。修复必须保 key 而不保过时正文权威；fresh content 改变时更新 copy/render、清除旧 selection/copy 缓存；未决缺行同样当前安全投影。回归需真实隐私/history服务与生产refresh入口，不用同文 synthetic page 冒充。
- **P2 顺序错误**：`ordinary_submissions.rs:188–201` 将缺失 ordinary 加到旧 window 后；`content.rs:127–137` 新 page 直接 append 到其后，`245–281` 遇 durable key 只更新 seq/skip，不重新归位。旧1–20 + S，再hydrate21–40形成1–20,S,21–40。须将新增历史合入未决 suffix之前、durable range到达时按序放置，保留原key/copy/preview和非tail anchor；覆盖未提交与已提交未启动、多页、唯一text/images与相邻顺序。

这是原隐私安全和 A2/A9/A11 的产品缺陷闭合，不授权用稳定 identity 保留已不安全内容，不降低原等价确认的对象/选择保留要求。以上审查不包含正在写入的两项队列补修；静态不是编译或行为结果。

## 队列补修中断检查点，2026-10-25T09:11:58Z

原单一 writer 在最终交接前中断，工具状态 failed / release released，checkpoint `01M5S54HSAK08K7Q93G18N22JB`。恢复调用历史回执为 unavailable tool，未成功恢复、未重放编辑。父已重新检查 HEAD `93c25ac4e593677ff80e70b6e4e40b38ddb12db7`、完整差异和 7 文件现状；代码 manifest 在中断后两次复算均为 `0c63e349f8bee0fc370b6f79d82b06f669d243ffbad93cebe8596206c712e40c`。未知失败 edit `call_OhMhM3kpcRbUgKoY9kj01NyN` 的处置是保留并审查当前实际字节，绝不按旧调用猜测或重放；当前未见冲突标记/截断，新增 Stop/Resume 测试和 generation cleanup 均已保存，但中断 writer 的完整 READY 不存在。

父针对当前字节执行 Rust 1.98.0 定向 rustfmt check（7 文件）、diff whitespace check exit 0；静态比对 44 个原测试完整块逐字保留，现有 46 个定义。此为父验证的恢复检查点，不是行为通过。现状包含独立 queue wait UI、真实 Stop guard/render、timeout paused/error、generation-scoped 清除和两项真实控件回归；上述三项编译和两项 history 缺陷仍未修。

新 fetch 发现上游 `f6723682f960f87941ffcd9cae477f4a5dbef777`（已合入 i18n，193 文件）。不改 index/worktree 的 merge 分析显示 `window/agent.rs`、conversation `agent/entry.rs`、`agent/pipeline.rs`、UI `conversation_stream/render.rs` 四冲突。先保存中断字节再集成，由唯一替代 writer 保留上游本地化/generated-notice/privacy 语义及本卡 admission/receipt/ordinary 不变量；不借合并回退已合入功能，不修改 workflows 或通用验证。实际恢复包将携本检查点、原日志与明确未决项，不重做已完成阶段。
