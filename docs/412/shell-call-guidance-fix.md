# Shell 调用说明与拒绝提示修复

## 范围与根因

用户手测截图出现 `shell session: workspace_sandbox_required`，要求检查给模型的参数说明与错误提示并修正误用点。本轮属于 #412 手测反馈；不改变冻结 v1.1 的权限、准入、执行与恢复规则。

已确认：只有非 null 的 `bash.wait_ms` 才进入会话式准入；`0` 同样启用异步。原工具总说明末尾已有 Full Access/外部写根限制，但 `wait_ms`、`timeout` 和外部写根字段没有就近说明，拒绝将不同原因合为一个内部码，没有说明未启动或如何恢复。截图不含原始工具参数及权限策略，无法证明模型当时的意图或具体命中条件，不访问其他任务运行数据推断。

## 验收用例（先于实现）

| 场景 | 可观察结果 | 验证 |
|---|---|---|
| 模型读取 bash 定义 | 默认阻塞；timeout 是总期限秒数；wait_ms 非 null（含 0）是异步选择，不是普通超时或性能调参；阻塞例子明确 null | 定义及参数 schema 静态核对 |
| Full Access 异步请求 | 启动前拒绝，指出当前策略不支持异步；保留错误码，明确未启动，可保持原权限并将 wait_ms 省略/null 后显式阻塞重提 | 实际解析及准入逻辑开发回归 |
| ReadOnly 异步请求 | 拒绝，指出只读策略不符合工作区写入沙箱；不建议自动升级权限 | 同上 |
| 外部可写根异步请求 | 拒绝，指出外部写根不支持异步；不建议删除必要授权或静默降级 | 同上 |
| 工作区沙箱，无外部写根；仅网络 scope | 保留原准入资格，不将网络 scope 误拒为外部写根 | 同上 |
| wait_ms 缺省/null/0 | 前两者仍为阻塞选择，0 仍是显式会话选择；不更改协议或自动重试 | 真实参数解析开发回归 |

## 最小实现计划

1. 将阻塞默认/异步资格前置到工具说明，并为 wait_ms、timeout、write_roots 添加就近说明，区分 bash_wait 的观察窗口。
2. 拆分同一拒绝码背后的策略/外部写根原因，附仅适用于启动前拒绝的恢复建议；不改授权、spawn、busy/blocked 规则。
3. 本地任务相关开发回归、严格 Clippy/fmt；固定源码及结果后清理 Rust 测试，非写入者审查，最终 head required CI。真实模型选参效果仍需用户手测，说明修正不能保证模型不再误用。

## 开发验证记录

- 基线 `f3688f5ae0b7becf1f46434c7b7d732e3a3c759a`。官方 Cargo `1.98.0 (797e8a9bc 2026-08-05)` / rustc `1.98.0 (88d9e12ae 2026-08-18)`；并发 `-j 2`，不安装/升级工具链。
- `rustup run 1.98.0 cargo test --locked -j 2 -p vega_runtime guidance_tests`：3 执行/3 通过/0 失败/0 忽略。真实参数解析与 `ShellRun::validate_launch`，不启动 Shell 或 Vega；fixture 的持久化 hook 一旦被调用即失败。
- `rustup run 1.98.0 cargo clippy --locked -j 2 -p vega_runtime --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` 均成功。工具定义字段 description 的包装链静态核对保留。
- 本轮验证命令首次误带 wait_ms，在启动前命中同一旧错误；明确去掉该参数、保留权限后阻塞执行。该拒绝不记为测试失败或执行成功，也未自动重放。
- 开发源码/独审/最终无测试 head CI 待后续固定；不以纯文本修改或这些回归宣称真实模型选参已验证。
