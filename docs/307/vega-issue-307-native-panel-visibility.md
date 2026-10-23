# #307 · 原生浮层跨应用可见性与后台跟随修正

## 当前状态与依据

用户在 #574 合并后对真实构建手测未通过：打开系统设置时浮层不可见，必须切回 Vega 才显示，此时系统设置被主应用遮挡或离开当前应用组，无法拖入；移动系统设置后浮层也不跟随。#307 已退回 In progress，Issue 保持开放。本轮基线 `80eaba474df5a23c3a7849e11b39a0864b7e0ad6`；分支 `feat/307-permission-panel-visibility`。

本次约 11 秒录屏由 ffmpeg 在本机提取 2fps 帧，再用 Vision OCR 读取文字和坐标。2–2.5 秒可读到系统设置的 Accessibility 页面，但未检测到浮层指引；3–5 秒可读到浮层指引，而原系统设置页标题不再可读；8.5 秒系统设置标题明显移位，9–10.5 秒浮层指引仍在旧位置。该时序支持用户反馈，但 OCR 不证明完整原生窗口边界、前台应用身份、遮挡原因、实际投递或跟随循环是否退出。Vision 返回成功但 stderr 有系统 Operation not permitted 诊断，不能将其当作原生 UI 验收。原始视频、帧、完整 OCR 和私人屏幕内容不提交或公开。

上一轮紧凑布局、静态审查及 Actions 通过不代表本轮原生效果合格。本规格补充并优先约束[紧凑浮层规格](vega-issue-307-compact-guide.md)的 C1/C2/C5/C6/C10；不重做400×140布局，不改 guidance/status-only 范围。

## 验收用例（先于实现计划）

| ID | 前置与操作 | 可观察结果 | 验证层级 |
| --- | --- | --- | --- |
| V1 | Vega 中点击准备或具体权限设置，等待系统设置成为活动应用 | 紧凑引导无需切回 Vega 即与系统设置同时可见，位于普通系统设置窗口之上；Vega 主窗口不被主动激活、带到前台或成为拖入前提 | 实际 API/配置审查 + 用户手测 |
| V2 | Vega 失活，点击系统设置与引导图标，开始向权限列表拖拽 | 引导不因应用失活自动隐藏；点击/开始拖拽不激活 Vega 主应用，不移走系统设置；当前运行 `.app` 的真实文件载荷可进入列表，接受与授权仍不由代码推断 | 原生构造/输入审查 + 用户真实拖放 |
| V3 | 未拖拽时，保持系统设置在前台并多次移动它 | 浮层在后台继续按150ms轮询、18pt内缩、1pt阈值跟随；更新不依赖 Vega 窗口前台、可见性或渲染帧 | 调用链审查 + 用户录屏 |
| V4 | 应用组切换、Stage Manager（若开启）、多屏/Spaces/系统设置全屏 | 引导能加入系统设置所在应用组/空间，不只附属于 Vega 应用组；保持可见区clamp与坐标映射；不更改用户 Stage Manager/Spaces 配置 | SDK契约审查 + 用户手测 |
| V5 | 先开始真实拖拽，再取消或完成，随后移动系统设置 | 首次 `begin_drag` 在外部交接前同步永久冻结本次引导跟随，取消不恢复；无自动完成/撤卡/任务重跑 | 不变量审查 + 用户手测 |
| V6 | 管理员授权对话框、系统模态提示出现 | 浮层不以极高层级遮挡验证界面；关闭、Esc、Finder与必要错误恢复仍可用；不靠持续抢焦/置前循环实现“跟随” | 层级/API审查 + 用户手测 |
| V7 | 关闭/替换引导或原任务上下文失效 | 既有失效、取消、清理管理与任务返回围栏不变；已排队的旧跟随更新也不能再次移动关闭实例 | 生命周期审查 + 必要开发验证 |
| V8 | 两项静态权限Granted或检测失败 | 原共享模型、独立事实、有效拖拽入口、false/false与实际抓取未验证保持；不增加driver提示、捕获、浏览器引导或Ready推断 | 保护路径/diff审查 |

## 已核实的调用链与关键未知

