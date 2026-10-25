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

### 恢复保存与独审：79a5657833db162f171a0cd92b20ee5c6f585268

[固定源码](https://github.com/puzige/vega/tree/79a5657833db162f171a0cd92b20ee5c6f585268) / [原始 patch](https://github.com/puzige/vega/commit/79a5657833db162f171a0cd92b20ee5c6f585268.patch)，tree `6bedce11982a0d8dcbc36cc189cc06e26f3c1450`。父从静态核实字节保存并推送；原独立非写入者 pinned 审查复算相同 7 文件 manifest，结论 READY-queue-delta-review / no new actionable findings。

- 原两个队列发现源码层关闭：独立 wait 投影进入真实 Stop render/focus/request guard，实际事件至 controller 取消/清状态/暂停；timeout 使用起始前 deadline、有界退出、paused/error 及 Continue；generation/stream/message/permission/queue gates 保留，fresh admission 一次分派。
- 两项新增 GPUI 测试点击真实 Stop / Continue，观察实际事件和 controller admission 决策、消息保留和 launch count。只截在 worker 发布边界，不证明 provider/事务/OS 呈现；未执行，不称通过。
- thread switch / queue replacement / window release 清除 wait；不把 wait 伪装 actions.running。审查者未识别新增具体 API 缺陷，未编译/测试，不能清除原三项 build 和两项 history 缺陷。
- 当前唯一替代 writer `544-repair-writer-replacement` 接收完整恢复包，允许修明确缺陷及 i18n 合并兼容；原 writer 不再写入。独立只读覆盖核对将验证现有开发测试是否真正观察关键启动/清理生产出口，不用源码数量冒充完整 A1–A12。

### 同版风险覆盖核对：46 定义不能代替真实入口事实

非写入者只读核对固定 79 的 46 个定义（无执行），发现的是验证缺口，不据此断言生产存在新缺陷：真实 prepare/事务覆盖有价值，但 `agent/ordinary_tests.rs::Fixture::finish_unstarted` 手动 `messages::finish`/`record_cleanup(Ok(()))`，app UI 回执测试手动绑定 receipt。它们不能证明普通 public run 的 actor/entry 清理出口会生成这些事实。

已静态核实可行的最小生产观察路线：`run_thread_task_with_images_reasoning_and_mcp<F>` 的 sink 返回 `Result<(), VegaError>`；MessageStarted/RunStarted 在 commit/actor start 后、primary Started 前。用 file-backed Store、真实 ordinary/authority/accepted snapshot、带 matching `model_snapshot()` 的隔离 Provider、真实 Tools 和拒绝权限 hook，sink 确定失败可驱动真实 actor error/close/cleanup，再正常 root finalization 观察一份 user/images、failed assistant、零 provider、drain 与恢复资格。另用临时 DB trigger 拒绝 assistant insert 证明实际 rollback，以及 RunStarted 回调取消证明提交后/primary前取消；不调用测试手动 finish_unstarted 来替代。

actor opener/ack 丢失及 SQL commit 后 receipt 发布前没有现成可用 fault hook，若选择验证只允许 per-run 开发期窄注入、保留真实责任层并最终清理，不造生产替身或赋值即断言。A10 当前 queue dispatch 验证 ordinary=None，但不等于真实 running Composer/steer/Codex/automation/plan/credential 全路由已验证；后续最小代表回归或静态保全分别报告，不建立新外部 E2E 门禁。H 的 OS 呈现/真实延迟仍由用户手测。现有两个 history 缺陷和三编译错误仍交当前 writer，不能因补测试清理掉未解决失败。

## i18n / 隐私修复与最新 CUA 集成：11c9677dd28f03987d0c06da5651d1f5825b2b6b

[固定源码](https://github.com/puzige/vega/tree/11c9677dd28f03987d0c06da5651d1f5825b2b6b) / [原始 patch](https://github.com/puzige/vega/commit/11c9677dd28f03987d0c06da5651d1f5825b2b6b.patch)，tree `6804a37ffb29393790140b54b1d995dd5dee6430`。父先保存替代 writer 的修复并完成 i18n 合并 `cfc50d1b617e49d33f90fbd6a8c36d1a33be0135`，再无冲突合入 master `76c164876542c679abe1bfb5f525f1ade2fcd2e4`。当前版本保留上游 generated notices、隐私和 CUA 错误反馈语义；不改通用 workflow 或验证脚本。

- 修复内容：CheckStatus 使用独立 stream clone；测试导入移除未使用项并避免同名 test 宏遮蔽；权威 history 内容改变时更新保留 copy/render 并失效选择/复制缓存；未决后缀及 durable seq 参与分页归位；本卡文案接现有双语 catalogs。
- writer 停写后父复算其 19 文件 manifest `e1793e42ad56ea83ee5e34c84b2c3248f6a4f92595d97ea46e7d5e47164d47e2`，对应 CUA 合并前 cfc 内容。15 Rust 文件定向 1.98 rustfmt 与 diff 检查 exit 0。原 46 测试保留（必要导入及五处语言 lookup 等价修正），新增三项隐私/分页回归，49 个源码定义，不是执行数。
- 2026-10-25T12:41:06Z 续接读回：PR #611 draft / OPEN / BLOCKED，head 如上；Issue 开放、In progress，无新外部认领。fetch 后 origin/master 仍为 76c16487 完整 SHA，任务分支与远端相同。原 writer/reviewer 均已停写/释放，但续接工具拒绝其历史 execution；不重试已拒绝调用。当前单一新 writer 仅修下列编译错误，新非写入者仅审 pinned 11c 隐私/分页与合并增量，working-tree 修改不算已审。

### Actions 40184612340：生产模块可见性错误，尚无 Rust 执行

[Run 40184612340](https://github.com/puzige/vega/actions/runs/40184612340) 绑定本节 head，completed / failure。Clippy `121142643716`、Nextest `121142643552`、required `121143398304` 均 failure。父读取原始日志并复核当前源文件：fmt success，Python 5 + 15 + 40 = 60 项 OK；Rust 在构建阶段失败，发现/执行/通过计数未取得。

```text
error[E0616]: field `stopping` of struct `ComposerActions` is private
crates/vega_ui/src/conversation_stream/pending_messages.rs:120:26
    self.actions.stopping = false;
crates/vega_ui/src/conversation_stream/pending_messages.rs:134:30
    self.actions.stopping = false;
```

责任层为本卡队列等待 UI 直接访问 sibling module 的 private 字段，不是 runner/依赖环境故障。修复使用所属 composer-actions 模块的窄状态操作，保留 `!running && !submit_pending` 才 reset 的条件、字段封装及实际 Stop/Resume 路由；不改可见性为宽 public，不降 lint、不删测试或盲目重跑。新 head 云端将验证编译及此前未运行的全部回归；旧失败不会被静态检查消解。后续真实 ordinary public-entry 的失败/清理/取消代表覆盖按契约 §6 推进，未取得前不宣称完整 A1–A12、OS 呈现、合并或用户验收。

## 模块封装修复：903287d0498e0f8f696a1571ec64916ca0132c7c

[固定源码](https://github.com/puzige/vega/tree/903287d0498e0f8f696a1571ec64916ca0132c7c) / [原始 patch](https://github.com/puzige/vega/commit/903287d0498e0f8f696a1571ec64916ca0132c7c.patch)，tree `6d32a73ad5272d7d44699a553506183a979c1551`。两个私有字段写入改为所属模块的 `reset_composer_stopping_if_idle` 操作，原 idle 条件不变；49 个开发测试未变。writer 停写后父复算 composer_actions SHA256 `ab0b2a96350cb724afb47ab659880cabeb7ef832881870d85786e62fcdb70f94`、pending_messages `af873e6d2da62d77eae7852594f879b784badebf873abd80e6cd205ee36a5c2f`，定向 1.98 rustfmt / diff 检查 exit 0，字段访问仅留所属模块。12:54:04Z 推送。

### Actions 40195173888：下一层编译 / lint 缺陷

[Run 40195173888](https://github.com/puzige/vega/actions/runs/40195173888) 绑定本节 head，completed / failure。Clippy `121174360023`、Nextest `121174360238`、required `121175189693` 均 failure。fmt success，Python 原始日志 5 + 15 + 40 = 60 项 OK。生产代码已越过前一 E0616；Rust 测试仍在构建失败，发现/执行计数未取得。

父读取原始 job 日志并复核 pinned 源码，以下不是外部环境问题：

| 责任层 | 原始错误 / 位置 | 修复边界 |
|---|---|---|
| 测试观察 API | E0624，app `window/ordinary_tests.rs:248` 调 UI crate-private `TextInput::content_revision`（`text_input/state.rs:169`） | 保留同文新 revision 的不等式断言；使用窄的开发期观察接口，不删除断言或把内部生产 API 宽泛公开，最终随测试清理 |
| 生产死代码 | `window/session.rs:279` 的 `VegaWindow::materialize_draft` 已无调用者；app 全路径只剩该定义和 ordinary background 直接调用 conversation service | 删除已替代的同步 wrapper，不恢复 UI I/O 或添加 dead_code allow，不删仍在用的 conversation materialization 服务 |
| 生产类型布局 | `app_agent.rs:32` 的 `PendingAgentRun::UserMessage(UserSubmission)` 至少 512 字节，与下一 variant 24 字节不对称 | 在大载荷边界引入最小 indirection，保所有 route 与 drop/cancel/worker 所有权，不 suppress large_enum_variant |
| 生产条件结构 | `window/ordinary_send.rs:38–46` 嵌套 draft/不存在判断触发 collapsible_if | 保原错误传播、存在性和 materialize 条件，只合并等价 guard，不降低检查 |

该轮没有 unchanged rerun、删测、ignore、放宽 lint 或本地编译。当前单一 writer 正补已约定的真实入口回归；上述确切修复在它停写后一次交付，防止并发修改接口。独立 reviewer 只审固定 11c 隐私/分页/i18n/CUA 增量，现已完成并要求补修下节 P2；903 的窄封装修复和后续变化另行 delta 审查。

### 11c 固定增量独审：活动投影归属 P2，待修

审查源为 `11c9677dd28f03987d0c06da5651d1f5825b2b6b`（tree `6804a37ffb29393790140b54b1d995dd5dee6430`），对比 `79a5657833db162f171a0cd92b20ee5c6f585268`（tree `6bedce11982a0d8dcbc36cc189cc06e26f3c1450`）。非实现者只读 Git 对象，未审 903 封装修复及当前真实入口测试工作区。原隐私内容权威修复在源码层面成立；i18n/generated-notice、真实 commit→receipt→title 顺序和 CUA 29 上游路径保留无新增缺陷。静态 diff exit 0，双语 1,790 keys 对齐，26 新 ordinary keys 非空/placeholder 匹配；不是执行或 A1–A12 通过。

唯一 actionable P2：`ordinary_submissions.rs:290–294` 跳过非 ordinary、seq None 行；`content.rs:580–611` 却正以助手 ID 和 None 序号构建 leading RunActivity。完成的 ordinary U45 与助手 A46 有 duration 时，真实 hydration 先得 `U45 → Activity(A,None) → A46`，ordering 将 U 插到 A46 前而越过 Activity，变成 `Activity(A) → U45 → A46`。父于 2026-10-25T13:15Z 前后复读两固定对象及 StreamEntryIdentity，确认 message_id 已可识别助手归属，retired submission 的视图跟踪仍参与排序。此为产品排序缺陷，不是 harness 故障或接受范围变化。

契约补充同一 A11：按下一 durable 消息的最早所属投影确定边界，或等价保留投影所属消息的时间位置；不能把所有 None 归成 pending 后缀。真实 history replacement/pagination 回归须含 completed assistant + duration，保 text/images/activity/reply 顺序、唯一 key、copy/preview、列表数和非尾锚点，既有断言不删。当时唯一 writer 仍执行真实入口回归；它停写后才交此 P2 与前节四项编译/lint 修复。当时 fetch 后 master 仍 `76c164876542c679abe1bfb5f525f1ade2fcd2e4`，已是 HEAD 祖先，无新上游需覆盖当时脏代码。

### 中断恢复检查点与上游同步

继续时确认 writer `01M5SHDJRV0EW173Z5QVD06NNC` 的 execution `01M5SJ3JDT592F1YXK0SCC2QXR` 已 interrupted/released，无 unknown effects。此前 resume 返回 stale recovery boundary，不重试或重放。父复算七文件 SHA256 与中断前记录一致，diff --check exit 0；保留新增七项 public-entry 与一项 running Composer table 的源码、per-run 故障接线和临时配置 override，尚未取得完整 READY、编译或执行结果。已知四项 CI 缺陷及活动行 P2 仍待修，保存检查点不表示通过。

fetch 现见 master `921d735ca0dda3f17ab99ea4949002a1fdc44ab4`，含 compaction activity `86d340d2` 与 #617 按需委派规则。先保存现有工作再同步，不覆盖脏文件；采用新规则后由主会话直接完成紧密耦合修复，独立非实现者审查、无本地执行和最新 required CI 门禁不变。

### 接续修复与开发验证候选

现有回归保存为 `81329ed2`，上游无冲突合入 `0d4329815e3a41363ba220a53c0a32888d0ed69b`。主会话直接修复：OrdinaryLaunch 在 UserSubmission 内改最小 Box；仅删除无调用的同步 materialize wrapper；等价合并 draft guard；为 app 的四处 revision 断言增加 vega_ui 独占 development-observation feature / 窄 getter（最终随测试移除），未公开 TextInput::content_revision。活动排序对 sequence None 投影按同 message_id 的 durable sequence 判断，不把活动当普通 pending 后缀；新增 completed/retired mixed ordinary 的真实 history replacement + newer-page 回归，断言 text/images/activity/reply、唯一 key、原 copy/selection/preview、列表数和非尾锚点。

本轮实际静态检查：13 Rust 文件定向 Rust 1.98 rustfmt --check / skip_children=true、git diff --check、两个变更 manifest TOML 解析均 exit 0。原 57 测试块保留，仅 app 四处观察 API 等义替换；新增一项后 58 源码属性。编译、发现、执行计数仍未取得；本轮没有本地 Vega/构建/Clippy/测试/CUA。新增 public-entry 的 first-drive cancel/error 样例不等同于真实 app Cancel→buffered late-event generation 交错，其他非普通 route 的运行证据仍须按实际结果区分，不因此宣称完整 A1–A12。后续固定保存 SHA，非实现者审增量与故障接线，并交现有 Actions；旧 903 失败不被静态检查覆盖。

### 8bab7e6a：独审及首次集成执行

固定完整 HEAD `8bab7e6a386a6882dade8541795bd7eee6cc67c4`，tree `c1c25c4c844e5d5a34ca4b9a6ee77e71c5e0cd03`。非实现者对比 11c 完成只读独审：未发现新增产品/测试 API 缺陷；原活动 P2 源码修复成立，真实入口与 per-run fault/drain 事实边界可行。独审实际 22 变更 Rust 文件 rustfmt、diff、TOML exit0，不是运行通过或合并批准。保留 A7/A9 appCancel→buffered late-event/newgeneration 及 A10 非普通运行证据缺口。

[Actions 40199681162](https://github.com/puzige/vega/actions/runs/40199681162) completed/failure，Clippy `121187999583` success（含 fmt），Nextest `121187999454` failure，required `121189358266` failure。Python 5+15+40=60 OK。Rust 已实际发现58、执行58、54通过、4失败、0跳过。七 public-entry 回归全部通过，completed ordinary activity 排序新回归通过；不以部分通过称全卡 GREEN。

父读取原始失败 stdout/stderr 并核对锁定 gpui-kit0.6.1 test.rs 与当前/上游 render_rows.rs：三项 app 控件测试报 Registered paths none（composer-stop、pending-resume、composer-running-send），真实控件有 id 但未 test_support 注册；这是观察接线故障，补三个真实构造点（shared pending_button 同时覆盖 remove），不改其行为/可达性断言。第四项 history replacement 阳性对照把 visible_text 当原始全文，实际既有 user_message_lines 使用 trim_end_matches newline；修为同时 assert original_text 完整及展示正文遵循该既有规范，不改变生产裁剪、不删内容检查。target_changed 编辑被拒后重读再定点修正，未重放未知写入。全部58定义保留，三文件定向格式/diff待最终复核；无本地执行、不改通用CI、不重跑同字节求绿。

### 31207655 观察包装编译缺陷

固定 `312076558d13360193fcb85e66fef36a4d464663`（tree `a302c9688dfa40ee548bffd00c9addeef3259e9c`）新增三控件观察和双正文断言后，独审确认正文观察修正未弱化、58测试保留，但报告 shared pending_button 返回 Stateful<Div> 与 Observed 包装不匹配。父复读锁定 gpui-base0.6.1 observe.rs 的 feature-sensitive ObservedElement 别名及 gpui-kit base re-export，确认此 API 接线缺陷。

[Actions 40200944424](https://github.com/puzige/vega/actions/runs/40200944424) failure：Nextest `121191800988`、Clippy `121191801185`、required `121192508212` failure，Python60 OK，Rust未完成发现/执行。父读取两 job 原始日志：E0308 与独审一致；另两处 E0599 是 Observed 不支持应用 InteractionExt::interaction_surface（该 trait 只实现 Stateful<Div>）。不是业务缺陷或 runner 故障。最小修复：shared helper 返回已有 ObservedElement<Stateful<Div>>；Stop/running-send 在既有 interaction_surface 之后 test_support，再绑定原 focus。保留产品交互 wrapper、观察、真实焦点和全部断言，不扩通用交互 API。两文件定向1.98 rustfmt/check及diff exit0；测试未改，待新head Actions及独审增量。最终清理须还原该别名并去观察接线/导入。

### 5bfb4f73：57/58 与剩余 i18n 观察错误

完整 HEAD `5bfb4f7392f67ff0abb39ee369c374e19d38c008`，tree `3139834b3cd0ccc78c73a76ad3f1fa69e31a9648`。窄增量独审无新发现：别名 feature 兼容，Interaction 实现满足观察 wrapper，focus 传递给 inner，58测试未变；两文件格式/diff0。独审不代表运行通过。

[Actions 40201631700](https://github.com/puzige/vega/actions/runs/40201631700) failure，Clippy `121193848734` success（fmt含），Nextest `121193848868` failure，required `121194652443` failure；Python60 OK。Rust58发现/58执行/57通过/1失败/0跳过。三控件测试（Stop、Continue timeout、running Composer）均通过。唯一失败 UI ordinary_tests.rs648，actual Cancel / expected 取消；父复读同测试剩余硬编码、生产 ordinary_controls 和双语 catalog，确认遗留中文标签观察错误，而非取消行为或默认语言缺陷。将同一断言的 Cancel/Check status 两分支改为生产实际 catalog lookup，保 visible、分支语义与全部后续断言；无其他中文标签残留，58定义保留。没有本地执行、删测或同字节重试。

### e46ed9d5：开发集成 58/58 GREEN，仍有范围内证据缺口

完整 HEAD `e46ed9d5af087869fdf241da83c7b177cdd577fe`，tree `a25cd1e7659842cfb9820ad007e694a48c09fed9`。[Actions 40202426538](https://github.com/puzige/vega/actions/runs/40202426538) completed/success：Nextest `121196223393`、Clippy `121196224183`、required `121197158397` success；fmt成功，Python5+15+40=60 OK。父读取原始日志：58发现、58执行、58通过、0失败、0跳过。独立非实现者审该标签增量无新发现；此前固定代码增量独审结论仍按版本适用，不把 GREEN 宣称为完整 A1–A12 或用户验收。

可恢复开发源码 [tree](https://github.com/puzige/vega/tree/e46ed9d5af087869fdf241da83c7b177cdd577fe) / [patch](https://github.com/puzige/vega/commit/e46ed9d5af087869fdf241da83c7b177cdd577fe.patch)；父实际 GitHub commit API 读回同 SHA，public-entry 源文件 contents API blob `04f872a59865b3b8c42e817cbdd85d19c8b42b4a` 与本地固定树一致，UI ordinary_tests blob `3e704e5c2621d49abf241b3354f0822ceeb9b150`。不可只依赖分支名或临时 Actions artifact。

剩余代表验证不是新产品缺陷：A7/A9 缺实际 app Stop 后真实 buffered batch 对新 owner/另一 thread 的隔离；A10 缺实际 steering 接受与 continuation。非实现者复核当前调用链，现有 launch capture 提前返回，不可事后手造 active/Finished 充当真实 worker；据此补契约后由唯一 writer `01M5SR7YV2EB9420FSGPQ1C9P4`（execution `01M5SR7YV2NN5PKX62BXHSNEA9`）仅写 app worker/window 开发接缝与两项真实回归，主会话不并发改代码，只写证据/契约并协调。

A10 其他路由当前静态证据：submit_composer 先 automation intercept、Native pending enqueue、active guard、Codex 分流，再 ordinary；window/automation.rs 与 master921字节一致。ApprovedPlan/CredentialContinuation 在 app worker 保留独立分支及原 cancellation/credential 检查，不带 ordinary handle；conversation entry 对 ordinary 混用 approved/delegated/pending/automation 配置显式拒绝。#535 app record_event 保 predecessor_message_ids 与 predecessor terminal 过滤，UI RunContinued 按匹配 active 结束旧助手，无自然 continuation divider 回退。以上不是 Codex/automation/plan/credential 真实后端运行通过；后续 steering 代表也不能冒充全路由/原生 H 验收。开发测试与独占支持尚未清理，不合并、不安装、不关卡。

writer 首阶段只读返回 BLOCKED/停写，无代码差异：app 无直接 futures 引用，无法构造 Provider::EventStream；没有以假事件绕过。父复核四文件 hash 与 e46 一致、provider.rs516/1902 真 API，exec-guide§5 白名单已含 futures，workspace Cargo55 已锁 =0.3.34，conversation 已直接使用。不是新增包/产品依赖裁决：补契约只允许 app dev-dependencies 复用已批准锁定版本并最终移除，扩大允许写入仅 crates/vega/Cargo.toml 开发段，不允许锁文件包升级/手改或通用 CI。execution `01M5SRWJWXKMJQ06N2BEBAVSE7` 接续同范围，原58测试保持；当时新源码/结果未取得，不称已补完。

### 真实 worker 两项回归交付候选

同一 writer execution `01M5SRWJWXKMJQ06N2BEBAVSE7` READY/停写，五代码/manifest 路径新增窄开发支持及 ordinary_worker_tests.rs。父复算全部交付 SHA256 一致：新测试文件 `60e9102de6fc9678e37f0b33e40b685565b0a6a2dca4402da272972e3bb7ce60`；旧 app ordinary_tests `64a3be6bef3728c18e9097d96c11144556337505da9f21f420a2b8273e9432f3` 不变，Cargo.lock `d0db90f75da7963241f17c906ed0f5819a24b8bb00adcf1f16ed09e038c97ca0` 未改。父逐块确认原58测试保留；新增2后预计60源码定义，未作发现/执行结论。4 Rust 文件1.98定向格式/check、diff、manifest TOML检查exit0；无本地代码执行。

交付称两项经实际worker/poll：ordinary真实Composer→prepare→firstdrive→Stop→drain，然后真实新generation回放已记录production batch；回放明确为duplicate/late故障，不是自然FIFO。另项真实非ordinarylaunch→runningSend-now入队→runtimeSteeringAccepted/persist→continuation第二primaryrequest。Provider override在worker完成正常model/config/key/authority校验后，仅替transport，仍经OwnerCredentialProvider；title/prediction独立响应分类。父已检查实际diff，所有新接线cfg(test)，但真实验证仍待固定SHA独审及Actions，不能凭READY将证据缺口标闭合。失败清理、join/drain及辅助生命周期观察也列独审重点。最终须删新模块、worker_test_launches字段/初始化、test_provider参数/分支、pollgate/tap/join、TitleWatchDrain/PollDrain与dev futures引用，不删生产guard。
