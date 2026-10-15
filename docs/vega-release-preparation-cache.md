# 发布准备目录与 Cargo 缓存隔离

2026-10-15 用户授权修复 master 发布失败。失败证据：https://github.com/puzige/vega/actions/runs/38673445138 。

## 问题与边界

xtask 已完成编译，在准备 Computer-use 执行器时因 `refusing to replace an unowned executor output directory` 退出。准备产物在 `target/computer-use/Contents`；rust-cache 的 cleanTargetDir 递归清除非 Cargo 文件，保留目录，归属标记随文件一起被清除。恢复后的目录触发既有安全校验。该故障不是增量更新协议错误。

准备产物与下载缓存改为工作区 `.vega-build/computer-use/Contents` 和 `.vega-build/computer-use/downloads`，加入 Git ignore；Python CLI 默认值与 xtask 调用必须一致，避免两套路径漂移。它们不得位于 Cargo target 或会在组装应用时清空的 dist 内。保留自定义 --output/--cache 能力和所有现有归属、哈希、目录内容、安全校验。

旧 target/computer-use 即使残留无标记目录也不得被使用、删除、补写标记或自动认领；新位置未知目录仍拒绝覆盖。现有下载验证与已归属目录可重复准备行为保持。不修改应用内更新协议、执行器版本、签名策略、Cargo 缓存主键或用户应用。无需新依赖。

## 验收矩阵（实现前冻结）

| 场景 | 操作与预期 | 验证 |
|---|---|---|
| 干净工作区 | 默认准备路径在独立构建目录，xtask 使用同一约定 | Python CLI 参数/调用回归，必要 Rust 单元测试 |
| 恢复旧缓存 | 构造旧 Contents 无标记目录，执行默认准备的进程内替身流程；新目录成功，旧目录及文件保持原样 | 快速 Python 回归，先记录旧实现失败 |
| 重复准备 | 新目录有合法归属标记，再次准备成功 | 快速 Python 测试，网络/进程使用替身 |
| 非归属目录 | 新输出目录无标记、错误标记、链接或多余内容时仍拒绝覆盖，不改原数据 | 保留并补充安全回归 |
| 发布集成 | PR 的 Python 准备测试加入既有 Nextest job；fmt、Clippy、Nextest 通过；合并后 master 从旧 Cargo 缓存恢复仍可准备并完成发布 | 云端 PR Check 与 master 发布 |

## 实施顺序与交付

主 agent 编写规格并评审；专职子 agent 在独立任务 worktree 修改 scripts/prepare-computer-use.py、scripts/prepare_computer_use_tests.py、xtask/src/package.rs、.gitignore、必要的 pr-check Python 测试命令、发版文档与 packaging/computer-use/README.md 默认路径说明，交付 docs/vega-release-preparation-cache-delivery.md。先补回归证明旧代码失败，再实施最小修改，保留失败与通过证据。禁止本地编译、构建和打包，只跑轻量 Python/格式/静态检查；云端门禁通过后 squash 合并，核对自动发布结果。用户手测前 Issue 保持 In review，不清理 worktree。

回滚可撤销本任务 PR，但会恢复缓存目录缺陷；不得通过取消归属保护恢复发布。失败版本的既有 tag/draft 不覆盖、不手工删除，新提交走既有自动版本分配。
