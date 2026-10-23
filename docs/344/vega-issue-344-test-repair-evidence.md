# #344 先行测试修订证据

## READY：修订已写入，待增量独立审查与 Actions 有效 RED

- 写入者为本轮唯一代码写入者；代码只修订既有 `message_anchor_tests.rs`，另新增本文。未重写原测试产物，保留原 9 个测试函数，增加 2 个有区分度的测试；零高尾行并入既有边界测试。最终为 802 行、29,611 bytes、11 个 `#[gpui_kit::test]`，均为静态源码计数，不是发现或执行数。
- 主 Agent 提供的当前 HEAD：`2ba904f7074115de5ee89cc18c376c083f803357`；分支 `feat/344-message-anchor-bottom`。主 Agent 已 fetch + ff；相对原生产基线 `871f91f246e90753743a256cee1eec315e74effc` 只新增 docs/554 文档，无生产/API 变化。本写入者未执行 Git 命令，未操作 index、commit、push、分支或远端。
- 冻结验收仍为[方案 v2](vega-issue-344-message-anchor-bottom.md)，SHA-256 `3d2cbd53f810042e9fc5330a670a8ee7ef9678c7c5d1fd898152be06ff10fec4`。本次修正[原 payload 独立审查](vega-issue-344-review-evidence.md)列明的测试接线/覆盖缺口，不改验收、不提前实现生产判底算法。
- 本次 READY 只表示写入与本地静态检查完成，不表示独立审查通过、有效 RED、GREEN 或用户验收完成。未运行 Vega、编译、构建、打包、cargo check、Clippy、test、nextest、benchmark 或任何本地行为测试。
- 本文完成后的两文件 SHA 由 READY 回报另列，避免本文自引用。交回后停止写入；主 Agent 审查并交非实现者增量独立 review，再由现有 draft PR Actions 取得有效行为 RED。未经后续授权不改生产。

## 来源与旧记录更正

原测试输入 SHA-256 为 `a58101c1ad60b01a216fa762b14a01f7c08c5ec316e5b8e5ce395fd75816b142`，666 行/9 个测试；原五文件 payload 清单标识 `da70196e0562a9152e9874993564fe897ee0543012f7178bc778ac7f0948570d`，原完整 patch 标识 `8d22db2786740bfe6dfd3c35359960835b43d1736d8f2ca87b17c39c802fcf05`。输入测试与 v2 规格在写入前均重新计算并与授权相符。

[旧 writer 证据](vega-issue-344-test-writer-evidence.md)把冻结输入写为“方案 v1”，但它同时记录的 SHA 实际对应 **v2**，不是 v1 的 `154fad30278fbb4a3208e29871bc666b5532ce7455bad82506db87ce341e81c5`。保留旧文档原样及错误 provenance，在本文更正；旧记录中的强制帧收敛说明、历史静态结果不能替代本轮验证。

旧 writer 证据 SHA-256 仍为 `4f1f00fcc92c0b4f59f77dc5a04cd85c277f4fd69adb7d7ae9bdc612f1016e7f`；旧 recovery checkpoint 仍为 `713f7581d151865aa37cb6e69883977ffb2ae10471d0a5accc77367120426eb9`。

## B 项与验收映射

