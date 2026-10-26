# Issue #188：并行运行 Clippy 与 nextest

## 用户要求
在 PR CI 中让截图中的 Clippy 和 Test the workspace 并行执行。

## 当前行为与目标
当前 `.github/workflows/pr-check.yml` 是一个 job，顺序运行 fmt、Clippy、nextest。本卡把 Clippy 与 nextest 放到各自独立的 macOS job，使 runner 同时执行；fmt 放进 Clippy job。

结构：

```text
Clippy job: fmt → clippy ─┐
                          ├→ check (fmt, clippy, test) 汇总 required check
Nextest job: nextest ─────┘
```

保留 GitHub required check context `check (fmt, clippy, test)`，作为依赖两个执行 job 的轻量汇总 job。汇总 job 必须在依赖 job失败或取消时运行并明确失败，不能因 `needs` 的默认跳过语义把失败隐藏为成功。两个执行 job 无相互依赖，分别在 GitHub macOS runner 上执行。

各测试命令、工具链与缓存契约不改：

- fmt：`cargo fmt --all -- --check`。
- Clippy：`cargo clippy --workspace --all-targets -- -D warnings`。
- nextest：固定 `cargo-nextest 0.9.146`，运行 `cargo nextest run --workspace`，默认并发和 `.config/nextest.toml` profile。
- 各执行 job 恢复同一 `vega-master-build` cache；PR 的 `save-if` 仍为 false，master CICD 仍为唯一写入者。
- 不加分片、额外构建、cargo test/doc、重试或归档。

独立 runner 意味着 Clippy 与 nextest 会分别恢复缓存并各自按需编译；并行能缩短墙钟时间，但增加一个 macOS job 和重复的 runner/cache 准备成本。

## 验收矩阵

| 风险 | 验收 |
|---|---|
| 两个执行 job 仍串行 | Clippy/nextest job 同时可运行，二者没有 needs 依赖 |
| required check 名改变 | 汇总 job context 仍为 `check (fmt, clippy, test)` |
| 依赖失败后汇总 job被跳过并误绿 | 汇总 job `if: always()`，读取并检查两项结果，任一非 success 返回失败 |
| 缓存语义改变 | 两执行 job同 key恢复；PR均不写缓存；master CICD未变 |
| 测试命令/并发配置漂移 | 固定 nextest 版本与原命令/profile 保持不变 |
| 结果覆盖减少 | PR云端 Clippy 与完整 nextest 都通过；nextest汇总数与基线1787 passed / 5 skipped 一致 |

## 计划与风险

实现限定为 PR workflow、相关 CI 规格与当前规范。先由专用实现 agent 修改，再由主 agent 审查 DAG、汇总失败传播和缓存配置。先做 YAML/结构静态检查，再推 PR 由 required check 验证真实 job 并行与命令结果。无产品测试/数据迁移；回滚通过 revert 本卡变更。

与原先单 job 相比，多启动一个 macOS runner、恢复一次缓存，并在两 runner 上分别按需编译。是否缩短整体墙钟时间以云端运行实测为准。
