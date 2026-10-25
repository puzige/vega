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
