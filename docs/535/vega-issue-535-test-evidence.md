# #535 测试先行证据

## 当前阶段

最小生产实现恢复/收尾。18 个现存开发测试定义不是云端发现或执行计数，尚未取得行为执行结果。已完成的接口缺失 RED 与历史审查见下文；当前按最新按风险验证政策推进，最终交付树不保留 Rust 测试及独占支持。

任务分支：`feat/535-queue-ui`。验收：同目录 `vega-issue-535-queue-ui.md` v2.1 的 A1–A7。

## 草稿恢复与基线同步

- 初始基线：`83fd51d79940516029cdc1eaf8e81566870c877b`。
- 恢复时的 15 文件测试包 SHA-256：`c90b5ca7c1453c5fe3b1330ee1042a52b3b9938bc0f526cebec1b5af7d91b5f3`。
- 主 Agent 先 fetch，再保护全部已修改与未跟踪任务文件，fast-forward 到 `871f91f246e90753743a256cee1eec315e74effc`，恢复草稿。上游为 #529 Markdown 引用渲染修复；合入无冲突，主 checkout 的无关脏改动未触碰。
- 同步后、收尾写入前的测试包 SHA-256：`2633f0acd9618873bda6af428f10f2261940dfd8bdd64270b1b6c3d13e4e0289`。整体变化来自 Cargo.lock 与 render_rows.rs 的新上游内容，三个测试源文件哈希未变。
- 哈希算法：排序所有本轮 tracked diff 与 untracked 非 docs 路径，对每个文件拼接 UTF-8 路径、NUL、8 字节大端内容长度和原始内容，再计算 SHA-256。最终冻结版本须另行记录，不以这份中间稿代替。

| 同步前后保持一致的测试源 | SHA-256 |
|---|---|
| `crates/vega/src/app_agent/continuation_tests.rs` | `bc039063d254d6273e4f24cdab6c67e5adcd212bf6b366928b4f4490de16328b` |
| `crates/vega_conversation/src/agent/events/continuation_tests.rs` | `cb3e67e50303858e8d3d95bb57f016ff5ad8a2b85ba8ea1ba56884111ab480b6` |
| `crates/vega_ui/src/conversation_stream/continuation_tests.rs` | `4bf574c221f6e34476cc2657e9ac631c13650c6045d2f9a5c6e498d831613583` |

上述是恢复检查点，不表示该稿已通过独立审查。原执行子任务在当前会话中不可续用，已确认没有当前活跃旧 writer，再交给一个专职 writer 继续原稿；未重新创建测试体系。

## 收尾 writer 异常后的检查点

收尾 writer 在写入 UI 测试后异常结束，未返回 READY 或静态检查报告；工具已确认该执行为 failed，停止写入后才启用一个恢复 writer。主 Agent 核对当前 HEAD 仍为 `871f91f246e90753743a256cee1eec315e74effc`，`git diff --check` 退出码 0；18 个测试定义仍为 app 1 / conversation 9 / UI 8。

- 当前中间稿包哈希：`42d9a2db9d3c083e315e2a08ea991e0246e9eb35c68bbc4dcb9ed193c35404db`。
- 当前 UI 测试 SHA-256：`fb986871ac3e56ee1435d97a973d204136df5d11350ecf7c3447505bd613a1ab`，50681 字节；另两个测试源哈希保持上述值。
- 已存在的实际改动：按宽度分别比较文字与最后图片 gap、溢出历史夹具与滚动位移/回程、真实 ComposerSubmitted 订阅及内容/图片断言。
- 未决：锁文件接线、当前稿静态检查、独立评审、云端有效 RED；不得把异常前的未回报检查写成通过。
- 恢复范围仅完成上述未决静态接线，不重做已有测试，不改生产行为或断言含义。

## 必要观测边界

