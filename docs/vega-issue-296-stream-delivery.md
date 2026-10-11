# Issue #296 · 信息流与 Composer 交付记录

## Freeze

- task_contract：`vega-desktop-ui-unification.md` R1–R4、R7、T1–T7；theme 另含 R5 Settings 共享尺度。无 spec 偏离。
- verified_at_utc：2026-10-11T03:26Z；verified_at_local：2026-10-11 11:26 CST。
- branch：`feat/296-stream-ui`；git_head：PR #295 合并后的公开 master `89e4c93842a8648fd40644f00d45bc2832fa4eed`；工作区与默认 Cargo target 独立。
- owned_code_diff_sha256：`970ba085185c1610685db92e92741f82b5b6b451e52096382c43d76fcdca5828`，包含本片全部 tracked 差异和新增测试，不含本文。
- tested_code_content_sha256：`c6cb36880c2bac1eea84a7f23689c811b1217779b6c4d32356586cba6716f111`，按 `git diff --binary -- crates` 加按路径排序的新增源码路径/NUL/文件字节生成；包括为独立构建借入的交互适配器，后者由其 owner 单独交付。
- os_arch：Darwin arm64；rustc：1.98.0；cargo：1.98.0；git：2.55.0；cargo-nextest：0.9.146；nextest 默认 profile，无重试。
- 证据仅合成输入的进程内 GPUI：不读取真实会话、用户配置，不操作正在运行的应用。

## 实现与验收矩阵

| ID | 前置与操作 | 可观察结果 | evidence class | 状态 |
|---|---|---|---|---|
| T1 | light/dark；主区域宽 1037/834/594/863/864；60 条混合历史 | rail 位于 20px gutter，正文与 Composer 同边且最大 768；实际绘制 1px track、4px marker、6px current | GPUI geometry/paint | PASS |
| T2 | following → detached → resume；detached 时注入新消息 | 固定 40px 导航行；32px pill 在 viewport 外；list top/bottom 恒定，手动阅读 identity/offset 保持，恢复点击有效 | GPUI behavior | PASS |
| T3 | 用户空行、用户图片、selectable/nonselectable 助手渲染 | 用户 24/16、助手 8/16 外距；图片仍右对齐；源码、复制与选择既有断言保留 | GPUI regression | PASS |
| T4 | 思考、单工具、多工具、run、终止与失败 | disclosure 28px，图标 16px 居中；保留 240/320 内容上限、自动折叠、事件順序；hover/leave 和键盘焦点不移动图标或文字 | GPUI behavior/paint | PASS |
| T5 | 多行错误；普通、危险、ACP 权限卡；键盘、重复与过期响应 | 中性共享表面、语义图标；多行图标对齐首行；按钮至少 28px；初始焦点、所有选项、危险文案、单次响应及 fail-closed 保持 | GPUI/security regression | PASS |
| T6 | Native/Codex × 主区域宽 594/400/320 × send/stop；长模型名 | footer 控件 28px 且不溢出；Codex 状态视觉截断但完整 aria/tooltip 保留；无底栏分支；model/permission popup 锚定保持 | GPUI regression | PASS |
| T7 | 分页、历史定位、锚点预览、嵌套滚动 | 稳定身份、外侧 preview gutter、窄视图 preview lane、历史加载与滚动契约保持 | GPUI regression | PASS |
| R5/R7 tokens | theme 常量冻结 | Settings 24/12/16/28/12/8；ICON_SIZE=16；Motion::INTERACTION_DURATION_MS=120 | UNIT | PASS |

实现使用既有原生 GPUI、主题语义色与 token。无新依赖、数据迁移或模型/权限业务配置改动。异常正文保留；过程内容仍可折叠和滚动；Composer 分支入口修复 #295 保持。交互适配器及 Settings/壳层不属于本片 patch，由对应 owner 交付。

## Results

主定向命令 C1：

```sh
cargo nextest run -p vega_ui -p vega_theme -E 'test(issue296) | test(issue98_scroll_button) | test(issue72_message_anchor_navigation) | test(issue78_message_bubbles) | test(issue147_markdown_selection) | test(issue146_run_activity) | test(issue151_latest_activity) | test(issue70_tool_activity) | test(permissions_cards) | test(permission_card::tests) | test(issue79_composer_error) | test(utility_bar) | test(conversation_stream::tests::thinking) | test(scroll_follow) | test(conversation_stream::tests::attachments) | test(conversation_stream::tests::core_flow) | test(permission_picker) | test(model_picker_levels) | test(r64_popup_deferred) | test(r68_popup_dismiss) | test(conversation_stream::tests::composer_actions) | test(conversation_stream::tests::hydration) | test(conversation_stream::tests::timeline)'
```

