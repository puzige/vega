# Issue #75 — CUA 可用性、截图交付与对话内交互

## 来源、基线与覆盖关系

2026-10-13 用户要求同时修复：内置并自动加载 CUA skill、完善截图交付接口、加入无进展检测；继续推进至真机顺畅通过，不能仅以最终成功验收。最新明确裁决是完全移除底部独立操作菜单，所需确认在对话内展示，不能改成另一条常驻状态栏。基线为 0.1.79 / c6dfd9e，已有工作区同步至最新 master。

本规格覆盖 desktop-access DA7 的独立控制条展示、window-observation OF2 中对“用户明确要求交付的截图”一律不保存的旧限制，以及 OF6 的无上限恢复。普通观察仍为临时数据；系统权限、精确窗口身份、帧绑定、动作 freshness、取消/接管、未知动作不可重试、外部 skill 信任与现有任务权限均不放宽。Playwright 方案继续暂停，不调整其执行或权限契约。移除全局底栏后，现有浏览器待确认入口也须在对话内仍可到达。

## 已确认的坏例

仅根据本地安全审计元数据和有限错误进行统计，没有读取受限应用截图或捕获正文：一次截图请求耗时约 12.95 分钟，105 次工具调用、82 次模型轮次；ComputerObserve 含 21 次目录、4 次窗口观察成功、16 次 ax_window_unresolved、20 次 InvalidAction、2 次过期。成功的观察元数据没有记录目标，不据此认定哪次产出最终截图。后续请求约 30.32 分钟、134 次工具调用，其中 97 次 ComputerAct 均为 click（49 成功、39 InvalidAction、9 过期）。没有 skill 激活审计。不能据此断言单一模型能力不足。

确定缺口是：没有自动加载的专属操作指引；截图作为临时观察而非明确交付物；参数/只读/定位等失败混成泛化 InvalidAction；可恢复失败没有有界的无进展收口；只读观察也显示独立“正在操控”底栏。此前最终截图成功属于功能可达证据，不代表可用性验收通过。

## 产品与接口合同

U1. 内置可信 CUA skill 作为随应用发布的版本化资源，包含简明 SKILL.md 元数据和正文。Native 桌面工具实际暴露给模型时自动加载，不依赖用户另行安装、手动提及或批准此内置资源；对未启用桌面能力、工具不可用或外部 backend 不注入无效指引。生产运行能够核验激活来源、版本或内容摘要，不输出私密观察。不能给任意外部 skill 新增隐式信任。压缩与后续 provider 轮次保留必要合同且不反复累积同一正文。

U2. 指引与工具描述一致：目录发现 → 以当前 opaque target_id 选择明确窗口；请求截图交付时使用专用截图工具并向用户呈现产物，不能用 bash/脚本、反复点击、文件搜索代替现有截图能力。用户明确要求多个窗口时逐个交付，不反问无需用户判断的内部 window_id。动作使用最新可行动观察，区分 element 与坐标、AX 与 PID 键盘可用性；有限错误只执行提示的恢复步骤；未知动作结果、撤权和无进展必须停止。指引以实际 API 的最小成功例说明参数，不把常规 CUA 任务扩成强制审批流程。

U3. 增加简单、显式的 ComputerScreenshot 接口，目标仅来自当前目录的 target_id，不接受任意 PID/window_id、shell、输出绝对路径或隐式前台。该调用直接获得并交付目标窗口截图，不要求先建立可输入的 AX 树。优先复用既有 driver get_window_state 的 screenshot-only 能力；必须核对固定上游实现和证明精确 PID/window、有效帧、snapshot/capture 绑定、可解码且有界图像。任何身份、帧或权限冲突仍拒绝。AX 不可用本身不能等同于已验证截图不可用；只读截图不能产生任何 ComputerAct 授权。每次 capture 是一次明确工具执行，不自动换窗/重试。

U4. 截图交付是受控附件：在产生截图的对话工具结果/消息中直接显示可查看的图片，提供现有附件打开/保存能力；成功结果须给模型明确的产物引用与交付状态，不能只说“已观察”或让模型猜文件位置。仅显式 ComputerScreenshot 保存附件，ComputerObserve 与动作后观察继续临时、不得自动保存桌面历史。使用现有 owner-only 应用附件存储和消息关联路径；不允许模型指定任意写路径、不将 base64/窗口正文放入审计或普通日志。历史切换/重启仍可查看明确交付的截图，普通观察仍过期。复用既有附件策略和清理入口；写入/绑定失败如实失败，不产生虚假成功或悬空历史引用；取消/停止后的晚到结果不能发布。

U5. 将本轮可恢复参数失败拆成有限可机器读取的 code、可重试性、恢复动作，例如 invalid_arguments、coordinate_element_conflict、read_only_observation、stale_observation、target_expired，以及具体受限字段名（只允许 schema 字段枚举）。反馈不得带原始 text/url/窗口正文、上游原始异常或任意参数值。保留兼容的安全审计形状，历史卡片合法、不再变成“工具结果损坏”。身份/权限/协议错误仍按既有失败边界处理，不能通过错误细化放宽动作校验。

U6. 无进展检测在 runtime 执行，不只写 prompt。至少覆盖三种真实循环：同一窗口同类失败跨“重新发现目录”重复；反复相同非法参数；同一语义动作在没有可观测变化时重复成功。窗口身份使用内部稳定身份，不把刷新出的 opaque ID 当作进展；新目录、observation_id 或调用序号本身不算进展。连续三次同类失败触发 no_progress；连续三次相同动作且前后可比状态未改变触发 no_progress（比较须排除生成的引用/时间戳，不能把元数据变化当作内容变化）。有效变化和实际恢复能够清除对应计数，不误杀合理滚动或不同动作；所有计数 run-scoped、有界，不保留原始输入。

U7. no_progress 给出有限原因与下一步，并终止当前自动操作循环，不能继续大量模型轮次或换 shell 绕行。触发后无新动作派发，已派发但结果未知的动作绝不重试；新用户回合可重新开始。目录/失败交替、每次刷新引用和上下文压缩不能重置本轮 guard。停止、暂停、任务删除、切换窗口与迟到回调仍遵守既有 epoch/lease 合同。仅降低全局 turn_limit 或让 provider 自觉停止不满足本条。

U8. 完全删除 composer 之下/全局底部 CUA 独立操作栏及其空占位。普通观察与截图只使用既有对话工具记录/过程入口，文案区分观察、截图与实际输入，不显示“正在操控”假状态。权限缺失、实际审批、暂停恢复或无进展等必须用户处理的事项放在所属对话内，不藏在折叠过程里；已有任务停止按钮能够及时取消 CUA，接管/继续如需单独动作也放在对应对话状态内。桌面全窗口模式不恢复逐窗授权。浏览器待审批仍可操作，但不恢复其方案调研/真机测试。保留非终止接管：仅用户主动打开已有对话“+”菜单且当前回合 CUA Running 时提供“接管电脑操作”；不新增 Running 常驻提示，暂停后的继续入口在所属对话内，操作须匹配当前 run/epoch。

## 先行验收矩阵

