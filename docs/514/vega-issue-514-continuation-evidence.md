# #514 续跑证据与阶段记录

## 当前状态（2026-10-22）

- 同一卡、同一 Draft [PR #529](https://github.com/puzige/vega/pull/529)，不重新启动任务。Issue OPEN，Project In progress；尚未 GREEN、正式 PR、合并或用户验收。
- 本轮计时起点：2026-10-22T06:32:17Z。恢复并核对历史工作、现行规则、原始 CI 失败及实际接口；2026-10-22T06:53:15Z 已完成 v3 规格差异静态检查。后续阶段按真实结果追加，不把委派受理当作完成。
- 公开旧 head：`ae1888267fd8b05db1c388c01ed80bfc40643a1b`。本地同步 head：`7ec15f2c44269d7e068e853ed797f651668dc019`，merge 主库 `e4f18c656aa69d192d095a4338a2108e6c9a3c2c`，保留已有测试/实现提交，无 force-push。
- v3 规格 SHA256（先行测试前）：`cf733965f647607b95fec9f95177e20334a01de1894b6e59ac39a90fca50e159`。代码实现仍由专职子 Agent；独立审查默认只读、绑定停写版本。
- 当前本轮委派：spawn 3（原测试/实现写入者、独立审查者、保留同补丁的恢复收尾写入者），followup 14（前13次记录不变；新增停写parser-only实现的独立审查）。不重复历史已完成工作。解析生产实现仅在下述本轮有效 RED 确认后授权。

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
- 主 Agent 08:03Z 读回原分支及 master 相关代码确认责任层为 UI 产品缺陷，不是 mdstream/测试接线故障。先更新 v4 spec（SHA256 `b4db40ec42db9ad0508d1defe0e9f56a52ba9da60b4da7a73d4755e29ec8d4fe`），列 quote-in-list/list-in-quote/深层算术与独立先测后修周期；本 mdstream 写入者未改 UI 或测试。原始发现记录 [PR comment](https://github.com/puzige/vega/pull/529#issuecomment-6335907535)。原计划先取得 v3 同24项 GREEN；在下述残余 pending 缺陷与 v5 review 发现后，先在当前实现上补齐 v4/v5 独立先行测试再修，原24项冻结不改，也不把 v3 RED 当 v4/v5 证据。
- 新返工分类：v4 属遗漏边界/产品回归，单独重开周期；mdstream 实现中直接 Cargo 的 toolchain selector 和格式 file/stdin CRLF 差异为静态工具层纠正，没有新 CI 失败或验收放宽。具体原始记录见 mdstream patch evidence。07:58Z vendor 新产物、08:11Z patch evidence 已观察，不是持续20分钟无产物。

## v3 残余失败及 v4/v5 先行测试重开

- 原 vendor 停写补丁已提交/推送为 `27b88c2e9934a01aa912ed95fc26b6ec3fd3470a`。独立 mdstream 审查 08:13:59Z–08:20:45Z，6m46s，独立确认来源/39文件/38文件token等价+唯一guard/367literal/291comments/双许可/924依赖图及冻结测试不变；另发现 guard 的两空格heuristic错误吞入较宽列表marker后的 dedented 顶层围栏，阻断，不标审查通过。
- [Actions 39746319972](https://github.com/puzige/vega/actions/runs/39746319972)，Nextest job `119715543602`：24 项启动/执行，23 通过/1 失败/0 跳过，执行1.179s；Clippy `119715543343` 与 Python60项成功，summary `119722007987` 失败。原UI复制、完整围栏输入已通过；实际日志明确编译本地vendor/mdstream0.3，证实patch生效。
- 残余是原冻结 `list_fence_snapshots_keep_unrelated_committed_nodes_frozen` 在 tilde/unquoted/未闭合首快照，多 `Paragraph("~~")`。主 Agent 08:31Z 读取原始失败，08:32Z 核对 pending pipeline/repair/context/inline：List pending终止器只识别反引号代码区，误以~~~为未闭合删除线，追加合成~~。产品pending repair，不改断言或UItrim；原v3冻结已覆盖并有效RED，不为该原目标事后补测。
- 新 parser dedent 边界为独立 v5，已先修规格（v5含两项新独立周期，SHA256 `1d259ed160b99cc483e0550f9d7c5e8bdadebb841c33edbb6539c91120d0edc4`）；v4/v5 可同一先行测试提交但各自须在云端观察目标失败，原24项保留。08:35Z 向同一已停写writer发送具体 TEST-ONLY followup，生产冻结不动，先新测试/独立审查/RED再实施。
- 返工归因：本次v3实现1次尚未GREEN，残余是原目标分支被早先backtick失败遮蔽而新可见；v4/v5为独立审查发现的两项产品/遗漏边界，分别重开周期，不放宽验收。不是外部CI或无头观察故障。不因原test函数再次失败自动重复盲修；已读原日志、识别不同首失败及责任层，下一验证为新边界测试各自RED。
- 第二轮CI run创建08:14:50Z，Clippy08:14:53Z–08:19:01Z（4m08s），Nextest08:15:14Z–08:30:30Z（15m16s），summary08:30:32Z–08:30:35Z。主会话08:20:21Z、08:25:20Z、08:30:34Z状态查询，首次两者相隔4m59s，为命令计时边缘，后续改用查询完成后完整sleep300s确保不少于5min；无watch/短循环。push08:14:44Z到确认Nextest完成08:30:34Z等待墙钟15m50s。
- 上述结果与责任层已固定 [PR comment](https://github.com/puzige/vega/pull/529#issuecomment-6336398417)。仍未全体GREEN/删测/正式PR/合并。

## v4/v5 冻结前方案审查

- 同一独立审查者 08:38:47Z–08:45:50Z，7m03s；只读固定 `27b88c2e9934a01aa912ed95fc26b6ec3fd3470a` 与 v5 spec `1d259ed160b99cc483e0550f9d7c5e8bdadebb841c33edbb6539c91120d0edc4`，未看移动测试。确认可通过现有逐行 BlockMachine 维护实际marker内容缩进与活动围栏，pending display共用该上下文，避免固定两空格/全buffer字符计数/新增全量解析。
- 审查补出一项冻结前实质边界：真实列表围栏 body 内的 `# literal`/空行也必须保护，不仅 dedented 顶层围栏。主 Agent 已在尚未冻结的新 v5 用例前补明确公开树与中间快照验收，后续 spec SHA256 `50fdca3643ed8c11c9655ee8a16680388d2f03589b3efafcda046d9426084aa8`；原24项不改。实现仍未进一步修改。
- 同一writer 08:36:44Z–09:08:42Z，31m58s 主动 TEST-ONLY，已停写 READY；在原两个测试文件后只追加133/307行，原10801/35114字节前缀逐字不变。新7项定义（4UI+3parser），总31源码定义，不冒充发现或执行数。测试diffSHA256 `b5df8f9f475cb82e5821806613e93423d3eaa92861a735ffb4a1b695dbc8c3cf`。原24项、生产、全部47vendor文件、Cargo/workflows不变。期间补齐冻结前澄清，无生产改动。
- 主 Agent 09:10:14Z 核对 HEAD/source/spec/evidence hashes一致、差异检查退出0；独立测试增量审查09:11:12Z–09:16:44Z，5m32s，PASS无实质阻碍，确认实际pending/final树/三路径几何/围栏literal观察与全部冻结断言。各自云端目标RED仍待取得，下一步提交/push精确版本。

## v4/v5 有效 RED 与完整31项冻结

- 测试提交 `e7afdfbcf40263f13831766148cc0bd58de9dc67`，基线 `27b88c2e9934a01aa912ed95fc26b6ec3fd3470a`；审查版本原样提交/push，GitHub parser test blob `b06a05f1122b3d37bd912d253cce317747776ad1` 与本地一致。
- [Actions 39754408243](https://github.com/puzige/vega/actions/runs/39754408243)，Nextest `119742673340`：Starting31/15binaries，31执行/25通过/6失败/0跳过，0.790s。Clippy/fmt `119742672896` 与 Python60项成功，required `119745242082` 因目标失败阻断。
- v4真实pending/final解析前提通过后，Ordinary实际rail ListQuote0（预期24）、NestedListQuote0（预期48）、QuoteListQuote内14（预期38、外0正确）。QuoteList外rail不跟随列表的控制通过。是几何产品RED，非fixture或观察故障；其余路径/resize/16gap全部原断言保留待GREEN。
- v5dedent2/backtick/whole实际list10后空code+Headingliteral+吞aftercode，预期字面code+after段落；真正列表围栏追加#literal实际拆出Heading，预期同一code字面内容。4/5列真延续/空行控制通过，构成v5独立有效RED。原v3tilde未闭合多~~同一根因继续失败；本次生产未改，仅新增测试，不作为盲重试。
- run09:18:34Z–09:24:33Z（5m59s）；Clippy09:18:36Z–09:23:26Z（4m50s）；Nextest09:18:37Z–09:24:27Z（5m50s）。查询09:24:14Z、09:29:31Z相隔5m17s。push09:18:29Z到完成读取09:29:31Z本轮CI等待墙钟11m02s。
- 主 Agent 读回上述原始错误与controls，确认责任层；09:30:49Z fetch master未变，随后向同一writer发送具体冻结31项生产修复followup，UI祖先原点与mdstream实际marker/fence/pending上下文分清作用层。无删测/放宽/新依赖/0.4。证据固定 [PR comment](https://github.com/puzige/vega/pull/529#issuecomment-6337487253)。

## 当前实现诊断检查点

- 09:51:35Z 已到当前修复主动工作约20分钟，父会话检查工作树仍只有本文件、无新代码产物；触发诊断，不以重复wait当作实现进展，也不自动取消/替换writer。
- 已复查原始v3/v4/v5失败与现有实际状态：BlockMachine仅有processed_line/start_line/id/mode，无活动容器围栏或当前item缩进；process_line与incomplete-tail各自有独立入口，pending ensure_for仅区分CodeFence。因此责任层需要已审查计划中的私有上下文，而非重复宽松两空格guard。不是CI/权限阻碍，暂无产品取舍未知项。
- 下一具体检查：writer产物是否把上下文接入完整行/半行预览/reset，并在pending repair前使用；同一31项冻结仍须字节不变。尚未READY或GREEN，不把未提交/未执行结果当作通过。若出现新边界须先上报，不能暗改断言。
- 09:56Z UI四文件、10:02Z fence_context、10:07Z pending接线陆续产生，继续保留原writer；约10:22Z后无新代码/静态回报，10:41Z父Agent读回现有差异及真实接线，10:43Z确认取消执行已interrupted。未按诊断阈值自动替换，而是在检查实物与长时间无READY后停止不可观察执行。其09:31Z–10:43Z约72分钟执行墙钟未被writer终态确认，不能精确称纯主动实现时间。
- 所有代码保留，source diff SHA256 `08681f4d54c01be4e8ae7de3bfc6a7520f885d700a15549605b9b7dcd1ba1d81`；父Agent格式/差异退出0，31测试/Cargo/workflows不变。恢复记录见 `vega-issue-514-implementation-recovery.md`，不重新启动任务或重做已有测试。
- 10:45:14Z fetch确认master未漂移，随后spawn专职恢复writer：输入同一补丁/原始RED/实际API/检查点，仅完成静态接线与必要最小修正并停写READY；parent仍Gitowner，同一范围只有一位活动writer。此为一次委派恢复返工，不伪称产品CI失败或替换后能力提速。

## 完整实现恢复 READY

- 恢复writer 10:46:20Z–10:56:57Z，10m37s 主动静态收尾，无外部等待；实现逐字等于检查点，未做源码修改，只写实施证据文档。source diff仍 `08681f4d54c01be4e8ae7de3bfc6a7520f885d700a15549605b9b7dcd1ba1d81`，15文件manifest `ec42fcf156458682f089416fe48f9e973a53c6991030ebd77c7b1b950b104dfb`。
- 新证据 `vega-issue-514-boundary-round-implementation-evidence.md` SHA256 `c8879038392125c696a3c1fb961a0e936d5886b276b03bfbc2b25d444846a805`，说明继承39个发行源文件与新增private fence_context的区分、实际逐行/半行预览/pending/reset/compaction接线、UI owner inset及31项freeze。静态命令均退出0，未编译/Clippy/测试，无GREEN声明。
- 父Agent10:57:57Z复算完整diff/reporthash一致，diff检查及测试/Cargo/workflow比较退出0。writer停止范围写入后将同一版本交既有独立reviewer，仅审新增15文件及受影响不变量，可与精确版本Actions并行；当前审查/云端GREEN仍待取得。
- 恢复writer的一次源文件编码比较错误按原始证据改为字节模式后通过；命令层纠正，不是产品/测试RED，也未改验收。父Agent恢复及review分别记录，不将READY冒充已审查。

## 完整31项开发 GREEN

- 精确实现head `f07edec9b00ff17654da55eb9ae3ba74a5c2c5ee`，由停止后的同一diff原样提交/push；source测试与RED head `e7afdfbcf40263f13831766148cc0bd58de9dc67`逐字相同。
- [Actions39767237956](https://github.com/puzige/vega/actions/runs/39767237956)，Nextest `119785628625`：明确日志31 discovered，Starting31/15binaries，31执行31通过0失败0跳过，0.886s。Clippy/fmt `119785629659` 成功，Python5+15+40=60项成功，required `119788304956` 成功。
- 所有原引用/交互/marker和v3pending/v4祖先几何/v5围栏字面/真实空行及生命周期断言通过。云端GREEN不替代独立实现审查；当前review仍待结果，不进入删测或合并。
- run10:59:46Z–11:06:11Z（6m25s）；Nextest10:59:51Z–11:06:03Z（6m12s）；Clippy10:59:50Z–11:03:51Z（4m01s）。查询11:05:53Z运行中、11:12:42Z完成，相隔6m49s；push10:59:42Z到读取完成的本轮等待墙钟13m00s，和审查并行。
- GREEN及四份原始测试可访问源码固定 [PR comment](https://github.com/puzige/vega/pull/529#issuecomment-6339385329)，父Agent逐个GitHub读回blob与本地相同：parser `b06a05f1122b3d37bd912d253cce317747776ad1`；quote `e08e29dc6b6926c40a8fb0ed05d70e5d7f4a1c65`；marker `b54bda863c3e549932472e3b187a89ef28c554d0`；Thinking lifecycle文件 `ecf58aff425261abd6d0d5213f08bec70e31ec07`。不依赖短期artifact或本机临时文件。

## GREEN 后独立审查新增边界（v6，先测后修）

- 独立实施审查10:59:01Z–11:19:20Z（20m19s），精确 `f07edec9b00ff17654da55eb9ae3ba74a5c2c5ee` 与diff/manifest一致、31项未变；没有给PASS，新增两项静态产品发现：quote-first list首段重复inset/marker列位置；未闭合prose repair在换行后加~~再拼protected fence破坏其物理行。父Agent11:20:52Z读回实际marker/pending代码确认，尚非执行复现。
- 先修v6 spec SHA256 `3199c04ffac1b446b9edccead5eef75103488ffea0b251a4c16790bd6a6264c7`，明确真实quote-first源/marker0rail24text38及depth1对应、三路径/复制/resize；pending prose synthetic closer须在prose行尾，原空行与code字节和final raw行为不变。31项GREEN保留，新两项独立先测/审查/云端各自RED，未删测或生产补丁。
- 11:22:47Z fetch发现master新增 `6e50833202bb45f05c08f7bbc30a68be475609ff`（#538上下文分母），2个无重叠文件；11:23:31Z安全merge到任务HEAD `87f02938fd2c723f2bff5d34c55e16c9ced2325e`，保留所有结果/父docs，无冲突/force-push。原31项将与v6新用例在新基线重新执行，不拿旧GREEN替代新head。
- 已向已停写恢复writer发送具体v6 TEST-ONLY followup，代码生产仍冻结，沿用真实API/观察；本次为两个遗漏产品边界的独立新周期，不是为了报告追加覆盖，禁止新实现先行。

## v6 先行测试停写与独立审查

- v6 writer11:25:13Z–11:38:37Z（13m24s）先追加6项；静态核对发现真实quote-first source被mdstream quote interrupt拆块，父Agent11:39:53Z独立读回确认。冻结前spec更新为 `fb32cc443c3e9e5179a35e8974f38d8cb977ecd1b75e53247090816b0d1c33f0`，先公开parser前提RED、parser-only修复、再同UI自身几何RED；不注入结构或把前提失败冒充几何RED。
- 具体followup增量11:42:17Z–11:48:07Z（5m50s），只增加1项parser quotecontinuation测试，depth0/1与dedented独立quote控制，原37个定义字节前缀保留。合计31+7=38源码定义，不冒充发现/执行数。
- 停写testdiffSHA256 `990101edac06cf79d337ca78e437ee2c9a4ec083638eb05a94245c32b534178c`，二文件manifest `61ceaf1555a615dfbdbf7c865afbc7b18be10583b5a03808e75383588af39d0a`，test evidence SHA256 `58c3a88e3edd8777aa8ea25a567a2158bf8c7a9a6e409e822ce71ab08ce0d4d9`；父Agent11:49:02Z复算一致、diffcheck0。
- 独立测试审查11:49:54Z–11:58:23Z（8m29s），PASS无实质阻碍；两个原测试前缀与507受保护tracked路径逐字不变，认可真实parser前提、pending/final修复差异、geometry/copy观察与分层顺序。现可commit/push现有PR，首轮仅确认parser前提与prosefence有效RED，UI前提失败不计几何RED。未做生产改动。

## v6 parser阶段有效 RED

- 测试head `c780eb5ead8711f364bec54728d94d4244897669`，只追加经独立审查的v6测试及spec/evidence；GitHub parser blob `5446aef5104880f50a4515bf5d1ab7b696cc6808`读回与本地一致。
- [Actions39774947305](https://github.com/puzige/vega/actions/runs/39774947305)，Nextest `119811411225` Starting38/15binaries，38执行32通过6失败0跳过1.412s；Clippy `119811410237` 与Python60成功，summary `119814365248`失败。原31全部通过。
- Prosefence的backtick/tilde两项stage0实际List(proseText before~~unfinished)后多Paragraph("~~  ```text literal")同类，预期同List内Strikethrough prose+Code literal。quote-first parser depth0/whole第一段提前committed，预期committed空/pending包含两paragraph的同一ListQuote。dedented与balanced-prose控制通过。均为责任层目标行为RED，不是compile/fixture故障。
- 三项UI新测试均停在pending真实树前提，明确**不是UI几何RED**；同一38项先冻结，parser-only修复后才继续验证UI自身RED。12:13:19Zfetch确认master未变，父Agent读回原始日志后委派仅vendor ownedquote continuation与pending拼接修复，禁止UI生产改动。
- run12:00:53Z–12:07:45Z（6m52s），Clippy12:00:57Z–12:05:08Z（4m11s），Nextest12:00:57Z–12:07:38Z（6m41s）。查询12:06:40Z/12:11:59Z相隔5m19s，push12:00:48Z到完成读取等待11m11s。原始证据 [PR comment](https://github.com/puzige/vega/pull/529#issuecomment-6340450279)。

## v6 parser-only实现停写

- writer12:14:37Z–12:24:32Z（9m55s）停写READY，4个Rust+README，48+/4-；diffSHA256 `b90d326d61fd410ce9d10e0edc1fd99d464c82372c325655f4efa33c2e579cf6`，5文件manifest `82863636d58696d7f61e8525f9b88c56244f79b42f0e618422b2cd1cafd79304`；证据report `c96b2d8df51fde04d140d61cbbe7735398851c53da5c2910590e20c6df2cfed4`。
- 无变更容器状态的quote continuation predicate，只在List模式抑制通用quote interrupt；pending repair将closer放在原separator前并原样重附换行/protectedcode。原38测试与UI生产、hooks/Cargo/workflow等509受保护路径逐字保持；本地fmt/vendorfmt/diff0，不编译或运行。
- 父Agent12:25:32Z复算diff/report一致、范围/测试/UI冻结检查0。已将同一停写版本交独立reviewer，可与该版本Actions并行；尚未parserGREEN或UI自身几何RED，不改UI。

## mdstream 0.3 来源核对（只读静态证据）

- 锁定版本与用户裁决：`=0.3.0`，不迁移 0.4；现有技术规格 §5.0 已列 vendoring 预案，无新 crate。
- crates.io 发行包 `mdstream-0.3.0.crate` 实测 SHA256 与 Cargo.lock checksum 相同：`f3327364edd79c4f66b36db667c0a28e078389351b0855e5f95ed4f22d778821`。
- 发行包 `.cargo_vcs_info.json`：上游提交 `950d0926f54667f04aed87fad6cc9921aa74ef5c`，子目录 `mdstream`。
- 上游固定提交许可：[MIT](https://github.com/Latias94/mdstream/blob/950d0926f54667f04aed87fad6cc9921aa74ef5c/LICENSE-MIT) blob `f7c57f575b1b78ac893b339ffafd9effe4bab286`，[Apache-2.0](https://github.com/Latias94/mdstream/blob/950d0926f54667f04aed87fad6cc9921aa74ef5c/LICENSE-APACHE) blob `cd9f8ad8d319325bf9237d5195735eebbfcdcfdc`。已读回 MIT 授权与原作者归属，发行 manifest 声明 `MIT OR Apache-2.0`。
- 采用 vendor 时保留许可与可重建来源，剔除不被生产使用的 upstream 测试/接线，记录与发行包的语义差异；不整套恢复历史 Rust 测试或引入 upstream dev dependencies。

## 本地与云端边界

本地仅做差异、格式和静态元数据检查；未运行 Vega、编译、Clippy、Rust/Python 测试、build 或打包。完整冻结测试已取得上述本轮有效 RED 与开发 GREEN，独立实现审查/删测/最终门禁仍未完成；后续只使用现有 PR Check，启动后五分钟状态查询，保留首次失败原始证据。

最终交付前仍需：同一冻结测试云端全部 GREEN → PR 固定原始源码/完整 SHA/run/job/准确计数 → 独立删测提交及审查 → 最新完整 head 的 fmt、全目标 Clippy、Python、Rust 0 发现/0 执行 `no Rust tests` 门禁 → 正式 PR、squash merge → Issue OPEN / In review 等用户手测。空集不称 Rust 测试通过。
