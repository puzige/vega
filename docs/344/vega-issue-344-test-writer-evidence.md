# #344 先行测试写入证据

## READY：测试与最少接线，待独立审查 / 云端 RED

- 生产基线与当前 HEAD：`871f91f246e90753743a256cee1eec315e74effc`。
- 分支：`feat/344-message-anchor-bottom`；使用主 Agent 已 fetch 并从 `origin/master` 创建的本卡 worktree。本写入者未操作 index、commit、push、fetch/rebase 或 CI。
- 冻结输入：[方案 v1](vega-issue-344-message-anchor-bottom.md)，SHA256 `3d2cbd53f810042e9fc5330a670a8ee7ef9678c7c5d1fd898152be06ff10fec4`；未改规格。
- 阶段：生产算法未改，新增 9 个 GPUI 行为测试及临时支持。9 是静态源码清点，不是 Actions 发现数或执行数。
- 本轮只读源码与执行静态工具；未运行 Vega、编译、build、check、Clippy、test/nextest、应用、benchmark 或打包。未恢复 #72 / #387 根窗口组合。
- 测试 payload 静态检查时间：2026-10-22 16:15 UTC。此记录不宣称有效 RED、GREEN、独立审查通过或用户验收通过。
- 本文提交为 READY 后，本写入者停止该范围写入；后续实现必须等主 Agent 确认有效云端 RED 并 followup。

## 文件范围与版本

| 文件 | 本轮变更 |
|---|---|
| `crates/vega_ui/src/conversation_stream/message_anchor_tests.rs` | 新增 666 行；轻量宿主、绘制/对象观测、9 个本轮测试 |
| `crates/vega_ui/src/conversation_stream/message_anchors.rs` | 只增加 rail / marker / preview 三个 `cfg(test)` 透明观察包装，共 30 行；非测试分支返回原元素 |
| `crates/vega_ui/src/conversation_stream/mod.rs` | 2 行 `cfg(test)` 模块接线 |
| `crates/vega_ui/Cargo.toml` | 3 行 dev-only gpui-kit `test-support` |
| `Cargo.lock` | 静态解析产生的测试支持闭包；109 行增加、9 行替换删除 |
| `docs/344/vega-issue-344-test-writer-evidence.md` | 本证据文档 |

未扩大公开 API、未新增代码注释、未改 CI / 生产 token / 定位服务 / 持久化。`content.rs` 与其 `reveal_loaded_message` 完全未改。原有未跟踪方案文档为主 Agent 输入，不属于本写入者变更。

### 冻结 payload 内容 SHA256

以下五文件构成测试 payload；证据文档不纳入该 payload，以免自引用哈希。按表中路径顺序组成 UTF-8 清单，每行为 `<sha256>  <relative-path>\n`，清单 SHA256 为 `da70196e0562a9152e9874993564fe897ee0543012f7178bc778ac7f0948570d`。

| 文件 | SHA256 |
|---|---|
| `Cargo.lock` | `d7c701e424bae4802ca725b937899e3875d0d0293b7d3be5558ff707849ab888` |
| `crates/vega_ui/Cargo.toml` | `3f27cebbfc927e48576b21a78e2684c05ff4da14c2a70cb56e7851cd175edde5` |
| `crates/vega_ui/src/conversation_stream/message_anchor_tests.rs` | `a58101c1ad60b01a216fa762b14a01f7c08c5ec316e5b8e5ce395fd75816b142` |
| `crates/vega_ui/src/conversation_stream/message_anchors.rs` | `bb628698b470f6b4799430c0e8ec53ffca22209814169762190dfb45889419d4` |
| `crates/vega_ui/src/conversation_stream/mod.rs` | `11a0206342c826ac8d3b22738f55374307cb056f80c0299b585e1ae12e51d279` |

payload diff SHA256：`8d22db2786740bfe6dfd3c35359960835b43d1736d8f2ca87b17c39c802fcf05`，34690 bytes。精确构造：在仓库根目录，先取 `git diff --no-ext-diff --binary --full-index 871f91f246e90753743a256cee1eec315e74effc -- Cargo.lock crates/vega_ui/Cargo.toml crates/vega_ui/src/conversation_stream/message_anchors.rs crates/vega_ui/src/conversation_stream/mod.rs` 的 stdout，再拼接 `git diff --no-index --no-ext-diff --binary --full-index -- /dev/null crates/vega_ui/src/conversation_stream/message_anchor_tests.rs` 的 stdout。后一命令退出码 1 表示存在新文件 diff，不是验证失败。不依赖 index 或临时 artifact 保存源码。

## 写测试前的 harness 静态确认