| ID | 前置与操作 | 预期可观察结果 | 层级 | 初始状态 |
|---|---|---|---|---|
| U01 | Native/桌面启用、禁用、外部 backend 及压缩后发送 | 仅适用运行自动带内置指引，来源可核验，不累积、不改变外部 skill 信任 | 生产 provider/conversation fixture | PASS（进程内） |
| U02 | 普通观察和显式截图分别走同一 provider 会话 | 只有显式截图产出可查看附件；模型收到明确产物结果 | runtime + conversation/storage | PASS（进程内） |
| U03 | AX 空/无法解析而窗口截图具有完整身份绑定；反例身份/帧/图像冲突 | 前者只读交付成功且零动作；后者拒绝且无附件 | adapter fixture | PASS（进程内） |
| U04 | 错字段、冲突定位、只读、过期引用 | 有限 code/恢复建议，无私密值；新旧安全卡片正常 | runtime + history | PASS（进程内） |
| U05 | 相同失败三次，中间目录刷新/不同 opaque ID/压缩 | 第三次触发 no_progress，后续驱动与 provider 操作循环停止 | 完整 run_agent fixture | PASS（进程内） |
| U06 | 重复点击成功但相同可比状态；另有真实状态变化/合理滚动 | 无变化第三次停止；真实变化不误判 | production runtime fixture | PASS（进程内） |
| U07 | 截图写入失败、停止竞争、历史回读/清理 | 无虚假成功/晚到附件；显式截图历史可用，普通观察不持久化 | storage + controller | PASS（进程内） |
| U08 | 观察/输入/成功/权限/暂停/浏览器待批，宽窄窗口 | 无全局操作栏/占位；所属对话内必要操作可达，任务停止有效 | GPUI 生产状态与布局 | PASS（进程内） |
| U09 | 真机新建受控 TextEdit 请求截图，单窗与明确两窗 | 直接对话交付，单窗正常路径目录+截图约2次调用、最多3次，无 Act/bash，无无效重试；两窗最多5次；无额外菜单 | 真机 | 待新版 |
| U10 | 真机受控 TextEdit 输入并验证结果；停止/恢复 | 指定窗口状态实际变化，确认结果而非仅工具成功；无循环、停止后不输入 | 真机 | 待新版 |
| U11 | 用户本人重试工具禁止访问的原应用 | 记录调用数/错误/耗时及用户确认图片正确；代理只读安全元数据，不能转用 Vega/driver 读取该应用内容 | 用户手测 + 脱敏审计 | 待新版 |

性能验收以所有尝试为证，不只留最终成功。正常单窗截图的模型轮次目标不超过3、工具调用不超过3；耗时目标60秒内，分别记录模型等待和工具时间。若模型服务等待超过目标，保留实际时间并定位，不用编造结果或重试到绿。受控真机至少两次独立新请求，覆盖单窗/两窗交付与正常输入。错误触发可用进程内 fixture 精确验证，不能伪称已在真机注入相同故障。

## 实施与交付计划

唯一代码执行子 agent 负责 runtime、conversation、附件和 UI 的完整变更，主控负责规格、review、集成、云端与真机。先保留以上缺陷的有意义 RED，再实现；不新增依赖，复用既有附件类型/存储和事件流，UI 不直接写数据库。若现有截图 API/附件链与合同冲突，先提交证据修改规格，不掩盖限制。

本地仅本卡定向 nextest、修改文件格式与 diff 检查；全量 fmt/Clippy/Nextest 由 PR 云端 gate。通过后主控 squash merge，等待自动版本发布，验证包身份并在 canonical app 空闲时更新。真机只使用获准的受控窗口，保护用户草稿；系统验证由用户本人完成。不得用工具访问已被拒绝的应用，也不得从历史截图绕过。用户已经授权持续真机推进，不能以“子任务进行中”或仅云端成功收尾。

回滚：本轮新增 schema 18，旧二进制会拒绝更高版本数据库，不能直接替换为前一发布包。优先发布保留 schema 18/附件数据兼容性的修复版本；如必须恢复旧包，先保全升级后的完整数据库和附件，再使用更新前一致备份，明确保留升级后的新增数据以便恢复。不得删除新附件或改低 schema 版本绕过检查。本轮不处理 Apple 开发者签名凭据缺失，升级后的系统授权真实性单独核验；不把旧开关等同于新二进制已有权限。

## 本地实现与验收记录（2026-10-13）

实现基线为 c6dfd9e。内置资源为 `crates/vega_runtime/src/computer_use/vega-cua/SKILL.md`，version=1，SHA-256 为 `2b3f039ea4571b410b27e1931f9427657d5f913d8adf1ae1f4af51b84f40baf0`；每个适用运行将同一资源加入 base system prompt，并将名称、版本和摘要记入所属 assistant 消息的激活审计。正文不因 provider 下一轮或上下文重建反复追加。

固定上游为 cua-driver-rs-v0.34.0，get_window_state 源文件 Git blob 为 `e582f274726e296df09b65bbf8ac64b37f6814f4`。正式参数是 `include_accessibility_tree:false`，仍要求精确 PID/window、有效 screenshot frame、snapshot/capture 绑定和解码通过。新截图路径复用该参数并强制只读；普通 Observe 的 degraded 合同不变。

无进展的实际计数边界：同一物理窗口、同一操作和同类有限错误在第三次失败完成后收口；未解析目标按操作与有限 code/field 计数，无关参数或过期引用变化不重置。动作按实际映射到 driver 的语义参数计数，排除无用字段和生成的引用；第三次成功派发后，若前后 AX 与解码像素均无变化，则在结果返回边界收口，不再发起第四次动作或下一 provider 轮。该第三次动作已派发，反馈不写 not_dispatched。用户同时停止时仍以 Interrupted 为准。

显式产物以 call/message/thread 所有权和捕获 epoch 提交，通过持久化 ack 后才报告成功。每轮沿用 4 张/16 MiB，单张沿用 8 MiB；历史读取页上限仍是 32 MiB。历史可以展示和保存专属 assistant 截图，但不会在后续模型请求中重新发送这些图片。全局 CUA 栏及其占位已删除；正常截图只有对话工具过程与交付图片。“接管电脑操作”仅在用户主动打开现有 + 菜单时出现。

| 证据 | 结果 | 原始记录 |
|---|---|---|
| 初始 RED：缺截图工具、未自动加载 skill、非法调用未收口 | 3 项全部 FAIL；非法调用继续到第 4 轮 | `/tmp/vega-75-usability-red.log`；run 16b1640f-b8d7-440b-bdf8-b5e351fa69dc |
| 扰动参数 RED：同类未解析目标错误改变无用字段 | 2 项全部 FAIL；完整 run_agent 继续到第 4 轮 | `/tmp/vega-75-usability-guard-red.log`；run 4d3a703d-4505-4da6-bf8b-419b176be08a |
| 最终 issue75 与现有 composer Stop 定向矩阵 | 88/88 PASS，1786 项按范围跳过 | `/tmp/vega-75-usability-final.log`；run 20916614-3a9e-4557-b9a5-cad8aaf82d9c |
| 修改的 Rust 文件格式 | rustfmt 49 个修改/新增文件通过，未运行全仓格式改写 | `--edition 2024 --config skip_children=true` |
| 内置 skill 校验 | quick_validate: Skill is valid! | skill-creator validator，uv 临时 PyYAML 环境；无仓库依赖新增 |

最终原始日志 SHA-256：`eb45b89ee8e243d213b4c1453ffe33f251a6939967b5bf6fb1237604f5f3919a`。定向命令：

```sh
cargo nextest run -p vega_runtime -p vega_conversation -p vega_ui -p vega \
  -E 'test(issue75) | test(r11_composer_stream_stop_retains_partial_and_next_draft)'
```

本地生产路径 fixture 证明单窗目录+截图为 2 次工具/3 个 provider 轮次，两窗为 3 次工具/3 个轮次，均零输入；交替目录与 AX 失败在第三次失败结束；无变化点击第三次派发后停止，真实视口变化不误停。存储回归覆盖重复事件幂等、跨线程拒绝、注入写入失败、取消与旧 epoch、数据库重新打开、下一轮不回喂和删除清理。GPUI 回归实际渲染并点击折叠后的截图展开/保存、现有 + 菜单接管/对话继续、浏览器待批和任务停止；Ready 先于 MessageStarted 的生产 ingress 也保持本轮提示可达，后续新回合清除旧提示。

