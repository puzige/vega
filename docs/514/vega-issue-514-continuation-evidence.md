# #514 续跑证据与阶段记录

## 当前状态（2026-10-22）

- 同一卡、同一 Draft [PR #529](https://github.com/puzige/vega/pull/529)，不重新启动任务。Issue OPEN，Project In progress；尚未 GREEN、正式 PR、合并或用户验收。
- 本轮计时起点：2026-10-22T06:32:17Z。恢复并核对历史工作、现行规则、原始 CI 失败及实际接口；2026-10-22T06:53:15Z 已完成 v3 规格差异静态检查。后续阶段按真实结果追加，不把委派受理当作完成。
- 公开旧 head：`ae1888267fd8b05db1c388c01ed80bfc40643a1b`。本地同步 head：`7ec15f2c44269d7e068e853ed797f651668dc019`，merge 主库 `e4f18c656aa69d192d095a4338a2108e6c9a3c2c`，保留已有测试/实现提交，无 force-push。
- v3 规格 SHA256（先行测试前）：`cf733965f647607b95fec9f95177e20334a01de1894b6e59ac39a90fca50e159`。代码实现仍由专职子 Agent；独立审查默认只读、绑定停写版本。
- 当前本轮委派：spawn 2（一个测试/观察修正写入者，一个独立审查者），followup 4（独立审查新停写测试补丁；有效 RED 后同一写入者实施最小 mdstream 0.3 补丁；独立审查固定 UI 生产差异；独立审查新停写 mdstream 补丁）。不重复历史已完成工作。解析生产实现仅在下述本轮有效 RED 确认后授权。

## 阶段与委派记录

| 阶段/调用 | 精确版本与新工作 | 实际时间/结果 |
| --- | --- | --- |
| 恢复、规格与接口核对 | 同步 head `7ec15f2c44269d7e068e853ed797f651668dc019`；v3 spec SHA256 `cf733965f647607b95fec9f95177e20334a01de1894b6e59ac39a90fca50e159`；读回两项旧失败并分层 | 06:32:17Z 起；06:53:15Z 规格静态检查退出 0；测试/实现未执行 |
| spawn：专职写入者 | 公开解析验收的先行测试 + 无头字体 test-only 观察修正；source diff SHA256 `f06d0ec343fd407b0595b5d7f03a64e4eb9628c366123b9731969849da2cafbf` | 06:54:52Z–07:25:09Z，30m17s 主动工作；停写 READY；5 个新解析测试 + 保留 19 个定义；未执行 |
| followup：同一独立审查者 | 上述新停写测试补丁，验证公开断言/字体实际输入观察/生产不变 | 07:27:00Z–07:32:27Z，5m27s；PASS，无实质阻碍，HEAD/patch/7 个文件 hash 首尾一致；只读 |
| spawn：独立审查者 | 仅审停写 v3 规格与实际 API 可行性，不审移动测试 | 06:55:33Z–07:00:30Z，4m57s；规格通过，无实质阻碍；源码/HEAD/hash 首尾核对一致；只读 |

规格审查不替代测试或实现审查。缓存身份观察应选已 committed 的非空节点切片并结合内容/version；字体观察需绑定实际 consumed runs 与已布局文本，不从 token 反推结果。主 Agent 于 07:26:17Z 核对 HEAD、7 个源码及 writer evidence SHA256，与 READY 一致；差异静态检查通过。原有复制预期不变，解析生产代码尚未改变。测试补丁独立审查已通过，可提交并复用 Draft PR 取得本轮 RED；该 PASS 不是 GREEN 或生产实现批准。

本轮返工/诊断：尚无产品修复返工、CI 失败或验收放宽。写入者首次格式工具命令将 `+1.98.0` 传给直接 rustfmt 二进制，导致工具调用退出 1；核对后改为 `rustup run 1.98.0 rustfmt`，实际格式与最终检查退出 0。此为命令层纠正，不算 RED；详见 writer evidence。07:12Z 已观察到 test-only 产物，07:18Z 新解析测试生成，不属于连续 20 分钟无新产物。父会话等待子任务不计作其主动实现时间；下轮 CI 等待另记。

## 两项旧失败的分类

原始 [Actions 39578674519](https://github.com/puzige/vega/actions/runs/39578674519)，Nextest job `119150532907`：19 发现/执行，17 通过/2 失败/0 跳过；Clippy job `119150532737` 成功。续跑已读回原始失败，不将旧证据冒充新周期验证。

1. `nonparagraph_first_list_items_copy_markers_in_visible_order`：非引用 Selectable 复制在代码与 `after` 之间多一个换行。静态根因为 mdstream 0.3 的列表围栏边界误拆；责任层为产品依赖，待新公开解析断言的云端 RED 确认。不 trim、不吞真实空行、不改 UI 复制预期。
2. `heading_first_list_items_keep_visible_marker_and_heading_style`：H1 字号及 marker 断言后，shaped run FontId 反查返回 None。GPUI NoopTextSystem 请求 FontId(1)，但 layout_line 忽略输入字体并生成 FontId(0)；责任层为测试观察。以真实渲染输入 TextRun.font 绑定已布局文本，保留原字重验收，先独立审查修正。

## v3 本轮有效 RED 与测试冻结

- 测试提交：`ea64ee4dc3cb682d67c84bf2963cf83c79acd59d`；生产未改基线 `7ec15f2c44269d7e068e853ed797f651668dc019`。停写补丁经独立审查 PASS 后提交/push，源 hash 与审查版本一致，GitHub 解析测试 blob 读回为 `df70d792c4f09e0f12395562887c73bc895c3b2e`，与本地一致。
- [Actions 39741381663](https://github.com/puzige/vega/actions/runs/39741381663)，Nextest job `119699148836`：日志 Starting 24 tests / 15 binaries，24 执行、21 通过/3 失败/0 跳过。失败 summary 未独立输出 list JSON 清点数；不把源码定义或未取回 JSON 值冒充清点证据。完整发现命令成功，实际启动和执行 24 项。
- Clippy/fmt job `119699149068` 成功；Python 5+15+40=60 项成功；required summary `119702057710` 因行为失败阻断。
- 新解析完整输入失败：实际多一个 `CodeBlock(None, "\nafter\n")`，预期为普通 after 段落；中间关闭围栏快照失败：实际多一个空 CodeBlock。原 UI 复制失败原样保留。三个均为产品边界行为失败，无编译或观察故障，构成本轮有效 RED。
- H1 精确字重/字号/marker/普通正文对照通过；原 16 项通过；真实代码空行、真实顶层围栏、后置引用控制通过。冻结全部测试，不按输出更改断言。
- 云端 run 07:34:03Z–07:41:34Z（7m31s）；Nextest job 07:34:06Z–07:41:27Z（7m21s），实际执行 1.128s；Clippy job 07:34:06Z–07:38:38Z（4m32s）。主会话 07:39:36Z 查询运行中，07:44:57Z 读取完成，符合五分钟节奏；从 push 07:33:57Z 到确认完成 07:44:57Z 的本轮 CI 等待墙钟为 11m00s。
- 原始证据已固定并读回 [PR comment](https://github.com/puzige/vega/pull/529#issuecomment-6335569822)。07:46:33Z 安全 fetch，master 未漂移。随后向同一停写写入者发送具体实现 followup，范围限定 vendor/mdstream、Cargo path patch/lock 来源、来源说明，不触碰冻结测试或 UI 生产。

## v3 实现与新发现的 v4 边界

- mdstream 写入者 07:47:50Z–08:10:49Z，22m59s 主动实现（含静态工具纠正）；停写 READY，source patch SHA256 `8cc3af5dd3ef76d2a27b94f542d658697554b482dec49741b8b6afc60e9e0249`，manifest SHA256 `c4b8ab9a4469078119ac76997bcd2ebb00355c8c932cb7254b11b91b571075d9`。43 个 vendor 文件 + root Cargo patch/lock，39 个 Rust 源来自确切发行包；唯一逻辑增量是列表延续围栏不 interrupt。测试/observer/UI 与冻结 head 逐字相同。
- 主 Agent 08:12:51Z 独立复算补丁 SHA256、许可证 Git blob、静态格式/差异检查，全部一致/退出 0。新 mdstream 停写内容已交同一独立审查者，只审该新语义与 vendor/锁文件增量；可与同版本云端 GREEN 并行，未完成不称通过。
- 独立 UI 生产审查在固定 `ea64ee4dc3cb682d67c84bf2963cf83c79acd59d` 上进行，07:50:05Z–08:02:37Z，12m32s，只读。其余引用连续/代码分组/marker/选序/Thinking mask/culling/缓存未见额外实质问题；发现新的 List→Quote 祖先顺序导致 rail 丢失列表缩进，阻断最终交付。
- 主 Agent 08:03Z 读回原分支及 master 相关代码确认责任层为 UI 产品缺陷，不是 mdstream/测试接线故障。先更新 v4 spec（SHA256 `b4db40ec42db9ad0508d1defe0e9f56a52ba9da60b4da7a73d4755e29ec8d4fe`），列 quote-in-list/list-in-quote/深层算术与独立先测后修周期；本 mdstream 写入者未改 UI 或测试。原始发现记录 [PR comment](https://github.com/puzige/vega/pull/529#issuecomment-6335907535)。先取得 v3 同24项 GREEN 再写 v4 新测试，绝不 retroactively 改24项或把本轮 RED 当 v4 证据。
- 新返工分类：v4 属遗漏边界/产品回归，单独重开周期；mdstream 实现中直接 Cargo 的 toolchain selector 和格式 file/stdin CRLF 差异为静态工具层纠正，没有新 CI 失败或验收放宽。具体原始记录见 mdstream patch evidence。07:58Z vendor 新产物、08:11Z patch evidence 已观察，不是持续20分钟无产物。

## mdstream 0.3 来源核对（只读静态证据）

- 锁定版本与用户裁决：`=0.3.0`，不迁移 0.4；现有技术规格 §5.0 已列 vendoring 预案，无新 crate。
- crates.io 发行包 `mdstream-0.3.0.crate` 实测 SHA256 与 Cargo.lock checksum 相同：`f3327364edd79c4f66b36db667c0a28e078389351b0855e5f95ed4f22d778821`。
- 发行包 `.cargo_vcs_info.json`：上游提交 `950d0926f54667f04aed87fad6cc9921aa74ef5c`，子目录 `mdstream`。
- 上游固定提交许可：[MIT](https://github.com/Latias94/mdstream/blob/950d0926f54667f04aed87fad6cc9921aa74ef5c/LICENSE-MIT) blob `f7c57f575b1b78ac893b339ffafd9effe4bab286`，[Apache-2.0](https://github.com/Latias94/mdstream/blob/950d0926f54667f04aed87fad6cc9921aa74ef5c/LICENSE-APACHE) blob `cd9f8ad8d319325bf9237d5195735eebbfcdcfdc`。已读回 MIT 授权与原作者归属，发行 manifest 声明 `MIT OR Apache-2.0`。
- 采用 vendor 时保留许可与可重建来源，剔除不被生产使用的 upstream 测试/接线，记录与发行包的语义差异；不整套恢复历史 Rust 测试或引入 upstream dev dependencies。

## 本地与云端边界

本地仅做差异、格式和静态元数据检查；未运行 Vega、编译、Clippy、Rust/Python 测试、build 或打包。新增解析测试已取得上述本轮有效 RED，GREEN 尚未取得；后续只使用现有 PR Check，启动后五分钟状态查询，保留首次失败原始证据。

最终交付前仍需：同一冻结测试云端全部 GREEN → PR 固定原始源码/完整 SHA/run/job/准确计数 → 独立删测提交及审查 → 最新完整 head 的 fmt、全目标 Clippy、Python、Rust 0 发现/0 执行 `no Rust tests` 门禁 → 正式 PR、squash merge → Issue OPEN / In review 等用户手测。空集不称 Rust 测试通过。
