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
| U01 | Native/桌面启用、禁用、外部 backend 及压缩后发送 | 仅适用运行自动带内置指引，来源可核验，不累积、不改变外部 skill 信任 | 生产 provider/conversation fixture | 待测 |
| U02 | 普通观察和显式截图分别走同一 provider 会话 | 只有显式截图产出可查看附件；模型收到明确产物结果 | runtime + conversation/storage | 待测 |
| U03 | AX 空/无法解析而窗口截图具有完整身份绑定；反例身份/帧/图像冲突 | 前者只读交付成功且零动作；后者拒绝且无附件 | adapter fixture | 待测 |
| U04 | 错字段、冲突定位、只读、过期引用 | 有限 code/恢复建议，无私密值；新旧安全卡片正常 | runtime + history | 待测 |
| U05 | 相同失败三次，中间目录刷新/不同 opaque ID/压缩 | 第三次触发 no_progress，后续驱动与 provider 操作循环停止 | 完整 run_agent fixture | 待测 |
| U06 | 重复点击成功但相同可比状态；另有真实状态变化/合理滚动 | 无变化第三次停止；真实变化不误判 | production runtime fixture | 待测 |
| U07 | 截图写入失败、停止竞争、历史回读/清理 | 无虚假成功/晚到附件；显式截图历史可用，普通观察不持久化 | storage + controller | 待测 |
| U08 | 观察/输入/成功/权限/暂停/浏览器待批，宽窄窗口 | 无全局操作栏/占位；所属对话内必要操作可达，任务停止有效 | GPUI 生产状态与布局 | 待测 |
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