这些均为进程内受控 fixture，不代表真机通过。U09–U11、PR 云端全量门禁和发布后真机耗时/正确性，由主控继续执行和补充。未读取受限应用捕获内容，未用真实 driver 代替用户测试。

### PR #320 首次云端反馈修复

HEAD 98aa8aba 的 Clippy 在 `Backends::observe` 报 8/7 参数；原始诊断保留于 `/tmp/vega-75-usability-ci-clippy-first.log`。将观察选项归为 `CaptureMode::Observe { include_image }` 和 `CaptureMode::Screenshot`，截图模式固定携带图片且只读，避免两个独立布尔值形成无意义组合；参数恢复 7 个，不添加 lint allow 或改动门禁。

受影响 runtime 回归 `cargo nextest run -p vega_runtime -E 'test(issue75)'`：69/69 PASS，275 项按范围跳过；run `d848b231-9886-4fe2-b15d-4d9d4c526f21`，原始输出 `/tmp/vega-75-usability-ci-fix-tests.log`，SHA-256 `8043daf8f3314757db73edf6c9314a9bbc80d01d881b167d21fcc409f84803e6`。仅修改的 backend.rs 已通过 rustfmt 与 diff 检查；云端 Clippy 是否通过须以新提交的 PR gate 为准。

针对已确认云端诊断，额外复验受影响包：`cargo clippy -p vega_runtime --all-targets -- -D warnings` 通过，exit 0；原始记录 `/tmp/vega-75-usability-ci-fix-clippy.log`。未运行本地 workspace 全量门禁。

### PR #320 全量 Nextest 反馈修复

HEAD 9ed8f9f 的云端 Nextest 为 2334 项中 2315 PASS、19 FAIL（`/tmp/vega-75-usability-ci-nextest.log`）。其中 13 项是 schema 精确期望仍为 17/29 表，现更新为 18/31 并在完整表集合加入 builtin_skill_activations、computer_screenshot_artifacts；旧数据、WAL、幂等迁移、外键和生命周期断言全部保留。另 5 项由新增的全局取消状态覆盖导致：审批前已有 Rejected 被写成 Cancelled，破坏其审批审计和持久化前置。修复仅对成功 ComputerScreenshot 在交付前取消时标记 Cancelled，恢复普通工具的原拒绝/取消合同，不改变原断言。

第 19 项 `issue67_concurrent_settings_fails_closed_for_both_permissions` 的云端失败发生在 B 工具提案前；未修改的 9ed8f9f 单独运行已 PASS（1.204s，`/tmp/vega-75-usability-issue67-red.log`，文件名不代表其结果为 RED），本轮组合中也 PASS（1.495s）。其根因尚未确认，不能称为被本轮修好；没有添加延时、放宽断言、降低并发或自动重试，下一轮云端继续执行原用例，若重现则继续定位。

本地仅执行这 19 个失败用例（更新名称的迁移测试按新名称选取）、issue75 与已有 composer Stop 的组合：108/108 PASS，1923 项按范围跳过；run `09bf8279-fa71-4dca-a876-278aeb63a59c`。原始输出 `/tmp/vega-75-usability-ci-regressions.log`，SHA-256 `00a1cea82d93b4037b5263427ee2f39da91499ea4e70acaeeed1edc88ff92336`；完整 executable/cwd/argv 保存在 `/tmp/vega-75-usability-ci-regressions-argv.json`，过滤表达式保存在 `/tmp/vega-75-usability-ci-regressions-filter.txt`。

受影响 runtime 的 `cargo clippy -p vega_runtime --all-targets -- -D warnings` 诊断复验 exit 0（`/tmp/vega-75-usability-ci-regressions-clippy.log`），6 个修改 Rust 文件的格式和 diff 检查通过。此次没有运行本地 workspace 全量测试；最终云端结果与真机结果仍待主控执行。

## 只读权限分层修复（2026-10-13）

0.1.81 真机首次单窗口截图在首个 ComputerObserve 的权限预检被拒。源码确认桌面连接统一要求辅助功能（AX）和屏幕录制（SR），导致 AX=false/SR=true 也无法走已实现的显式只读截图路径；错误聚合后无法判断具体缺哪项，不能据此声称真机一定缺 AX。固定 `check_permissions` 源码的顶层 `accessibility`、`screen_recording` 均为布尔值，与 Vega 解析一致；错误不是字段名不匹配。固定 Cua 0.34 截图的 include_accessibility_tree=false 跳过 AX；目录只对特定无标题隐藏辅助窗口尝试 AX 过滤，AX 无权或不可读时保留，不构成强制 AX 前提。

RP1. 桌面建链与操作权限分开；目录（含分页）和 ComputerScreenshot 只要求 SR=true，仍严格解析两个权限布尔值，协议异常不能当授权成功。普通目标 ComputerObserve 与所有 ComputerAct 继续要求 AX=true 且 SR=true，不放宽现有 degraded、后台输入及 PID/window/frame/snapshot/capture 校验。显式截图保持只读，不产生输入授权。

RP2. 授权入口按实际工具/参数检查本次所需权限；执行前刷新检查，所有输入在目标复核后、派发前再次确认 AX/SR，不能借旧 observation 或已连接状态绕过撤权。取消/epoch 变化优先，权限刷新不得发布迟到截图或派发新输入。授权失败仍 Rejected；已派发动作保持 outcome_unknown，不能自动重试。

RP3. 保留 AX 缺失、SR 缺失、两项缺失的有限分类。对话默认只显示简明中文和实际所需的设置入口；executor/stage/error_class 在用户主动展开的诊断详情显示，Enter/Space 和焦点可达。不新增底栏/常驻操作提示；新回合或新错误收起旧详情。只报告本次所需且缺失的权限，不要求仅截图的用户开启 AX。设置中的初始说明同步按用途分层，不将组件检测声称为实时权限状态。

RP4. 授权中的 CUA 失败使用既有 ComputerUse + Deny 审计；仅增加现有 codec/工具终态对该组合的合法性，仍拒绝 Always、note、danger 及非 CUA 混用。Plan/ReadOnly 的原 run_mode 拒绝语义、Rejected 生命周期和 schema18 不变。无新依赖、无签名重构；Apple Developer ID 路径既有，当前 ad-hoc 升级后的 TCC 有效性由真机单独核验。

| ID | 前置/操作 | 预期与自动证据 | 状态 |
|---|---|---|---|
| RP01 | AX=false/SR=true，production run_thread 目录→显式截图 | 两次工具产生持久附件，重开可见；模型知道已交付，无输入/AX 请求 | PASS（进程内） |
| RP02 | AX=false/SR=true，普通 Observe 或持有旧可行动观察的 Act | 派发前拒绝，有限 AX 指引，0 输入 | PASS（进程内） |
| RP03 | SR=false 或两项 false、权限字段类型错误 | 目录/截图拒绝，正确 SR/两项/协议分类，不存附件 | PASS（进程内） |
| RP04 | 授权后撤权、连接已缓存、停止/epoch 变化 | 重新检查，不派发输入/不发布晚到附件，保留取消终态 | PASS（进程内） |
| RP05 | 单项/两项缺失对话卡 | 简明中文、对应设置入口、详情默认收起并可键盘展开，新回合清理 | PASS（进程内） |
| RP06 | CUA Deny 审计持久化与历史重载 | Rejected 合法、不误标 run_mode；非 CUA/Always/note/danger 仍拒绝 | PASS（进程内） |

实施顺序：先补本表与失败用例；复用既有 test-support 外部执行 fixture，让权限响应进入生产解析和授权路径；实现分层预检/有限诊断/对话展开；只跑本卡 runtime、conversation、UI 定向 nextest。完整命令与首次失败/最终成功日志保留在临时证据目录；真实 driver/系统权限/应用安装由主控验收，不以进程内替身宣称真机通过。

### 本轮实现与原始证据