- Conversation 测试调用真实接受事务、事件转换及独立发送管线，验证明确 predecessor、原子性、事件顺序、原始起点和历史负对照。
- UI 测试通过真实 store/history API 构造历史投影，再驱动真实 ConversationStream。写入数据库的 fixture 不被称为真实接受事务到 UI 的单链路测试。
- Controller 测试验证续接期间仍 active、队列不 ready，真实终态才释放相应门禁。
- 错误恢复应观察真实 `ComposerSubmitted` payload，而不是仅检查私有 pending 字段；不调用外部模型。
- 几何应比较实际文字/图片底部、header、裁剪范围与各自普通发送对照。滚动应有实际位移证据，再验证重新测量；短夹具发送两次滚轮事件不能证明已滚动。

不为上述边界扩大生产 public API，不建立含外部服务依赖的完整 VegaWindow 测试，不恢复真实外部 E2E 门禁。真实 UI 外观与系统集成留给用户手测。

## 应用入口静态审查

在基线 `871f91f246e90753743a256cee1eec315e74effc` 核对：

1. `window/artifact.rs::apply_agent_batch_ingress` 校验 generation/thread/stream 后按顺序处理 batch.events；对普通会话事件先调用 `AppAgentController::observe_terminal_message`，再调用 `ConversationStream::apply_event`。
2. PendingMessageDelivered 只增加队列投递 bookkeeping；MessageStarted 负责 durable composer/queue 投递确认。续接新事件不会匹配这些既有专用分支。
3. 入口在 `batch.finished` 缺失时返回 Running；只有该值存在才调用 controller.finish。不能把消息段切换变成 batch 完成。
4. Controller 的终态匹配只接受 MessageFinished/Interrupted/Error；新 RunContinued 应保留通配无动作路径，不能清除 active 或设置 queue_ready。
5. Meter 的 MessageStarted 已重置当前输出估计并保持 in_run；新增事件必须在 `types/meter.rs::apply` 和 UI `core.rs::feed_meter` 的穷举匹配中接线，不应靠假的旧终态结束整个 run。

这是静态链路审查，不是整窗运行证据。最终实现后仍须复核同一不变量。

## 验证记录

- 基线同步后的 `git diff --check`：退出码 0。
- 恢复稿 `cargo metadata --offline --locked --format-version 1`：退出码 101，明确报 `cannot update the lock file ... because --locked was passed`。GPUI `test-support` 启用的传递依赖尚未完整写入 Cargo.lock；仅手工添加三个直接边不够。这是测试接线失败，不是有效 RED；在独立审查/提交前修正并复查版本与依赖差异。命令不编译、不执行 Vega。
- 2026-10-22T17:13Z 恢复核对：当前会话没有活跃旧 writer。锁文件修订保留；fetch 后 fast-forward 到 `2ba904f7074115de5ee89cc18c376c083f803357`，上游仅新增 `docs/554/vega-issue-554-panel-motion-study.md`，不改变被测生产代码。
- 当前停止写入的 15 文件非 docs 测试包 SHA-256：`92d771034f2ff9aa24c66a3aba0f72f74b5b5f02db6ea82f34c9a665b00ce729`。三个测试源仍为 app `bc039063d254d6273e4f24cdab6c67e5adcd212bf6b366928b4f4490de16328b`、conversation `cb3e67e50303858e8d3d95bb57f016ff5ad8a2b85ba8ea1ba56884111ab480b6`、UI `fb986871ac3e56ee1435d97a973d204136df5d11350ecf7c3447505bd613a1ab`。
- 当前 `cargo metadata --offline --locked --format-version 1`、`cargo fmt --all -- --check`、`git diff --check` 均退出码 0；它们仅为依赖解析、格式和差异静态检查，不是编译或行为测试。
- 锁文件 TOML 静态身份对比：新增 10 个 GPUI test-support 传递包，既有 package 身份、版本、来源和 checksum 未变，无包删除。新包为 bit-set 0.8.0、bit-vec 0.8.0、convert_case 0.11.0、proptest 1.11.0、proptest-macro 0.5.0、quick-error 1.2.3、rand_xorshift 0.4.0、rusty-fork 0.3.1、unarray 0.1.4、wait-timeout 0.2.1；除新增测试边外的同名多版本依赖标签变化是 disambiguation，未升级生产依赖。
- 非写入者独立审查执行异常结束，未返回正式 READY，不以其进度信息代替放行。其已核实本稿哈希与 app/conversation 测试、依赖身份及 GPUI API，并留下“直接 hover 默认透明 copy 按钮会在工具层失败”的具体发现；本条仅保留调查证据，完整放行仍待后续独立审查。

