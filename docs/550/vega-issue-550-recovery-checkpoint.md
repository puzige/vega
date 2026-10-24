# #550 补齐执行异常后的保存检查点

- 阶段：原完整先行测试补齐，非 READY、无有效 RED/GREEN、无生产实现。
- 时间：2026-10-22T19:24Z。
- 分支/HEAD：`feat/550-disclosure-motion` / `6615748f91273bfb8ee424011b3b5a09b3777d2b`。
- 续接执行已工具确认 failed，未给具体原因；执行已结束、停止写入。父 Agent 不推断产品/测试失败，保留全部工件；fetch 后已确认基线无新增。

## 保存的内容

原 16 测试文件 SHA-256 仍为 `4e29d257bad073cfd64d70989b1c00e618a8053485fc5ec7a28cdf8b465dacc4`，没有新增或删减原断言。新保存支持包括：

- `disclosure_paint_probe::observe_configured_icon` 接受真实图标的配置 rotation，旧普通 `kit_icon` 仍记录准确零旋转；新增只在 cfg(test) 中调用实际 KitIcon.rotate 的阳性对照构造接点。
- `disclosure_layout_probe` 记录现有 invalidate/remeasure 边界及透明 list item request_layout/bounds；不改原调用。
- 五类实际 header 增加 cfg(test) 原生 TestSupport 接线，目标是读取真正 Element aria-expanded，非自制语义值。
- `vega_conversation/test-support` 默认关闭，新增纯内存 ArtifactCard/preview 投影 fixture；测试期 nonce/seal 和投影沿 owner 规则，非真实 Git/文件权限证据。
- 新支持尚未消费于完整测试、未做最终格式/类型/运行验证；首次 writer 检查点只对应旧部分包，不代表本次新内容。

按路径排序的 `SHA256  两空格  相对路径\n` 清单（20 个变更/未跟踪文件，不含本检查点）SHA-256 为 `29816f8a7f413315fe9557cdfe163e33f1ca9940f6fbe221bdee5badbd579c58`。关键 hash：

| 文件 | SHA-256 |
|---|---|
| `docs/550/vega-issue-550-disclosure-motion.md` | `d30a0308791c63589e7ff5ddea43e2c9f8c3a87011c22c3651ab82407eb38b33` |
| `crates/vega_ui/src/disclosure_paint_probe.rs` | `4e656db637e20755cf6fa258b6b6f5a722c4214246b15be3f55b5df00a8f97a6` |
| `crates/vega_ui/src/disclosure_layout_probe.rs` | `c4482295f67a7e65d57b1e754b9d257cbd7246db1439a946a8cd3ad51d1b6651` |
| `crates/vega_conversation/src/artifact/test_support.rs` | `5b639b7624468000465998928fd370288c2d3802dc137736a4fec30211077c94` |

## 下一步（不扩大原验收）

单一替补 writer 只补 [规格 §5.1](vega-issue-550-disclosure-motion.md#51-父-agent-停写后诊断与最小补齐契约) 与首次检查点表中的未齐测试：旋转配置/语义/测量 recorder 阳性对照；D06 真实对照；D09 产物焦点；D04 键盘/disabled；D05 无 inactive draw 恢复与 shimmer/离屏；D07 真上滚前置；D10 权限入口；D11 长流式/输入。保留原 16 测试及全部 D01–D11，更新真实范围和准确静态证据后停止写入。未知 API 只定点读，不重做全仓调查。

所有支持只开发期、默认关闭或 cfg(test)，普通生产展开/Final/权限/输入/列表算法不得变化；跨 crate fixture 不作安全集成证明。不能修改 registry、添加第三方依赖/CI设施、运行真实外部进程/网络、本地 Vega/编译/测试/CUA。完整停写版本经非写入者审查后才由父建立现有 draft PR/Actions；环境/观察故障不算产品 RED。

## 第二次补齐执行的保存版本（2026-10-22T19:48Z）

替补执行已由工具确认 failed 并终止，无具体错误详情；不推断产品失败。已保存原 16＋新增 12，共 28 个 GPUI 测试属性，包括旋转/语义/测量/产物/键盘/恢复/shimmer/权限/流式组合。测试 SHA-256 `db70f73a94dbee5adc32c9c067b5f0d8524290424603c3b9f4a5211f79a65ea8`；22 文件清单（含本文件旧版）SHA-256 `85b9ea2a19ca041812064d9a2f71d62efca2bbe11e5096e33b18cd8267d14a72`。旧首次 writer 文档未更新为当前交接。

父已读取新增测试并发现最后静态格式尚未齐，formatter 检查输出仅排版差异；该管道输出不当作 exit 0 的验证。尚未编译/执行，类型/夹具/Actions 均未取得。下一 writer 只做窄静态核对、原 28 测试前置与完整交接，不重做/新增验收。

- 父已 fetch 并安全 ff 至 `5fb9a3d0491c620a901e67a0e3f348ba05bd76c6`（#556），保留原测试。新增 runtime/conversation/store 行为不能回退；本卡目标 UI/terminal 源无冲突。权限请求新增 activity 可选边界，默认 PermissionQueue 仍无 activity，实际字段/调用以新基线核对。
- 跨卡当前有效故障证据：#552 首轮 Actions `39842518890` 在测试宏生成的裸 `#[test]` 递归展开时失败（不是 RED）。锁定 gpui-pre-macros 0.3.4 的 test.rs:186/279 输出裸属性；本卡 `use super::*` 需核对宏导入与 `gpui` 别名，不能重蹈错误或提高递归上限。修接线不改测试含义。
- 真实 PermissionQueue::request 同步入队并返回 future，fixture 的 request 未先 poll 并不是错误；请求需要有效 notifier，这一点必须核对实际 Stream 初始化。不要误改成镜像队列。
- 原 28 个测试断言保留，若有 API/夹具错误只修责任层，不加生产实现，不删除困难用例。交接须将本段与首次部分包区分，报告完整 hash/静态退出码/仍未运行和停止写入。
