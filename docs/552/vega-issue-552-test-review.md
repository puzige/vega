# #552 先行测试非写入者审查

## 固定输入

- 审查者：父 Agent，未写本卡 Rust 测试或支持源码；writer 已 READY 并停止全部写入。
- 生产基线：`6615748f91273bfb8ee424011b3b5a09b3777d2b`。
- writer 20 文件清单 SHA-256：`79b67e8fab6349378849f99d8b5d8ad2547e6ba77560e8101a97cdf92ad1528f`，父逐字节重建并核对匹配。
- 测试文件 SHA-256：`ac11a1d03e441b2ec090886d7ae25f096760d7663fe3296a5f136580f4e47298`；UI observer `8921fdd36c29f6bd24339f2ffff36b3e1f3e991d33d71de5d03b7673fa35422e`，terminal driver `683c56c492a9ece62c71d68f033ccc0a7af383997dd0a3f2848ba4ad813b9539`。
- 发布前父只将交接里的本机绝对路径替换为任务工作树名称，并新增本审查文档；源码、原数据和断言未变。交接自身及最终文档包哈希随提交重新固定，旧 writer 指纹仍准确表示审查输入。

## 审查结论

**静态审查没有发现阻止进入现有 draft PR/Actions 的验收或源码结构缺口；仅放行先行测试云端验证，不放行生产实现。**

1. W1–W12 与原 16 用例保留，新 6 用例补真实正文/错误/恢复、终端输入输出/scroll/session 生命周期、Diff/File 输入隔离、无 inactive draw 快恢复及流式组合。原型 root 委托真实 `render_workspace` 和 handler，外部 fixture 不替代选中/focus/scroll/恢复决策。重排仍只作为投影输入，不冒充不存在的拖拽入口。
2. 普通 TerminalView 先 start_target，再调用机械抽取的 with_session；原 30ms poll 与字段初始化等价。测试注入真实 bounded Command/parser/session，未运行 PTY/shell 或网络。普通构建 feature 关闭，不扩大一般 public API。
3. PaintLog wrapper 委托真实 layout/prepaint/paint 与 a11y 方法。小 quad 记录同一真实 paint 边界的继承 opacity，配 1→0.25→1/hidden 对照；不是 GPU glyph 像素回读。正文观察委托原 selection.rs，不复制选择/渲染算法。每个必要观察仍须首轮 Actions 证明阳性对照，不能静态宣称已能运行。
4. 実体/焦点、bounds/clip、真实 quads/输入结果/scroll 与恢复前后内容一起断言；没有以 entity 字段单独代替实际上屏。没有新 ignore/should_panic 或为了通过修改预期。
5. 原生产未有 motion sampler、aria_selected。SelectionSamples 的 None 不合成目标；它不是有效 motion RED 的独立证明。原生 selected 缺失属于原 W12 既有语义目标，单独归因。首轮有效 RED 需要结合真实背景立即跳变/缺中间帧等目标断言与已工作的阳性对照；无关编译、依赖、API、runner 或观察错误只修责任层。
6. 本卡不写 #549 共享 Motion/helper，不改 #344/#535 算法；恢复/主题/几何不变。未来实现按同一冻结测试验证，任何真正验收变化重新 TEST-FIRST。

## 本轮父执行的静态核验

- fetch＋ff：already up to date，HEAD 仍上述基线。
- `cargo fmt --all -- --check`：exit 0。
- `git diff --check`：exit 0。
- `cargo metadata --offline --locked --format-version 1`（pipefail＋JSON 解析）：exit 0，13 workspace members。
- lock 身份审计：914 个原包全部保留，当前 924，仅新增 10 个 test-support 传递包，无版本/source/checksum 升级。
- default-off feature、Vega dev-only 启用、源码 22 测试属性、原数据/断言与固定哈希、暂存区空：静态核验通过。

本地未编译/运行 Vega、Clippy、test/nextest、bench/profile 或 CUA；未取得 Rust 发现/执行/通过/失败/跳过数。没有 Actions RED/GREEN。下一步固定测试提交与可访问源码/patch，创建 draft PR 用既有 macOS fmt/Clippy/Nextest/Python 完整门禁，读取原始结果按责任层分类；有效 RED 前不实施生产。