| requirement | evidence class | exact command | result | duration | bounded footer / raw log |
|---|---|---|---|---|---|
| T1/T2/T4/T6 初始反例 | GPUI geometry | `cargo nextest run -p vega_ui -E 'test(issue296)'`（当时 4 个新增测试） | RED，exit 100 | 0.100s | `4 tests run: 0 passed, 4 failed, 563 skipped`；`vega-296-stream-red.log` |
| T6 Codex 窄 footer | GPUI geometry | `cargo nextest run -p vega_ui -E 'test(issue296_native_and_codex_footers)'` | RED，exit 100 | 0.042s | `1 test run: 0 passed, 1 failed, 570 skipped`；`vega-296-stream-compact-red.log` |
| R7 disclosure 焦点 | GPUI behavior/paint | `cargo nextest run -p vega_ui -E 'test(issue296_disclosure_focus_rings)'` | RED，exit 100 | bounded 1 test | Thinking focus handle 未设 tab stop；`vega-296-stream-focus2.log` |
| 本片定向回归 | GPUI/UNIT | C1 | RED，exit 100 | 2.959s | `232 tests run: 227 passed, 5 failed, 370 skipped`；`vega-296-stream-final.log` |
| 本片最终回归 | GPUI/UNIT | C1 | PASS，exit 0 | 2.325s | `232 tests run: 232 passed, 370 skipped`；`vega-296-stream-final4.log` |
| 差异格式 | static | `git diff --check` | PASS，exit 0 | — | 无输出 |

初始四项反例分别检出正文被 rail 占用 20px、缺失固定导航行、过程行高 24px、Composer 加号 24px。后续 Codex 400px 反例检出 312px 状态字符串挤出卡片；修复只对文字使用 shrink/truncate，保留完整可访问名称、tooltip 与实际状态。焦点反例只缺 Thinking 的 ring；设置既有 FocusHandle 的 tab stop 后，四类 disclosure 的 ring 与图标/文字 bounds 均通过。

C1 首轮的五项旧几何冻结由 R4 明确替代，未放宽误差：model trigger 高度 29→28；slider/list/project popup 的 y 各 +1px；permission popup 的 left +4px（加号 24→28）与 bottom +0.5px（permission 控件改为 28px）。popup 的 width/height 原值保留，并增补相对 trigger 的精确左右边与 8px gap 断言。project popup 继续锚定 32px 外盒，因此比居中的 28px chip 上边高 2px。旧测试记录仍存在原始 RED 日志；新的常量反映 R4，不是 deferred 或权限业务回归。

中间修复日志也保留：`motion2` 记录重复 hover 注册导致的 GPUI panic，移除 Composer 加号二次适配后 `motion3` 34/34 PASS；`final2` 是测试误用不存在 token 名的编译失败；`final3` 231/232 PASS，唯一失败是新增 project 锚定断言漏算既有外盒 2px。修正测试公式后 C1 最终通过。其他早期独立编译修正日志不作为通过证据。

原始日志仅本地保留于 `/private/tmp`，公开记录使用上面的文件名和 bounded footer；不发布编译输出中的本机路径。

| raw log | sha256 |
|---|---|
| `vega-296-stream-red.log` | `2630f4e42b3866ab0f82477e0d7d6dba7e4f19af06ec7fffa8f8298857aa38f2` |
| `vega-296-stream-compact-red.log` | `b2ddfe6dc26327887cf3bd0ba027591d8e0400b07c66eeed8d184602adf77882` |
| `vega-296-stream-focus2.log` | `c2ee4aa4a0f65f656d50b1227864531cb287d19b20bcf34a4dcf699268377c37` |
| `vega-296-stream-final.log` | `ef783902c490194ef7428ebc2be98b2a4c66e4f65f2a6de3b09a7a7d465b94f7` |
| `vega-296-stream-final2.log` | `b156dda82e188a020542ed3d4d28ff49d07220d5774ed6e79da22513a12d61e2` |
| `vega-296-stream-final3.log` | `f618d7c10453b13278ca20254ab611ce117b6a993257024235dd8226fa538ca6` |
| `vega-296-stream-final4.log` | `3641647e2403438e6629945871037a34d160d1d3f28c3f64188fea0c5d8d2493` |
| `vega-296-stream-motion2.log` | `aeb09567e71414b915ad891af48220be82e65bc19ad4d108755065f84fee4c82` |
| `vega-296-stream-motion3.log` | `f190ce970b8bc8f5d99efc919c54801be8b224810e05611ccf71dac827331ccd` |

## Residuals

- NOT RUN：合并后的完整 owner 组合。最终共享 icons、Settings 与壳层仍须在集成后跑一次直接相关定向集；本片结果不替代该结果。
- NOT RUN：真实 macOS 新版本像素、真实模型/权限交互，由用户合并后手测。当前安装应用未更新、停止或重启。
- NOT RUN：本地 workspace 全量、全量 Clippy；交由云端 pr-check。
- LIMIT：测试中的 594/834/1037 等是主内容区域尺寸，模拟窗口扣除侧栏后的可用宽度；不宣称实际安装应用截图验收。
- 回滚：撤销本片 UI 与 token 变更，无数据迁移。