实现从 a47abb0 开始，提交前 fetch 发现主线新增 #321 后无冲突同步至 5947e042；原始 a47abb0 结果独立保留。测试替身仅启用既有 test-support，替换驱动边界，权限响应仍经过 production 解析、授权、执行及 durable storage；正式构建无 fixture 入口。本轮未新增 schema、依赖或内置 skill 修改。

权限检查按实际 PreparedRuntimeCall 参数判定，不读取已脱敏的工具审计 JSON。目录/分页与显式截图只要求 SR；普通目标 Observe 和所有输入要求 AX/SR。已有连接也刷新检查，输入在目标复核后刷新，实际派发前仍检查 epoch。截图路径保留现有身份、帧、图像上限和只读校验；授权入口的权限失败仍为 Rejected，只有授权入口真实 CUA 失败使用既有 ComputerUse + Deny；Plan/ReadOnly 保持 RunMode + Deny。

| 阶段 | 结果 | 原始记录 |
|---|---|---|
| RED，新增 production 目录→截图→存储用例 | 0 PASS / 1 FAIL；预期 3 个 provider 轮次，旧统一预检在第 1 轮终止 | `/tmp/vega-75-permissions-red.log`，run `4fa9ce24-2909-4285-9ee8-bba55f2cac41` |
| 首轮编译及接线修正 | first/second 修正新测试的历史 projection、方法可见性和焦点借用；third 修正接线时对已有 arguments 的多余解析；fifth 修正新取消测试对事件终态的访问 | `/tmp/vega-75-permissions-{first,second,third,fifth}.log` |
| 首轮可执行矩阵 | 89 PASS / 2 FAIL；新存储测试误用 SQL 列名、UI 测试缺少上一消息结束事件，修正实际前置；未删除或放宽断言 | `/tmp/vega-75-permissions-fourth.log`，run `4f6b208f-a25e-4c7b-a0a4-845cacb6edde` |
| a47abb0 最终定向矩阵 | 105/105 PASS，1797 项按范围跳过 | `/tmp/vega-75-permissions-final.log`，run `62bf642d-e8d1-4133-9f50-df7286dee3af` |
| 同步 5947e042 后同一矩阵 | 105/105 PASS，1806 项按范围跳过 | `/tmp/vega-75-permissions-rebased.log`，run `512d94cc-e76c-4a4b-8e96-fc179134011a` |

RED 日志 SHA-256：`30681855a6c36a33249125d2a663a3f9df069f6c9f49c6e45fe130a283621cbb`。a47abb0 最终日志 SHA-256：`88ea329a62e5a22d31db53355bb8e79346e45997c03ce1123b95c2c2573eacd8`。完整 executable/cwd/argv 分别保存在 `/tmp/vega-75-permissions-red-argv.json`、`/tmp/vega-75-permissions-first-argv.json`（中间三包矩阵）、`/tmp/vega-75-permissions-final-argv.json`。

最终定向命令：

```sh
cargo nextest run -p vega_runtime -p vega_conversation -p vega_ui -p vega \
  -E 'test(issue75) | test(types::tests_audit_wire)'
```

新增回归证明：AX=false/SR=true 在完整 run_thread 中 2 次工具/3 个 provider 轮次交付持久截图，历史重开可见；5 种输入 × 3 种撤权均零派发；缓存连接后撤权仍拒绝；截图批准后立即取消以 Interrupted 结束且无捕获/附件。缺 AX、缺 SR、两项缺失及畸形字段分别覆盖，审计可持久化并正常重载。GPUI 实际渲染验证对应设置入口、详情默认收起、Enter/Space 和点击展开、新回合清理及旧回调无效。普通观察、截图交付、无进展、composer 停止和无全局栏的既有 issue75 回归同时通过。

13 个修改的 Rust 文件使用 `rustfmt --edition 2024 --config skip_children=true --check`，以及 `git diff --check` 均通过。本次尚无云端 Clippy 诊断，未运行本地 workspace 全量门禁。新权限分层的真机结果、PR 云端门禁和发布由主控继续验证，以上进程内结果不代替真机验收；未调用真实 driver、修改 live DB 或访问受限应用内容。

同步主线后的最终日志 SHA-256：`b506ed9ce09dea7c2b008983b7afba113e9127d834761fde04965ee5ad56ed02`，完整 executable/cwd/argv 为 `/tmp/vega-75-permissions-rebased-argv.json`；所有本轮原始日志摘要及 hash 另存 `/tmp/vega-75-permissions-evidence.json`。#321 进入同一会话组件后无冲突，合并后的权限、截图布局和 lifecycle 定向回归通过。

## 成功观察循环与动作后截图引用修复（2026-10-13）

0.1.81 受控真机 input-01：142241 ms 后用户授权的主控停止；10 次工具（9 Observe + 1 type_text）全部 success，输入只发生一次，0 Screenshot/0 附件。动作后目录与目标观察交替，引用变动但没有交付。安全元数据单独保留；本节不读取实时数据库或捕获内容。

源码发现两个独立缺口：Act 派发前的 discard_observations 同时清空目录和 TTL，使动作后原 target_id 无法截图；普通/动作观察回执无 target_id，且模型中的旧目录和 assistant 工具参数已分别失效/脱敏，模型难以恢复引用。动作后完整观察本应通过 apply_observation 进入模型，需 production 请求证实，不能以持久摘要推断丢失。既有 guard 只统计失败及无变化 Act，全 success 的目录/观察循环没有计数。

SP1. 动作执行前仅消费旧可行动观察；保留已存在、未过 60 秒 TTL 的目录及其原身份，不更新生成时间。暂停、停止、压缩/显式失效继续全量清理。普通窗口及动作回执中的截图目标只来自同 epoch、Running、未撤权且当前有效目录中精确匹配的 target_id；过期/被清理则只提示 refresh_directory_once，不发新引用、不延长 TTL，不接受从 observation_id 越权构造截图目标。模型下一轮实际收到动作后图像/AX 内容及该引用，旧观察继续按原合同失效。

SP2. 成功 ComputerObserve 同样受到 runtime 无进展约束。窗口按 PID/window 身份计数，目录页按页内稳定窗口身份集合计数，忽略新 target_id、element_token、snapshot/capture/observation ID、游标和时戳。第一次读取建立基线；同一语义内容第 3 次成功读取触发 no_progress，并复用既有 run_loop 终止，不发下一 provider 轮。新目标、不同目录页、新目录内容及有意义窗口变化分别推进自己的基线。目录刷新、不同目标间切换和上下文压缩不能抹除其他目标已有重复计数；最近至多 64 个读目标采用有界淘汰，不把正常第 65 个新目标误判无进展。

SP3. 窗口读状态比较保留稳定 AX 字段（包括 value/selected/frame）及解码图像。仅当 AX 中存在 AXTextArea/AXTextField/AXSearchField、AX 内容完全不变、图像尺寸相同，才将至多 4 个相邻像素列的变化视作光标闪烁，不重置读取计数；不忽略更宽画面变化、任何 AX 内容变化或无文本控件的视觉变化。以列哈希保存有界比较状态，不缓存额外原始截图/文本。算例：800×600 图像中 2 列×20 像素的 caret 变化，列跨度 2≤4，被忽略；视口滚动改变 480 列，480>4，计为进展；哪怕只 1 列像素改变，AX value 变化仍计为进展。既有 Act 的无变化比较保持严格像素规则，不能借本条放宽动作校验。

SP4. 有可比状态变化的成功 Act 重建普通成功读取基线，并只清除本窗口的重复截图计数；其他窗口的重复截图计数保持。以动作返回的新状态作为当前窗口基线（0 次额外读取）；仅 action success 字样不算变化。显式 Screenshot 产生新交付是进展，不计为冗余观察；原附件 ack/取消合同保留。目录刷新/成功 Observe 不清其他操作的失败计数；同操作实际恢复沿用既有合同。skill 升为 v2：动作已经返回验证后的新状态，完成输入且需要图片时使用返回的当前 target_id 直接 Screenshot；只有明确 expired/missing 引用才刷新目录一次，不为验证同一状态反复发现窗口。不单靠 skill 限制循环。

