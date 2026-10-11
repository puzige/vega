# Issue #296：共享交互与图标交付

对应 `docs/vega-desktop-ui-unification.md` R7、T15–T16。基线为 `89e4c93842a8648fd40644f00d45bc2832fa4eed`，独立功能分支与构建目录；未提交、推送、运行或安装正式应用。

## 实现边界

- 共享 SVG 图标的 KitIcon、inline SVG 两条路径统一为不可压缩的 16px 居中容器。
- 新增 `InteractionExt`，复用锁定 GPUI Base 的 `transition`，只插值背景，统一 120ms 进入/离开、中途反转及结束停帧。保留原 Div 的布局、命中、焦点、无障碍、hover callback 和 tooltip。
- 覆盖共享图标按钮、菜单行、标题栏、Sidebar 新建/设置入口和项目/会话主要导航行。会话重排后的 hover 抑制、菜单保持状态与真实选择状态沿用原逻辑。Settings 页面及信息流接入由相应实现片完成。
- 标题栏的 Sidebar/历史按钮预留焦点边框，项目/组织行用不占布局空间的 1px shadow 环表示焦点。传给 track_focus 的原 handle clone 显式设置 Tab 可达性；历史按钮按 enabled 决定，保留原 identity，disabled 跳过。
- 公开 `NSWorkspace.accessibilityDisplayShouldReduceMotion` 只读桥接在启动、窗口激活、hover 进入同步；读取失败使用静态反馈。偏好 reader 可在测试中替换，不读写用户系统偏好。未新增依赖。

`Stateful<Div>.interactive(colors, enabled, selected)` 用于透明或选中表面；非透明底色使用 `.interaction_surface(rest, hover, pressed, enabled)`。适配器拥有背景绘制，调用端应移除旧 hover 背景，不依赖后续 `.bg()` 覆盖；原 focus 样式仍由 GPUI 应用。控件动作与 disabled handler 仍由调用端保留。不要给已经适配的 `icon_button` 再套适配器。

Search 与 Sidebar Settings 保留原生 Button 的唯一 click/keyboard/focus 路径。原生 active 底色的真实测试发现拖出后仍保持按压色；这两处因此使用 `capture_native_button_press()`，仅在原 Div 命中范围观察 down/up/up-out，既不阻止传播也不改变默认行为。离开只隐藏按压色，按住拖回恢复，释放或窗口失活清除状态。

## 证据

全部为进程内 GPUI/编译证据，不宣称原生屏幕像素或用户系统集成验收。

| 阶段 | 可复现结果 |
|---|---|
| 动画 RED | `Duration::ZERO` 原型在 40ms 中间帧断言失败；3 项中 1 项失败 |
| 按压 RED | 原 Div active 拖出后仍为 `bg_active`；7 项中 1 项失败 |
| 原生 Button RED | Search、Settings 设置 custom active 底色后，拖出清理断言均失败 |
| 最终定向 | 下列命令 24/24 PASS，nextest run `2eefc534-2dc0-481c-9966-f9efa029b3ec` |
| Reduce Motion 停帧加强 | 同一静态用例增加“无待执行帧”断言后 1/1 PASS，run `61f63119-37fa-4ba2-a752-05bbfa9d5f1e` |
| 导航最终加强 | 同一导航用例增加真实 Tab 循环与 disabled 跳过后，导航/原生入口 4/4 PASS，run `815da1c4-a448-4002-b8a2-996c8e092337` |
| 主程序 | `cargo check -p vega` exit 0，最终增量 2.72s，覆盖生产 main 的启动及激活桥接 |
| 格式 | 仅 owner 文件 rustfmt；`git diff --check` 通过 |

```sh
cargo nextest run -p vega_ui -E 'test(issue296_interaction) | test(issue296_native) | test(issue296_navigation) | test(shared_search_control) | test(r31_sidebar) | test(r38_organization) | test(r41_search) | test(r48_) | test(r42_mounted_sidebar) | test(r157_)' --no-fail-fast
cargo nextest run -p vega_ui issue296_interaction_reduced_motion --no-fail-fast
cargo nextest run -p vega_ui -E 'test(issue296_navigation) | test(issue296_native) | test(shared_search_control)' --no-fail-fast
cargo check -p vega
```

覆盖真实测试指针进入/离开中间帧、反转连续性、结束停帧、两个控件状态独立、tooltip 与原 hover callback 共存、静态 reduced/disabled、系统偏好启动/实际窗口激活/再次交互/读取失败替身，以及两种图标尺寸和首行盒中心。原生 Search/Settings renderer 覆盖按压、松开、拖出、按住拖回与键盘触发；Search 两个有效 pointer 手势各一次，Enter/Space 各一次。标题栏三个原 FocusHandle 的 focus ring 绘制与图标边界稳定。Sidebar 原有几何、选择、重排悬停、菜单快捷动作回归通过。

集成需使用信息流实现片提供的 `Layout::ICON_SIZE` 与 `Motion::INTERACTION_DURATION_MS`。本片 patch 排除为独立编译复制的 theme/spec。真实 macOS Reduce Motion 与最终应用外观在用户任务空闲后手测；没有触碰当前真实任务、模型、权限配置或 Composer 底栏分支入口。