1. `card.rs` 以 `WindowKind::Floating`、`focus:false` 打开窗口，再在构造回调中通过marker找到并配置自己的原生窗口。锁定的 `gpui-pre-macos 0.3.4` `window.rs:939–951` 明确将Floating分配为 `GPUIPanel : NSPanel`，因此没有证据支持“创建的不是NSPanel”的初始猜测。
2. GPUI的Floating分支仅设置floating level；NonactivatingPanel样式在PopUp分支的原生初始化之前设置。Vega当前在窗口已初始化后才添加该样式。是否导致此录屏中的点击激活，尚无直接原生状态读取；修复不得仅用事后styleMask读回作为点击不激活的证明。
3. `native/macos.rs:171–200`只设置NonactivatingPanel、AllSpaces、FullScreenAuxiliary、level3与隐藏标题，未显式设置或验证 `hidesOnDeactivate = false`。AppKit的该属性正是失活隐藏契约；单独提升窗口level不能替代它。
4. SDK `NSWindow.h` 将 `NSWindowCollectionBehaviorCanJoinAllApplications`（1<<18，macOS13起）定义为可加入其他应用集合/全屏空间的浮动窗口与系统叠层行为。当前只有AllSpaces/FullScreenAuxiliary，不等价于加入其他应用组。该位与Primary/Auxiliary互斥，不能无条件OR而不清理冲突位；项目最低macOS14，无需新增依赖。
5. `card.rs:207–219`当前通过 `spawn_in` + `weak.update_in` 驱动跟随，任何更新失败就永久break；tick实际上不使用Window。锁定GPUI的 `WeakEntity::update_in` 查询实体当前窗口关联，可能因缺关联/关闭/更新栈而失败，不等价于实体已释放。后台失活本身是否导致本轮退出尚未证实，不能伪造根因；可移除不必要的窗口上下文依赖，使用主线程实体更新并在队列内再次检查取消。
6. 实际系统设置定位仍由bundle/PID对应的CG窗口读取，移动由marker限定自己的NSPanel；现有屏幕映射与跟随数学未发现本次证据支持的修改理由。不能把CanJoinAllApplications或失活隐藏修复直接当作“已证明跟随正常”。

## 最小实现计划与允许范围

- 仅修 `crates/vega_ui/src/cua_onboarding/card.rs` 和 `native/macos.rs`，确有必要的native包装接线可改 `native.rs`。2026-10-23 用户在委派故障后明确要求本轮不使用子Agent、由当前Agent直接修复；本轮据此直接实现与自查，不将自查称为独立审查，不改变仓库其他任务的协作规则。
- 采用锁定GPUI真实支持的原生构造路径，让nonactivating在首次显示/交互前成立：`WindowKind::PopUp` 在 `initWithContentRect` 前设置 NonactivatingPanel，配合 `show:false`、`focus:false`、无tabbing identifier隐藏创建；不使用其默认高层级显示。`App::open_window` 完成root构造、首次绘制并注册handle后，再对唯一自有panel配置并读回level3、失活不隐藏、跨应用collection，随后仅一次 `orderFrontRegardless` 非激活显示。没有NonactivatingPanel样式即拒绝，不靠事后补style修复初始化语义。
- 配置/显示失败时关闭新建的隐藏卡片，经既有invalidate/release取消跟随并释放准备owner，返回host现有持久失败入口；不得留下不可见活动卡片或回退到高层级/激活显示。保留GPUI内部 `window.focus` 的键盘焦点初始化（该API只修改内部focus并刷新，不激活原生窗口）。
- 精确匹配且唯一的自有panel上显式禁用失活隐藏，采用适合跨应用组的collection行为并读回验证；保留floating level3，不升级到popup/menu/screen-saver层级，不注入外部窗口，不修改系统设置或用户权限。
- 只在必要初次显示/显式打开时使用非激活排序；不得150ms反复orderFrontRegardless/activate/makeKeyAndOrderFront强夺前台，不用轮询掩盖错误生命周期。
- 跟随改用不需要Window的主线程实体更新，保留150ms、永久冻结、marker、失效/cancel，在排队更新内再次核对取消。不要为移动重写系统设置识别、坐标数学或服务层。
- runtime/driver/ProbeClient与共享model/session/Settings/任务路由、布局资产、依赖/vendor/Cargo、工作流与发布/用户数据均保护不改。无新增注释（unsafe邻接SAFETY例外）、非测试unwrap/expect或lint抑制。

## 验证、阶段与恢复