| ID | 生产前置与操作 | 预期 | 状态 |
|---|---|---|---|
| SP01 | 目录→目标 Observe→type_text→Screenshot | 下一 provider 看见输入后的图像/AX及有效 target_id；4 tools/5 rounds，1 次输入、1 附件，无额外目录 | PASS（进程内） |
| SP02 | 动作后目录↔Observe，opaqueID/elementToken 每轮变化，语义内容稳定/光标闪烁 | 第 3 次重复成功观察收口，后续 provider/driver 不再运行 | PASS（进程内） |
| SP03 | 合法多页、不同窗口及真实 AX/视口变化 | 能继续读完整页和目标；真实进展不误停 | PASS（进程内） |
| SP04 | 已有引用 TTL 到期/全量失效/暂停 | 模型不给已过期截图 target_id；不得绕过现有 scope/epoch/取消 | PASS（进程内） |
| SP05 | 同类失败/动作无进展、截图 ack、权限分层原回归 | 原合同保持，skill v2 内容摘要审计有效 | PASS（进程内） |

先保留 production RED，再修改引用路径、guard 和 skill；本地仍只跑 issue75 及受影响审计定向组合。真实输入、像素和新版顺畅程度由主控发布后复测，不能仅凭自动最终成功判通过。

SP6. 已交付截图的 acknowledgement 与临时观察分离。截图回执当前以前置交付 JSON 加 observation 正文构造，不能只按 starts_with External 清理：只对本 run 经 durable event ack 确认交付的截图 call，在观察失效时保留同一有限交付 JSON，移除失效正文与 pixels。已交付 Screenshot 不附带 computer_screenshot 下一步指引；当前截图目标仅随普通 Observe/Act 返回。正常临时观察仍整体替换为失效说明，其他工具不受影响；后续 provider 不因暂停/继续丢失既有附件交付事实。进程内补充“2 张已持久交付→最后 provider 阶段暂停→继续→确认已交付”用例，预期 3 tools/2 artifacts，恢复后的请求保留两个 ack 且不含旧观察图像。takeover-01 的 1 ms unknown_safe_failure 已由 production 源码确认：旧 Screenshot JSON 前缀使 trim 漏清图片，两批截图累积 4 张 tool image，OpenAiProvider 的 validate_messages 在网络前按 2 张工具观察预算拒绝。回归须覆盖 2+2 捕获间的 pause/cleanup/resume，保留 4 条 ack、清理旧 pixels、完整工具配对，并通过实际 validate_messages、Chat Completions 与 Responses 请求构造；4 张未清理图片作为拒绝对照。MockProvider 自身不校验该预算，单独使用它不能证明此边界。


SP7. 对话既有安全工具卡按实际操作使用“观察”“截图”“操控”：ComputerObserve/BrowserObserve 不展示操控授权，ComputerScreenshot 不展示操控授权，Act 保持既有审批文案。只调整展示映射，不更改审计字段、权限或 Rejected 生命周期，不新增操作栏。

### 本轮结果与失败保留

SP01–SP05 已由 production run_agent 请求和原有跨层矩阵验证。SP06 的两个时序（最终 provider 前暂停、最终流中暂停）均保留 2 个交付 ack，3 tools/2 artifacts，无重截；额外 2+2 捕获夹具保留 4 个 ack，旧画面清除，最终只有最新窗口 1 张有效工具观察图，满足原 2 张预算。全部实际模型请求通过 production validate_messages；最终请求通过 Chat Completions/Responses 构造、6 对工具调用/回执一致，手动恢复全部 4 张旧图的对照被精确拒绝为 tool observation image budget exceeded。此证据是生产图片校验与两种请求构造，没有调用 OpenAiProvider::chat_stream 或 HTTP transport；真实 provider 由主控新版本验收。SP07 的 Running/Success/Rejected、桌面/浏览器只读及输入标题均通过。

截图缓存现有 capture identity 增加 run 内 delivered 状态，必须等 ScreenshotDelivered sink 成功后按 call/原 capture epoch 标记；未知 call/epoch 不可标记。trim 仅为已 ack 的截图恢复交付 JSON，未确认 capture 在失效后只能变为失效说明。原 sink 失败、取消/epoch、持久化/历史显示和截图不回喂历史模型用例继续通过。

| 阶段 | 实际结果与原因 | 原始日志 |
|---|---|---|
| 首次 RED | 3 项全部失败：动作后 target 失效、全成功目录/窗口循环未停、光标变化绕过重复读检测 | `/tmp/vega-75-successful-reads-red.log` |
| 暂停回执 RED | 1 项失败：失效截图仍保留 External 正文和 pixels | `/tmp/vega-75-successful-reads-pause-red.log` |
| 首轮修复 | 4/4 PASS | `/tmp/vega-75-successful-reads-first.log` |
| 新增边界与只读标题 | 7 PASS / 3 FAIL；标题是真实 RED，另两项为新夹具多转义 JSON、误把有效图数写死为 2（实际最新窗口 1 图） | `/tmp/vega-75-successful-reads-second.log` |
| 上述修正 | 10/10 PASS；TTL 改解析真实 JSON，图像断言严格为最新 1 图，4 图拒绝对照保留 | `/tmp/vega-75-successful-reads-third.log` |
| 首次跨层 | 114 PASS / 1 FAIL；新成功读 guard 在第 5 轮的第 3 次相同目录提前停止，旧 AX 失败夹具原期望第 6 轮 | `/tmp/vega-75-successful-reads-final.log` |
| 区分两种 guard | 115/115 PASS；旧夹具每次目录有实际 title 更新，保留严格 6 轮、3 次同一物理窗口 AX 失败、取消优先及禁止后续 Bash，未放宽次数 | `/tmp/vega-75-successful-reads-complete.log` |
| 已交付指引 RED | 1 项失败：已交付截图仍附 computer_screenshot 下一步 | `/tmp/vega-75-successful-reads-delivered-guidance-red.log` |
| 最终同范围复验 | 115/115 PASS，1806 项按范围跳过 | `/tmp/vega-75-successful-reads-verified.log` |

最终 run ID 为 `22707bec-e2d9-4263-aa28-3e6a80697d97`，日志 SHA-256 为 `62d1991f86c3fbe70d3c591e1eb7ff98ce13b5fa0b9860f3ffdb21c284374ec6`。完整 executable/cwd/argv 保存在 `/tmp/vega-75-successful-reads-verified-argv.json`，命令为：

```sh
cargo nextest run -p vega_runtime -p vega_conversation -p vega_ui -p vega \
  -E 'test(issue75) | test(types::tests_audit_wire)'
```

11 个修改 Rust 文件的 rustfmt 检查和 git diff --check 通过，命令与退出码为 `/tmp/vega-75-successful-reads-checks.log`。新 TTL 测试首次 rustfmt 的转义语法失败单独保存在 `/tmp/vega-75-successful-reads-fmt-initial.log`。本地 Python 缺 PyYAML 的首次 quick_validate 环境失败保留；随后用 uv 的独立环境执行同一校验器通过，日志为 `/tmp/vega-75-successful-reads-skill-validation-final.log`，没有增加项目依赖。

内置 skill v2 SHA-256 为 `ba27d178bc85735e9f237a3bac4dd08bb81e9c14e5ceabcd0a274f015d3fb094`。全部本轮日志摘要、hash 和最终 argv 汇总在 `/tmp/vega-75-successful-reads-evidence.json`。提交前 fetch 确認 master 仍是 `5947e042`；保留权限修复 `58ee2d6`，此追加未新增 schema 或依赖。真机 input-01/takeover-01 失败仍为待新版复测状态，进程内成功不代替发布后的顺畅验收。

