# #552 Workspace 选中背景实现续接

## 当前阶段

2026-10-23 暂停恢复续接；本轮为 #552 的窄实现阶段唯一 writer，前任已停止、无活跃写入权。本 worktree `/Users/puzige/Workspace/worktrees/vega-552-workspace-motion`，分支 `feat/552-workspace-motion`，HEAD `490ccae81beed7668b6e4bc4f270345d8f10ab24`。父 Agent 已 fetch 并合入 master `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`；本轮只读核对其为 HEAD 祖先，不再次 fetch/merge，不修改 Git/index 或外部状态。

恢复入口的 tracked `git diff --binary` SHA-256 精确匹配 `11a8227d48db6a8b56487832c3ccce7e2f4f5373a7455fb0576b54fb16dd6d0c`。两份 Rust 源码哈希也精确匹配下表；本轮检查后没有必要的源码/API/格式修正，保存的 product/harness diff 原样保留，唯一新增写入是本续接报告。原 spec §1 的 W1–W12、§2–3 的视觉与生命周期验收不变；当前政策不强制 TDD，旧文档的有效 RED/冻结顺序只作历史。

| 当前内容 | SHA-256 |
|---|---|
| `crates/vega/src/window/workspace.rs` | `52000be16fcd49228ad2a45885b30d34c484537260fc80e489f334f78cf36965` |
| `crates/vega/src/window/workspace/motion_tests.rs` | `8c8f98b460be3014d8ba6dd98b70284d6547bae3be07d6f5a0b4772f8c75b153` |

### 历史检查点与首次云端失败

上一份续接报告记载的 master `9f9fa092a3ca391a65f08419c9206f32979119e6`、merge head `07fb63240ef9c1a9488de65a2a5261d5ce2335de`，以及仅 builtin 导入修正时的测试 SHA-256 `16df038551c4cb9eb83352284d92a95e8fa8828661f18e43d91fffe71db852db`，均为历史版本，不代表本轮基线。

