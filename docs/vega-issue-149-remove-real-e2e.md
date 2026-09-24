# Issue #149：优先 Mock 外部依赖，移除真实 E2E 执行

## 2026-09-25 用户裁决

用户先要求「移除所有的真实的 e2e 测试，耗时太高了」，随后澄清「能尽量把它 mock 掉就 mock 掉」。以后一条作为实施优先级：保留有价值的测试并替换外部依赖，只有专门验证真实系统契约且 mock 后失去意义的测试才删除。本修订取代 #140、#149 历史规格和执行规则中要求保留真实适配层 E2E 的限制；旧报告继续作为历史证据。目标是减少测试耗时与维护复杂度，不再逐批建设真实外部执行契约测试。

## 范围

- 对实际启动 Git、Shell、PTY、MCP stdio 等子进程的测试，优先使用进程内执行替身，保留业务分支、安全断言、调用参数及失败行为；脚本子进程故障注入也改为进程内响应。仅删除专门证明真实 OS/进程契约、无法通过 mock 有意义验证的测试。
- 网络、模型和 UI 外部边界同样优先 mock，保留协议解析、请求构造、业务状态和错误处理覆盖；真实监听/连接/原生应用驱动的适配契约由手测覆盖。
- 清理仅供被删测试使用的 helper、fixture、测试二进制、dev dependency 和调度配置。
- 保留进程内纯函数、业务、安全策略、解析、存储及 GPUI TestAppContext 测试。临时文件、SQLite 或名称含 e2e 本身不是删除条件；现有进程内命令替身测试继续保留。
- 产品行为不变；允许在 cfg(test)/test-support feature（仅由 dev-dependencies 启用） 下暴露已有执行边界以供跨 crate 测试注入，不扩大正常生产公共 API。不新增通用模拟框架，不用 ignore、feature gate 或 CI 排除表达式伪装删除。
- 真实 Git/终端/网络/UI 的系统集成行为改由用户手测；移除后不再声称自动测试证明这些外部契约。
- CI 仍是单 job 顺序 fmt、clippy、cargo test；共享 master 缓存与 cicd.yml/name: master 不变。

## 测试依赖许可

允许将 Cargo.lock 已锁定的 `http` crate 作为受影响 crate 的 dev-only 直接依赖；跨 crate 测试响应构造需要时可设为仅 test-support 启用的 optional 依赖，用于构造可转换为 reqwest::Response 的内存响应。此许可仅支持保留原 HTTP 状态、headers、SSE 和重试处理测试，不改变默认生产依赖图、不引入新版本；由本卡协调者按依赖白名单审批规则记录。

已在白名单中的 `tempfile` 可作为仅 test-support 启用的 optional 依赖，以复用跨 crate 的有限命令夹具；默认生产依赖图不变。

## 验收矩阵

| ID | 需求/风险 | 操作 | 预期 | 证据 |
|---|---|---|---|---|
| R1 | 外部执行测试残留 | 按调用链盘点测试及共享 helper | 自动测试不再依赖真实外部执行，逐项记录保留断言和 mock 边界或删除理由 | 实现清单及 diff |
| R2 | 误删进程内断言 | 对照保留测试与删除清单 | 原有业务/安全断言尽量迁移保留，已有进程内测试保留 | 审查及定向测试 |
| R3 | 遗留入口与死代码 | 审查模块声明、manifest、fixture、nextest 配置 | 无孤立测试入口，无新 ignore 或过滤 | diff、云端 clippy |
| R4 | 构建与回归 | PR 云端 fmt/clippy/workspace test | 同一 PR head 的 required check 通过 | GitHub Actions |
| R5 | 规则回退 | 更新 AGENTS、exec-guide、delivery skill、README | 后续实现不再强制增加真实 E2E | 文档审查 |

数据库迁移、运行时持久化变更和产品状态切换不适用：本卡调整测试、必要的测试专用注入边界与规范。

## 实现计划

