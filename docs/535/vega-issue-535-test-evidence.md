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