## 独立审查异常后的夹具修正

当前旧审查执行已确认为 failed，未再写入。主 Agent 重新核对原始源码，记录两项测试层错误，尚未运行故不称为云端失败或有效 RED：

1. GPUI Kit 0.6.1 `test.rs::hover_target` 先调用 `target_position`，后者要求 snapshot.visible；GPUI Base 0.6.1 的 snapshot 可见性要求 opacity > 0。现稿直接 hover 默认 opacity(0) 的 message-copy，不能触发真实父 group hover。应先 hover 已可见父 message-with-copy 行，再验证按钮可见并操作，保留原来的 clipboard 与键盘断言。
2. `echo_does_not_discard_old_output_arriving_before_acceptance` 的旧段仅有 TextDelta，无 Final。生产 `render.rs` 仅在 `final_copy_for_entry` 为 Some 时包 message-with-copy；因此使用旧 copy.id 查该控件必不成立。改为渲染后的真实 ListState.bounds_for_item，验证原文/身份、行在 viewport、正高度及 user < old < next 的真实非重叠几何，不给夹具虚构 Final，不改变产品行为。

fetch 后同步到 `9b901a34ee00611f73fbd58275c7087282af5e2c`；上游为 #547 图片预算/首次输入压缩修复，未触碰本稿 15 文件，包哈希保持 `92d771034f2ff9aa24c66a3aba0f72f74b5b5f02db6ea82f34c9a665b00ce729`。只委派一个 writer 修上述 UI 测试观测，不写生产或其余测试；A1–A7 与断言含义保持不变，修订后须重新独立审查，当前未获放行。

## 两项夹具修正就绪

writer 在 `9b901a34ee00611f73fbd58275c7087282af5e2c` 交付 READY（仅 test-fixture-repair），明确停止写入。唯一改动为 UI 测试文件：统一复制 helper 先 hover 父行并确认按钮可见及非零大小，再真实 click；echo 旧正文改用渲染后的真实 list 行边界，并核对所属 ID、原文唯一、viewport/非零高度和非重叠顺序。未改验收、数据预期或生产行为，18 个测试定义仍为 app 1 / conversation 9 / UI 8。

主 Agent 对最终文件重新独立核对：

- 15 文件非 docs 测试包 SHA-256：`c3f29824f98896c3d5e707e2f9de6d77450d011574e5b179399fd7be24bd7a45`。
- UI 测试 51,401 字节，SHA-256：`2b99d6414c7c4b45f4e313413d947b07c65355bf2aed391fd882db3f90cded91`；app/conversation 源哈希未变。
- 当前 `cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --offline --locked --format-version 1` 均退出码 0；无 ignore。
- 其余 14 个非 docs 文件内容未变，已获非写入者有限范围 READY：核对 app/conversation 10 测试、真实接受事务与独立发送管线、两条 runtime 接受入口、3 种页外关系查询、历史负对照、controller 门禁及 ingress 静态接线；未发现目标缺失 API 以外的 harness/可见性阻塞，非测试路径未见生产行为变化。这不是全包放行或有效 RED。
- fetch 后同步到 `6615748f91273bfb8ee424011b3b5a09b3777d2b`，增量为 #546 Composer 模型/权限胶囊及其 theme 常量、文档，不改本稿 15 文件；全包内容标识不变。相同 reviewer 继续只读审固定 UI 修订及受影响不变量；完整包放行待取得，不以 writer READY 或部分审查代替。

## 完整测试审查、测试提交与首轮 Actions

非写入者已在生产基线 `6615748f91273bfb8ee424011b3b5a09b3777d2b` 完成全包静态审查并返回 READY；既有 14 文件结论与 UI 8 测试结论合并，核对 A1–A7 的真实可观察边界，两项夹具修正无已知阻塞。审查者独立复算上述 15 文件包哈希与三个测试源哈希一致，确认提交动作未改变内容。