1. 从最新 master 建立独立 worktree，继续使用 #149 跟踪。
2. 专用实现 agent 先盘点直接和间接外部执行，按模块将外部边界 mock 化，仅删除纯真实适配契约测试及专用辅助代码；主 agent 更新规格和规则并审查边界。
3. 本地只运行受影响的定向测试；完整 fmt/clippy/workspace test 交给云端。记录真实命令与结果，不估算或承诺提速数字。
4. 开 PR，required check 通过后 squash merge；#149 进入 In review 等用户手测，不提前关闭。

回滚通过 revert 本卡 squash commit 恢复测试及规则，不影响数据兼容性。

## 实现清单与结果

### 已完成的定向验证

Git snapshot 原始定向执行完成 23 项，测试阶段 12.80s；该捕获命令的后续阶段被主动中断，整体退出码 130，不能记为整条命令成功。其中一项仅验证真实 Git 夹具环境清理，本卡移除。

迁移后命令：`cargo test -p vega_conversation --lib git_workspace::tests::snapshot:: -- --test-threads=1`，退出码 0。

```text
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 420 filtered out; finished in 0.16s
```

17 个原真实仓库场景改为有限的命令响应夹具，执行生产 service/解析/版本与权限逻辑；原业务断言保留。严格检查 argv、stdin 和消费序列，未知命令不能回退到真实进程。其余保留测试为进程内解析/边界测试。以上数字仅代表 snapshot 子集，不代表整个流水线提速。


### 最终实现与本地证据

- Git：快照、版本/ABA、分支、提交证明、SHA1/SHA256、特殊路径、gitlink、竞态等业务断言保留。命令响应夹具严格匹配参数、stdin、环境及文件状态，不支持未知命令回退。
- 网络/Provider/OAuth/UI 93 项及 Artifact 11 项原测试迁移，共 104 项，业务测试函数零删除。HTTP 使用内存响应，MCP stdio/OAuth callback 使用双向内存管道；业务解析、协议大小上限、权限和恢复逻辑继续执行。
- 控制器的 151 项定向测试通过；侧栏拖拽使用临时配置路径保留持久化断言；终端布局和复制内容迁移到固定 UI 状态。
- Shell 的去重、取消落库、FullAccess 审计等使用执行 callback，保留原业务断言。硬链接预检、作用域和预取消仍验证真实进程启动前拒绝。
- 52 个纯外部契约、夹具自测或已有 mock 等价覆盖的测试函数删除；完整处置清单见 [迁移清单](vega-issue-149-mock-test-inventory.md)。没有新增 ignore、CI 过滤或分片；5 个旧计时 ignore 改为虚拟时钟后激活。全仓另有 5 项未改动的既有忽略（conversation 单元 2、conversation restart integration 2、runtime 1），均不作为通过证据。
- 测试模式遗漏 Git/HTTP/Bash mock 会明确失败；默认生产配置继续使用原生执行路径。nextest 仅保留零重试、失败报告和挂起保护。

| 定向命令/范围 | 结果 | 测试阶段耗时 |
|---|---|---|
| `cargo test -p vega_conversation --lib git_workspace:: -- --test-threads=1`（最终改用虚拟时间） | 158 通过，0 忽略；相同范围在禁止 process-exec 的沙箱再次通过 | 沙箱 1.50s |
| `cargo test -p vega_conversation --lib artifact::tests:: -- --test-threads=1` | 13 通过；禁止 process-exec 再次通过 | 1.02s / 沙箱 1.16s |
| `cargo test -p vega_conversation --lib project_branch::tests:: -- --test-threads=1` | 3 通过；禁止 process-exec 再次通过 | 0.09s |
| `cargo test -p vega_conversation --test s6_acceptance -- --test-threads=1` | 2 通过 | 0.27s |
| `cargo test -p vega_runtime openai::tests -- --test-threads=1 --nocapture` | 39 通过 | 0.02s |
| `cargo test -p vega_conversation provider_settings::tests -- --test-threads=1 --nocapture` | 16 通过 | 0.48s |
| `cargo test -p vega_ui settings:: -- --test-threads=1 --nocapture` | 47 通过 | 2.13s |
| `cargo test -p vega tests::automatic_titles:: -- --test-threads=1 --nocapture` | 5 通过 | 0.12s |
| MCP / MCP settings / agent MCP / images / credential 定向模块 | 54 / 23 / 14 / 4 / 1 通过 | 0.54 / 0.83 / 0.26 / 0.25 / 0.03s |
| 剩余 UI / navigation / S5 定向模块 | 42 / 1 / 2 通过 | navigation 4.47s，S5 0.11s |
| `cargo check -p vega_conversation -p vega_runtime -p vega_mcp --lib` | 默认非测试配置通过，无警告 | 编译 9.00s |

