# #494 交付记录

记录阶段：v2.0 生产提交 `0139e8d0f7d79383476824b696bad810a37f41e8` 已完成主 agent 审查，同一冻结测试在 Actions 取得 27/27 GREEN；本文所在提交独立清理开发期测试。最终空集 head 门禁及合并证据在 [PR #495](https://github.com/puzige/vega/pull/495) 后续固定，空集不替代 GREEN。未运行本地编译、Clippy、测试或 Vega。GREEN 原始证据已固定于 [PR 评论](https://github.com/puzige/vega/pull/495#issuecomment-6265953287)。记录时间为 2026-10-19，时区 UTC+8。

## 范围与基线

- Issue [#494](https://github.com/puzige/vega/issues/494)，draft PR [#495](https://github.com/puzige/vega/pull/495)。工作分支 `feat/494-chat-auto-adapt`，生产基线 `39b706e2e646c53fcfb25d010b3eecd19b323eb3`。
- 按 [v2.0 冻结规格](vega-issue-494-chat-auto-adapt.md) 实现 Chat Completions 内部适配、Step 工具回传和偏好设置收敛。专职子 agent 唯一写入生产代码，主 agent 负责规格、审查与集成。
- 用户取消旧配置兼容后，旧 `reasoning.toml` 不再读取、迁移或改写。新 `reasoning-preferences.toml` version=1，仅保存精确 provider/model/preference。加载缺失文件不会创建文件，旧偏好需要用户重新选择。
- 没有改动用户运行中的配置、凭据或历史；没有真实模型调用、生产依赖增加、版本升级、DDL 或新增原始思考落盘。

## 两个先测后修周期

| 周期 | 先行提交与 Actions | 结果及用途 |
|---|---|---|
| v1.0 首次测试接线 | `de112c562715806ca4a61e27797ae21f95136fa4`；[run 39237364347](https://github.com/puzige/vega/actions/runs/39237364347) | GPUI ElementId 的 owned/reference 借用错误，行为测试未执行，不计 RED；原证据保留。 |
| v1.0 接线修正后 | `973e3857a4983852bf4ec809856c2ecdab2f95e1`；[run 39238140388 / job 118012146061](https://github.com/puzige/vega/actions/runs/39238140388/job/118012146061) | fmt、全目标 Clippy 成功；22 执行、8 通过、14 行为失败、0 跳过。18:38 主 agent 审查后授权生产。 |
| 用户取消旧配置兼容 | 未提交 v1 草稿归档后恢复生产 | 用户改变配置契约，v1 RED 仅保留历史，不能证明 v2。归档补丁 SHA256 `c697bc9da408ce16ea02e49b69fd51a94b1c4a51705b2dd31ce1d4010ad096fc`，13 份源码另有逐文件哈希；归档目录 `/tmp/vega-494-v1-draft`。恢复至 `973e3857` 后生产与该提交一致，随后重新写齐 v2 测试。 |
| v2.0 新冻结测试 | `eb079d6018b8176041b864ad82f162aa1da8b1d0`；[run 39240808870 / job 118020881294](https://github.com/puzige/vega/actions/runs/39240808870/job/118020881294) | fmt、全目标 Clippy 和保留 Python 发布检查成功；27 发现/执行、6 通过、21 目标行为失败、0 跳过，无编译或环境失败。19:00 主 agent 审查后授权本轮生产实现。 |
| v2.0 同一测试 GREEN | `0139e8d0f7d79383476824b696bad810a37f41e8`；[run 39242802623 / job 118027425612](https://github.com/puzige/vega/actions/runs/39242802623/job/118027425612) | 27 发现/执行、27 通过、0 跳过；全部冻结测试字节不变。fmt、全目标 Clippy 与完整 required check 成功；Python 三组分别 5、15、14 项通过。固定证据后才授权独立删除开发期 Rust 测试与独占支持，最终空集 head 门禁另记。 |

v2 测试覆盖 runtime 8、内部事实及真实应用加载 6、偏好 codec/控制器/CAS 7、GPUI 4、设置错误 1、运行失败投影 1。生产实现期间测试数据、断言和独立源码字节不变；删除能力 UI 后，仅将既有测试专用类型导入收至 cfg(test)，随 GREEN 后专用支持一并清理。

## 生产改动

| 文件 | 最终职责 |
|---|---|
| `crates/vega_runtime/src/chat_compat.rs`、`lib.rs` | 现有 headless runtime 内集中模型事实；Step 精确 ID 采用 low/medium/high，不声明关闭。保留官方 DeepSeek/GLM 的完整端点身份匹配，无 URL 子串识别或第二份 App 品牌表。 |
| `crates/vega_runtime/src/provider.rs`、`agent/loop_.rs` | Unknown/StandardChat 在工具请求周期保留真实 absent/empty/nonempty 文本状态，两条 Assistant 路径均覆盖无显式 profile。移除旧模板专用策略；保留严格服务和 foreign Responses 校验。收集结束缺 Provider Done 则在工具审批/执行前失败，不改 SSE finalize 语义。 |
| `crates/vega_runtime/src/error.rs`、`crates/vega_conversation/src/types/events.rs` | 移除“选择高级协议模板”错误指令，保留失败及重试/新对话的恢复动作。 |
| `crates/vega_store/src/reasoning.rs` | 删除旧 v1/v2 能力 codec、迁移、能力 patch 和合并；严格新偏好 grammar，拒绝旧字段、重复身份及版本。保留原子写入、进程内协调和读回校验；仅合并异身份偏好更新，同身份冲突、记录删除及文件删除拒绝。 |
| `crates/vega_conversation/src/types/reasoning.rs`、`crates/vega/src/reasoning_capabilities.rs` | 共享内部事实并叠加偏好，传入投影不能覆盖能力。语法合法但模型不支持的偏好可投影和修复，运行 freeze 拒绝。实际 API 决定 Responses 边界。 |
| `crates/vega/src/app_agent.rs`、`window/session.rs`、`window/reasoning.rs` | 运行、设置及既有压缩入口使用同一新文件名常量。保存仅接受偏好变化，回读后使用 loaded_replay_config 中真实 provider/API/base_url 重新解析；缺记录回内部默认，不从旧目录偏好补缺。没有 provider 时提前失败，不信任捕获的能力字段。 |
| `crates/vega_ui/src/settings/{mod,state,reasoning_state,reasoning_render,provider_management,model_catalog}.rs` | 页面标题“思考偏好”；移除模板、协议、支持范围、允许关闭、允许档位和对应隐藏动作/状态。移除目录应用档位入口，保留其余目录信息。实际偏好按钮绑定各自焦点，保留 reload、保存、冲突与非法偏好就近修复反馈。 |

## 本地检查与冻结证明

本地仅执行快速格式、差异和静态检查。`cargo fmt --all` 已完成；交接前 `cargo fmt --all -- --check` 与 `git diff --check` 均通过。静态检索确认生产 Rust 不再包含旧文件名、模板动作、旧 codec 或能力 patch 字段；没有修改 Cargo 文件或锁文件。新增生产代码无注释、unwrap/expect。

冻结测试 SHA256 与 `/tmp/vega-494-v2-frozen-tests.json` 一致：

| 测试源码 | SHA256 |
|---|---|
| runtime `openai/chat_compat_tests.rs` | `21fb7a9c150cb1066a31c75a567dba2af9384ebf0e8b487f26d7465936fadfe3` |
| app `reasoning_capabilities_tests.rs` | `65673d18ef20d05c2daef8aacb9124b4f3974ddbba43cc64d6b28c46260f2247` |
| window `reasoning_preference_tests.rs` | `0e4af78ca312326e006dd3b9cca854c35eec7943f7ab1a52123a5b93947d8dd3` |
| conversation `reasoning_failure_tests.rs` | `ba12be254134db8e4f4e452874368c9e202fa73cbdd7f66f5bd3a4dd1abc253c` |
| UI `reasoning_ui_tests.rs` | `ba472c6d47378efe335699c0f0f53f51638f8e2be98492319d09f9a63117af21` |
| `settings_errors_do_not_direct_users_to_removed_templates` 完整 cfg/test 源码段 | `dbbbe877f2588329b0305dc10e3733d2b15e6c407c765899f8e166d3ac045df4` |

## 证据与风险边界

独立清理删除五份开发测试源码、inline 错误用例、模块/观察/测试导入支持和两项 dev dependency；十个独占锁包移除，Cargo 文件及锁文件恢复原生产基线。通用设置 helpers 恢复基线，实际偏好焦点、键鼠、修复和 runtime 行为保留。18 个生产文件中 13 个字节完全不变，另 5 个只有已批准的 cfg/test/观察分支清理，OpenAI 接线单独恢复。格式及 diff 检查通过；源码静态未检出 Rust 测试属性或测试专用支持。最终 0 发现/0 执行以本提交云端日志为准，不以本地静态检索代替。

- Provider 替身只隔离网络，工具、SSE 组装、run_agent、wire 编码、偏好真实入口和 GPUI 控件操作使用生产路径；不等同于真实 Step/用户代理联调或原生应用整条路由手测。
- 无 tools 的公开 AgentRequest 主循环因内建工具定义不可构造；冻结测试只验证真实 ChatRequest 编码和既有策略契约，不将其称为完整 runtime 生命周期证据。
- E 覆盖 malformed reasoning 和缺 wire finish 的流片段/终态，不声称测试了第三方 Provider 在完整 finish 后故意省略 Done 的另一种抽象契约。实际 OpenAI finalize 保持原语义，不要求额外 wire 结束标记。
- 原子保存对不合作的外部进程仍是原有最佳努力 CAS，不宣称跨进程强原子比较交换。
- 原始思考仍限运行内存与受控 warm 状态；既有权限、凭据 fencing、回放身份、冷历史、预算、取消及压缩边界未放宽。
- 旧自定义能力与偏好主动失效，未知服务仅内部默认与通用文本回传；用户需在新设置重选合法偏好。回滚只 revert 本卡，不改旧文件或用户数据。
- 交付顺序：本文所在独立清理提交 → 云端最终门禁 → 转正式 PR/集成 → 用户真实 Step/代理与键盘手测。最终门禁及集成结果以 PR 后续证据为准，本记录不声称应用安装或真实代理手测通过。