- 先行测试提交：`afb5ee6c04ad8d044350d9ca917599ce1a8396a5`，生产行为未修改。
- Draft PR：[563](https://github.com/puzige/vega/pull/563)。PR 固定测试源码、完整 SHA 与原始 commit patch 链接；主 Agent 通过 GitHub contents API 按该 SHA 读回三份源码，内容哈希全部一致。
- 首轮 Actions：[39828573161](https://github.com/puzige/vega/actions/runs/39828573161)，测试 head 为上述提交。
- Nextest：[job 119998319417](https://github.com/puzige/vega/actions/runs/39828573161/job/119998319417)，构建测试目标失败：7 个 E0609/E0599，均为 `MessagePage.continuations`、`ConversationEvent::RunContinued`、`HistoryEntry::RunContinuation` 目标接口缺失。未完成测试发现，行为断言尚未执行，不报告发现/执行计数为通过。
- Clippy：[job 119998319678](https://github.com/puzige/vega/actions/runs/39828573161/job/119998319678)，除目标接口缺失外还报告 UI 测试第 3 行 `unused import: gpui`，由 `-D warnings` 拒绝。它是无关测试接线错误，不能计入接口缺失 RED。
- Required check：[job 119999998412](https://github.com/puzige/vega/actions/runs/39828573161/job/119999998412) 失败。原始 failed-job 日志 803 行，SHA-256 `870007faddb030b334ed9a091a92f62d18e19a80d5180d42ac9752ffa4b45305`；永久原始证据为上述 Actions run/job，本机日志不是唯一来源。

主 Agent 检查完整失败签名，当前不将整轮认定有效 RED，不进入生产实现。仅修测试接线并重新验证，不 rerun 覆盖首轮证据。

## 导入接线修订（无验收变化）

同一 writer 仅移除 UI 未使用 `gpui` 导入，并按 rustfmt 交换相邻导入顺序；曾因严格单行范围保留 fmt=1，主 Agent 授权相邻排序后 fmt=0。未加 allow、ignore、retry，全部数据、断言、测试定义和生产行为保持不变。

- 当前 15 文件非 docs 包 SHA-256：`e1af873b425215fb515e55745579f0ffaa980027d5ddca01240bb6c811d6f707`。
- UI 源 51,393 字节，SHA-256：`de91195caef46ae787344ce0da6c6b3024a82edb0c933dc6ac4b873e6044856d`。
- app/conversation 哈希、其余 14 个文件内容未变，仍为 18 测试定义。
- 主 Agent 逐字节核对：除顶端导入行外，UI 从 `use vega_conversation` 起与 `afb5ee6c04ad8d044350d9ca917599ce1a8396a5` 完全一致。
- writer 停止写入后，独立 reviewer 核对原始 unused-import 错误、宏命名空间与逐字节差异，返回增量 READY；复用完整包审查。主 Agent 当前 fmt/diff/offline locked metadata 静态检查退出 0。
- 修订提交：`731c887b4588b27c05582b45f9fe7fea69a8eec2`，原始测试源码与 patch 已固定在 PR，首轮 run 未 rerun 或覆盖。

## 有效接口缺失 RED 与冻结

第二轮 [Actions 39830669812](https://github.com/puzige/vega/actions/runs/39830669812)，head `731c887b4588b27c05582b45f9fe7fea69a8eec2`：

- [Nextest 120005637079](https://github.com/puzige/vega/actions/runs/39830669812/job/120005637079) 和 [Clippy 120005637305](https://github.com/puzige/vega/actions/runs/39830669812/job/120005637305) 各报告 7 个根诊断，仅 `MessagePage.continuations` E0609、`ConversationEvent::RunContinued` 与 `HistoryEntry::RunContinuation` E0599；多余导入错误不再出现。
- Nextest 的 `cargo test --no-run ... --workspace exited with code 101` 为上述测试目标构建失败的包装，不是独立 runner 故障或行为断言失败。
- [required 120007496104](https://github.com/puzige/vega/actions/runs/39830669812/job/120007496104) 失败。原始 failed-job 日志 784 行，SHA-256 `5438ba3b2eab91431580d896e1339be6206559a94d012df18d3fbf226c804fb8`。
- 主 Agent 已读取原始失败签名及包装上下文，确认有效 **接口缺失 RED**。行为断言尚未执行，测试发现/执行/通过/失败/忽略/跳过数量未取得；SQL 列断言尚未运行。

冻结版本为上述测试提交：app `bc039063d254d6273e4f24cdab6c67e5adcd212bf6b366928b4f4490de16328b`、conversation `cb3e67e50303858e8d3d95bb57f016ff5ad8a2b85ba8ea1ba56884111ab480b6`、UI `de91195caef46ae787344ce0da6c6b3024a82edb0c933dc6ac4b873e6044856d`，18 定义及所有数据/断言不再修改。后续新验收须重开先测后修；确定 harness 错误须先记录与独立审查，不能暗改或放宽。

主 Agent 再次 fetch，确认上游 master 仍为 `6615748f91273bfb8ee424011b3b5a09b3777d2b`；同一专职 writer 已获派最小生产实现，范围按规格 v2.1。保留全部冻结 Rust 测试与支持到同测云端 GREEN，随后才可独立删测与验证最终空集 head。

## 未执行阶段

- 本地 Vega 运行/编译/构建/测试/CUA：未执行。
- 带测试 GREEN、动态发现/执行/通过/失败/忽略/跳过数量：未取得。
- 生产实现正在进行；独立实现审查、独立删测及最终空集门禁尚未进行。

## 2026-10-23 实现恢复检查点

当前会话无可续用的旧子任务，`list_agents` 返回空集；未收到旧 writer READY，不能把恢复稿视为已完成。主 Agent 检查全部未提交与未跟踪文件后保存恢复检查点：原 HEAD `731c887b4588b27c05582b45f9fe7fea69a8eec2`，14 个 dirty 文件包 SHA-256 `72674730623698a00beb12d2fe7720a4156231f49da0de59f046e6ba9234b841`（含证据文档）。

fetch 后保护全部改动，合入最新 master `9f9fa092a3ca391a65f08419c9206f32979119e6`，集成 HEAD `0ea2910ce3d59824dd379af10490c009cddf89fd`；恢复实现无冲突。保留原远端测试提交，不 rebase/force-push 丢弃来源。三份测试源 SHA-256 仍与上文相同。新基线包含 #569/#573 验证政策，规格 v2.2 只同步流程/基线，不改 A1–A7 产品验收。

恢复的 13 个非 docs 生产文件内容包 SHA-256 为 `9e9e0878532959de8c99f32d5d326503a5c2debc02ae147f9b73323ce4c15417`。已存在：0033 递增迁移、真实接受事务记录、页内/页外明确关系查询、RunContinued/历史投影、UI 单计时和旧段隐藏状态；未决为完成静态自检、独立实现审查及 Actions 行为结果。当前 `cargo fmt --all -- --check`、`cargo metadata --offline --locked --format-version 1`、`git diff --check` 均退出 0，仅静态检查。本地未编译、运行 Vega、Clippy、测试或 CUA。

原 writer 已无活跃执行，后续只由一个恢复 writer 接续该精确稿，不重做已完成工作。现有开发测试保留用于必要回归；取消强制 TDD 不作为放宽断言、删除失败测试或伪造 GREEN 的理由。

## 恢复实现 READY（待独立审查与 Actions）

恢复 writer 已停止写入，13 个生产文件内容包 SHA-256 `0719c917f44fb5b1daf862e6b93faf78200b42d1646743a91dc44ba36b8d88f0`。相对恢复稿只补 `content.rs`：明确的 continuation successor 不执行上游 `begin_computer_use_task`，独立运行仍正常执行，避免把同次运行的消息切换当成新任务。其余已有最小实现保留，普通间距 token 未改。

writer 实际执行 fmt/diff/offline locked no-deps metadata 均退出 0；主 Agent 再次独立复算生产包与三份测试源哈希一致，fmt/diff 退出 0。未执行本地编译、测试、Clippy 或 CUA。下一步固定提交，非写入者审查实现，现有 Actions 在同一版本执行 18 个开发测试；尚无行为通过结论。

## 实现首轮 Actions：测试宏接线失败（2026-10-23）

生产实现已固定并推送为 `be839cc63da09643c852f3bc2496e08ef932eb47`，生产内容包仍为 `0719c917f44fb5b1daf862e6b93faf78200b42d1646743a91dc44ba36b8d88f0`。独立实现审查针对该停写提交进行。

[Actions 39891674978](https://github.com/puzige/vega/actions/runs/39891674978) 失败：Format、Python 发布脚本检查步骤成功；Clippy job `120212647786` 与 Nextest job `120212647991` 均在 `crates/vega/src/app_agent/continuation_tests.rs:4` 报 `recursion limit reached while expanding #[test]`。Nextest 未完成测试发现，行为断言未执行；发现/执行/通过/失败/忽略/跳过计数未取得，不记录为产品失败或行为通过。required job `120213914438` 随之失败。

原始 failed-job 日志 676 行，SHA-256 `e97b64e73cc5e292e15c5a209c886fc6a3620cfc5e2ed9a536d5be684bddab09`。主 Agent 读取两个 job 的原始诊断，并静态核对锁定版本 `gpui-pre-macros 0.3.4` 的 `src/test.rs`：GPUI 宏生成未限定的 `#[test]`；app 测试的 `use super::*` 从父模块 `use gpui_kit::*` 导入同名测试过程宏，导致递归展开。责任层是开发测试导入接线，不是已证实的运行展示缺陷；不增加 recursion_limit 或更改产品实现来绕过。

修复已限定委派为 app 测试头部的显式导入，保留全部测试正文、数据与断言。下一验证为同一生产版本配修正接线后再次运行现有完整 Actions；修复完成、类型检查及行为结果均另行记录。Rust 测试清理仍在未解决失败诊断与必要验证之后，不以清理制造通过。

该 writer 子会话在改动导入后异常结束，终态为 failed，未交付 READY。主 Agent 确认该执行已停止，检查工作树仅新增预期 app 导入差异（其余脏文件是主 Agent 的本证据文档），当前 app 文件 SHA-256 `e2d289453b0a5eee1e298c81e73e0f5332592d23b251172d185311f6b3372d0c`；从 `#[gpui_kit::test]` 至文件尾与 `be839cc6` 逐字节一致。保留补丁交替补 writer 完成静态验证，不重新实现已完成部分；此时类型检查、行为结果和实现独立审查仍待取得。

原独立实现 reviewer 子会话随后也以 failed 终止，未返回 findings 或审查结论。其只读范围没有改动；父会话确认生产仍为 `be839cc6`，不将异常结束记为通过。替补只读 reviewer 继续同一生产提交及上述接线差异，保留已完成的静态检查和实际 CI 失败证据，不重做根因或产品裁决。

替补 writer 已 READY 并停止写入，未追加改动。当前 app 源 3,181 字节，SHA-256 `e2d289453b0a5eee1e298c81e73e0f5332592d23b251172d185311f6b3372d0c`；属性至 EOF 的 2,884 字节后缀与原版一致，SHA-256 `ee529494f57c8fce1d8495ab597fabf2e6cf6f9d60db86b1a575cd4cd74ef4fc`。已核对 TestAppContext 的 update/add_window_view、App::set_global 为固有方法，无需补入 trait wildcard；当时保留了 gpui 别名，其必要性判断被下一轮 Clippy 的 unused import 诊断推翻，见后续记录。writer 的 Rust 1.98 focused rustfmt 与 focused diff check 均退出 0。接线修正不等于编译或行为通过，下一步由新提交的现有完整 Actions 验证。

## 首轮行为结果与实现审查修正（2026-10-23）

接线修正提交 `9327fa4fe5b8bb41a3ea01cf2f870ffe634a1566` 的 [Actions 39893667074](https://github.com/puzige/vega/actions/runs/39893667074) 仍失败。Format 与 Python 检查步骤成功；Nextest job `120218998456` 成功编译并执行 18 个测试（15 个 binaries），17 通过、1 失败、0 skipped，耗时 11.909 秒；该结果不是全部通过。原始 failed-job 日志 731 行，SHA-256 `5d41a03473068af02f810530f5279a0bc628b468f11b74bf4626e2f93de81c7c`。Clippy job `120218998235` 唯一错误为 app 测试第 2 行的 unused `gpui` import，不能通过 allow 或删测试规避。

17 项通过包括接受事务、两个 runtime 接受入口、普通独立投递负对照、三分页 API 与历史投影、controller 基础门禁、UI success/failure/echo/route，以及真实普通/queue/echo 气泡与最后图片间距、窄窗口/重测/滚动/复制对照。该进程内几何测试通过，未发现需额外改 token 的间距差异；不等于真实应用像素验收或整窗/外部系统 E2E。

唯一测试失败位于 UI `repeated_continuations_keep_one_clock_and_late_terminal_cannot_restore_old_clock:1208`。主 Agent 读取原始失败与 fixture：工具已合法走完 Proposed→Approved→Running→Finished Success，而 `ToolCard::matches_call` 在第 260 行专用于 PendingApproval + 无 result 的重复提案判定，完成态必定 false。此为观测 API 误用，不是已证实的工具丢失；不得修改生产 matches_call 状态机迁就测试。修正为前后同一工具实体及实际目标、成功状态和真实展开输出，保留所有时钟、正文/Final、复制/剪贴板、工具开合/输出可见性、再次续接与停止断言。

替补独立 reviewer 完成 `be839cc63da09643c852f3bc2496e08ef932eb47` 全部 13 个生产文件审查，独立复算内容包 `0719c917f44fb5b1daf862e6b93faf78200b42d1646743a91dc44ba36b8d88f0`，未发现其他可操作问题，但提出一个 P2 阻断：`window/artifact.rs:379` 在 UI 过滤前调用 controller，`app_agent.rs:526–589` 会让续接 predecessor 迟到终态覆盖 `queue_ready`、`completion_outcome`、`terminal_message_id`、`terminal_failure`。当前 successor 成功后迟到的旧 Error 可阻止 `window/agent.rs:1005–1008` 释放普通 queue，并使完成通知报告失败；仅 ConversationStream 过滤不够。主 Agent 已复核原始调用链，不将该版审查记为通过。

规格 v2.3 在不改变 A1–A7 的前提下补齐 A4 的控制器显式续接终态归属。已委派同一 writer 仅修改 app controller、app 开发测试及 UI 开发测试：修复真实产品遗漏，保留当前 successor 的成功/失败/停止和无 ID 启动错误，补必要迟到终态回归；同时修正上述观测与导入错误。下一验证为 READY 停写版本的独立增量审查及完整 Actions；测试清理、最终空集门禁与合并均未进行。

该修复 writer 在诊断后以 failed 结束，未留下任何代码改动。父会话确认终态和 diff：相对 `9327fa4f` 仅两份父会话文档变更；app_agent.rs、app 测试、UI 测试 SHA-256 仍分别为 `a7020e892d24e705683675f60d1ddc8f4d5b5af8a9f600bd3d0b90a3fb7ed34a`、`e2d289453b0a5eee1e298c81e73e0f5332592d23b251172d185311f6b3372d0c`、`de91195caef46ae787344ce0da6c6b3024a82edb0c933dc6ac4b873e6044856d`。这是协作执行异常，不是第二次产品测试失败；不重跑 CI 或恢复已完成的根因工作。将剩余 product 单文件与测试两文件拆为非重叠的小范围执行，沿用同一证据、spec 与下一个完整 Actions 验证动作。

## 续接恢复与上游迁移整合（2026-10-23）

控制器 writer 已 READY 停写：仅 `app_agent.rs` 增加显式、所属运行校验后的 predecessor 集合和迟到终态前置过滤，文件 SHA-256 `497c762c91d66759070236695206e9f604eae79ec0880e4e658c017980fbc0d1`；single-file rustfmt、diff 与新增行静态检查通过，尚未编译或执行。测试 writer 达到执行时限，未交付 READY；父会话确认已停止，保留其有效 UI 观测修正，文件 SHA-256 `b1ef5df4af9d26f3b17db989d6688594cd7a1133c24f80d70f9108a0b9ee9166`。app 测试仍为 `e2d289453b0a5eee1e298c81e73e0f5332592d23b251172d185311f6b3372d0c`，控制器补充回归和无用导入修正尚未完成，不记录通过。

中断恢复检查确认无活跃旧子任务，原改动完整保存于检查点 `e58142f1d82adb099d8dba7a9b2a84cb610f258d`。原合并暂停在 store migration 清单冲突；重新 fetch 确认上游仍为 `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e`。专职 writer 将本任务未发布的 `0033_run_continuations.sql` 原样顺延为 `0034_run_continuations.sql`，保留上游已发布 `0033_delegated_stage_recovery.sql`，父会话独立静态确认 34 项连续唯一、前 33 项顺序与 SQL 字节均和上游一致、本任务 SQL 字节与旧文件一致。合并提交 `e05de7baf350691beb1485138bc30927ec68f9dd`，检查点已无冲突恢复；没有重写已推送历史或修改用户数据。

合并全量 `git diff --cached --check` 首次因上游原有 `docs/522/paired-existing.csv` 的 25 行 CRLF 报 trailing whitespace，退出 2；已逐字节确认该文件与上游一致，不为本任务改写。针对实际迁移解决范围的 diff check 退出 0，恢复后 `git diff --check` 退出 0。这些均为静态检查，不是数据库迁移执行结果。

上游成功、完整 read 现在发 `ToolReadFinished { result, identity }`，而原 runtime 开发测试只观察 `ToolCallFinished`；父会话核对实际 loop 分支，适配两种真实事件，同时保留 read-call ID 和 retained tool output 断言。新专职 writer 仅完成三份开发测试的最小剩余修正及控制器 A4 回归，不重做原 17 项已通过内容；后续独立审查和新 head Actions 必须覆盖整合后版本。当前仍未取得全部行为通过、最终删测、合并或用户验收。

新独立非写入者 reviewer 对停写生产版本 `e05de7baf350691beb1485138bc30927ec68f9dd` + 控制器补丁完成增量审查，前后独立复算 production14 内容包 SHA-256 `a02f7d3450193bc2d7806bf2f9a53cb4ba216d5e326d40717d32f5e9192a7e23` 一致。确认旧 P2 已在任何四项控制器终态字段变更前过滤，generation/thread/stream 与续接所属校验、多次/重复续接、当前合法终态和启动 Error(None) 分支保持；同事务续接与委派继承、迁移 1–34、上游工具/阶段事件和权限 UI 合并均未发现可操作生产阻塞。此结论不涵盖仍在写入的开发测试，不等于新 head CI、合并批准或用户外观验收；reviewer 已停止且无写入。

三文件开发测试收尾阶段再次达到时限，仅留下 read 事件兼容行；父会话确认停写、复核差异后保留该行，未重跑 CI。余下任务收窄为单个 app 测试文件，由新 writer 完成并 READY：去除 unused gpui、将相同 Thread fixture 提取为 helper（原有测试断言不变），新增两个行为测试。一个循环覆盖一次/两次续接与当前成功/失败/停止的六种组合，各自在当前终态前后送入所有 predecessor 的成功/错误/停止；另一个覆盖错 generation/thread/stream 不能安装过滤或改终态、finish 所属校验及无 ID 启动错误。实际观察四个终态字段、活跃运行、取消标记和 finish 结果，不以内部 set 内容替代业务结果。

三份停写开发源 SHA-256：app `9206160d8b7578f7be2220ffecfd965ed6b01ddcaf19931a90b25b196a18960e`、conversation `f3b36ee6ed99f288d554684f7d645d5d0a60b64e946deac2645417a756da8248`、UI `b1ef5df4af9d26f3b17db989d6688594cd7a1133c24f80d70f9108a0b9ee9166`。静态定义为 app 3 / conversation 9 / UI 8，共 20，不是发现/执行计数。父会话在该版实际执行 `cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --no-deps --offline --locked --format-version 1` 均退出 0；未本地编译、运行或测试。下一步整合上游新出现的消息锚点修复，固定完整停写提交，继续增量审查与现有 Actions。
