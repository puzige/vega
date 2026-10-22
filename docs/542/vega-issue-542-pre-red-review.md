# #542 首轮测试补丁预审记录

阶段：测试仍在写入，尚非固定版本独立审查；生产未实现、无 Actions RED/GREEN。
基线：`871f91f246e90753743a256cee1eec315e74effc`。
记录来源：主 Agent 静态读取 2026-10-22T16:27–16:32Z 的未提交补丁。
最终以两位写入者 READY 后的完整 diff 为准，以下发现可能被后续修正。

## 需在 RED 前核对的测试责任层问题

1. `artifact_card.rs::tests::ObservedText` 当时的 paint 记录使用 `self.0.to_string()`
   和 paint 时 `window.text_style()`；这不是已经核实的 TextLayout 文本及布局时颜色。
   应按 `vega-issue-542-test-observation.md` 透明委托，记录实际 TextLayout::text、
   layout 时实际继承颜色和 paint/clip 事实，不把期望色注入探针。
2. `projection(slot,generation,size)` 当时总用 `ArtifactCardId::for_test(1,slot)`；
   同文件三次捕获因此复用了同一 card ID。真实 ArtifactService 每个不同 call 创建
   不同 card ID。去重测试必须使用不同 capture ID、相同显示文件键，并继续验证
   最新 file capability 与 metadata，避免现有按 ID 合并也能蒙混通过。
3. `ordered()` 当时仅比较传入探针的内部列表 index；`observe_stream_item` 从
   `entry_identities`/entry.row_count 复制 owner/rows。应核查可观察顺序/归属是否
   由实际渲染的容器/几何、真实文字和控件路径证明，而不是测试包装私有状态断言。
4. 错误测试的 preview/open 失败当时发生于点击 Show more 之后。捕获错误虽验证
   收起仍可见，但既有验收也要求文件 preview/open 错误不被六行上限折叠，需在
   生产未改时保证相应断言覆盖（不是实现后补测）。
5. 控件禁用测试必须验证原因可获取；snapshot.disabled 与无事件不能单独证明原因。
6. controller 补丁的 ArtifactProjection 字面量当时尚无 size_bytes，而 UI/projection
   测试已引用新字段。冻结测试补丁应约定该字段，使后续实现添加字段不会要求
   追改原始测试。该字段缺失应如实报告接口缺失 RED，行为断言尚未执行。
7. controller 仍需覆盖早于 stream proposal 的失败和迟到 worker 失败归属。
   仅 UI apply_artifact_capture_error 测试不能证明 controller 的原始顺序与 ownership
   传输；当前只看到登记/preview fence 回归。跨层观察接线由两位写入者停写后协调。

这些是测试数据/接线与既有验收覆盖问题，不是改变产品期望的理由。不放宽断言、
不跳过必需回归，不把观察失败算作目标行为 RED。独立审查须绑定最终停写内容版本。

## 继续执行检查点（2026-10-22T16:54Z）

- 用户明确要求继续推进。旧会话写入者已不在当前运行列表；保留全部未提交产物，
  由一个新的专职写入者统一收口，不再等待不可访问的执行者，也不重新开始调研。
- 重新 fetch 后 HEAD 与 origin/master 均为
  `871f91f246e90753743a256cee1eec315e74effc`，无需 rebase。
- 接管时 tracked `git diff --binary` 的 SHA-256 为
  `959dafe3e79949490d116a1de3a1a4a4110bc2008719d10beb3e8c92f72f6865`；
  spec v2.1（含跨层测试例外）的 SHA-256 为
  `a3edebd8d929a6a7b2a564b22c2c1b8c41b36ca86a850973c1fc5e0786f6197b`。
  这是未提交检查点，不是已审查或冻结版本。
- 重新读取确认：ObservedText 已改用 `layout.text()`，但颜色仍来自 paint 阶段；
  流探针仍从私有 entry identity/index 复制归属/顺序；controller 仅有登记和预览
  fence 的两个回归。新增的 tooltip 观察接线也纳入同一收口范围。
- `git diff --check` 退出码 0。未编译、未运行 Vega 或测试，无 CUA、云端
  RED/GREEN、commit 或 PR；当前缺口仍属于测试数据/接线/既有验收覆盖。
- 下一步：写入者停写并回报精确版本，非写入者独立审查；通过后提交 draft PR
  取得有效 RED，才允许功能实现。Issue 检查点：
  <https://github.com/puzige/vega/issues/542#issuecomment-6346331392>。

## 再次恢复与收口（2026-10-22T17:12Z）

- 用户要求继续。当前工具返回的运行列表为空；前一个测试收口执行未返回 READY，
  不再等待不可访问的执行者。保留其全部增量，由单一新写入者接续缺口，不重写已完成部分。
- fetch 后远端只新增无关的 `docs/554/vega-issue-554-panel-motion-study.md`。
  已在保留脏补丁的情况下 fast-forward 到
  `2ba904f7074115de5ee89cc18c376c083f803357`；本卡生产基线与 `871f91f2` 相同。
- 接管时 tracked diff SHA-256：
  `797b30cf0d771a4739a86b558af5418706ed2dd9d0d21a05f48bdc7b67f663dd`；
  新增 `artifact_card/observation.rs` SHA-256：
  `8b9be2e189a1cf9a040bad27d272a46df6dd437b996a9143a59df1df2fa7a4e9`。
  这是恢复用未提交内容标识，不是冻结/独立审查通过版本。
- 已观察到的修正：独立捕获 ID、布局时样式与 TextLayout 文本、窗口/裁剪/绘制归属、
  真实 assistant 几何顺序、收起时第七个文件的错误、禁用 tooltip 断言。
- 尚需收口：必要阳性对照、全部组内错误断言、controller 早期拒绝与迟到失败归属、
  运行中隐藏与真实登记的组合覆盖、测试 fixture 的 `size_bytes` 契约，以及过期证据文档。
- 当前写入者只负责测试与独占接线；生产实现仍须在固定测试独立审查和 Actions 有效
  RED 后开始。本地仅确认 `git diff --check` 退出码 0，无编译/测试/运行或 CUA。

## 依赖接线与格式静态核查（2026-10-22T17:42Z）

- 临时启用 GPUI `test-support` 后，`cargo metadata --offline --locked --format-version 1`
  最初退出码 101，明确为锁文件需要更新，不是目标行为失败，不作为 RED。
- `cargo generate-lockfile --offline` 因既有锁定的 `zip = 8.6.0` 已被撤回而失败；
  未变更锁文件，也未升级或更换 zip。改用保留当前锁记录的增量解析
  `cargo metadata --offline --format-version 1` 成功，随后同命令加 `--locked` 成功。
  两者仅解析依赖，没有编译/执行 Vega 或测试。
- 增量结果：10 个仅由既有 GPUI 测试 feature 引入的传递包；既有包身份、版本、来源、
  checksum 均不变。按解析后的依赖身份比较，既有图仅 `gpui-pre` 新增 `backtrace`
  和 `proptest` 两条边；同名多版本的标签补全不是生产依赖升级。锁记录须随独占接线
  在 GREEN 后清理，纳入独立审查。
- 当前 `cargo fmt --all -- --check` 和 `git diff --check` 均退出码 0。
  静态属性清点为 UI 12、controller 6、证据投影 4，共 22 个测试定义；不是 Actions
  发现/执行计数，类型、运行、有效 RED/GREEN 均尚未取得。