原测试提交 `0382a72ecf94f893a6d545b75cdad2b71e1c45d4` 的首次 [Actions 39842518890](https://github.com/puzige/vega/actions/runs/39842518890) 在生成的裸 `#[test]` 递归展开时失败，行为未执行；本轮沿用父 Agent 提供的该失败证据，没有查询、重跑或派发云端任务。当前保留 `use core::prelude::v1::test;` 和既有 `use gpui_kit as gpui;`，静态复核锁定宏确实输出裸 `#[test]` 及 `gpui::run_test`，修正责任层正确但尚未取得修正后的编译/执行结果。不提高 recursion_limit、不放宽断言、不重做 22 个测试。

## 实现范围与验收

- 按主规格 §2–3，仅过渡现有 tab 选中背景强度 0/1，120ms、Kit 默认 ease_out_cubic；文字、图标、关闭按钮、焦点环、命中、真实选择及 body 即时。无宽高/整层 opacity/body crossfade。
- 按真实 pane + TabKey + 属性稳定 key；初挂/重挂当前终态、连续重定向、不排队；关闭/移动/重排不复制业务实体，不延长 Terminal/Diff/file 生命周期。
- 消费 #549 唯一共享接口：`Motion::TAB_SELECTION_MS = 120`（`u64`，供 `Duration::from_millis` 使用）；`vega_ui::interaction::decorative_value(id: SharedString, target: f32, duration: Duration, visible: bool, window: &mut Window, cx: &mut App) -> MotionValue<f32>`。按当前委派包，共享 writer 在独立 worktree 提供；本树尚未集成这两个符号。本卡不修改 theme/interaction、不自建计时/activation 引擎、不加重复实现或 fallback，也不轮询等待共享源码，由父 Agent 在本 writer STOP 后集成并对组合不可变版本云端验证。
- 在真实采样返回处连接现有开发观察器 `motion_tests::observe_selection_sample(pane, key, status, cx)`；真实渲染背景必须使用同一返回 value。不得给观察器常量或目标伪造中间帧。
- W12 现有缺陷明确纳入最小补齐：tab `.aria_selected(active)` 来自即时业务 selected，不能从视觉进度推导。不更改导航/焦点语义。
- 原有 reveal 请求、hover/pressed/focus、loading/error/retry、input/scroll/session isolation、stream + terminal 并发 W1–W12 均保留。隐藏/失活/Reduce Motion 终态及无 inactive draw 恢复由共享 lifecycle 保证。

## Harness 与验证边界

父 Agent 合入的 master 增加了 VegaWindow 的 CUA onboarding 字段；保存的 `isolated_owner` 已机械补齐 `onboarding: cua_onboarding::OnboardingHost::default()`。本轮核对真实 `VegaWindow` 字段、生产 constructor 的相同默认初始化及 `OnboardingHost` 的 `#[derive(Default)]`，没有调用 `VegaWindow::new`、CUA 或其他启动路径。除此项和 builtin 导入，整个测试文件与当前 HEAD 的原内容逐字相同；22 个 `#[gpui_kit::test]` 定义、所有既有断言和 W1–W12 回归均保留。22 仅是静态定义数量，不是云端发现/执行/通过数量。

允许源码写入 `crates/vega/src/window/workspace.rs` 和本目录 `motion_tests.rs` 的最小 harness 修正；文档 `docs/552/`。其他跨 crate 测试支持已存在，若发现其真实错误先报告给父，不并行改共享范围。当前 `vega_ui::test_support` 经 `#[cfg(feature = "test-support")]` 门控，由 vega dev dependency 启用；不把它当作新增生产支持问题。本轮无跨 crate/support/依赖/registry 写入，无新增代码注释。

## 本轮静态检查

以下命令均在本 worktree 执行；未运行 Vega、compile/run/check/Clippy/test/build/bench/profile/CUA，未运行 Cargo，也未触发依赖下载。

| 命令或只读检查 | 退出码 | 实际结果 |
|---|---|---|
| `git branch --show-current`、`git rev-parse HEAD`、`git status --short --branch` | 0 | 分支/HEAD 与恢复包相同；仅两份已修改 Rust 文件和本报告（未跟踪） |
| `git merge-base --is-ancestor 5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb HEAD` | 0 | 父 Agent 更新后的 master 基线已在本 HEAD 中 |
| `git diff --binary \| shasum -a 256`；`shasum -a 256 crates/vega/src/window/workspace.rs crates/vega/src/window/workspace/motion_tests.rs` | 0 | 保存的 diff 与两份源文件哈希均精确匹配恢复包 |
| `rustup run 1.98.0 rustfmt --version` | 0 | `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)` |
| `rustup run 1.98.0 rustfmt --check --edition 2024 --config skip_children=true crates/vega/src/window/workspace.rs crates/vega/src/window/workspace/motion_tests.rs` | 0 | 两份允许源码格式通过；不遍历子模块、不写文件 |
| `git diff --check` | 0 | tracked patch 无空白错误；未跟踪报告另作最终静态检查 |
| `rg -c '^#\[gpui_kit::test\]' crates/vega/src/window/workspace/motion_tests.rs` | 0 | 22 个定义；不是宏展开、发现或执行 |
| 只读 `python3 - <<'PY'` inventory（正文在本轮工具记录） | 0 | 断言当前 HEAD/分支/祖先、两份文件及 tracked diff 哈希、仅允许路径、空 staged diff、本报告为唯一未跟踪文件；去掉两条已申明 harness 新增行后测试与 HEAD 逐字相同，22 定义保留；tracked diff 无新增注释 |

定位阶段有非产品失败：一次末尾 `ls` 因不存在可选 `.rustfmt.toml` 返回 1；一次 zsh 无匹配候选 registry glob 返回 1；两次候选源路径不存在的 `rg` 返回 2。随后读取实际 `window/mod.rs`、`window/workspace.rs` 与现存 pinned registry 路径完成所需核对；没有用这些定位故障认定 RED，也未重跑任何带副作用命令。

## 实际 API 依据与待组合验证项

静态读取 `Cargo.toml`/`Cargo.lock` 的真实锁定版本为 `gpui-kit/gpui-base 0.6.1`、`gpui-pre/gpui-pre-macros 0.3.4`；不是设计守则的历史 0.6.0。只读本机对应 registry 源码，无修改：

- `gpui-base/src/motion.rs:116–123, 237–343`：`Transition::new` 默认零 delay、ease_out_cubic；`MotionValue<f32>` 的公开 `value/status` 可供同一返回值读取；Kit 首值采用 target、从当前 sample 重定向、Reduce Motion/零 duration 同步 target，仅 Delayed/Running 请求下一帧。这里只核对 Kit，不声称已验证未集成的 #549 wrapper。
- `ThemeColors::bg_active_alpha` 的实际类型是 `Rgba`；`gpui-pre/src/color.rs:126–133` 的 `Rgba::opacity(f)` 乘原 alpha，因此 `bg_active_alpha.opacity(1)` 保留主题终点，`opacity(0)` 为透明。保留 light 0.05 / dark 0.03 的原主题 alpha，没有对标签树/body 设置 opacity。
- `gpui-pre/src/elements/div.rs:1352–1355`：`aria_selected(bool)` 直接写 `Some(selected)`。本实现传即时业务 `active`，不传视觉 progress；平台 AX 完整性与实际键盘行为未验证。
- `gpui-pre-macros/src/test.rs:185–208`：裸 `#[test]` 与 `gpui::run_test` 需要当前 builtin 显式导入和 Kit alias；这只是修正的源码依据，不是宏展开已成功。

组合版本必须由父 Agent 确认 #549 的真实接口与上述签名一致，token 类型/值匹配，返回的 `MotionValue` 来自真实 Kit sampler。shared lifecycle 必须按 stable key 记录 activation 边沿，再通过 Kit 零 duration snap；仅检查恢复帧的 `window.is_active()` 不足以覆盖 deactivate → 无 draw → restore。隐藏/失活/Reduce Motion 的终态、恢复不重播、首挂/重挂及连续 retarget 仍待组合版本 Actions，不能由本轮静态检查宣称通过。当前没有在获准两份源码中发现其他需要修正的 API 问题；跨 crate 编译兼容性未取得，不添加 fallback 掩盖。

## READY / STOP 与父 Agent 下一步

本窄阶段 READY、停止写入；不是独立审查通过、编译/行为 GREEN、已合并或用户验收。保存的 product diff 为 21 行局部变更，harness 为 2 行新增；本轮只更新本报告。tracked diff SHA-256 保持恢复包的 `11a8227d48db6a8b56487832c3ccce7e2f4f5373a7455fb0576b54fb16dd6d0c`。报告自身 hash 与包含未跟踪报告的完整 patch hash 在最终 STOP 回报中给出，避免在报告中自指；完整 patch 定义为依次拼接 `git diff --binary` 和 `git diff --no-index --binary -- /dev/null docs/552/vega-issue-552-implementation-continuation.md` 的 stdout，后者表示新增文件时退出 1 属正常。

下一步由父 Agent 独立审查该停写内容，集成 #549 并固定完整不可变版本，再沿用现有 draft PR #566 Actions。保留全部 22 个开发测试，不重写预期求绿；原始宏递归之外可能存在的编译/接线/观察或产品问题按真实日志归责，跨 crate 支持问题交相应 owner。当前组合版本的编译、测试发现/执行/通过/失败/忽略/跳过数均未取得；W1–W12 的阳性对照、真实选择/正文/aria/focus/reveal/几何、连续性/生命周期、Terminal/Diff/file 实体及输入/滚动、真实 stream + terminal 控件更新均待云端验证。真实 PTY、平台 AX、显示器呈现/外观/性能仍仅用户手测，未声称收益。

开发验证与可恢复源码/patch、完整 SHA、run/job 证据固定后，按当前政策由父 Agent 另阶段清理 Rust 测试及独占支持；清理不消解未解决失败。最终无 Rust 测试 head 必须独立审查并通过完整 required CI（0 发现/0 执行写 `no Rust tests`，不是行为测试通过），之后才能合并，Issue 仍开放等用户手测。本轮无 commit/push/PR/Issue 变更，无 #553 实现，无规格偏离。