固定提交审查发现 ComputerObserve schema 仍有“any action dispatch 后目录失效”的旧说明，与 SP1 不一致；只将它修正为动作保留原始 60 秒期限、仅消费可行动观察，pause/takeover、stop、compaction 和显式 invalidation 作废目录。现有 discovery schema 合同加入正反断言，先 RED（1 FAIL，`/tmp/vega-75-target-description-red.log`），修正后受影响 runtime issue75 用例 84/84 PASS、275 skipped（run `22fadbe7-fb90-410f-bf81-95d2d849d70b`，`/tmp/vega-75-target-description-verified.log`，SHA-256 `0a46b99b66caf21fd08a49b7ec7578d64cc7be8cd4d2b2fda49c8315057de768`）。命令为 `cargo nextest run -p vega_runtime -E 'test(issue75)'`，完整 argv 为 `/tmp/vega-75-target-description-verified-argv.json`；前述 115 项跨层结果独立保留。

## 截图卡片与预览优化（2026-10-13 用户追加）

用户静态截图显示：白底截图与聊天背景融合，图片盒子占据会话全宽，底部两枚文字按钮松散。按当前设计守则，仅改已交付截图的呈现与操作，不改变捕获/持久化/模型上下文及原图 bytes，不裁白边、不新增依赖或 schema。

SV1. 缩略图基于附件 width/height 同步确定几何：原尺寸等比缩小到最大 360×240，不放大；图片区域最小 64×32，极端图居中 contain。外层只包实际图片区域和 32px 紧凑顶栏，1px border_subtle、8px 圆角，不用会话 w_full 作为图片宽度；容器不足时按可用宽度同比收缩图片区域。图片保存为顶栏右侧 28×28、16px 共享图标操作，默认可见且与像素区域不重叠。顶栏可用原始尺寸作简短元数据，不保留独立文字操作行。共享数值进入 Layout。
算例：4000×80→360×7.2，图片区域360×32；400×8000→12×240，图片区域64×240；1200×800→360×240；16×8→16×8原始像素，居于64×32命中区，Img 自身最大尺寸不超过原尺寸。最小图片区域使极端图仍有独立可点击/聚焦目标，按钮不遮挡图片。小幅留边仅来自有界命中尺寸，不修改原图。

SV2. 异步图像准备前用同一 width/height 预留尺寸并显示轻量“加载中”状态；准备后仅替换内容、不改行高。图片区域默认可聚焦，点击或 Enter/Space 打开预览，hover/focus 有反馈及可访问名称/tooltip，不能依赖 hover 才能发现入口。保存使用已有原生路径选择器，复用 busy 防重入、取消不报错、失败/成功就近反馈。反馈替换固定顶栏内的尺寸标签，窄卡片以状态图标和完整 tooltip 表达，不新增底部状态行，保存不改变对话行高。

SV3. 预览挂所属 ConversationStream 根节点，deferred priority 3，与列表测量分离。使用主题容器，最大960×720、四周24安全边距；顶部紧凑保存/关闭图标，主区完整 contain 图片。Esc、关闭图标或背景点击关闭，回焦原缩略图；Tab/Shift-Tab 限于预览操作，背景滚动/误点被阻断。会话切换或所属卡片离开当前历史窗口时关闭并使用会话焦点，不残留旧图片，不推动聊天行高/滚动锚点。多张截图依原交付顺序纵向紧凑排列，预览/保存始终绑定该卡原图；不做轮播或图片管理器。

| ID | 生产前置/操作 | 预期 | 状态 |
|---|---|---|---|
| SV01 | 宽/窄容器，横/竖/极端比例，解码前后 | 实际GPUI边界受限、原比例/全内容、加载前后同几何，按钮不侵入图片 | PASS（进程内） |
| SV02 | 浅/深主题绘制 | 实际painted quad使用border_subtle，白底图片边界可辨，hover/focus几何不变 | PASS（进程内） |
| SV03 | 图片点击/Enter/Space打开、Tab/Esc/外部点击关闭 | 根级预览命中、焦点限制与恢复，列表高度/阅读锚点不动、背景不滚动/误点 | PASS（进程内） |
| SV04 | 卡片/预览保存，busy/取消/失败/成功，多图 | 原生选择器、一次pending、精确原图bytes，各图不串，状态就近 | PASS（进程内） |
| SV05 | 历史重读、折叠过程、移除所属卡片/切换会话 | 图片继续可见且操作可达、交付幂等，旧预览消失 | PASS（进程内） |

实施顺序：先用现有GPUI测试暴露全宽盒及缺预览问题，再改布局token/卡片/根层挂载，跑本卡相关UI和原存储历史矩阵。真实像素外观、TextEdit单/双截图与本机保存由主控新版验证，进程内几何/行为不冒充真机。

### 截图呈现结果与失败保留

SV01–SV05 的进程内验收通过：1100/360/240px 可用窗口、浅深两主题、横/竖/极端横长/极端竖长/16×8 小图均有真实 GPUI 布局和 painted quad 断言；缩略图始终完整 contain，16×8 不放大，最小命中区仍64×32。异步准备和 GPUI 解码均有加载占位，准备前后卡片与图片区域几何相同。白底图片边框与不透明图上的 hover/键盘焦点边框可观测；图片自身下圆角和预览顶栏自身上圆角分别断言，不依赖父 overflow_hidden。

预览点击、Enter/Space 打开，Tab/Shift-Tab 陷获，Esc/关闭/外部点击回焦均通过。背景滚轮被阻断，相同滚轮在关闭后能移动列表；覆盖 composer“+”的点击不会穿透，相同点击在关闭后可打开原菜单。真实回归发现 mouse-down 提前关闭导致后续 mouse-up 穿透，现已改为消费 mouse-up 后关闭。存储历史图片仍在折叠过程之外可见并幂等；原生保存覆盖卡片/预览键盘入口、busy、防重入、取消、成功、拒绝覆盖旧文件及两张图各自精确原图 bytes。保存状态固定在顶栏，不改变聊天行高；历史窗口替换和会话切换均不残留旧预览。

| 阶段 | 实际结果与原因 | 原始日志 |
|---|---|---|
| 初始 RED | 0 PASS / 2 FAIL：卡片宽736、图片点击无根预览 | `/tmp/vega-75-screenshot-ui-red.log` |
| 初次编译 | focus() 缺少 App 参数，按固定 GPUI API 修正 | `/tmp/vega-75-screenshot-ui-first.log` |
| 首轮界面 | 2/2 PASS | `/tmp/vega-75-screenshot-ui-second.log` |
| 扩展夹具编译 | 使用了私有菜单字段/不存在的列表方法；改成真实按钮和既有 scroll_to_end | `/tmp/vega-75-screenshot-ui-matrix-first.log` |
| 扩展夹具第1轮 | 5 PASS / 1 FAIL：点击真实背景按规格会关闭，原夹具错误要求仍打开；保留背景误点断言并改为实际“+” | `/tmp/vega-75-screenshot-ui-matrix-second.log` |
| 点击穿透 RED | 5 PASS / 1 FAIL：预览关闭后 mouse-up 打开背后的“+”菜单 | `/tmp/vega-75-screenshot-ui-matrix-third.log` |
| 消费完整点击修复 | 6/6 PASS | `/tmp/vega-75-screenshot-ui-matrix-fourth.log` |
| 圆角/解码与键盘 | 7/7 PASS | `/tmp/vega-75-screenshot-ui-matrix-fifth.log` |
| 跨层矩阵 | 121/121 PASS，1806 skipped | `/tmp/vega-75-screenshot-ui-final.log` |
| 小图/保存布局 RED | 0 PASS / 2 FAIL：16×8被放大到64×32；保存后行高274→301.5 | `/tmp/vega-75-screenshot-ui-small-and-save-red.log` |
| 对应最终 UI 复验 | 14/14 PASS，710 skipped | `/tmp/vega-75-screenshot-ui-small-and-save-verified.log` |

