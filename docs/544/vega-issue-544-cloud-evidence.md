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