本地仅快速静态API/SDK/格式/diff/hash检查，不运行Vega、编译、build、cargo check、Clippy、测试或UI harness，不调用桌面输入/修改权限/安装应用替代用户验收。开发测试按风险决定，不为流程制造原生伪验证；若有Rust开发测试，必须实际Actions验证、留存可恢复源码/完整SHA/结果并在合并前清理。最终无Rust测试完整head由既有required `check (fmt, clippy, test)`验证；5分钟CI查询。本轮按用户明确要求不使用子Agent，最终报告单列自查、云端门禁及未取得的独立审查/用户手测，不混称。

初始检查点（实现前）：录屏已读取，原生配置缺口已静态核实，跟随退出触发仍为待验证；当时未修改任何源码、未创建修复PR或取得本轮CI/审查。委派工具两次返回capacity_exceeded；历史执行被工具报告为running/release_state unknown，取消请求未得到已释放确认，复用同一旧writer也返回agent_busy。全量list_agents返回receipt too large且当前接口无筛选参数；精确wait/cancel读回仍未释放。不能以该执行环境故障冒充实现完成或绕过代码委派/独立审查要求。

恢复更新：用户已明确要求本轮不使用子Agent、由当前Agent直接修复，故不再等待委派槽位；此前没有新原生修复writer被成功启动，修复分支干净且源码未改。按上述最小范围直接实现、自查和最终云端门禁。用户仍需在包含修正的新构建手测V1–V6；即使CI绿与合并，也只转In review、Issue开放，不能宣称跨应用拖放/跟随已验收。回滚仅revert本轮原生修复，不回退#574的紧凑界面与安全修复。

## 直接修复检查点（2026-10-23）

已直接修改三份源码，未改保护路径。`card.rs` 隐藏构造原生nonactivating panel，在完整window/root注册后才请求显示，失败走既有Close→CancelPending→释放owner→移除窗口及host错误恢复入口；原卡内Configuration分支已由该失败路径替代。`native.rs`仅更新显示桥接名称。`native/macos.rs`限定唯一自有NSPanel、主线程和已隐藏/已nonactivating前置；清理与AllSpaces/FullScreenAuxiliary/CanJoinAllApplications冲突位，设置level3、hidesOnDeactivate=false、worksWhenModal=false并读回，全部成功后仅一次orderFrontRegardless，确认isVisible。没有新增激活、周期置前、捕获或权限操作。

跟随改为`Context::spawn`（锁定GPUI明确在主线程执行）与`WeakEntity::update`；timer后与实体更新内均检查取消，tick仍检查open，follow仍检查following。150ms、首次拖拽同步冻结、18pt/1pt/clamp、寻找/重获设置窗口和关闭清理不变。GPUI内部focus不等于平台activate；不强制禁止panel成为key，保留原键盘入口。实际非激活输入、后台可见与授权窗口遮挡仍需手测，SDK配置不能替代。

静态自查覆盖V1–V8对应的构造顺序、失败清理、collection互斥、BOOL/void ABI（arm64 bool、x86_64 i8）、owned-marker查找、主线程/生命周期及未变保护路径；未取得独立审查。`rustfmt --check --edition 2024 --config skip_children=true`（三份源码）、`cargo fmt --all -- --check`、`git diff --check`均退出0。保护路径相对基线diff为空。首次新增代码扫描使用了不受grep -E支持的lookahead而失败；改用rg PCRE2静态扫描后无违规新增，未把首次扫描当通过。

未新增开发Rust测试：此delta是AppKit构造/排序与GPUI任务绑定，镜像mask或源码字符串测试不能证明真实跨应用交互；选择锁定API/SDK核对、云端完整required门禁及用户原生手测。最终树静态无Rust测试属性、测试路径、独占dev wiring；现有doctext只有text围栏，源码字符串中的Markdown不是可执行doctest。无本地运行/编译/测试，无CI修改、安装或权限变更。当前云端结果待取得，后续精确提交、run/job和实际计数写入PR/Issue。

| 源码 | SHA256 |
| --- | --- |
| `card.rs` | `2b0cc42fb17a4849d2d7d3fa2458278f8fe07c562df6ec1a0c921d381b61860d` |
| `native.rs` | `0501f2a263e6762c2f962c4517c2abcab43c235fc7b087f3706053f4039837bd` |
| `native/macos.rs` | `294b6a74a7a80a5b39a71acdc57b530dae975cbfb7bac9ff8fcd35cd106d1cab` |

相对初始规格提交`d32b61726074c49820024f28e8574c501f83e01c`的三源码binary diff SHA256：`2e93bdac0642d7cf8252f9fc8113c8849f8af53a7a8079fbc9ea8278f7a3154b`。
