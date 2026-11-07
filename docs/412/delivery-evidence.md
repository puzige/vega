# #412 异步 Shell：交付证据与手测清单

## 合同与实现

- 冻结合同：[v1.1](https://github.com/puzige/vega/blob/6c0cb43e3db3cd7d0e4368acb8915a1af73fa9f2/docs/412/best-effort-shell-contract-review.md)，blob `de5da7970c4686b392c4ef784d679b642abd83d9`。未更改用户接受的 best-effort 边界。
- 相关 PR：[ #906](https://github.com/puzige/vega/pull/906)，Issue 保持开放。本文初次提交时最终清理 head 的 required CI 尚待；合并与用户结论以 PR/Issue 最新记录为准。
- 集成 master 基线：`4b604cb8c2a8c33ae8e5ed3a1ec9f941bbd19b01`。保留主线迁移 0060/0061，追加 `0062_shell_operations.sql`。
- 缺省/null `wait_ms` 保留旧阻塞协议；显式等待窗口采用一次启动、运行句柄、wait/cancel。独立命令结果、清理结果与工作区准入；保留活动 child/reader/resource owner，不把退出 0 等同整体成功。
- 可信恢复面板提供当前命令取消、原 owner 清理重试、只观察的重新检查，以及绑定完整范围世代和 operation 集合的风险接受。已提交但回执未知时保留原决定身份；重放不重跑命令、不清除新增 blocker。

## 可恢复的开发测试源码

已发布并读回远端注释标签 [`shell-412-dev-b2b90ea2`](https://github.com/puzige/vega/tree/shell-412-dev-b2b90ea2)，其剥离后提交为 **`b2b90ea2fb8c5fdeefe5071e0c0e5225b6db697f`**；标签对象 `752f637a7ff7f9c79b477f999c293cf8a067862c`。已通过 GitHub tree API 核实下列文件 blob，不依赖本地文件或会过期的 artifact。

| 开发源码（固定提交） | 实际执行数 | Git blob |
|---|---:|---|
| [输出、reader、retry、旧读失效](https://github.com/puzige/vega/blob/b2b90ea2fb8c5fdeefe5071e0c0e5225b6db697f/crates/vega_tools/src/bash/session_tests.rs) | 9 | `f3611300b5d2b0c7ecc477a6c9e0c1186a4afd64` |
| [进程身份与共同预算](https://github.com/puzige/vega/blob/b2b90ea2fb8c5fdeefe5071e0c0e5225b6db697f/crates/vega_tools/src/bash/session_process_tests.rs) | 10 | `0b456bc9bc7c56e5e8a51b98e4f686dbc51c6793` |
| [准入、容量、启动持久化失败](https://github.com/puzige/vega/blob/b2b90ea2fb8c5fdeefe5071e0c0e5225b6db697f/crates/vega_runtime/src/agent/shell_sessions/tests.rs) | 14 | `d348a43f15c6b2aba09832a149ba0db8529f220d` |
| [持久事实、重启、精确决定与别名](https://github.com/puzige/vega/blob/b2b90ea2fb8c5fdeefe5071e0c0e5225b6db697f/crates/vega_store/src/shell_operations_tests.rs) | 9 | `550e085884f1c819d5e23acb944fca24287f0b36` |
| [严格工具回执投影](https://github.com/puzige/vega/blob/b2b90ea2fb8c5fdeefe5071e0c0e5225b6db697f/crates/vega_conversation/src/agent/shell_operations_tests.rs) | 3 | `989137032e21c64daf5ecd2b7a64562856f60429` |
| [命令卡终态叠加](https://github.com/puzige/vega/blob/b2b90ea2fb8c5fdeefe5071e0c0e5225b6db697f/crates/vega_ui/src/tool_card_shell_tests.rs) | 1 | `0ef3727e36d0339577a886de260b89892a1f704e` |
| [未知决定关闭/重入状态](https://github.com/puzige/vega/blob/b2b90ea2fb8c5fdeefe5071e0c0e5225b6db697f/crates/vega_ui/src/conversation_stream/shell_recovery.rs#L505) | 2 | `31eed978fd4e50f33e9560f243bc77c7d8166810` |

本地官方 Cargo `1.98.0 (797e8a9bc 2026-08-05)`、rustc `1.98.0 (88d9e12ae 2026-08-18)`：

```text
rustup run 1.98.0 cargo test --locked -j 2 -p vega_ui -p vega_runtime -p vega_store -p vega_tools -p vega_conversation
48 运行 / 48 通过 / 0 失败 / 0 忽略；doctest 0 执行
rustup run 1.98.0 cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings
退出 0
cargo fmt --all -- --check；git diff --check
退出 0
```

开发 head 的云端 [run 42362587642](https://github.com/puzige/vega/actions/runs/42362587642)，Rust 1.98.0、Nextest 0.9.146、默认测试并发：

- [Nextest job 128373885684](https://github.com/puzige/vega/actions/runs/42362587642/job/128373885684)：日志明确 `Starting 48 tests across 16 binaries`，`48 tests run: 48 passed, 0 skipped`，约 0.822 秒执行（不含编译）。三组 Python 分别 5、15、40 条通过。
- [Clippy job 128373885452](https://github.com/puzige/vega/actions/runs/42362587642/job/128373885452)：fmt、workspace/all-target strict Clippy 成功。
- required `check (fmt, clippy, test)`（job 128375243738）成功。

## 独立审查

非实现写入者分别审查持久化/恢复/进程边界与 UI/可信交互。原三项 P1、后续预算/路径 P2，以及 UI 一 P1/三 P2 均修复并由对应 reviewer 增量关闭；最后对开发 head `b2b90ea2…` 的世代碰撞修复无新增发现。UI 原四项在 `a353ab927dfc8f211a52c3f41cf1c26e54f83614` 已关闭，后续未改 UI 控件与命令卡逻辑，恢复版本类型变化由安全 reviewer 核对。静态审查不冒充 reviewer 重跑测试、云端或原生验收。发现与修复过程保留在 [实施检查点](implementation-checkpoint.md)。

## 测试清理与最终门禁

本次清理仅删除上列六个测试专用文件、七个混合文件的测试接线/内联测试。逐文件将已验证开发提交去掉指定测试段后与清理树比较，生产内容一致；Cargo manifests/lock、Python、workflows、迁移和资源未改。无测试专用新增依赖需要删除。

- 静态清点 `crates/` 无 Rust test/cfg(test)/bench 测试入口、dev-dependency/test-util 残留；唯一 rustdoc 围栏为既有 `text` 示例，不是可执行 doctest。
- 清理树完整 workspace/all-target strict Clippy、fmt、diff 成功。
- 本机已安装 Nextest **0.9.151**（未安装、升级或修改配置），`cargo nextest list --workspace --message-format json --locked --build-jobs 2`：`test-count=0`；`cargo nextest run --workspace --no-tests=pass --locked --build-jobs 2`：16 binaries、0 发现/0 执行/0 跳过。标记 **no Rust tests**，不是行为测试通过。
- 最终发布 head 仍须由现有云端 Nextest **0.9.146** 和完整 required CI 确认同一空集；具体 head/run/result 回填 PR/Issue。未取得结果前不合并。

## 覆盖范围与用户手测

48 条开发回归覆盖身份撤销、到期后零新观察、reader 保留与取消安全、输出 UTF-8/脱敏/游标/缺口、双向外部根冲突、容量、独立 blocker、丢失回执精确读回、重启未知事实、路径失效/重现别名、完整世代向量、旧读失效与 UI 纯状态。**它们不是 BE01–BE26 或 legacy A–N 全部逐项通过的证明。** 未直接运行真实 Shell 启动/退出/取消/关闭及 scoped 网络凭据集成，不将静态调用链或 adapter 注入称为真实进程验证；真实系统集成按仓库规则交用户在获准构建上手测。

建议用户在隔离的临时工作区进行以下手测，涉及残留时先手动确认进程，不向来源不明的 PID 发信号：

1. **旧协议 / 会话等待**（BE01/02，A–C）：缺省/null wait_ms 的快退、非零与总超时保持原行为；显式短窗口返回 running 且无退出码，重复 wait/cursor 不重跑、不延长总期限。
2. **退出、取消与 Stop**（BE04–06/09，D–F/L）：自然 0/非零、timeout、重复 cancel、用户 Stop、新输入替换、正常关闭；命令结果与清理分开，先停止准入，未确认不开放写入，无旧队列自动启动。有限系统样例不能证明所有逃逸后代均退出。
3. **权限、句柄与冲突**（BE03/18/23/24，G/H/K/M）：跨 run/workspace 的旧句柄不可用；同范围写冲突与额外外部根/Full Access 双向冲突；无关工作区可用；scoped 网络租约覆盖命令并在收尾撤销，撤权触发停止。
4. **输出与未解决清理**（BE07–12，I/N）：长输出、敏感值、已知退出但 reader/资源未收束时不显示整体成功；检查真实错误和恢复入口，保留退出与历史输出缺口。
5. **重启与恢复**（BE13–19/26，J）：重开、新会话及路径别名仍能看见 blocker；不恢复旧 PID 控制、不重派命令。Retry 只重试原资源，Check 只观察；已知活进程/资源故障不能风险绕过。接受风险后回执未知，Escape/关闭/重入仍保留同一决定；新 blocker 或事实变化使旧确认过期，重放不清新问题；恢复后旧文件读取失效。
6. **原生 UI**（BE20–22）：原命令卡展开/收起/历史终态一致，恢复入口不随过程折叠隐藏；Tab/Enter/Escape、保守默认焦点、重复提交、960×600、长命令/错误、浅深主题。纯状态回归和代码焦点请求不等于这些原生验收。

用户明确通过前 Issue 不关闭、不 Done；合并后置 In review 等用户手测。未执行的项目保持未执行，不由 CI、独审或测试清理代替。

## 残余风险与回退

best-effort 只处理可观察且身份仍匹配的已知进程；隐藏逃逸/重挂后代和 OS 检查到信号间竞态不可能由本次有限验证消除。用户已接受该平台残余风险；不作“全部后代安全退出”承诺。

回退须先关闭新会话准入、尽力收束当前 owner 并保留 unresolved blocker/追加迁移；不得直接运行忽略持久未完成记录的旧二进制，也不得删表、重放命令或按旧 PID 清理。应用安装、正式发布和用户数据修改不包含在本轮本地验证中。
