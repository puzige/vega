# 模型兼容 v4 验证记录

关联 [#494](https://github.com/puzige/vega/issues/494)、[PR #504](https://github.com/puzige/vega/pull/504) 和[冻结规格](vega-issue-494-model-compat-baseline.md)。本轮只在现有 Actions 编译/执行；本地仅差异、格式和 metadata 静态检查，不运行 Vega 或测试。

## 可恢复的先行来源

- 生产基线 `8c57f7efb3d886c1c10429edb2bc1219a7b7a753`。
- 先行测试提交 `80f4fe45b8f8f960d2f582e2b9768c729de64f3f`：[源码树](https://github.com/puzige/vega/tree/80f4fe45b8f8f960d2f582e2b9768c729de64f3f/crates)、[完整原始补丁](https://github.com/puzige/vega/commit/80f4fe45b8f8f960d2f582e2b9768c729de64f3f.patch)。补丁已读回5,818,097字节/50文件，SHA256 `bd6b80064b7b820b0403acb65f005e6470f97229ce672c915bc7b9d842a1e3ba`；49个相关源码/接线/证据 blob 与GitHub tree逐项匹配。
- [冻结测试与逐源断言映射](vega-issue-494-frozen-model-compat-tests.json) SHA256 `59105217ffc4096445de39a03976ea00ad8d2f106f73df616294a9cddcfedef7`。133个Rust定义/19测试文件不是执行数；447个候选源定义不是447整例移植或通过。元数据758条不表示逐服务已调用。
- 三行测试接线修正 `f00a64c30cbc64bf74fba3362e266987629c62f3`：[独立更正清单](vega-issue-494-test-wiring-corrections.json)、[精确补丁](https://github.com/puzige/vega/commit/f00a64c30cbc64bf74fba3362e266987629c62f3.patch)。更正补丁已读回9,596字节，SHA256 `652e5b69c38e25b0b83457749b21eaeddec54d657e279d91dfae7cb6128291b8`。只改SQL宿主数值类型，输入/精确断言/133个定义不变；原始冻结清单未覆盖。

## 先行 Actions

首轮 [39360808726](https://github.com/puzige/vega/actions/runs/39360808726) 混合报告目标接口缺失和测试自身FromSql<u64>错误，未用它推进实现；[先审查再修正](https://github.com/puzige/vega/pull/504#issuecomment-6282232482)。修正后 [39361620256](https://github.com/puzige/vega/actions/runs/39361620256) 才确认有效接口RED，见[逐错误与原始日志SHA](vega-issue-494-interface-red.json)。Rust发现数未取得、执行0；全部行为断言仍未执行。Format和Python5+15+14成功。

[明确授权记录](https://github.com/puzige/vega/pull/504#issuecomment-6282337866)之后才开始生产实现；不把旧v3 GREEN、空集或来源清点当作本轮行为证明。所有测试/fixture/预期保持冻结，测试自身错误须先审查留证；验收变化重开完整周期。

冻结用例的[工具续接边界审查](https://github.com/puzige/vega/pull/504#issuecomment-6282663679)保留两类原断言：DS/GLM 工具提议缺真实 raw 时零工具效果；合法工具提议携带真实空字符串 raw、执行后收到无 raw 的纯正文终态时可正常完成。仅注册工具不代表本轮调用工具，不合成缺失字段，不借此放宽 Responses 的原始布局与跨 API 检查。

## 生产数据静态审查

统一目录由固定官方 npm 的当前两 API 数据单独提取，758条，与先行独立 fixture 的全部保留字段做 multiset 比较：0遗漏/0额外。生产JSON SHA256 `3c7bfe4d340715cdba2ddc689d7e302ba62f51377025d622fe33cb223bdae1f1`；fixture SHA256 `8987a9a0dbacf211062e8563ae98c9e65419ce5a3e81de565b3a1e254aae5942`。来源与每个分发文件SHA记录在 `crates/vega_runtime/src/model_compat/pi-ai-0.84.4.sources.json`，继续保留pi MIT许可。此为静态来源核验，不计行为测试或758个真实服务通过。

主会话独立核对全部25个冻结测试源码/支持/fixture 的文件哈希，与原始清单及先审查的三处 SQL 类型修正逐项一致；两个大型来源/冻结清单和修正清单保持原摘要。全 workspace 生产 dependency/build-dependency 与基线相同。`cargo metadata --locked --offline --no-deps --format-version 1` 静态读取成功；它没有编译或执行测试，不能代替 GREEN。

[生产提交前静态清单](vega-issue-494-production-static.json)固定48个生产修改/新增/删除路径及摘要、25个冻结 artifact 和133个原测试定义，SHA256 `08c7299ed4a619debfad4c962989126a2638ba35dff347d1c86bf77793e78b1d`。主会话逐项读回当前生产文件摘要一致，并独立执行 fmt check、diff check；清单 head 是实现开始前的父提交，生产内容以逐路径摘要识别。尚未取得本轮行为 GREEN。

## 后续门禁

首个带实现提交 `f4528d3837c9a9369a6a0b2756ff647aa4d25905` 的 [Actions 39365322166](https://github.com/puzige/vega/actions/runs/39365322166) 在测试编译阶段失败：GPUI 宏生成的标准 test 属性被同名导入遮蔽而递归，两个测试 MutexGuard 的显式 drop 仍触发 Clippy 锁作用域告警，另有无用测试 import。Rust 发现未取得、执行0，尚无行为结果；Format和Python34成功。[先审查的机械修正范围](vega-issue-494-implementation-compile-corrections.json)包括解除 test 宏名称遮蔽、保留同一组断言的锁作用域、删除无用 import/旧焦点 getter，以及既有真实设置路由方法的跨 crate 可见性，不改变验收输入/断言/阈值。

这些修正提交为 `268ca25da7b4f8963b6572a059de72248672142b`，[精确补丁](https://github.com/puzige/vega/commit/268ca25da7b4f8963b6572a059de72248672142b.patch)已读回49,742字节、SHA256 `7abf9bbdbe567e47196b2181dc71ae56be386222bac5a83786d25c660a847c62`，25个冻结 artifact 的 GitHub tree blob 与本地逐项一致。[Actions 39366258750](https://github.com/puzige/vega/actions/runs/39366258750)解除前述问题后，继续发现缺少 `TestWindowExt` 测试扩展导入与测试 helper 的纯值 `then` 写法 lint；仍为发现未取得/执行0，Format及Python34成功。[两处机械修正的先审查记录和摘要](vega-issue-494-compile-wiring-third.json)继续保持原输入与断言，不修改生产行为。

第三处接线提交 `2971b357daac35187593c4aae3c485397a4096ae` 的 [Actions 39367011160](https://github.com/puzige/vega/actions/runs/39367011160) 首次完整运行本轮行为基线：**133 发现、133 执行、123 通过、10 失败、0 跳过**。Format 和 Python34成功；Clippy另报App重复声明同一HTTP测试helper。[逐失败名称、原始job和完整日志SHA](vega-issue-494-behavior-red.json)保留本次结果，不把123部分通过称完整GREEN。

主会话独立读取固定npm来源、真实工具schema/图片生产者和GPUI事件/滑块实现后，确认其中包含测试资料与物理接线错误。[修改前公开审查](https://github.com/puzige/vega/pull/504#issuecomment-6283260542)固定了[精确更正范围](vega-issue-494-behavior-red-review.md)：模型/API身份、明确调用方输出上限、非法类型负例、无Off时的索引、异步事件观察、唯一HTTP helper和真实工具图片链。它们保留逻辑验收及原结果断言，但确实改变测试资料/输入/操作位置，不能声称仍是原始字节未变。原始冻结和源映射清单不覆盖；完整更正须有独立源码/摘要和前后证据。

更正提交不同时修复已确认的Responses凭据检查、当前模型容量权威和InvalidRead执行前终止缺口。先再次运行同一133定义，保留资料更正后的行为RED，再按未通过的冻结断言授权最小生产修正。

更正提交 `f7128d6d25c7d4ea864b8974d975de9125053eed` 的 [Actions 39369557697](https://github.com/puzige/vega/actions/runs/39369557697) 完整执行 **133发现、133执行、129通过、4失败、0跳过**，Format、全目标Clippy和Python34成功。[原始失败与日志摘要](vega-issue-494-corrected-behavior-red.json)确认三个未改测试的真实实现缺口；其余失败是容量测试第二分支错误假定OverLimit返回Ok(failed)，而生产未改8c与当前都在发送前返回准确typed Err。[修改前公开审查](https://github.com/puzige/vega/pull/504#issuecomment-6283447290)先单独更正这一测试接口假定，再限定四个生产文件修复上述三个真实缺口；不修改三个目标测试，不把任意Err当通过。完整更正、范围与顺序见[剩余修正审查](vega-issue-494-remaining-repair-review.md)。

完整带测试GREEN仍待取得。固定可恢复证据后才独立删Rust测试及专用支持；最终head应由现有全门禁确认0发现/0执行和no Rust tests，空集不代表行为GREEN。

真实模型服务、原生应用和外层Trusted Git进程流程由用户合并后纯手动验证，不能把网络替身测试称真实服务或系统E2E。Issue在用户明确通过前保持开放，当前In progress。


## 合并后的用户手测

1. 更新到包含本轮集成提交的版本。选一个未知代理模型，在设置思考页多选 low/high，保存并重载；输入框应只提供当前允许项及真实可用操作。另一个提供方/模型保持原值。
2. 会话选 high，再在设置中将允许集合改为空。会话应保留可见的失效选择并允许选择提供方默认；实际重发恢复，能力保存不能偷偷改写偏好。恢复自动只移除当前模型的能力覆盖。
3. 在基本信息切换图片输入，分别用当前图片和带图片的历史尝试发送。不支持时应明确阻塞并可恢复，开启后保留真实图片；真实服务若不接受图片则保留其错误，不声称自动识别保证任意代理服务能力。
4. Step和DeepSeek进行连续工具调用，收到真实结果后再续问；空finish中间帧不能提前失败，不应重复同一工具效果。对已知只支持开关或部分等级的模型核对选项与实际服务接受范围。
5. 验证草稿取消、保存失败/重载、恢复自动、键盘焦点。保存档位不应固化未编辑的图片继承值，修改图片同理；容量和价格设置保留。

回滚以整体revert本轮PR恢复先前生产逻辑，不删除用户能力/偏好文件或历史，不恢复读取旧reasoning.toml。手测反馈只提供脱敏错误与场景，不公开凭据或原始私有思考内容。
