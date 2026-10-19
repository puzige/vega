# Issue #75 — 桌面全窗口访问（用户覆盖裁决）

## 来源与优先级

2026-10-12 用户明确要求移除 Vega CUA 内部逐窗口授权，默认可以访问所有桌面与窗口。本条覆盖原 #75 R3/R4/R8/R15、CU03/CU17 及 runtime-fixes 中对桌面逐窗审批、目标切换或恢复重新授权的旧要求。仅修改桌面 CUA，Playwright 调查和方案暂停，其浏览器授权逻辑不变。能力开关仍是用户对 Native 桌面能力的总开关，保留已有显式关闭与配置默认；开启后全部可发现窗口都属于可访问范围，不额外询问。

当前窗口截图降级、观察失败恢复与 live 工具卡行为以[窗口观察修复规格](vega-issue-75-window-observation.md)为准；本文件的目录/目标/接管边界保持。

## 需求与实现合同

DA1. ComputerObserve 使用严格 schema 的 target_id:string|null、cursor:string|null。target_id 与 cursor 不能同时非空；null/null 枚举真实运行应用和窗口并返回有界目录；每条包含宿主生成的 opaque target_id、应用、窗口标题及真实 PID/window ID 元数据。目录不是授权提示，也不自动选第一项。模型再次调用 ComputerObserve 指定目录中的 target_id，观察该窗口。可重复发现新窗口、明确选择不同应用的窗口。

DA2. 目录、目标 ID、AX、图像仅属于 live run。目录分页每页最多 200 个窗口、正文最多 64 KiB，外部名称/标题有界并以 JSON 编码；首轮 null/null 刷新目录；opaque next_cursor 继续下一页，不丢弃 200 之后的窗口。cursor 与 target_id 仅当前 run/epoch/目录世代且目录创建后 60 秒有效，超时、刷新、动作派发、压缩或接管均须重新发现。目录原始数据受 MCP 256 KiB 总限额，最多 4096 条，超限明确失败而非静默截断。list_windows 返回但 list_apps 未列出的辅助进程窗口同样保留有效 PID/window ID，以 window.app_name 作为名称；不因应用枚举过滤而丢弃系统对话框。失败/终态立即清除目录正文。目录作为 CuaWindowCatalog 工具观察，不作为用户消息或指令。历史仅保留安全摘要，压缩/暂停/停止/重启使目录 ID 和观察失效。目标 ID 是定位引用，不是逐窗访问授权。

DA3. Cua 0.34 正式使用 --permission-mode standard，且不传 capability/session manifest；不使用 unrestricted。专属 owner-only policy.yaml 仅含 allow.tools，固定允许 check_permissions/list_apps/list_windows/get_window_state/click/type_text/press_key/hotkey/scroll，通过 CUA_DRIVER_POLICY_FILE 显式传给 daemon。上游 policy 默认拒绝未列出的工具，Vega adapter 同时保持原有限工具映射，模型不能传任意 MCP 名称、PID/window ID、policy、socket 或可执行路径。新 PID/窗口天然可发现和选择；每次动作依然绑定模型所观察的 exact-window，而非模糊的当前前台窗口。TCC、目标归属、snapshot/token 与 background gate 不变。

上游枚举范围为 layer-0 顶层窗口，包含其他 Space、最小化和隐藏窗口；系统确认不属于 AXWindows 的无标题/离屏辅助窗口仍由上游过滤。不能将全桌面范围描述为绕过系统可见性或访问任意合成 UI 节点。