- 以锁定 gpui-kit 0.6.1 / gpui-pre 0.3.4 缓存源码为准；没有升级依赖。
- `TestAppContext::add_window_view` 返回实体与 VisualTestContext；`simulate_mouse_move/down/up/click/keystrokes/resize`、`simulate_event(ScrollWheelEvent)` 可走真实 GPUI 输入派发。
- `TestSupportExt::test_support` 在既有 ElementId 外透明转发布局与事件，读取原有 aria label / bounds / visibility；rail 在 `track_focus` 之前包装，使观察的焦点绑定来自真实 FocusHandle。生产无测试分支不创建观察对象。
- `Window::painted_quads` 直接观测 scene，无需截图 renderer；bounds 是 ScaledPixels，使用 `.0 / window.scale_factor()` 换为逻辑像素，颜色使用 `Background::as_solid()`；断言同时检查 content mask。
- `ListState::bounds_for_item` / `viewport_bounds` / `logical_scroll_top` 是布局事实；`is_scrolled_to_end()` 遇 unknown height 返回 None。测试不注入“到底”“当前 ID”或预览 bool。
- `ConversationStream::new`、`set_reading_preview`、`apply_history_page` 产生生产 entries / stable IDs / anchor projections。宿主只渲染真实 rail / preview 与真实 ListState；ListState 每行包一层确定高度，里面调用现有 `render_rows::render_entry`，因此可控制变高、回填与 overdraw，同时观测实际用户正文气泡绘制。
- 宿主订阅实际 `MessageLocationRequested`，记录完整请求并调用现有 `reveal_loaded_message`；不模拟 ID 查找、pause 或 scroll 算法，不接数据库、provider、外部进程或完整根窗口。
- 测试设 FollowMode::Normal，隔离到底判断对 tail-follow 的依赖；手动到底用真实 wheel 派发，而非伪造底部状态。测试中的显式 `position_at` 只建立场景，不用于替代被测点击定位。
- `settle` 固定两次正常 frame：第一帧完成 ListState 布局，第二帧使 rail 读取最新几何，并 drain executor；不是失败重试或等待到断言变绿。
- 键盘测试发送真实 `shift-tab` 以改变窗口 modality，再用 GPUI `Window::focus_prev` 导航真实 tab-stop 链；轻量宿主没有完整 Root 的 Tab action handler。之后上下 / Enter / Space / Esc 全部由生产 rail key handler 处理。它不声称覆盖原生 OS Tab 集成。

## 冻结测试与预期观测

| 测试 | 验收 | 操作与必需观测 |
|---|---|---|
| `observed_rail_hover_and_loaded_location_are_live` | 阳性对照、A4/A6 | 溢出四消息初始 marker 宽度/颜色；hover 第二条的真实预览标签与面板；mouse-down 只聚焦、未请求；mouse-up 请求稳定 ID，正文进入阅读区；浅深主题下重新核对实际 token 绘制 |
| `short_last_message_click_reveals_stable_target_and_current_marker` | A1/A3 | 点击 80px 末条；完整请求 ID / thread ID / restore anchor；实际正文 bounds 与气泡；top index 早于末条且末边等于有效阅读底边；鼠标移出仍保留 focus、预览消失、只有末 marker 使用 active token；上滚 400px 后回到首锚点，尾行仅 overdraw 测量且在实际视口下方 |
| `manual_bottom_with_unknown_heights_selects_last_without_tail_follow` | A2/A3 | 128 条、尾条 80px，定位倒数第四行后末条仅 overdraw；真实 wheel 到底，明确断言 None 与非 following tail；实际尾部几何/正文/末 marker；再上滚恢复顶部对应 marker，零定位请求 |
| `visible_last_anchor_does_not_hide_non_anchor_tail` | A3 | 三消息后追加真实历史 SkillActivation 非锚点条目，高 700px；末消息已可见但尾行仍下溢，保持顶部消息高亮；wheel 到真实尾部才选择最后有效消息锚点 |
| `mouse_preview_closes_after_click_and_preview_hover_without_blurring` | A4 | hover marker → hover preview → 移出；另一次 click → 移出而不 blur；再次 hover 别条并进入预览/退出；断言对象标签、可见面板、面板消失和请求 ID 不额外增长 |
| `keyboard_navigation_and_mouse_to_keyboard_keep_preview_and_location_accessible` | A5 | 真 tab-stop 焦点与 keyboard modality；上下和边界、Enter/Space 定位、Esc 关闭；鼠标点击/离开后 focus 保留，再发键盘恢复预览与定位；最后正文可见、末 marker 当前 |
| `empty_single_and_short_conversations_hide_navigation` | A6 | 空、单个溢出锚点、两条不溢出消息，查询不到 rail 或 preview，也没有预览面板绘制 |
| `missing_or_unusable_list_geometry_does_not_expose_navigation` | A6 | 未挂载 List、零宽、零高、foreground 占满阅读区；先断言实际无有效 viewport，再断言 rail / preview 不存在 |
| `growing_tail_resize_and_foreground_recompute_current_geometry` | A7 | 到底 → 末条从 80 增至 240 并 remeasure → wheel 到底 → 宽高 resize → wheel 到底 → 恢复窗口 → foreground 增减；每次使用新 bounds 检查非底/底部 marker，且不产生请求、不启用 tail-follow |

基线预期 RED 信号（仅预期，尚未执行）：

