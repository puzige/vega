# 历史编号清理规格

对应 Issue #338；本次明确授权优先于历史的保留测试要求。

## 范围与规则

- 扫描所有 tracked 非 `docs/` 文件及路径，清除 GitHub Issue 和旧编排任务来源编号（issueNN、iNN_、#NN、RNN、Sn/Tnn、A2-09 等），不是清除普通数字。
- 测试函数、模块或专用测试文件包含来源编号时直接删除；混合文件只删除受影响项，保留无关测试。删除专属 fixture 和孤立 helper/import/module 引用，不通过改名保留受影响测试。
- 生产代码仅语义化重命名符号、移除来源注释，行为和协议不变；GitHub 集成的运行时 Issue 编号/URL、用户数据、协议值和普通编号不属于历史来源。
- 根文档去掉具体历史编号；指向 `docs/` 内现有文档的链接目标保留，链接文字使用语义名称。这些路径是文档导航而非实现来源；不为清理来源破坏链接或改名历史文档。
- 带历史任务编号的非测试资源目录改为语义目录，并同步引用。

## 验收

先扫描后修改，记录精确删除数；复查非 docs 文本与文件名中的来源模式、测试 helper/import/module 依赖及生产差异。初轮按仓库默认限制只做快速静态检查；用户随后明确授权本任务运行本地 PR 检查，按下文记录串行执行格式、Clippy 与 Nextest，不修改仓库的全局验证政策。云端 required checks 仍是合并门禁；按用户后续授权提交并推送 PR。

## 实施静态记录

- 初轮静态清点：3145 → 1612，删除 1533 个带测试属性函数，删除 Rust 文件 47 个、fixture map 条目 122 个。补齐漏扫编号和遗留 helper 后的最终清点以 [删除测试清单](vega-clean-code-references-deletions.md) 为准。
- `cargo fmt --all -- --check`、`git diff --check` 通过；所有现存 tracked Rust 文件经临时 tree-sitter 静态解析无语法错误；13 个现存 tracked JSON 均可解析。
- 非 docs 实际文件路径不再包含 issueNN/RNN/Sn/Tnn 来源模式。保留的广义编号命中是文档导航目标、SVG 色值、风险文档章节 `risks #4`，以及普通业务/协议/fixture 值（例如 `S256`、`t1`）。
- 删除定义的静态调用交叉检查仅余已核对的 `settings` 模块导入、`Query` 上游类型导入、`is_text` 局部闭包。此检查不替代 Rust 名称解析/Clippy。
- 初轮实现未执行 Vega、编译、构建、打包或测试，交付时尚未提交、推送或开 PR。用户随后授权提交，已创建 PR #465；类型/unused 检查与行为回归交 Actions。

## 定向复核修正

- 从 HEAD 原样恢复 Unicode scalar 估算边界测试和原生贴底跟随 epsilon 测试；只移除其前方文件/章节来源说明，保留测试属性、函数体和断言。现有 import 已满足恢复项，无需改动。
- 只读 tree-sitter 复核删除清单：32 项仅在前置注释命中来源模式，其中 16 项位于来源编号专用文件；其余 16 项中的 2 项为本次恢复的文件/章节说明误判，14 项为直接描述单个测试的来源文档注释，未扩展恢复范围。
- 临时清理脚本仍存在前置注释归属边界和 `test` 子串匹配（例如 `latest`）缺陷；未修改或重跑破坏性清理。后续复用前须修正分类器并静态核对回归样例。

## 首轮云端检查与修复

- PR #465 首轮提交 `afb7b28c` 的 [Actions 原始日志](https://github.com/puzige/vega/actions/runs/39106075877) 显示：Nextest 在编译阶段因 Git fixture 的重新导出缺失而失败，尚未执行 Rust 测试；Clippy 因计算机操作测试辅助方法失去调用方而失败。
- 恢复保留测试需要的 Git fixture 重新导出及控制器别名；清理删除测试后失去使用方的私有 helper、导入与空测试模块，不降低 CI 门禁、不以 lint suppression 绕过错误、不改变保留测试的断言。
- 用户随后明确授权本次在本地运行 PR 检查，作为仓库默认本地限制的任务级例外。主会话串行运行格式、Clippy 与 Nextest，使用 `CARGO_BUILD_JOBS=2` 控制编译并发；子 agent 只做修改和静态检查。云端 required checks 仍是合并门禁，本地结果不替代对应提交的 Actions。
- 第一次低并发本地 Clippy（`--workspace --all-targets --locked --offline -- -D warnings`）复现 runtime 五项私有 fixture 方法的 dead-code 错误，退出码 101；此结果仅用于修复定位，不记为通过。
- 本地 Python PR 脚本测试已执行：`python3 scripts/release_tests.py`（5 项）、`python3 scripts/prepare_computer_use_tests.py`（15 项）、`python3 -m unittest discover -s scripts/tests -v`（14 项），均通过。仅清理本轮运行生成的 `__pycache__` 文件。
- 后续 Clippy 使用 `--keep-going` 一次收集各 crate 的独立诊断；缺失导入按编译器证据恢复，未使用项按实际消费者清理。一次 sccache 临时目录失效只影响本地工具环境，后续命令以 `TMPDIR=/tmp RUSTC_WRAPPER=` 绕过 wrapper，未修改机器或仓库配置。
- Store/MCP 定向 Nextest：215 项通过、0 跳过。首轮 workspace Nextest 已成功编译，执行 1608 项：1594 通过、14 失败，另有 3 项沿用既有跳过状态。14 项失败均为仍带 `iNN_` 历史编号的测试缺少已清理的 Git capture；补扫确认初轮漏掉共 35 个此类测试，按原授权删除编号测试及独占 helper，不恢复编号 fixture、不放宽断言，最终计数与复跑结果另记。
- 补齐漏扫后的本地完整 Nextest 已通过：`cargo nextest run --workspace --locked --offline --no-fail-fast --test-threads 2`，1573 项通过、0 失败、3 项沿用既有跳过，退出码 0（同步后续 master 前）。一次工具 120 秒等待超时导致中断的测试运行不计为通过；延长等待时限后的完整运行耗时 45.308 秒。
- 本地 workspace Clippy（`--all-targets --keep-going --locked --offline -- -D warnings`）已在本轮修复树上通过，退出码 0；同步 master 后仍须针对最终树复核。依赖 `block` 的 future-incompatibility 提示是 Cargo 提示，不是项目 Clippy 警告，未改依赖或关闭门禁。
