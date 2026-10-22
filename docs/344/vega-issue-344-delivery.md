# #344 交付与云端验证记录

## 当前阶段

先行测试修订已停写并通过独立增量静态审查，准备提交 draft PR 取得有效 RED；生产行为未改，尚无本轮有效 RED、GREEN、合并或用户手测结果。

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
- RED：待 draft PR 现有 Actions。没有 run/job、原始失败或发现/执行计数，不宣称已取得。
- 实现：未开始；必须先确认目标行为导致的有效 RED。
- 同一测试 GREEN：未取得。
- 测试来源可恢复性、独立删测提交、最终 head 0 发现/0 执行完整门禁：待前述阶段完成。
- Issue 保持开放、In progress；最新 head required check 与独立审查均通过后才可合并，合并后 In review 等用户手测，不以自动关闭关键字收尾。
