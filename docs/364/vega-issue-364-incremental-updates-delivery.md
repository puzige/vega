# 文件级增量更新交付记录

## Freeze

- 日期：2026-10-14（Asia/Shanghai）。
- 分支：`feat/364-incremental-updates`。
- 契约：[冻结规格](vega-issue-364-incremental-updates.md)。范围仅文件级增量与全量回退。
- 原始规格已先提交；实现由专职子 agent 完成，发布切片在独立 worktree 实现后集成。

## 实现

独立签名清单绑定全量归档摘要、完整目标文件及最多五个直接补丁；旧 v1 签名格式保持不变。客户端始终验证 v1，优先尝试增量，任何不适用或验证失败转为一次全量下载。下载进度使用当前资产真实字节数。

复用内容通过目录文件描述符逐层 `openat`、`O_NOFOLLOW` 打开，下载前验证且复制时再次计算摘要。补丁只能写入签名清单声明的普通文件；新建目标树自然删除旧文件。路径、大小、条目数、大小写、权限与 ZIP 内容均有边界校验。文件及目录权限显式设置，最终沿用应用身份与代码签名校验。

安装主进程及 helper 都在退出前独立重建验证；失败时仍可全量准备。helper 在退出后替换前再次验证和重建，不依赖可变缓存或 unsigned install.json 授权内容。保留原有父进程、inode/device、旧版本/哈希、备份、回滚与启动确认流程。准备临时副本用完即清理。

发布流程在最终完整 ZIP 签名后获取最多五个旧稳定版本，验证旧签名与归档，生成并重建验证直接补丁，独立签名增量清单。没有可用基线时仍发布空清单。当前包存在不支持的路径/权限时仅停用增量，不影响全量；损坏或缺身份的包拒绝发布。历史发布兼容检查和新 draft 完整性检查按发布版本能力区分。

## 自动验收矩阵

| 需求/风险 | 进程内覆盖 | 状态 |
|---|---|---|
| 正常增量、跳版本直达、新增/修改/删除/权限变化 | delta reconstruct / direct selection tests | 待云端 Rust CI |
| 无补丁及增量失败 → 全量一次，成功增量零全量请求 | production choose_download seam | 待云端 Rust CI |
| 复用文件变化、symlink 叶子/目录、路径越界、大小写/前缀冲突、异常权限和大小 | delta manifest/reconstruction tests | 待云端 Rust CI |
| ZIP 额外/重复/链接/错误权限及内容变化 | synthetic archive tests | 待云端 Rust CI |
| 清单边界、未知字段、无效签名 | delta parsing and verification tests；xtask signature domain test | 待云端 Rust CI |
| 退出前准备失败全量回退，下载失败丢弃准备状态，未完成不退出 | production prepare_installation seam | 待云端 Rust CI |
| 最终候选篡改、已验证源之后篡改 | tree verification / repeated reuse tests | 待云端 Rust CI |
| 安装替换失败、新程序失败恢复、确认超时 | 保留既有 helper 回归与生产检查；真实启动用户手测 | 待 CI / 用户手测 |
| 发布基线验证、候选上限、重建、大小比较、完整性/历史兼容 | Python 发布测试 | 本地轻量测试已运行；云端待运行 |
| 真实正式版升级及实际传输节省 | 正式发布后的升级、跳版本、失败回退手测 | 待合并后用户验收 |

## Results

本地只执行快速格式/静态检查与轻量 Python 测试，没有编译、构建、打包、安装或运行 Vega。

| 命令 | 结果 |
|---|---|
| `cargo fmt --all` | exit 0 |
| `git diff --check` | exit 0 |
| `python3 -m unittest discover -s scripts/tests -p 'test_delta_update.py'` | exit 0；14 tests，0.022s；OK |
| `python3 scripts/release_tests.py` | exit 0；5 tests，0.018s；OK |
| `cargo clippy --workspace --all-targets -- -D warnings` | NOT RUN 本地；待 PR CI |
| `cargo nextest run --workspace` | NOT RUN 本地；待 PR CI |

## Residuals

- 当前本地结果不证明 Rust 编译或真实 macOS 升级成功，必须通过云端 PR gate，合并后由用户验证真实升级。
- 旧客户端先升级至支持增量的版本，之后才优先下载补丁；超出最近五个基线时全量回退。
- 实际下载节省取决于变更文件与运行版本；未承诺固定大小。
- 未改全量包构建优化、二进制差分、组件生命周期或用户数据。
- 实现与规格无有意偏离。CI 和最终集成结果由主 agent 补充。

## 云端首次检查与修复

PR #377 的首次云端 Nextest job（run 38580288159）在编译阶段以 exit 101 失败，尚未执行 Rust 测试。诊断为 `E0502`：安装准备持有更新状态中的版本字符串借用，同时全量回退回调需要更新下载进度并克隆状态。修复为进入准备流程时持有独立的版本 `String`，两个回调仅借用该局部版本，解除状态字段借用冲突。另将 `now()` 移至测试模块前以满足 Clippy 模块顺序约束。

修复后仅执行本地 `cargo fmt --all -- --check` 和 `git diff --check`；Rust 编译、Clippy 与 Nextest 需重新由 PR CI 验证，不将失败运行标记通过。

## 云端首次通过与基线同步

[PR CI 38581650473](https://github.com/puzige/vega/actions/runs/38581650473) 已通过格式、Clippy、Nextest 与发布脚本测试。Nextest 原始摘要：`Summary [ 212.098s] 2613 tests run: 2613 passed, 5 skipped`；Python 两组分别 `Ran 5 tests in 0.036s`、`Ran 14 tests in 0.045s`，均 `OK`。本卡增量重建、路径/权限/签名/篡改、下载回退和退出前准备测试均 PASS。真实升级仍待用户验收。

等待期间 master 合入独立的上下文预算修复，无本卡文件交叉；分支保护要求同步最新 master 后重跑门禁。最终同步后的 CI 和合并身份以 [PR #377](https://github.com/puzige/vega/pull/377) 的验证记录为准，不沿用旧基线结果冒充最终集成验证。
