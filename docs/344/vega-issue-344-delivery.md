# #344 交付与云端验证记录

## 当前阶段

先行测试已提交 draft PR [#562](https://github.com/puzige/vega/pull/562)。首轮云端在测试宿主编译阶段失败，不是有效 RED；正在做经独立准入的最小接线修正。生产行为未改，尚无本轮有效 RED、GREEN、合并或用户手测结果。

## 内容与先后顺序

- 原生产基线：`871f91f246e90753743a256cee1eec315e74effc`。
- 当前更新基线：`2ba904f7074115de5ee89cc18c376c083f803357`；两者只差新增 `docs/554/vega-issue-554-panel-motion-study.md`，无生产/API/依赖变化。
- 分支：`feat/344-message-anchor-bottom`。
- 冻结验收：[方案 v2](vega-issue-344-message-anchor-bottom.md)，SHA-256 `3d2cbd53f810042e9fc5330a670a8ee7ef9678c7c5d1fd898152be06ff10fec4`。本轮恢复不改变验收。
- 原 9 个测试 payload 的四项审查阻塞与共享入口后续决策见[独立审查记录](vega-issue-344-review-evidence.md)。原 patch SHA-256 `8d22db2786740bfe6dfd3c35359960835b43d1736d8f2ca87b17c39c802fcf05`，不把被拒版本称为已冻结或有效 RED。
- 本轮唯一修订作者 `01M5J9KYT7SY2BCQ2E4Y2FDQW2` / execution `01M5J9KYT76KYQSTDBSCXQRKVR` 已 READY 并停止写入。仅改测试文件与[修订证据](vega-issue-344-test-repair-evidence.md)，生产算法、支持接线、Cargo/lock 不变。
- 修订后测试源码：`crates/vega_ui/src/conversation_stream/message_anchor_tests.rs`，SHA-256 `5d5f7d00482143fe83cf747e06d8366b753dab36c69e9d7e90c63ed27897656f`，802 行、11 个 GPUI 测试属性。11 是静态计数，不是 Actions 发现或执行数。
- 五文件测试 payload manifest SHA-256：`d9b4bd8193cb4521d7406f2ce51449b364fca058abf0814adcac7607f374ee6d`；文件清单及哈希见修订证据。
- 相对更新基线的完整测试 patch SHA-256：`541f34e2af8fb933c5b93a97ba812ff0ce5812ad4c5d72a57d568fa9398caa03`，39,747 bytes。构造顺序沿用原 writer 证据：tracked Cargo.lock / vega_ui manifest / message_anchors.rs / mod.rs 的 binary full-index diff，再拼接测试新文件相对 `/dev/null` 的 diff。测试来源提交与可访问源码链接待提交后在 PR 固定；本机临时副本不能替代远端可恢复证据。

## 本轮主 Agent 本地静态检查

2026-10-22 UTC，在上述停写测试版本上实际执行：

| 命令 | 结果 | 边界 |
|---|---|---|
| `git diff --check` | 退出 0 | 已跟踪差异无空白问题；新测试格式另由 rustfmt 检查 |
| `cargo fmt --all -- --check` | 退出 0 | 格式/解析，不是编译或行为证据 |
| `cargo metadata --no-deps --offline --locked --format-version 1` | 退出 0 | 离线元数据解析，不构建 |
| SHA-256 / patch / manifest 重算 | 与作者报告相符 | 只证明内容身份 |
| 生产范围差异检查 | `core.rs`、`render.rs`、`content.rs`、PR workflow 与 HEAD 无差异 | message_anchors.rs 仍仅透明测试观察包装，未改选择算法 |

本轮未运行 Vega、编译、构建、打包、Clippy、cargo check、Rust/Python 测试、nextest 或 benchmark。

## 独立审查与云端阶段

- 独立测试增量 reviewer：`01M5JB0VZGJH448QXX457Z6KNC` / execution `01M5JB0VZG095BJPSQT53FFHJM`，只读完成上述精确版本并停写，结论静态可接受、无阻塞提交 draft PR 的测试接线或 fixture 缺陷。详见独立审查记录；B3 连续测量防护仍为后续生产静态审查义务。
- RED：首轮 Actions 未到行为测试，具体见下文；目标行为有效 RED 仍待取得。
- 实现：未开始；必须先确认目标行为导致的有效 RED。
- 同一测试 GREEN：未取得。
- 测试来源可恢复性、独立删测提交、最终 head 0 发现/0 执行完整门禁：待前述阶段完成。
- Issue 保持开放、In progress；最新 head required check 与独立审查均通过后才可合并，合并后 In review 等用户手测，不以自动关闭关键字收尾。

## PR 与可恢复来源

- 先行测试提交：`47611acbc93011486d38897e57f731989318c634`；实际生产 parent 为 `9b901a34ee00611f73fbd58275c7087282af5e2c`。
- 提交前同步 #547 的图片预算、初始输入压缩 guard 和错误文案。独立增量 execution `01M5JBM7B88WMA3S8QKKSVA3FA` 完整读取生产 delta，确认未改 UI/GPUI/API/依赖或本 fixture 路径，payload 与前审逐字节相同，前审结论可复用。
- [原测试源码](https://github.com/puzige/vega/blob/47611acbc93011486d38897e57f731989318c634/crates/vega_ui/src/conversation_stream/message_anchor_tests.rs)远端 contents 读回 Git blob `a8bb855620c9475a0695e5a403205ba447000992`，与本地一致，29,611 bytes。
- [完整测试 patch 留存](https://github.com/puzige/vega/pull/562#issuecomment-6347454344)已远端读回，SHA-256 `541f34e2af8fb933c5b93a97ba812ff0ce5812ad4c5d72a57d568fa9398caa03`、39,747 bytes，与本地一致。分支删除及 artifact 过期不影响该来源。

## 首轮 CI 失败诊断：测试接线，不是有效 RED

- head：`47611acbc93011486d38897e57f731989318c634`。
- [run 39826148101](https://github.com/puzige/vega/actions/runs/39826148101)；[Clippy job 119989840115](https://github.com/puzige/vega/actions/runs/39826148101/job/119989840115)、[Nextest job 119989841133](https://github.com/puzige/vega/actions/runs/39826148101/job/119989841133)、[aggregate 119991992896](https://github.com/puzige/vega/actions/runs/39826148101/job/119991992896)均失败。
- 原始诊断：测试文件第2行 `use gpui_kit as gpui;` unused import（Clippy `-D warnings` 变为 error）；第166行 `cx.observe(&stream, |_, _, cx| cx.notify())` E0282，宿主泛型尚不能从第170行 `host.requests` 推断。
- `cargo nextest list --workspace --message-format json` 构建测试目标时，底层 `cargo test --no-run` 退出101。未完成发现，未执行断言，发现/执行/通过/失败/忽略/跳过计数均未取得；不是成功空集，也不是行为 RED。
- 主 Agent 已读取两份原始日志并分类责任层。[PR 失败记录](https://github.com/puzige/vega/pull/562#issuecomment-6347603463)保留原错误与计划，不盲目重跑。
- 独立 reviewer execution `01M5JC689XY6SD068P3N0X29BX` 复核真实 GPUI `add_window_view<F,V>` 签名及 observe/subscribe，准入严格两处测试接线修正：删除 unused alias；在原 build closure 显式 `cx: &mut Context<AnchorHost>` 固定 V。不改变 fixture/动作/通知/断言/验收；不是改预期，也不需重开验收周期。修订产物仍须停写增量复核，编译/行为结果仍交 Actions。
- 同次同步 master `6615748f91273bfb8ee424011b3b5a09b3777d2b` 的 #546，以普通 merge `0c0b21cb3642bbd40cfc3b5d33c826c98f557877` 保留原测试来源。实际源码增量仅 theme 三个 Composer trigger token 与 render.rs 模型/权限 chip 样式；锚点挂载顺序未变，测试不渲染 Composer。独立 reviewer 确认可复用已有测试审批及 B2/B3 义务。
- 原 writer 通过 follow-up `01M5JCD0RW16GHZMKYM561TAZ9` 仅接续上述两处修正；主 Agent 不并写测试代码。作者已 READY 停写，测试新 SHA-256 `c84c479c6f8f3b6d00290f7091abebd1537dcf7142cd4ab3f86e6ebdc2651565`，29,615 bytes、801行、11个属性。
- 独立产物复核 execution `01M5JCG77F3NXPNZSGBKQDFZVV` 确认精确diff仅两处；内存反向还原后与前审输入逐字节一致，全部fixture/动作/notify/断言不变，无新阻塞。主 Agent 在此版本实际执行 `cargo fmt --all -- --check`、离线 locked/no-deps metadata、`git diff --check` 均0；无本地编译/行为执行。下一次 Actions 原始结果仍待取得。
