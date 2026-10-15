# 发布准备缓存隔离交付记录

## Freeze

- verified_at_utc: 2026-10-15T03:11:13Z
- verified_at_local: 2026-10-15T11:11:13+08:00（Asia/Shanghai）
- branch: `feat/release-preparation-cache`
- git_head: 实现前与 origin/master 同步；最终实现 commit、PR 与 CI 由主 agent 集成后补充。
- tracked_diff_sha256: `f4b006394f0b736c55b9c286cee9b2e3a433ca0cd697ed71406decf61ed05235`（验证时七个已跟踪变更文件，不含未跟踪规格与本报告）
- task_contract: [冻结规格](vega-release-preparation-cache.md)
- 无本地编译、构建、打包、cargo test/check/clippy；未安装或替换用户应用。

## 实现

Python CLI 默认输出和下载缓存分别改为 `.vega-build/computer-use/Contents` 与
`.vega-build/computer-use/downloads`，xtask 显式传入的输出同步更新。
`.vega-build/` 加入 Git ignore；发版指南和 Computer-use README 同步目录说明。
PR 既有 Nextest job 加入准备脚本测试，不新增 job、不改 Cargo 缓存键。

归属校验、归档提取、哈希验证、签名策略和自定义参数逻辑没有修改。
旧 target 目录保持原样；新位置的未知输出仍拒绝覆盖。
新增测试通过生产 `main → prepare` 路径执行，替身仅替换平台信息、下载、解压和 npm
外部边界，保留真实参数解析、固定清单/lock 校验、输出归属检查、文件组装、包身份验证、
manifest 生成与最终目录替换。它们不是实际网络/系统打包验收。

## Results

| requirement | evidence class | exact command | result | duration |
|---|---|---|---|---|
| 原缺陷先红 | 进程内生产入口，外部边界替身 | `python3 scripts/prepare_computer_use_tests.py ComputerUsePackagingTests.test_default_preparation_ignores_restored_cargo_directories_and_repeats -v` | 旧实现：1 项 ERROR，unowned 输出目录 | 0.003s |
| 新默认目录、旧缓存不动、重复准备、自定义参数、安全拒绝与既有校验 | 进程内生产入口与既有单元测试 | `python3 scripts/prepare_computer_use_tests.py -v` | 15 passed | 0.031s |
| Rust 格式 | 静态格式检查 | `cargo fmt --all -- --check` | exit 0，无输出 | 3.25s |
| 差异空白 | 静态检查 | `git diff --check` | exit 0，无输出 | <1s |
| xtask/Python 目录一致 | 差异审查 | 阅读两处默认/显式路径及打包复制调用 | 两者均使用 `.vega-build/computer-use/Contents` | 不适用 |
| 完整编译与全量测试 | 云端 PR Check | fmt / Clippy / Nextest | 待 CI | 未运行 |
| 真实缓存恢复与正式包发布 | 云端 master 发布 | 自动 release workflow | 待合并后验证 | 未运行 |

首次失败 footer：

```text
ValueError: refusing to replace an unowned executor output directory
Ran 1 test in 0.003s
FAILED (errors=1)
```

修复后 footer：

```text
Ran 15 tests in 0.031s
OK
```

原始日志保留在临时目录，文件名 `vega-release-preparation-red.log` 和
`vega-release-preparation-green.log`。SHA-256 分别为
`48a3d8df923dee76bd5ddd5f14e649eb0f8210ab07c486168de9793c02e6d8f6`、
`f9b16f0e241c2be8ffb021bf2d2e4f4eaa3c1d54fae74b9feae0a882b44cbd29`。
首次失败发生在新增回归后、修改生产默认路径前，随后替身定位方式由模块注册调整为
`mock.patch.object`，未修改场景和断言。

## Residuals

- 与规格偏离：无。
- NOT RUN：本地 Rust 编译、测试和实际打包；云端 PR Check 与发布结果待集成补充。
- LIMIT：新目录不进入现有 Cargo 缓存，每个干净云端 runner 会重新下载固定执行器；不引入独立下载缓存。
- ACCEPTED：历史 target 残留不迁移、不认领、不删除；显式自定义为旧不受信目录仍会拒绝。
- 用户手测前不关闭 Issue、不清理任务 worktree；失败版本既有 tag/draft 不覆盖或删除。
