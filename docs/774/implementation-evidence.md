# #774 实现与验证记录

## 固定版本

- 原始基线：`5d042fcd861a00af97486e948f0cc6291f33ca8f`
- 带开发回归的实现：`72c0b8ee3cad9814f0d6bcf3b59c7f7ccec9234c`
- [PR #780](https://github.com/puzige/vega/pull/780)，规格见 [conversation-timestamps.md](conversation-timestamps.md)。
- 本地只做静态差异、定向 rustfmt 和 TOML 解析；未运行 Vega、编译、check、Clippy、测试或打包。

## 开发回归来源（清理前确认 GitHub 可访问）

| 源码 | 数量 | SHA-256 |
|---|---:|---|
| [响应时钟／事务／重试／手动断点](https://github.com/puzige/vega/blob/72c0b8ee3cad9814f0d6bcf3b59c7f7ccec9234c/crates/vega_store/src/messages/response_starts/tests.rs) | 5 | `90f881af083df660597a37a45befc93f48e51c390e03766a62aaee347c6daf3f` |
| [日期／locale 格式化](https://github.com/puzige/vega/blob/72c0b8ee3cad9814f0d6bcf3b59c7f7ccec9234c/crates/vega_ui/src/conversation_stream/timestamp_format/tests.rs) | 9 | `018d284a3fa53fd3ca76292391cc29e04ba624e89833ef89513dee95e066500d` |
| [阈值／断点／页头重算](https://github.com/puzige/vega/blob/72c0b8ee3cad9814f0d6bcf3b59c7f7ccec9234c/crates/vega_ui/src/conversation_stream/timestamps/tests.rs) | 9 | `52f2a99a8ed8cf3310524b5f40208eb54ca1c6e5d345e771d944a14476c7531c` |

测试通过后移除上表 23 条及三个 `cfg(test) mod tests` 接线；生产 `TimestampAdjacency`、`format_from_calendars` 均由真实功能使用，保留。无新增测试 feature/dev-dependency、mock 或 executable doctest。

基线 #694 的 10 条诊断测试受[该卡用户单独授权](https://github.com/puzige/vega/issues/694#issuecomment-6518598138)保留，本卡不删除／修改／恢复。故最终 Rust 集合预期非空，不声称全仓 0 测试，也不把该例外推广到本卡。

## 实际云端结果

[Actions 41045804549](https://github.com/puzige/vega/actions/runs/41045804549)，head `72c0b8ee3cad9814f0d6bcf3b59c7f7ccec9234c`：

- Format、`cargo clippy --workspace --all-targets -- -D warnings`、required `check (fmt, clippy, test)`：成功。
- Nextest：**33 发现、33 执行、33 通过、0 跳过**（本卡 23 + 基线 10）。原始 footer：`Summary [1.021s] 33 tests run: 33 passed, 0 skipped`。
- Python 三组：5 + 15 + 40 = **60 通过**。
- Nextest job `124029649394`，Clippy job `124029649853`，required job `124033006259`。
- 清理后最终 head required CI：待执行，不能复用以上带测试版本结果冒充最终门禁。

## 失败与修正留痕

- [41042062233](https://github.com/puzige/vega/actions/runs/41042062233)，`1489ba2bd70f8bbbbec686acd93fcb93b7ed7151`：编译完成，27 执行／25 通过／2 失败／0 跳过。两个测试遗漏前置用户，触发规格首用户特例；补前置用户隔离相邻阈值，保留阈值断言，另补缺时间抑制特例回归。Clippy `let_and_return` 已直接删除冗余绑定。
- `271ae6c0258bec424b7cafad9a11be06e13bd026` 的 [41043257688](https://github.com/puzige/vega/actions/runs/41043257688) 被后续 head 取消，无通过结论。
- [41043590363](https://github.com/puzige/vega/actions/runs/41043590363)，`91e9cff0424ba87edda61dfe379ba92dd4f7ffb6`：开发测试把更新行数当 bool，编译失败；按真实 API 改为精确断言 `1`。
- [41044622631](https://github.com/puzige/vega/actions/runs/41044622631)，`124789938c6f91c4929c0ddec9f9dd832f151ff4`：SQL 读取 usize 缺 FromSql，编译失败；改 i64 + checked usize 转换，不扩依赖特性。
- `92e168186b4d2669e85c4a1c91f097cb0156e4da` 的 [41045368257](https://github.com/puzige/vega/actions/runs/41045368257) 被后续修正版取消，无通过结论。

## 独立审查

非实现写入者按固定 SHA 审查，静态发现自动压缩嵌套事务、Codex 用户 echo 未绑定持久化 ID、手动压缩未保存历史断点；均已修复。增量审查又定位序号 u64/i64 及 SQL usize 类型阻塞，受检转换解决。

最终增量复审版本 `72c0b8ee3cad9814f0d6bcf3b59c7f7ccec9234c` 相对 `124789938c6f91c4929c0ddec9f9dd832f151ff4` **无发现**。静态审查不替代云端类型检查或实际 UI/DST 验证。

## 待用户手测／限制

- 原生 hover、键盘焦点、仅图片、禁用动作、复制／选区排除；分页接缝与滚动锚点。
- 历史与实时／queue／steer／重开／Fork 时间一致性；真实系统时区及 DST 转换。
- civil DST fixture 只验证日期格式，不证明系统时区转换。未运行本地原生 UI 或读屏。
- GPUI 无 Separator 角色，用 Label + 完整日期时间标签；未宣称 machine-readable time 语义。
- 旧响应缺开始时间则隐藏；旧压缩／Fork 缺精确锚点不推造。Codex 助手缺响应开始事件，按确认的缺时间策略隐藏；用户权威时间正常透传。
- 合并后仍为 In review，Issue 保持开放，用户明确手测通过后才关闭／Done。