| 项 | 修订与可观察断言 | 对应验收 |
|---|---|---|
| B1 | `settle` 只运行 `run_until_parked`，并最多 8 轮公开 `Window::simulate_next_frame`；遇到 0 个已排队回调即返回，持续排队则明确失败。没有 refresh/draw/render_frame、断言前补 notify、Kit click/hover/press helper 或重试。深色主题在首个测试 fixture 创建窗口前初始化，原有 token/quad/mask 与 preview label/panel 观测保留，不再靠运行中切换主题触发几何刷新。A7 尾高动作合并为 fixture 更新 + remeasure + 一次 host notify。 | A1–A7 的真实自然收敛；保留浅/深色观测 |
| B2 | 保留现有函数签名及 `AnchorHost` 的 `render_message_anchor_rail` → list element → `.child(rail)` 顺序；正文与 rail 的相对位置未改。生产 `render.rs` 亦先挂 body/list、后挂返回的 rail。本轮不新增 API、观察算法、判底 bool 或 test-only 镜像决策。 | A1–A3/A6/A7 的共享布局后入口 |
| B3 | 新增 `cached_tail_with_unknown_middle_keeps_top_marker_current`。先正常布局缓存末行并确认 gap 行原先实测；再一次动作公开 `scroll_to(default)` + `splice(5..tail_index, tail_index - 5)` + notify。自然收敛后断言 top=0、gap=126、tail=127、`top < gap < tail`、两个中间 bounds=None、tail bounds=Some 且尺寸/坐标明确、unknown 返回 None、非 tail-follow、不同顶部/末条 ID、neutral/non-keyboard/no-preview。绘制要求 0..9 中仅顶部 marker 0 当前，末 marker 不在当前轨道窗口中。 | A3 cached tail + unknown middle；A2 原 unknown-before-top 用例保留 |
| B4 | 在 A6 的四种既有无效几何实例（未 mount、零宽、零高、foreground 吃满视口）各自恢复 mount/尺寸/foreground。恢复只一次 host notify，之后仅 settle 与读取/断言，没有鼠标移动、额外 notify/draw。断言有效阅读区 900×420、top=0/offset=0、rail 的九项真实绘制、marker 0 当前、无 preview/请求及非键盘焦点状态。 | A6 同实例隐藏 → 自动恢复阳性 |
| 长轨道 | 新增 16 锚点用例。正常预置 [4,8,12] 的布局覆盖全部行，确认 `Some(false)` 后点击 12，移出；上滚 2000 到顶部规则对应 4，断言 0..9 的实际 marker 绘制和旧点击 12 消失，focus 仍在且无键盘资格/preview。再按 up 预览稳定 ID 11、Enter 定位 11、down/Space 定位 12、Esc 关闭；检查请求完整序列与正文绘制。 | A3/A4/A5，防止旧点击对象锁定九项窗口 |
| 零高尾行 | 在 `visible_last_anchor_does_not_hide_non_anchor_tail` 原高非锚点尾部对照后，增加 `[700,300,80,0]`、末行为非锚点 activity 的最小 fixture。要求末行返回实测宽 900/高 0 的 Some(bounds)、边缘等于阅读底边、`Some(true)`，top=0 却必须绘制末消息 marker 2 当前，且无 preview/请求。把合法零高实测与未知明确区分。 | v2 合法零高连续尾段；A3/A6 边界 |

B2 的后续生产义务保持原独立决策：RED 后由现有返回元素携带始终挂载的 post-list observer；隐藏 rail 时不能空返回或 display:none 隐去 observer。它不得占正文布局、拦输入或改变 rail 定位参照；采样同一帧传给 list `.pb(...)` 的 foreground，post-list 当场形成一致投影，defer 只比较/提交变化并在变化时通知。此次没有实现该算法，后续不得为实现追改测试宿主。

## 实际 API 与静态几何依据

使用已锁定缓存源码 gpui-pre 0.3.4 / gpui-kit 0.6.1，没有依赖升级或新依赖。

