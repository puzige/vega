# #575 开发验证与交付记录

## 实现范围与版本

- 用户裁决：最近一次 Native 主调用的总 token，即 checked `input + output`；缓存属于 input，不重复加，不累计整个任务。
- 规格：[v2 验收与边界](vega-issue-575-context-usage-calibration.md)。集成基线为 `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`。
- 开发验证通过的完整提交：`5e659b9fb30b7b16c734dff1212be2e5b092c28b`。
- PR：[#578](https://github.com/puzige/vega/pull/578)，关联 [#575](https://github.com/puzige/vega/issues/575)，不用自动关闭关键字。
- app 在既有 generation/thread/stream/model 所有权内接 usage，再继续通用 meter 消费；UI 以 response id 隔离同调用去重，区分预检估算与最近调用总量。
- 新调用与压缩后允许下降；迟到投影不覆盖已知总量；结束/取消保留同 stream 的最近值，新 run/消息/模型/会话边界清理归属。
- 不改分母、预算/压缩决策、provider 协议、费用累计、布局和交互；只更新必要来源标签。Codex 后端不新增适配。

## 原始开发回归可恢复来源

- [测试源码](https://github.com/puzige/vega/blob/5e659b9fb30b7b16c734dff1212be2e5b092c28b/crates/vega_ui/src/conversation_stream/context_usage_tests.rs)，SHA-256：`a98cf407c10e43cc362ceb7edea8d0d47da0cb36fca65bbf64cf24c2fc1d4430`。
- [完整开发提交 patch](https://github.com/puzige/vega/commit/5e659b9fb30b7b16c734dff1212be2e5b092c28b.patch)，读取内容 SHA-256：`88a3b3cdd6f722c65802332ba35cd60dba3f8136d4da09e311e43c7496d07172`。
- 源码和 patch 已从 GitHub API 实际读回；源码与本地逐字节一致，不只依赖本机临时文件或过期 artifact。
- 测试仅供开发验证，按当前政策不进入 master；六组回归的执行结果与最终无测试门禁分开记录。

## 实际云端验证

### 首轮：回归通过，完整门禁失败

- head：`eac303632fb1ae467e3aa248c157b1370f468e84`。
- [Actions 39915367755](https://github.com/puzige/vega/actions/runs/39915367755)。
- [Nextest 120287638960](https://github.com/puzige/vega/actions/runs/39915367755/job/120287638960)：6 发现 / 6 执行 / 6 通过 / 0 失败 / 0 跳过；三组 Python 为 5、15、40 执行，全部 OK。
- [Clippy 120287638579](https://github.com/puzige/vega/actions/runs/39915367755/job/120287638579)：fmt 成功，Clippy 因测试文件第 3 行未使用的 `use gpui_kit as gpui;` 失败；required 汇总失败。
- 已读原始日志并归类为测试接线 lint 故障，不是产品断言失败、验收变更或外部环境故障。仅删除该 import；生产七文件和全部测试断言逐字节不变，无 suppression、ignore 或放宽断言。

### 修正轮：开发 head 完整通过

- head：`5e659b9fb30b7b16c734dff1212be2e5b092c28b`。
- [Actions 39917164808](https://github.com/puzige/vega/actions/runs/39917164808)，完整成功。
- [Nextest 120293100775](https://github.com/puzige/vega/actions/runs/39917164808/job/120293100775)：**6 发现 / 6 执行 / 6 通过 / 0 失败 / 0 跳过**，源码无 ignored 测试。三组 Python 5、15、40 执行，全部 OK。
- [Clippy 120293101119](https://github.com/puzige/vega/actions/runs/39917164808/job/120293101119)：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 成功。
- [required 120294433742](https://github.com/puzige/vega/actions/runs/39917164808/job/120294433742)：`check (fmt, clippy, test)` 成功。
- 原始 Nextest 输出的六个 PASS 与实际汇总已读取；不把发现数冒充执行数。

## 覆盖与证据限制

六组开发 GPUI 回归使用真实 ConversationStream 窗口、生命周期/响应事件与显示边界，覆盖：

1. 160000 或 100000 的预检均可校准为 120000 + 30000 = 150000；缓存 80000/10000 不重复加，上限 400000，占比 37.5%，来源及可访问名称真实。
2. 同调用重复/较小 usage 不回退；同 message 下一 response 可降至 100000，不取任务最大值或累计。
3. 旧 accounting/投影不盖回真实值；新有效预检可切回估算。
4. 摘要与子任务 usage 不替代主调用，PostSummaryTarget 可降至 40000，下一主调用总量 50000。
5. 零输入、溢出、错误 thread/model/message、非运行/无响应归属和模型重置后的旧事件不改显示；Codex 不冒充支持。
6. 完成/取消保留最近值，新 run/冷投影保持回退语义；无预算时也允许下一 response 将 150000 替换为 100000，且不伪造分母。

app generation/thread/stream/model 栅栏由独立静态路由审查覆盖，不声称上述 GPUI 测试执行了 app ingress。usage 本身没有 response id，不能独立鉴别任意同 message 乱序旧调用；依赖既有有序通道。真实模型/服务与 UI 集成仍由用户手测，静态包研究不构成全部 ChatGPT 发送时机的证明。

## 独立审查与本地静态检查

- 非实现者审查 `eac303632fb1ae467e3aa248c157b1370f468e84`，无可操作发现；对 `5e659b9fb30b7b16c734dff1212be2e5b092c28b` 的单 import 增量独立确认后，结论延续。
- 已执行改动文件单文件 rustfmt、`git diff --check`，退出 0；离线 locked Cargo 元数据解析退出 0，只解析依赖，不编译或执行 Vega。
- 本地没有 Vega 运行、构建、Clippy、Rust/Python 测试、打包或 CUA。

## 最终交付边界

开发回归实际完整通过后，仅清理其源码、模块、gpui_kit test-support dev-dependency 和独占锁文件差异；不改变已验证生产行为，不修改 Python、工作流或通用 CI。

- 清理停写补丁 SHA-256：`817ee212880b91d209a4a74b188474755da9243d0d8eec7a8cb4d570cd11197b`（full-index binary tracked diff，不含本交付文档）。
- 4 个清理路径：删除 `context_usage_tests.rs`；移除 `mod.rs` 两行测试模块接线；移除 `Cargo.toml` 独占 dev-dependency；清除 Cargo.lock 的 10 个测试独占包及其依赖边/消歧变化。
- 七个生产文件与已通过开发 head 逐字节一致。Cargo.lock、manifest 和 mod.rs 与集成基线逐字节一致；锁文件 blob `1b2d4d421cab11a6431317f673808bbebb01dae6`，914 个包，基线 identity/version/source/checksum 全保留。
- 清理者静态清点 451 个存续 tracked Rust 文件（crates 401 / xtask 9 / vendor 41）和 16 个 manifest：测试/cfg 属性、测试独占候选、测试/fixture/mock 路径、可执行 doctest、独占 manifest 接线、未跟踪 Rust 文件均为 0。原有一个 `text` 文档围栏和 9 个 `doc(hidden)` 不属于可执行测试。
- 清理者单文件 rustfmt、diff check、精确范围/生产不变/锁文件与接线基线比对均退出 0；Parent 独立确认该 patch 哈希、七生产文件不变、接线/锁文件基线一致，离线 locked 完整元数据解析退出 0。
- 静态清点不替代云端测试发现/执行证据。最终 head 的 full required CI 仍待运行，不提前称为通过。

本文件固定开发验证与清理边界；**最终删测 head 的完整 SHA、独立审查和新一轮 required check 由 PR/Issue 后续记录固定**。最终门禁必须明确 Rust `no Rust tests`（0 发现 / 0 执行），它不是六个功能回归“再次通过”，更不是用户验收。未取得的最终结果不在本文件预先标为成功。

合并后 Issue 保持开放 / In review，等待用户手测；不表示应用已安装。建议核对真实响应后的最近调用总量及来源、同消息下一调用可下降、压缩后不带旧高水位、完成/取消保留值；没有新请求的工具等待期应保留最近值，不伪造增长。回滚撤销本卡生产差异，无数据迁移。
