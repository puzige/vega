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

- 仅修 `crates/vega_ui/src/cua_onboarding/card.rs` 和 `native/macos.rs`，确有必要的native包装接线可改 `native.rs`。父Agent维护本规格与关联文档；实现由专职子Agent写入，非实现者独立审查。
- 采用锁定GPUI真实支持的原生构造路径，让nonactivating在首次显示/交互前成立；若需要先隐藏构造再配置并非激活地显示，先核对 `show:false`、window-kind及显示API的实际生命周期，不能把激活主应用作为补救。
- 精确匹配且唯一的自有panel上显式禁用失活隐藏，采用适合跨应用组的collection行为并读回验证；保留floating level3，不升级到popup/menu/screen-saver层级，不注入外部窗口，不修改系统设置或用户权限。
- 只在必要初次显示/显式打开时使用非激活排序；不得150ms反复orderFrontRegardless/activate/makeKeyAndOrderFront强夺前台，不用轮询掩盖错误生命周期。
- 跟随改用不需要Window的主线程实体更新，保留150ms、永久冻结、marker、失效/cancel，在排队更新内再次核对取消。不要为移动重写系统设置识别、坐标数学或服务层。
- runtime/driver/ProbeClient与共享model/session/Settings/任务路由、布局资产、依赖/vendor/Cargo、工作流与发布/用户数据均保护不改。无新增注释（unsafe邻接SAFETY例外）、非测试unwrap/expect或lint抑制。

## 验证、阶段与恢复

本地仅快速静态API/SDK/格式/diff/hash检查，不运行Vega、编译、build、cargo check、Clippy、测试或UI harness，不调用桌面输入/修改权限/安装应用替代用户验收。开发测试按风险决定，不为流程制造原生伪验证；若有Rust开发测试，必须实际Actions验证、留存可恢复源码/完整SHA/结果并在合并前清理。最终无Rust测试完整head由既有required `check (fmt, clippy, test)`验证；5分钟CI查询。独立审查绑定停写完整提交。

截至此规格：录屏已读取，原生配置缺口已静态核实，跟随退出触发仍为待验证；未修改任何源码、未创建修复PR或取得本轮CI/审查。委派工具两次返回capacity_exceeded；历史执行被工具报告为running/release_state unknown，取消请求未得到已释放确认，复用同一旧writer也返回agent_busy。全量list_agents返回receipt too large且当前接口无筛选参数；精确wait/cancel读回仍未释放。不能以该执行环境故障冒充实现完成或绕过代码委派/独立审查要求。

恢复条件：委派工具释放至少一个实现执行槽位，并能完成后续非写入独立审查；随后按上述最小范围实现、停写、审查和最终云端门禁。用户仍需在包含修正的新构建手测V1–V6；即使CI绿与合并，也只转In review、Issue开放，不能宣称跨应用拖放/跟随已验收。回滚仅revert本轮原生修复，不回退#574的紧凑界面与安全修复。