初始 RED SHA-256：`50b8e52fd71a114db02d98fd22d0fc7836ef35b14eae91e0c4f8c7b58c053e24`；小图/保存 RED SHA-256：`71aab5ed14394965b61142922661a7b97ab533738ced77ca0c054807b20dc4d4`。121 项跨层日志 SHA-256：`7dfe8a8919e080f21e9104db4d7d20ce3d8daf388cf44456de7d58fb072e25eb`；最终14项 UI 日志 SHA-256：`f68ee97468f46c60231a4b202ea6b6cf0844c1fbfcbe3ad5f54936118ba0b980`。全部原始记录、run ID/摘要和 hash 保存在 `/tmp/vega-75-screenshot-ui-evidence.json`，完整命令在 `/tmp/vega-75-screenshot-ui-final-argv.json` 与 `/tmp/vega-75-screenshot-ui-small-and-save-verified-argv.json`：

```sh
cargo nextest run -p vega_runtime -p vega_conversation -p vega_ui -p vega \
  -E 'test(issue75) | test(types::tests_audit_wire)'
cargo nextest run -p vega_ui -E 'test(issue75)'
```

7 个修改的 Rust 文件 rustfmt 检查与 git diff --check 通过（`/tmp/vega-75-screenshot-ui-checks.log`）。没有新 schema/依赖，没有裁剪或改写交付原图，五张纯白 PNG 是受控尺寸测试 fixture。以上为进程内 GPUI 行为/几何/绘制数据和存储证据；真实像素、系统原生保存面板及新版 TextEdit 流程仍由主控发布后验证。

提交后无冲突变基至 `f1cf5ee7c2b47c8e36a0d24385b5fb4020df969f`（#322 工具操作图标）。最终补测原本卡矩阵及上游实际修改的 6 个图标/分组/历史用例，共 **127/127 PASS**、1801 skipped，run `d8b916f2-b895-4ac5-8327-7664ce8a46f3`。原始日志 `/tmp/vega-75-screenshot-ui-rebased.log`，SHA-256 `0c9956a975bf5d132408257da9d356b21b6cfa6a20835c6be7d75d54686ca71d`，完整 executable/cwd/argv 在 `/tmp/vega-75-screenshot-ui-rebased-argv.json`。本分支相对主线修改的26个 Rust 文件 rustfmt --check 与 git diff --check 全部通过，原始命令/退出码在 `/tmp/vega-75-screenshot-ui-rebased-checks.log`。前述115、121、14项及全部失败证据独立保留。未执行 workspace 全量门禁或真实 driver；PR/CI/发布和真机验收由主控继续。

发布前 fresh fetch 又发现 `5c7885c7517b3f1ba4978fd40ef9ead7501d78b1`（#323 仅按运行终态展示尾部提示），再次无冲突变基。本轮未改本卡生产逻辑，仅补测受影响 UI：本卡14项 + #323核心8项，**22/22 PASS**、704 skipped，run `fe75b4e8-fdf5-4c93-886d-5c04cfd962fb`；未重跑无变化 runtime 矩阵。原始日志 `/tmp/vega-75-screenshot-ui-terminal-notices-rebased.log`，SHA-256 `41476564c79eea0b537e538ff0cd6f5380bc0224183ef5dddab5712e1f5b8251`；完整命令为同名前缀 `-argv.json`，10个本卡 UI Rust 文件格式与 diff 检查通过（`/tmp/vega-75-screenshot-ui-terminal-notices-checks.log`）。此前127项与全部RED记录继续保留；提交数仍为3。


## 已交付截图重复采集修复（2026-10-13，0.1.85 真机回归）

multi85-01 已在前20秒交付两张 TextEdit 图片，仍继续目录→截图A→截图B，最终9次工具、4张附件、79.660秒后达到图数上限。源码确认两处缺口：prepared CUA call 已替换为审计摘要，但运行时 assistant tool_calls 直接复用该摘要，导致模型看不到自己实际使用的窗口/动作参数；成功 Screenshot 又无条件清理成功读取计数，重复交付被误算作进展。历史脱敏本身不是故障，修复不得把窗口内容或输入写进数据库。

DP1. CUA 的当前 run 模型调用保留实际已解析参数，与该轮原 tool call 的语义一致；只用于临时 ChatMessage 和 provider 请求。RuntimeEvent、CompletedToolCall 的持久审计继续使用既有 ComputerUseAudit 摘要。目录/Act/截图调用和 tool result 保持完整顺序与 call_id 配对；Chat Completions 与 Responses 均须覆盖。进入可持久化压缩请求及 AgentOutcome 前，原参数恢复为审计摘要、截图仅保留有限ACK并去除临时窗口名称/身份与pixels；当前活跃模型请求仍可用原始参数。模型可看见当前操作参数不延长 target/observation 寿命，也不解除任何权限/身份/只读限制。

DP2. 每次 Screenshot 仍重新检查权限、当前目录 scope、epoch、PID/window/frame/capture 及图像，再以已验证的 bundle/PID/window 身份和解码后的 RGBA 尺寸/像素，与同一窗口最近一次已 durable ACK 的交付比较。相同像素（即使PNG编码不同）复用原 artifact_id，返回 delivered + unchanged 回执，不再次 emit ScreenshotDelivered、不增加图数/字节预算；不同窗口即使画面相同也分别交付。真实像素/尺寸变化可产生新交付，A→B→A 依各次最近图比较，保留变化。只有同 run、已 ACK 的记录能被复用，未ACK、失败或取消不能成为已交付依据。暂停/继续保留 ACK，但不保留失效观察和输入授权；继续后仍须新目录和新捕获才能判定重复。

DP3. 截图回执给模型明确完成状态、原 artifact 引用、所属窗口的临时身份/名称，以及 continue_remaining_requested_work_or_finish；若全部请求的图已交付，应直接回复完成，不再为确认附件而重新发现/截图。当前或已失效观察的回执都保持相同完成事实，旧pixels继续裁剪。窗口名称为不可信外部数据，不能成为指令或权限。有限持久回执仅保留状态/artifact/unchanged/next_step，不能持久化目标名称、ID或输入。

DP4. 新且成功交付的图才算进展；重复截图不清读循环计数。经验证发生真实状态变化的 Act 可以重置本窗口的截图计数，不能抹除其他窗口的截图计数；跨窗口交错输入不能绕过同一窗口第3次重复截图的收口。同窗口相同内容的重复截图第3次至迟触发 no_progress，跨目录/另一窗口/Observe插入也不能重置其计数；已交付附件保留，并明确停止的是无变化重复操作。真实变化的动作/观察、合法分页和不同目标可继续按原合同推进；不能因已经交付两张就强行结束仍有实际工作的任务。图数上限仅约束真实新交付，不用来收口重复循环。

DP5. 内置 CUA skill 更新指引与版本摘要：附件 ACK 表示截图工作已完成，按尚未完成的用户请求继续或直接回答；unchanged 回执复用已有图，不能再次采集相同状态。保留旧图片预算、pause/resume、取消优先、durable sink失败及存储历史限制，不改UI、签名、schema或依赖。

