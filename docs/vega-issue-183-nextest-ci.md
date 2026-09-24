## 用户要求
CI 测试只使用 cargo-nextest，保持简单单 job，无分片。

## 范围与验收
PR workflow 安装固定 cargo-nextest 0.9.146，顺序 fmt、clippy、cargo nextest run --workspace。沿用 .config/nextest.toml 的 retries=0、fail-fast=false、600s 挂起保护，默认测试并发；不加分片、归档、自定义调度或额外 build。保留 required check 名及 master/PR 共享缓存。

nextest 不支持 doctests，按用户只使用 nextest 的要求不添加 cargo test --doc；说明此覆盖边界。生产代码、测试断言及5项既有ignore不变。

更新当前规范及规格，旧执行结果保留为历史。验证工作流结构与固定版本安装方式，完整测试交云端required check。通过后squash merge，进入In review等用户验收。

## 验收矩阵与实施计划

| 用例 | 预期 | 证据 |
|---|---|---|
| workflow结构 | 单check，fmt→clippy→nextest，无cargo test/分片 | 静态审查 |
| 安装与配置 | 固定0.9.146，读取仓库默认profile，零重试 | 静态审查及云端日志 |
| 测试回归 | 所有未ignore单元/集成测试通过 | 当前PR head required check |
| 兼容性 | check名称、缓存、master打包不变 | diff审查 |

先建立独立worktree并更新基线，专用agent修改工作流及当前规范，主agent审查并提交PR。无需新增产品测试；本地仅结构检查，云端执行完整门禁。回滚通过revert本卡commit恢复cargo test。数据迁移和产品状态变化不适用。

## 本地结构验证

- `cargo nextest --version`：`cargo-nextest 0.9.146`，退出0。
- Ruby YAML解析及结构断言：单PR check、固定安装版本、fmt/clippy/nextest命令顺序、默认profile均正确；去除安装步骤并恢复旧测试命令后与基线工作流结构相同，缓存及其他字段未变。退出0。
- `git diff --check`：退出0，无输出。
- 未运行本地全量构建/测试；云端结果以PR和Issue交付记录为准。
