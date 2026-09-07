# R0a · 受信 Git 来源与能力诊断

版本 v0.2 · 2026-09-05 · Owner Codex · Executor 原生 Luna / max

## 目标与裁决

用户 goal 明确要求恢复 R0 Git 能力与诊断，继续 Phase 1 工程收口。系统 Git 2.39.5 不支持目标树属性查询，12 项真实分支验收因此失败。本卡修订 S6 C2 中固定 `/usr/bin/git` 的 executable 来源；其余 exact argv、环境清洗、属性/路径/操作守卫、lease、owner cleanup、进程预算全部保留。

Git 2.40 文档已明确支持 `check-attr --source`：https://git-scm.com/docs/git-check-attr/2.40.0 。Homebrew 是官方列出的 macOS 安装途径：https://git-scm.com/install/mac 。本卡不捆绑或复制 Codex 自带 Git，不使用 PATH 搜索，不更改系统/Xcode Git，不增加 Rust 依赖或 DDL。

## 冻结行为

1. production 只允许确定的 executable 候选。macOS arm64：`/opt/homebrew/opt/git/bin/git`，然后 `/usr/bin/git`；macOS x86_64：`/usr/local/opt/git/bin/git`，然后 `/usr/bin/git`；其他既有 Unix 路径保持 `/usr/bin/git`。禁止任意 config/env、模型输入、仓库路径或当前目录指定 executable。
2. Homebrew 候选须 canonicalize 到对应 prefix 下 `Cellar/git/<单层版本>/bin/git` 的普通可执行文件；拒绝逃逸、非法结构、group/world 可写文件。系统候选只接受固定系统文件。记录文件 identity（dev/inode/size/mtime，必要时纳秒），成功选定后在进程内固定 canonical executable，所有 read、switch、stage、commit、summary 使用同一来源，不能在一个操作中或后续失败后静默切换。
3. 首次解析在既有后台 worker 中进行，不在 render / UI critical path 做 IO。使用环境清洗、无 shell、固定 `--version` 参数、进程组、取消与已有 bounded collector 验证；版本需可严格解析且至少 2.40。版本是准入诊断，实际目标树 `check-attr --source=<captured_oid> -z --stdin --all` 仍是每次分支 preflight 和 execute 前的真实能力/属性检查，不能用版本判断替代它。
4. 不缓存失败为永久全局失败，使新操作能在安装依赖后重试；成功来源可缓存。每次 spawn 前复核固定 executable identity；变化/删除则 typed fail-closed，要求重启重新选择，绝不把候选 fallback 混入旧 permit 的执行链。same-user 在 verify 与 spawn 之间修改文件/动态库，以及系统shim背后的OS管理工具链变化，仍是残余，不声称检查top-level文件identity可原子隔离所有依赖或提供OS签名信任。Git子进程环境在既有GIT_*清洗之外去除DEVELOPER_DIR、TOOLCHAINS与DYLD_*/LD_* loader overrides；不修改用户全局环境或PATH。
5. 共享错误码仅通过 `vega_conversation::types` 增加必要的 GitUnavailable / GitUnsupported / GitExecutableChanged（名称可保持一致风格）。缺失和旧版本必须在 UI 中给出可操作短诊断，例如“需要 Git 2.40+，请安装 Homebrew Git 后重试”；文件变化提示重启。不泄漏原始路径、stderr、repo内容。保留其他现有错误语义，不把任意 GitFailed/TimedOut 统统改成依赖错误。
6. 现有的 test-only fault/delegating seams 继续保留。只允许将真正 delegating 脚本的 `/usr/bin/git` 目标更新为与 production resolver 一致的受信 executable（正确 shell quoting，不能由测试选择成功分支）；owned fixture 初始化可继续使用系统 Git。不得为测试扩大 public API；必要 accessor 为 private/cfg(test)。原有安全断言和测试不可删除/放宽；新增错误码闭合表可按本规格追加。
7. 打包/安装说明明确需要上述受信来源的 Git 2.40+；应用不自动安装、不在启动时请求网络，不宣称任意 Mac 无依赖。主 Agent 可在本机用现有 Homebrew 安装 Git 作为验收依赖；禁止读取 Keychain、provider请求、用户仓库mutation或远端写入。

## 文件归属

Executor：git_workspace runner / 新私有 executable 模块、必要的 service 接线和相关测试；types/artifact.rs 中错误码及闭合映射；branch_selector 与其他直接消费新增错误码的 UI 文案；xtask package 安装说明；S6 来源条款和 packaging 文档；本卡交付文档 `docs/vega-r0-trusted-git-delivery.md`。

主 Agent：本 SDD、独立验收、集成和外部进度台账。不得触碰 R1/R4 行为、thinking、@file、性能口径、Cargo.lock 或不相关用户 worktree。生产实现必须由 Luna/max 完成。

## E2E-first 验收

- 实际 production resolver + owned temp repo：原12项branch相关验收通过，尤其 s6_acceptance 的真实分支切换；同时复核 current branch、工作树、filter recorder zero-spawn、ignored collision、exact authority/lease cleanup，不能只看 exit0。
- 最少必要测试：固定来源解析/版本语法；缺失/不支持 typed 诊断；executable identity 变化 fail-closed、zero mutation；原 fault-injection tests 不退化。path/env 不能劫持 production resolver。
- 原有全量运行并保留 raw log：fmt / clippy --all-targets -D warnings / test --workspace --locked --no-fail-fast / build --workspace --locked。R1四项与UI115继续作为联合回归。
- 先保留本卡首次失败；共享 target 只给本卡使用，切换worktree要注意源码cache复用，主 Agent 验证编译来源。
- 若剩余 Artifact/F3 时序失败，完整报告具体用例/差异；允许本地review提交，不能宣称总门禁通过。R0b另卡分诊，禁止在本卡放宽timeout/断言或隐藏失败。

## 交付

工作树基于联合 R1/R4 d01ae6c；branch codex/vega-r0-trusted-git。每卡≤3本地commit，提交使用分支相关feature ID；禁止push/PR/master merge/release。报告包含源码hash、命令、结果、日志摘要hash、首次失败、剩余风险。raw日志放任务专用 `/private/tmp`；仓库文档使用脱敏路径与摘要。

## 变更记录

- v0.1：主 Agent 冻结有限来源、版本准入、identity与真实目标树校验。
- v0.2：本机证据确认系统Git为工具链shim，明确其剩余信任边界，并补齐Git子进程工具链/loader环境清洗。