以上范围存在包含关系，不将各行相加宣称迁移数量或完整测试总数。本地没有执行 workspace 全量测试。

关键原始输出（有界 footer）：

```text
test result: ok. 158 passed; 0 failed; 0 ignored; 0 measured; 309 filtered out; finished in 1.50s
test result: ok. 54 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.54s
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 454 filtered out; finished in 1.02s
```

首次失败留存：Git 首轮 155 通过/2 失败，原因是 gitlink 删除目录和源文件重建状态未被夹具表达；S6 首轮 1 失败，原因是捕获了变更后的文件状态；MCP 首轮 44 通过/1 失败，原因是虚拟时钟启用时点。均修复夹具或控制时序，保留原业务断言；不是自动重试到绿。原始日志暂存本机 `/tmp/vega149-*`，云端全量以 PR check 为准。

证据限制：不再证明实际 TCP socket 关闭/绑定、非 loopback peer、原生进程/PTY 回收、reqwest 自带的底层超时、真实 Git 或系统启动器契约；这些由用户手测。默认发布图不启用 test-support。历史性能报告仍保留，不能据此证明本轮完整流水线的耗时。

MCP mock 缩小 stdio 分支后触发 `large_enum_variant`，主 agent 批准将 `McpConnection::Http` 的 `HttpClient` 置于 `Box` 中；仅调整内存布局与构造点，协议与生命周期行为不变，不通过 allow 属性绕过检查。

### 云端交付

PR：[#182](https://github.com/puzige/vega/pull/182)。首轮 [required check](https://github.com/puzige/vega/actions/runs/36032245026) 在 Clippy 阶段发现 Bash 预检与 MCP mock 响应写入的两处 `collapsible_if`；fmt 通过，测试阶段未执行。修正写法后重新提交云端门禁，未绕过检查。最终 check 与合并身份以 PR/Issue 交付记录为准。

第二轮云端 [check](https://github.com/puzige/vega/actions/runs/36032868414) 的 fmt/Clippy 通过，全量测试发现 15 项迁移遗漏：runtime MCP/skills 9、Bash 调用方 5、旧 ignore 名单冻结断言 1。前两类补进程内 mock 并保留原断言，最后一项同步本卡已迁移的名单；未通过删除或 ignore 失败项绕过。

第二轮补齐：runtime MCP 8 项与 skills 1 项原真实 stdio 脚本迁移到 duplex mock，保留凭据轮换、防泄露、错误分类和预算/权限断言。定向 `agent::tests::mcp_registry::` 18 项（0.03s）与 `agent::tests::skills::` 12 项（0.15s）通过；相同编译产物在禁止 process-exec 的沙箱再次通过（0.02s / 0.14s）。网络/Artifact 迁移数在首批 104 项基础上再增加 9 项，共 113 项；不包含 Git 与 Bash 迁移数。

旧 ignore 从 10 降至 5 的五项全部保留并激活：OpenAI retry/backoff cancel/midstream cancel、Git draft deadline、MCP probe+catalog shared deadline。扩展静态审计覆盖 shell 脚本夹具、客户端/PTY 构造器、Bash ToolUse 及共用 helper，未知 mock 继续明确失败。

Bash 间接调用补齐 5 项原测试：runtime permission_flow 3、conversation history_permissions 1、app permission ingress 1。定向 permission_flow 15 项、history 1、app ingress 1、R52 freeze 1 通过；五个补齐用例均在禁止 process-exec 环境再次通过。六个修改包最终 Clippy `--all-targets -- -D warnings` 与格式检查通过，无新增 ignore 或失败用例删除。
