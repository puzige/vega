# #902 验证证据

## 精确版本

- 基线：`d36ce592c26450a0d243f77bec9bbaa11dc2e546`。
- 首轮开发：`e9a1d105b649945d5bea08f7af33cda308c1a90f`。
- 修正后开发：`265bb1aeb01663e1b9137f4995c17528da51bddc`。
- PR：[908](https://github.com/puzige/vega/pull/908)；Issue：[902](https://github.com/puzige/vega/issues/902)。

## 开发回归：真实云端结果

[Actions 42189698218](https://github.com/puzige/vega/actions/runs/42189698218)，head `265bb1aeb01663e1b9137f4995c17528da51bddc`，required `check (fmt, clippy, test)`、fmt、全目标 Clippy、Nextest 均成功。

Nextest 0.9.146：10 发现 / 10 执行 / 10 通过 / 0 跳过；Python 三组为 5 + 15 + 40 = 60 全通过。6 项真实 runtime 循环回归、4 项 SkillRun 边界测试：

- 调查读取后加载，新正文进入后续 Provider 请求；重复加载不重复注入。
- ReadOnly 拒写且无文件，Auto 阳性对照写入成功且内容准确；allowed-tools 不授予权限。
- 首轮加载、禁用/未知项拒绝、工具结果不改变设置。
- 读取后撤权触发真实 runtime authority fence，无新激活或后续 Provider 请求。
- 混合批次拒绝普通工具；取消、非直接用户、automatic gate 保持。
- 未激活资源不可读；已激活正文/资源冻结；新激活源内容变化和同内容链接换靶拒绝。
- 三项上限、预算拒绝不变。

设置控制器、立即引导原子持久化和历史只读恢复为静态路径核对，未声称完整原生集成自动验证；原生体验待用户手测。

### 可恢复源码

开发测试只存于开发提交，不进入 master。完整 514 行源码：

- [固定 SHA 源码](https://github.com/puzige/vega/blob/265bb1aeb01663e1b9137f4995c17528da51bddc/crates/vega_runtime/tests/on_demand_skills.rs)
- [远端证据标签](https://github.com/puzige/vega/tree/evidence/902-development-regressions)，读回确认直接指向上述完整 SHA。
- SHA256：`c4d21c43123e4c1be6b3ce4322e760c472c048ee799f11d809043bb7b04d4fd3`。
- Git blob：`0d6480d1a3b4b86dca77ee6afa5ee3521ec08e3a`。
- 通过 GitHub contents API 独立读回源码并重算 SHA256 一致，不仅依赖本地路径或短期 artifact。

### 首轮失败保留

[Actions 42188540919](https://github.com/puzige/vega/actions/runs/42188540919)：10 执行 / 8 通过 / 2 失败。两个写入测试的 checkpoint 夹具位于项目内且未创建；`vega_tools::MutationContext::new` 要求已存在、非 symlink、项目外目录，因此在权限判断前准备失败。修正仅为外置 sibling checkpoint 的创建与清理，保留所有断言；另修测试嵌套 if 的 Clippy 风格错误。生产代码在此增量未变。不是产品权限放宽，也不是删除失败回归求绿。

## 独立审查

非实现者 Agent 对 `e9a1d105b649945d5bea08f7af33cda308c1a90f` 差异与受影响契约完成只读审查，无发现；对 `265bb1aeb01663e1b9137f4995c17528da51bddc` 测试夹具增量复核，无发现。审查未本地执行测试/编译，不冒充云端结果。实现者与审查者不同，生产范围在审查期间停写。

## 最终清理及门禁

删除唯一开发测试文件 `crates/vega_runtime/tests/on_demand_skills.rs`；无 test-only 模块、manifest、feature、依赖、fixture、helper 或 workflow 接线。生产两文件与已验证开发提交逐字一致。保留 Python 测试、生产安全断言和通用 CI。

最终无测试 head required CI 待运行；需要 Rust 0 发现 / 0 执行（no Rust tests）、完整 fmt/Clippy/Python/required aggregate 成功。空集只是清理门禁，不替代上面的 10 项行为验证。最终 head 和合并完整 SHA 回写 Issue/PR。

本地仅执行 `cargo fmt --all -- --check` 与 `git diff --check`（退出码 0），未运行 Vega、编译、Clippy、Rust 测试或打包。用户未手测通过前 Issue 保持开放；合并后 In review。