- `gpui-pre/src/app.rs:1711–1739`：test-support 的 `flush_effects` 自动绘制 dirty windows，继续排空 defer effects；`app/test_context.rs:207–209,748–751,765–766` 提供正常 App/window update 与 executor drain。
- `gpui-pre/src/window.rs:2530–2541`：公开 `simulate_next_frame` 仅取出并执行 `on_next_frame` 已排队回调并返回数量，不强制 refresh/draw。`TestAppContext::test_window` 的 crate-private 入口未使用。
- `gpui-kit/src/test.rs:205–220`：`find/try_find` 只读取 observation；`render_frame` 则直接 refresh/draw，已完全移除后者。输入沿用 GPUI 的 `simulate_event`/mouse/keystrokes，不改用 Kit 强制绘制 helper；`app/test_context.rs:925–929` 的输入模拟只是 platform input + executor drain。
- `gpui-pre/src/elements/list.rs:458–474,501–549`：remeasure 保留 size_hint；splice 新行是无 hint 的 Unmeasured，范围外 suffix 缓存保留。本次 gap 用 splice，不用 remeasure 伪造 unknown。
- `list.rs:711–737`：bounds 只对 Measured 行返回 Some；坐标以前缀高度差计算，因此可查询未在当前视口绘制、但仍缓存的尾行。`list.rs:1643–1666`：无 hint 的 unknown 高度贡献为 0，合法 Measured 高 0 仍不是 unknown。`484–498` 的全局 unknown 查询仅作辅助前提，不被当作 gap 位置证据。
- `list.rs:1062–1124,1178–1206`：自然布局会测量 viewport + trailing overdraw，且保留范围外已测量缓存；`1526–1575` 显示宽度变化会失效全表，故 cached-tail fixture 保持宽度不变。
- 生产 `core.rs:427` 的 overdraw 为 600；宿主视口 600、foreground 180，有效阅读高 `600 - 180 = 420`。B3 正常布局先测量顶部五行，`5 × 260 = 1300 >= 600 + 600`，第 5 行及之后 gap 无需测量；splice 后仅保留前五行与 80px 尾行的有效高度，故缓存尾行 top=1300/bottom=1380。测试逐项断言这些真实前提，不注入判底或伪造 bounds。
- **B3 可行性边界**：在同宽、正高正常布局收敛后，不能同时要求 top 后的未知 gap 仍存在、缓存尾行又落在有效阅读底边内；该 gap 若位于正在扫描的 viewport/overdraw 内，会先被真实布局测量。本轮采用保留 top<gap<tail、未知中段和实测缓存尾部的可行负例，缓存尾行明确位于视口下方。此为静态 API/geometry 约束，不是运行结果，也不把它声称为“未知高度把尾行压进视口”的可执行 fixture。
- 零高 fixture 总高 `700 + 300 + 80 + 0 = 1080`，有效阅读高 420；到底 top offset 为 `1080 - 420 = 660`，仍在第 0 行内部。最后消息 y=340..420，零高非锚点尾行 y=420..420，顶部 ID 0 与末消息 ID 2 不同。因此把零高误当 unknown 会丢失本例必须具备的末锚点选择。
- 长轨道全部测量后的总高 `15 × 260 + 80 = 3980`，最大 top 为 `3980 + 180 - 600 = 3560`。点击 12 得 top=3120，上滚 2000 得 top=1120=`4 × 260 + 80`，可观察九项窗口必须由 7..16 转到 0..9，而非继续围绕旧点击 12。
- `list.rs:1432–1437` 的 `ListOffset` 只有 Debug/Clone/Copy/Default，没有 PartialEq；新断言分别检查 `item_ix` 与 `offset_in_item`，没有依赖不存在的 trait。

## 本轮写入范围与内容身份

本轮只写入：

1. `crates/vega_ui/src/conversation_stream/message_anchor_tests.rs`。
2. `docs/344/vega-issue-344-test-repair-evidence.md`。

既有 `message_anchors.rs` 的透明 cfg(test) wrapper、`mod.rs`、Cargo/lock、生产 message_anchors/core/render/content 均保持输入字节；未恢复 #72/#387 根窗口组合，未改 token、定位服务、持久数据、CI 或产品文案，未加代码注释、ignore、重试或放宽预期。

新版完整五文件 payload 清单按下表顺序，以 UTF-8 `<sha256>  <relative-path>\n` 拼接；清单 SHA-256：

`d9b4bd8193cb4521d7406f2ce51449b364fca058abf0814adcac7607f374ee6d`

| 文件 | 完整 SHA-256 |
|---|---|
| `Cargo.lock` | `d7c701e424bae4802ca725b937899e3875d0d0293b7d3be5558ff707849ab888` |
| `crates/vega_ui/Cargo.toml` | `3f27cebbfc927e48576b21a78e2684c05ff4da14c2a70cb56e7851cd175edde5` |
| `crates/vega_ui/src/conversation_stream/message_anchor_tests.rs` | `5d5f7d00482143fe83cf747e06d8366b753dab36c69e9d7e90c63ed27897656f` |
| `crates/vega_ui/src/conversation_stream/message_anchors.rs` | `bb628698b470f6b4799430c0e8ec53ffca22209814169762190dfb45889419d4` |
| `crates/vega_ui/src/conversation_stream/mod.rs` | `11a0206342c826ac8d3b22738f55374307cb056f80c0299b585e1ae12e51d279` |