| ID | 生产前置/操作 | 预期 | 状态 |
|---|---|---|---|
| DP01 | 完整run：目录→A/B截图→final | 3 tools/2附件/正常final；两种provider构造包含原target参数和对应ACK，持久事件仍无目标原值 | PASS（进程内） |
| DP02 | 目录→目标Observe→Act→截图 | 下轮模型保留真实observation/element/text参数；事件仍摘要，无原输入；原权限和图像预算保持 | PASS（进程内） |
| DP03 | A/B交付后重复目录→A/B，opaque引用/PNG编码变化 | 无重复附件，不吃4图cap，明确unchanged/完成指引；第3次无变化重复时收口 | PASS（进程内） |
| DP04 | 同窗口真实变化、不同窗口同像素、A→B→A | 不错误合并；新交付允许，动作/分页原反例保持 | PASS（进程内） |
| DP05 | 已交付2图→暂停/继续→重采相同状态；未ACK/取消/权限变更 | 原ACK复用，新capture仍检权限/epoch；未ACK绝不谎报交付；不恢复旧授权/图片 | PASS（进程内） |
| DP06 | 四次实际不同交付 + 暂停清理 | 全请求通过production图片校验和两种请求构造，旧像素清除、4个ACK配对；未清理4图对照仍拒绝 | PASS（进程内） |
| DP07 | A重复截图与B真实输入交错；A自身真实输入后继续截图 | 第3次重复A收口，不派发后续B输入；A自身真实变化可重建基线，继续剩余工作 | PASS（进程内） |

先保留 production RED，再改接线/去重/guard。自动证据仅为进程内生产运行链；自然提示、真实provider与新版顺畅交付仍由主控发布后验收。


### DP 本轮结果与原始失败保留

DP01–DP06 已通过进程内生产链：正常双图3 tools/2 artifacts/final；目录/截图/Act 的当前模型请求保留原参数，Chat Completions 与 Responses 的调用和结果精确配对。实际 Store 批处理用例先一次模型响应发出2个截图调用，再各重复一次捕获，共5个工具均success、仅2个持久附件；5条审计输入与输出均不含目标、窗口名或bundle原值，数据库重开保留原两张图片身份。当前provider请求、压缩hook输入、AgentOutcome及数据库分别断言其不同隐私边界，不以持久摘要推断模型请求。

PNG不同编码但相同RGBA复用原图；另一窗口同像素分别交付，同一窗口真实A→B→A依次交付。已交付2图后暂停/继续，实际重新验证并捕获两次，但只复用原ACK，无新附件；旧pixels失效且最新模型图满足原预算。原2+2预算用例后两张改为真实像素变化，仍严格保留4个ACK、完整调用配对、最新1张模型图，并以4张未清理图被production validate_messages拒绝作对照。此为生产校验及两种请求构造证据，没有发真实HTTP。

重复目录/双图脚本只交付2张，在最多10个工具内由无进展检测收口，不再触发4图cap；该上界仅为重复策略防护，不代表自然交付顺畅验收。另一窗口的新交付和观察不会清除已有重复截图计数；真实动作/视口变化、分页与不同目标原反例保持。取消与第3次重复截图guard同时到达时以Interrupted结束；未ACK截图不可复用，暂停/旧epoch捕获不能创建晚到回执。未ACK External正文在AgentOutcome中被丢弃，压缩请求只含审计参数和有限交付ACK。

| 阶段 | 实际结果与原因 | 原始日志 |
|---|---|---|
| 生产RED | 0 PASS / 3 FAIL：模型截图参数被摘要替换、Act.text丢失、重复交付4张 | `/tmp/vega-75-delivery-progress-red.log` |
| 首轮修复 | 3/3 PASS | `/tmp/vega-75-delivery-progress-first.log` |
| Store/编码/暂停/原读取矩阵 | 15/15 PASS | `/tmp/vega-75-delivery-progress-second.log` |
| 完成出口隐私RED | 1 FAIL：新增临时参数仍出现在AgentOutcome | `/tmp/vega-75-delivery-progress-privacy-red.log` |
| 压缩夹具首次编译 | 2个E0609：误用AgentRequest.messages，修正为真实history入口 | `/tmp/vega-75-delivery-progress-third.log` |
| 取消/未ACK/压缩矩阵 | 18/18 PASS | `/tmp/vega-75-delivery-progress-fourth.log` |
| 审查补充RED | 1 FAIL：Screenshot分类遮蔽了未ACK External正文清理 | `/tmp/vega-75-delivery-progress-unacked-red.log` |
| 最终跨层矩阵 | 132/132 PASS，1808 skipped | `/tmp/vega-75-delivery-progress-final.log` |

最终run ID：`f491c32c-ab07-4592-8529-3dda17244687`；日志SHA-256：`158ee9a6834fe015814cb3142fc0ffa97909c00edb7e30af01b9e07574cef538`。完整executable/cwd/argv在 `/tmp/vega-75-delivery-progress-final-argv.json`：

```sh
cargo nextest run -p vega_runtime -p vega_conversation -p vega_ui -p vega \
  -E 'test(issue75) | test(types::tests_audit_wire)'
```

9个修改Rust文件定向rustfmt与git diff --check通过；内置skill v3通过quick_validate，内容SHA-256为 `f08550ee4a08d2ed778318a8eacec93a06d15046c0dcb4805c19d7ebe2e91df4`。所有原始阶段、命令、run ID/hash及检查路径汇总于 `/tmp/vega-75-delivery-progress-evidence.json`。本轮无UI/签名/schema/依赖修改，没有调用真实driver或读取live数据库/截图。0.1.85真机失败原证据继续保留；新版自然双图、输入、权限和接管验收由主控在发布后执行，以上自动结果不代替真实顺畅通过。

提交后无冲突变基至 `bb6997e2857bef7833b09f41a44af3f602d0a742`（#325 仅inline code/table与主题/UI规范）。range-diff确认本卡生产/测试补丁完全等价；未重复无变化runtime矩阵，只补测 `cargo nextest run -p vega_ui -p vega -E 'test(issue75)'`，**19/19 PASS**、969 skipped，run `fd7961bb-a1aa-4736-99de-696c48ad2844`。原始日志 `/tmp/vega-75-delivery-progress-rebased-ui.log`，SHA-256 `6a404b9bd2443ff0f03ecb327810dd1d37f42c5d4b816c05cbef5b6df142c0b7`，完整命令为同名前缀 `-argv.json`。变基后9个Rust文件格式与完整diff检查通过（`/tmp/vega-75-delivery-progress-rebased-checks.log`），前述132项及全部失败证据保持。

固定提交审查发现成功 Act 的全量 reads.clear() 仍会抹除其他窗口的重复截图计数。新增生产回归先复现 **1 PASS / 1 FAIL**：B真实输入两次后，旧实现仍派发第三次B输入，未在第3次重复A截图收口；同窗口真实变化可继续的反例通过。修复将真实动作与成功交付共享按目标重置：重建普通读取基线，仅清本窗口截图计数，保留其他窗口的截图计数。修复后的跨窗用例严格为9 tools、1附件、2次输入，最后返回no_progress，不再调用后续provider/driver；同窗变化仍正常final，不误停。

新增RED日志为 `/tmp/vega-75-delivery-progress-interleaved-act-red.log`，SHA-256 `7651bc3bcb10886c1174bf1bd6126d38bdc75614e6bb5631415a54a90a652023`。受影响定向复验 **25/25 PASS**、346 skipped，run `d6a16207-951e-4f14-ba69-f01c0e40c5c7`，原始日志 `/tmp/vega-75-delivery-progress-interleaved-act-verified.log`，SHA-256 `68e686df35fb477850fd1055f9fe7047857885423576164f8323ccd560c01b85`；完整executable/cwd/argv在同名前缀 `-argv.json`：

```sh
cargo nextest run -p vega_runtime \
  -E 'test(issue75_delivery_progress) | test(issue75_successful_reads) | test(computer_use::progress::)'
```

2个修改Rust文件格式与diff检查通过（`/tmp/vega-75-delivery-progress-interleaved-act-checks.log`）。未重跑不变跨层/UI矩阵，既有132项、19项和全部RED证据独立保留。