固定来源：[standard 观察](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/tool.rs#L1967)、[standard 输入](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/tool.rs#L2117)、[policy 默认拒绝及 schema](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/policy.rs#L389)。这是用户新全桌面范围授权下的正式模式，不把旧 bounded manifest 写成虚假 wildcard。

DA4. 桌面连接通过组件、协议和 TCC 检查后直接 Running，不进入 AwaitingApproval。不保留桌面 grant_started/last_used 人工授权期限；运行结束回收执行通道，不显示重新授权，也不在失败动作后自动重试。浏览器审批和寿命不变。

DA5. ComputerAct 必须引用当前目标的最新非目录 observation_id；目录观察不能直接用于 mutation。切换或重新发现先失效所有旧桌面观察；模型不能把窗口 A 的引用用于 B。指定旧目录 ID、窗口已关闭/PID不一致或返回目标不符均拒绝并要求重新发现/观察，不回落到其他窗口。新目标成功观察前不允许动作。

DA6. 暂停/停止先撤 epoch，阻止新 dispatch，晚到目录/选择/观察不能发布。Pausing/Stopping 保持独占 lease，旧 owned 通道退出后才完成状态转换。继续可重新发现/观察所有窗口，无逐窗审批；旧目录和截图不恢复。read-only/Plan/普通 MCP Once/shell 权限不改变，外部 Agent 不获得新能力。

DA7. UI 删除桌面选窗授权按钮与相应事件，保留真实目标/全桌面范围、观察入口及接管/继续/停止。浏览器审批入口仍有效。设置文案明确模型可发现/切换所有桌面窗口、系统权限首次使用检查、后台输入由执行器能力决定。工具描述区分桌面定位与浏览器授权。

DA8. TCC 仍需系统授权。后台热键可能因同进程其他窗口或可见性被拒，继续显示精确 route/refusal，不启用前台 fallback，不绕过上游 gate。已派发 action 后的失败仍 outcome_unknown，禁止自动重试。全桌面授权不等于能够越过 macOS/目标应用能力限制，也不新增任意 shell/JS/脚本能力。

## 验收矩阵

| ID | 前置/操作 | 预期 | 层级 | 状态 |
|---|---|---|---|---|
| DA01 | 已启用桌面，Observe null，含多个应用窗口 | 返回真实有界目录，Running，无 AwaitingApproval，不自动选首窗 | runtime/control fixture | PASS（进程内） |
| DA02 | Observe 指定目录 target_id | 绑定该窗口并发布带图/AX观察，零UI审批 | runtime/adapter fixture | PASS（进程内） |
| DA03 | A观察后发现/选择B | A观察失效，重新发现时旧目录世代也失效；B须新观察，动作仅绑定B | state/adapter | PASS（进程内） |
| DA04 | 新窗口/窗口关闭/旧ID/目标不匹配 | 新发现可选择；旧ID/消失/错目标拒绝，无回落 | adapter fixture | PASS（进程内） |
| DA05 | 暂停→立即继续、清理→继续→发现 | 暂停中拒绝继续；清理后无重新授权；旧观察拒绝 | lifecycle/control | PASS（进程内） |
| DA06 | stop/cancel与晚到目录/观察竞争 | 零新增派发，旧结果不复活，独占释放保持 | control/mock transport | PASS（进程内） |
| DA07 | TCC缺失、组件/schema失败 | 明确有限失败；没有自动系统授权或UI假成功 | runtime fixture | PASS（进程内） |
| DA08 | 旧桌面grant时间超过2h/15m | 不再触发人工授权过期；浏览器寿命保持 | control | PASS（进程内） |
| DA09 | 目录/图像历史回读、压缩、重启 | 不保留标题正文/图像/操作引用，安全摘要合法 | runtime/conversation | PASS（进程内） |
| DA10 | 设置、控制条与工具schema | 桌面无grant按钮/旧措辞，PW行为与配置保持 | GPUI/schema + 静态审查 | PASS |
| DA11 | background拒绝/未知动作 | 新范围不绕上游gate，unknown不重试 | runtime fixture | PASS（进程内） |
| DA12 | 合并版真实多窗口发现、切换、接管恢复 | 用户手测，系统权限正确，指定窗口输入且不覆盖其他窗口 | 真实手测 | NOT RUN |

## 实施计划

1. 更新规格入口，先写目录/schema/自动连接与切换失效的失败回归并保留原始输出。
2. 在 runtime control/media 增加 live-only 目录观察和模型选择入口，移除桌面审批及人工授权期限，保留浏览器状态。
3. backend 使用标准模式与独立白名单，严格核验目录身份、动作 exact-window 与窗口观察；复用现有托管进程/TCC/取消收口。
4. 更新工具定义、设置和控制条，删除无效桌面授权事件，保留观测与生命周期UI。
5. 定向 nextest 覆盖 runtime/conversation/GPUI本卡用例，独立只读审查后提交；主控负责PR/云端/发布，真实验收由用户完成。

不新增依赖、不修改持久化DDL、不执行真实执行器自动测试、不安装/操作UI，不改Playwright方案。回滚可回退本轮提交；已有总能力开关可以停止后续运行使用桌面能力。

## 证据

基线 a90d71d93151b7a1ad684c7b345bf382d675743d；开始前 fetch/rebase 已确认同步。全部自动测试使用 fixture/mock/inprocess，不启动真实执行器、系统权限弹窗或浏览器；真实新版验收 DA12 保持 NOT RUN。

首次业务失败命令：

```text
cargo nextest run -p vega_runtime issue75_desktop_access
Summary [   0.022s] 2 tests run: 0 passed, 2 failed, 307 skipped
```

日志 /tmp/vega-75-desktop-access-red.log，SHA256 17ebcf8b457c33f8912749b33299df48dfaf998a1728580e5ba7176f34600320。旧实现缺少 target_id/cursor schema，发现流程进入 AwaitingApproval，两个新需求断言真实失败。

独立审查发现辅助进程窗口被应用 join 丢弃，先补回归保留失败：

```text
cargo nextest run -p vega_runtime issue75_desktop_access_discovers_helper_owned_windows
Summary [   0.018s] 1 test run: 0 passed, 1 failed, 313 skipped
```

日志 /tmp/vega-75-desktop-access-review-red.log，SHA256 68a9055913ff1c85980c0d4f739e5a5a74544c53d9d605568a04d749bf8873fa。修复保留合法 helper-owned pid/window_id 后纳入最终测试。实现过程旧 status.windows 测试编译失败记录在 /tmp/vega-75-desktop-access-first.log；改为观察/目标/能力均无副作用的等价断言，不把编译失败算作需求 RED。

最终命令与原始 footer：

```text
cargo nextest run -p vega_runtime -p vega_conversation -p vega issue75
Summary [   2.375s] 48 tests run: 48 passed, 1104 skipped
```

日志 /tmp/vega-75-desktop-access-final.log，SHA256 ff41251c2681befd4ceb490c06305a20d66cd725456da533d93f4eb73e47d315。未执行本地 workspace 全量门禁；修改 Rust 文件的 rustfmt --check 与 git diff --check 均通过，无新增代码注释。

主要覆盖：

- DA01–DA04：explicit_discovery_schema、discovery_does_not_require_approval、pages_all_windows_and_invalidates_cross_window_frames（450窗、第三页目标449）、standard_policy_and_discovery_contract、discovers_helper_owned_windows、missing_window_invalidates_without_fallback、catalog_cannot_dispatch_action。launcher 检查使用生产参数/path/env helper，policy.yaml 后缀与独立 allow.tools 均来自真实生产构建路径。
- DA05–DA08：takeover_waits_for_cleanup_and_keeps_single_lease、pages_all_windows_and_invalidates_cross_window_frames、has_no_window_grant_expiry、directory_expiry_size_and_failure_cleanup（61秒失效、201个中文长标题完整分页、每页≤64KiB、失败清目录）、permissions_require_explicit_valid_booleans，以及既有取消/窗口关闭/任务删除与 disabled/ReadOnly/Plan/普通 MCP Once 回归。
- DA09–DA11：directory_is_ephemeral_and_not_an_action_frame、观察缓存/历史 codec/provider 来源回归、GPUI 控制条与资源回收、background_keyboard_preflight_preserves_ax_routes、background_available_report_never_bypasses_fresh_executor_refusal、action_failure_keeps_unknown_and_does_not_restore_revoked_epoch。

两位独立只读审查者分别核验状态/生命周期和固定 Cua 上游合同。已修辅助进程窗口发现缺口、终态目录清理与描述准确性。上游 daemon 在 registry 调用前执行独立 policy，proxy 检查不替代 daemon；此为固定源码证据，并非本次真实执行器测试。

真机待测：更新后 TCC 归属、standard+policy 启动、发现其他应用/Space及辅助进程窗口、明确选择与跨窗口输入、接管恢复无窗口授权、关闭目标后的拒绝与重新发现。后台键盘在同进程多窗口下仍可能被上游拒绝，不承诺前台回落或任意光标定位。

## 目录应用身份修复（2026-10-12）

### 证据与范围

0.1.76 真机已完成桌面多窗口发现/观察、TextEdit type_text 和接管恢复无逐窗审批。后续用户指定 Codex 时，模型表示目录存在 ChatGPT 但没有名为 Codex 的应用。主控的独立应用列表返回 displayName=ChatGPT、id=com.openai.codex、isRunning=true。代码已证实 DesktopTarget 保留 bundle_id，而 catalog_page 投影仅有 application/title/pid/window_id/target_id；显示名与用户常用名称不同时，模型失去区分应用的稳定元数据。这是明确的通用信息缺口；没有读取此前原始目录，不宣称已经唯一证明每次失败都来自此处。

DI1. 每个目录项增加 bundle_id:string|null。上游已提供、非空且 UTF-8 长度不超过 1024 字节时原样投影；缺失或超长返回 null，不截断成另一身份、不虚构应用别名。内部完整 bundle_id 仍用于窗口身份比较。辅助进程缺少应用记录时保持合法 pid/window_id 与 app_name，不丢项。

DI2. bundle_id 计入既有每页 64 KiB 预算；分页、opaque target_id、60 秒时效、目标验证及停止/接管边界均不变。工具描述要求结合 application/title/bundle_id 识别目标；显示名可能与用户常用名不同，bundle_id=null 表示不可用，不能从显示名缺乏精确字符串匹配就断言应用不存在。所有元数据仍是不可信外部数据。

DI3. 目录只在当前运行有界保留；选择窗口后的目录观察淘汰、暂停/压缩/停止失效保持原合同。bundle_id 不进入新增持久化字段、日志或审计正文，不为了诊断延长内容寿命。原始页面、截图、输入和令牌均不收集。

DI4. 仅修通用目录元数据及工具说明，不修改 Cua 枚举规则、系统权限、后台输入、Playwright 或 UI 权限。真机只需通过 Vega 的目录结果确认显示名与 bundle_id 的对应；不需要观察或截图 Codex，也不得用其他执行通道绕过当前工具拒绝访问该应用的限制。

### 先行验收矩阵

| ID | 操作 | 预期 | 层级 | 状态 |
|---|---|---|---|---|
| DI01 | 上游显示 ChatGPT，bundle com.openai.codex，目标在第201项 | production parser/分页/discover/publish/apply 到 Chat Completions 和 Responses 请求均保留对应身份与同一 call_id | inprocess/codec | PASS |
| DI02 | helper 无应用记录、空 bundle、1024字节 bundle、1025字节不同后缀身份 | 空/过长返回 null，边界原样，内部身份不截断、不合并窗口；每页有界 | control/parser | PASS |
| DI03 | 目录选择窗口或暂停/压缩/运行结束 | 旧目录正文和 bundle 从模型消息/返回 outcome 清除，旧 ID 规则不变 | lifecycle/codec | PASS |
| DI04 | 发布后由 Vega 仅枚举目录 | 模型可同时报告显示名与真实 bundle_id，不用截图证明应用身份 | 真实手测 | PASS（0.1.77） |

计划：先保留 DI01/DI02 失败回归，再补目录字段和工具说明；定向 nextest 后独立只读审查，提交给主控做 PR/CI/发布。无需新依赖、DDL 或真实执行器自动测试。回滚可回退本轮提交。

### 本轮验证证据

主控在 0.1.76 新建仅目录诊断任务，模型列出多个 ChatGPT 窗口但 bundle_id 均为“未提供”，另将 ChatGPT Computer Use 单列。没有选窗、截图或动作。此真机 RED 与独立应用 inventory 的显示名/标识对应一致；不记录真实 PID、窗口 ID 或任务内容。后续 0.1.77 DI04 真机 PASS 见本节末尾。

首次自动 RED：

```text
cargo nextest run -p vega_runtime issue75_catalog_identity
Summary [   0.022s] 2 tests run: 0 passed, 2 failed, 316 skipped
```

原始日志 /tmp/vega-75-catalog-identity-red.log，SHA256 5f49c49c66caff91e1293db4cfaeb32117209d6474919a9793222f96517a5dcf。两项分别在实际请求正文缺少 bundle_id、目录缺少 1024 字节边界身份处失败，保留旧实现行为证据。

最终定向 GREEN：

```text
cargo nextest run -p vega_runtime issue75
Summary [   2.272s] 43 tests run: 43 passed, 275 skipped
```

原始日志 /tmp/vega-75-catalog-identity-green.log，SHA256 05853663efe3d0b9f1c954df1a7daa254e7bb49608853569ee33c6a66188e5a9。2026-10-12 09:19 UTC / 17:19 Asia/Shanghai，代码 diff SHA256 68c4cfd3f47c8e278e7c22ad09b1a2585fcc266c8c1db98e721549b3290c03b0。只跑本卡定向测试；修改文件 rustfmt --check、git diff --check 通过，无新增代码注释。主控与独立只读审查均未发现剩余阻断。

固定上游合同已独立复核：[应用显示名与 bundle 标识分别读取](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/platform-macos/src/apps/mod.rs#L876)，[窗口枚举及通用筛选](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/platform-macos/src/windows.rs#L72)。这些路径没有 Codex/OpenAI/宿主 PID 排除；本轮不修改上游行为。目录首次普通投递可到达模型；后续选窗、翻页或压缩淘汰目录仍遵守原合同，本轮不把这一保留限制改作历史持久化。

DI04 已在 0.1.77 通过真实目录验收：4 个窗口同时包含 application=ChatGPT 与 bundle_id=com.openai.codex，见 [Issue #75 真机记录](https://github.com/puzige/vega/issues/75#issuecomment-6121718891)。没有选窗或捕获该应用画面；后续窗口截图问题独立记录于 [观察修复规格](vega-issue-75-window-observation.md)。