- 点击后移出仍渲染 `message-anchor-preview`；对应 A1/A4/A5 的消失断言失败。
- unknown-height 非跟随到底以及动态几何到底时，末 marker 仍是普通宽度，前一 marker active；对应 A2/A7 的真实绘制断言失败。
- 无布局/零宽几何仍走估算 fallback 显示 rail；对应 A6 缺布局用例失败。
- 阳性对照、短/空边界与非锚点尾部对照必须在本轮执行；若失败原因是 harness / 编译 / 依赖 / runner，不把它当产品 RED。
- 组合测试会在首个失败断言停止；后续步骤源码已先行冻结，必须在同一测试 GREEN 阶段完整执行，不能把未到达断言算作已经验证。

### 几何验算

- 四行 `[700, 320, 300, 80]` 总高 1400；视口 600 / foreground 180，有效高 420。到底 scroll top 为 980，即第 2 行内部 280；末条位于 y=340..420。上滚 400 后 top=580，末条 y=740，处于 600px 实际视口之外且仍在 600px overdraw 内。
- 长列表末四行 `[260, 260, 260, 80]` 共 860；从第 125 行开始时末条 top=780，低于阅读底边420；到底回填到第126行内部180。再上滚300得到第125行内部140。以上采用源码零基索引分别为124/125/124。
- 末条增至240后总高1560；非跟随保持 top=980，末边580大于420。再次到底 top=1140，随后阅读高缩至300时末边仍420，需要真实 wheel 才回到底。foreground 的改变同样作用于有效底边，不对生产加入固定高度阈值。

## 静态检查与真实执行状态

最终测试 payload 已执行以下检查，全部退出码 0：

| 命令 / 静态方法 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 格式通过；仅解析/格式检查，不编译 |
| `git diff --check` | 已跟踪 diff 无空白错误 |
| `cargo metadata --offline --locked --format-version 1` | 离线解析通过；解析结果确认锁定两个 GPUI 包的 test-support 启用 |
| `cmp Cargo.lock /tmp/vega-anchor-test-support.Cargo.lock` | 写回的锁文件与临时目录中 Cargo 离线解析产物字节一致 |
| Python 静态 scope / production projection 检查 | HEAD 等于基线；4 个原有修改文件恰在授权范围；去除3个透明 test-only wrapper与test module行后，两份生产源码与基线逐字节相等 |
| Python 静态源码清点 | 9 个 `#[gpui_kit::test]`；新测试无注释、pub、ignore、retries |
| Python TOML 依赖身份比较 | 新增10个测试支持传递包；既有包版本/source/checksum不变，无删除；唯一既有包边变化为 gpui-pre → backtrace / proptest；其余是版本消歧 |

静态阶段曾发现并在本次 READY 前修正：`add_window_view` 方法名、ScaledPixels 的 `.0` 访问、FluentBuilder 的 `map` 方法、resize 后滚轮坐标、格式换行。第一次 `cargo metadata --offline --locked` 因锁文件缺少 test-support 闭包退出101；在临时目录使用 `cargo metadata --offline --format-version 1` 静态解析后，经审查使用 edit 写回，最终 locked 解析退出0。初次及中间 fmt 检查退出1，最终为0。这些属于测试接线/格式，不是有效 RED。

本轮锁文件新增传递包：bit-set 0.8.0、bit-vec 0.8.0、convert_case 0.11.0、proptest 1.11.0、proptest-macro 0.5.0、quick-error 1.2.3、rand_xorshift 0.4.0、rusty-fork 0.3.1、unarray 0.1.4、wait-timeout 0.2.1。仅来自已批准的 dev test-support；未增加生产直接依赖。

## 待主 Agent / 云端验证与限制

1. 非写入者先审查上述精确内容版本，特别是轻量宿主、固定两帧、真实 painted quad / content mask / aria label 的观察前提。
2. 主 Agent 统一 index / 提交 / push / draft PR。现有 Actions 继续使用 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest list --workspace --message-format json`、非空后的 `cargo nextest run --workspace`；不改 CI、不新增执行阶段。
3. 云端发现 / 执行 / 通过 / 失败 / 忽略 / 跳过计数、run/job、原始失败：全部未取得。只有实际缺失行为导致的断言失败才是本轮有效 RED。类型/借用/Clippy 未在本地验证，不把 rustfmt 或 metadata 称为编译通过。
4. 输入、布局和绘制是进程内 GPUI 场景；外部服务、根窗口路由、自然长文本排版、真实 compositor 像素外观与原生键盘焦点仍待用户手测，不由本测试冒充集成验收。
5. 没有新增验收或接口缺口；测试只是落实 v1 A1–A7。若云端发现观察前提错误，先保留原始证据并独立审查接线修正，不能放宽断言或追改预期换绿。
6. 有效 RED 经主 Agent 确认前不实现；同一冻结测试 GREEN 前不删测。GREEN 后独立删除本轮测试、3个观察包装、module wiring、dev dependency及独占锁文件闭包，保留生产实现。