另固定未修改文件：

| 文件 | 完整 SHA-256 |
|---|---|
| `core.rs` | `7f5ac9f5fcacbcd9bc76c053211a52fce06d587da28747d9a7a6fd96dbb86498` |
| `render.rs` | `2e5b1dd887dc3ccbd85a6c5b443be7839ab9e2c28857e032e2258cb9211f5d79` |
| `content.rs` | `80d01626e0ac512b7c7440ee5a351c5a97ebc54966423b7415ccfd7cf76984c6` |
| 根 `Cargo.toml` | `b2745ea61e0b9434b97562a71653c99a88a4f32436be778b911ee4edf914156f` |
| 既有独立 review 证据 | `50aaa3d06af6204bdb02b56a379f2fb3202444bce221605e4ed2e20d92add61a` |

## Results：仅快速本地静态检查

本轮静态校验工具：rustfmt 1.9.0、cargo 1.98.0；记录时间 2026-10-22 UTC。未调用 cargo nextest list，因为它会编译，静态属性计数不冒充发现数。

| 检查 | 准确命令/方法 | 退出码与结果 |
|---|---|---|
| 格式 | `rustfmt --edition 2024 --check crates/vega_ui/src/conversation_stream/message_anchor_tests.rs` | 0；只解析/校验格式，不编译 |
| 静态 metadata | `cargo metadata --no-deps --offline --locked --format-version 1 > /dev/null` | 0；不构建、不写锁文件，未声称完成类型或依赖行为验证 |
| scope/禁止项/身份 | `python3 -` 静态读取源码、hashlib SHA-256/正则校验 | 0；11 个测试属性，无代码注释/ignore/retries/强制绘制调用，settle 无 notify，B4 恢复只有一次 notify、之后无鼠标模拟；10 个生产/支持/规格/review 文件哈希未变 |
| 空白/清单 | `python3 -` 检查两授权文件各行 `line.rstrip() == line`、末尾换行，并重算五文件 payload 清单 | 0；最终两文件身份见 READY 回报；本写入者未执行 git diff/check 或其他 Git 操作 |

首次格式检查退出 1，只报告两处长 assert 的换行；按提示修正后退出 0。静态源码/API 复核另发现 ListOffset 不支持 PartialEq，已在最终检查前改为同语义的逐字段断言。首次最终两文件静态脚本在输出哈希后退出 1：正则只匹配五个 payload 路径及三个 Rust 文件名，共 8 行，校验器却要求 10 行；根 Cargo.toml 与 review 的两行带文字前缀，需要独立校验。修正校验器计数而不修改被测断言，再检查全部文件身份。工具调用中参数/路径被拒绝的 grep 不构成验证结果；以随后聚焦 read 的实际源码为准。以上都属于测试接线/静态检查或校验器错误，不是目标产品有效 RED。

## Residuals / 后续门禁

- **NOT RUN / 待 Actions**：11 个测试的发现、执行、通过、失败、忽略、跳过计数均未取得；无本轮 run/job，无本地行为通过声明。组合测试会在首个失败处停止，尚未到达的恢复或后续断言不得计为已验证。
- **待独立 review**：需在本 payload 上确认 B1 的自然收敛、B3 可行前提及 coverage 边界、B4 同实例恢复、长轨道键盘路径与零高尾行。B2 入口已有独立决策不等于本版测试通过审查。
- **预计但未确认的行为 RED**：旧生产仍可能在短尾部/手动到底错误投影当前位置、鼠标点击后 focus 造成 preview 粘住、无效几何仍显示 rail；缺少布局后观察通知也可能使恢复或几何更新后投影不更新。只有 Actions 的目标行为断言原始失败才算有效 RED；类型/依赖/runner/harness 失败不算。
- **阶段未变**：先由主 Agent 固定提交、源码与本文清单并取得有效 RED，之后才最小生产修复与同一冻结测试 GREEN；再固定可恢复证据、独立删测、最终 head 完整空集门禁。本次不提交、不推送、不实现算法，也不改变该交付流程。
