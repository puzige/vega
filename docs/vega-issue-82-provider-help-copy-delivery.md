# Issue #82 Provider 说明收纳交付记录

## Freeze

- 已验证时间：2026-09-25 14:28 UTC / 2026-09-25 22:28 CST。
- 分支：`feat/82-provider-help-copy`；实现前及交付前均检查 `origin/master`，交付前未发现基线漂移。
- 任务契约：[冻结规格](vega-issue-82-provider-help-copy.md)。实现与冻结范围一致。
- 实现文件源码差异 SHA-256（不含本交付记录）：`f9b4148708ee4d583eae4f343da50ef3c433d8746e9d0a352c15f94903d301a2`。
- 环境：Darwin arm64；rustc 1.98.0；cargo 1.98.0；git 2.55.0。

## 验收矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
|---|---|---|---|---|---|---|---|
| H1 | 说明文字不再常驻；提示文本和冻结规格一致 | 已选中供应商 | 悬停发现帮助图标，再移出 | 图标附近出现同一说明，移出后隐藏；原段落已从详情布局移除 | GPUI | 当前树：`issue81_provider_settings_remove_pi_import_and_keep_credential_states` | PASS |
| H2 | 键盘用户能读取说明，焦点离开后提示隐藏 | Provider 页面已渲染 | 聚焦发现帮助图标，再把焦点移到模型帮助图标 | 两个位置均显示同一提示；原位置提示随失焦隐藏；控件有 focus-visible 样式和无障碍名称/描述 | GPUI | 当前树：`issue81_provider_settings_remove_pi_import_and_keep_credential_states` | PASS |
| H3（历史基线，#223 前） | 凭据状态与当时的 Pi 导入入口 | 一个本地已存储 key 的供应商和一个缺失 key 的供应商 | 分别检查两个供应商 | 存储/缺失状态按原分支渲染，Pi 导入入口可见；不把 key 正文放入视图 | GPUI | `mounted_pi_import_*` 四项旧回归 | HISTORICAL：#223 移除该入口与旧测试；不代表当前行为 |
| H4 | 帮助图标不遮挡测试、编辑和删除操作 | 960px 窄窗口、至少一个模型 | 检查发现和模型行控件边界，并切换深浅主题 | 图标紧邻对应操作；按钮之间不重叠；图标在窗口内 | GPUI | 新增的 #82 用例 | PASS |
| H5 | 模型发现/测试行为与详情行布局不回退 | 固定配置与 mock provider transport | 发现模型、测试模型；在小窗口检查 URL 与滚动区 | 既有请求路径和结果保持；窄窗下 URL 行完整，内容溢出由 viewport 滚动 | GPUI | `pointer_settings_uses_real_service_config_and_mock_transport`；`small_provider_detail_retains_url_height_with_multiple_models` | PASS |
| H6 | Provider help tooltip 不被滚动视口裁切 | 已选中供应商，已渲染 Settings → Providers | 960/800/680px 窗口分别 hover/聚焦发现模型与模型测试图标，并切换浅色/深色主题 | 全文完整显示；提示留在详情视口内，680px 下自动换行 | GPUI | `issue82_provider_tooltip_remains_visible_in_narrow_windows_and_themes` | PASS |
| H7（#223 后当前行为） | Pi 导入入口移除后，凭据状态和帮助交互保持正确 | Provider 页面已渲染，含已存储与缺失 key 的供应商 | 检查入口、凭据状态，并验证帮助 hover/focus | Pi 导入入口不存在；存储/缺失状态与帮助交互正确 | GPUI | `issue81_provider_settings_remove_pi_import_and_keep_credential_states` | PASS |

### v0.1.22 回归修复补充

- 桌面回归发现，“发现模型”帮助提示在 hover 与键盘聚焦时只露出尾部文案，浅色和深色模式均复现；模型测试帮助提示完整。检查未点击模型测试，也未发送 Provider 请求。
- 旧布局复现：960px GPUI 窗口中 tooltip 边界为 x=422…606，Provider 详情视口从 x=524 开始，左侧 102px 被裁；只剩右侧约 82px 文案可见。
- 修复：tooltip 现以整行操作区为定位容器，限制最大宽度为 224px；窄列按详情栏宽度收缩，文案容器显式允许换行。发现模型提示左对齐操作行，模型测试提示右对齐对应行，并通过 deferred 绘制离开滚动内容遮罩。
- 新回归覆盖 960、800、680px，以及浅/深主题和 hover/键盘焦点。680px 下断言文案区增高，证明发生换行；所有提示边界都落在 Provider 详情视口内。
- tooltip 修复不修改网络行为或凭据处理；Computer Use 检查期间不点击模型测试，也不发送 Provider 请求。Pi Agent 导入入口后来按 #223 单独移除，当前行为由 Issue #81 规格管理。

#### Fix Freeze

- 验证时间：2026-09-26 03:36 UTC / 2026-09-26 11:36 CST。
- 分支：`feat/82-provider-help-overflow`；实现前 fetch/rebase 后与 `origin/master` 一致。
- 实现提交：`d2b51a8`；PR [#224](https://github.com/puzige/vega/pull/224) 已创建，未合并；Clippy/Nextest 云端检查正在运行。
- Issue #82 评论已记录实现、测试结果和交付状态；Issue 保持开放，Project 状态为 In progress。
- 任务契约：[补充规格](vega-issue-82-provider-help-copy.md)；实现范围与补充验收条件一致。
- 源码、测试与补充规格差异补丁 SHA-256（不含本交付记录）：`d6450ef327afc3e9527242c44f79183edc1aee9427968e85fa1136a0bfc5c6bd`。
- 环境：Darwin arm64；rustc 1.98.0；cargo 1.98.0；git 2.55.0。

#### CI 完成记录

- GitHub Actions run [36215367050](https://github.com/puzige/vega/actions/runs/36215367050) 在 #223 合并前的 PR head 上完成：Clippy、Nextest 与 required `check (fmt, clippy, test)` 全部通过。该结果不覆盖下面记录的 rebase head。
- #223 合并后，PR #224 分支已 rebase 到 `origin/master` 的 `ecd24282435763f7ed21d79873ec6b943af4774e`。推送该 head 后需等待新的云端 required check。

#### #223 后 rebase 整合

- 冲突仅出现在 `provider_management/tests.rs`。保留 #223 的 Pi 凭据导入移除和当前无入口断言；丢弃冲突带回的四个旧导入测试，保留 #224 的 tooltip 窄窗/主题回归，并将其 fixture 切换到当前 Provider 凭据状态 fixture。
- 最终 Provider 设置实现不含 `ImportPiCredential`、`import_pi_credential_background`、`provider_service` 或 `provider-import-pi` UI/API；测试中仅用 `provider-import-pi` selector 断言该入口缺失。
- 当前 rebase head 的 GPUI 定向测试、`cargo fmt --all -- --check` 和 `git diff --check` 结果见下方 Results。
- master 集成后的桌面 Computer Use 与真实 Provider 服务验收仍待执行。

## 实现与兼容性

- 移除 Provider 详情中的常驻说明段落；在“发现模型”和各模型“测试”旁加入紧凑帮助图标。
- hover 与键盘 focus 共用相同的主题 tooltip 和冻结文案。提示不需要点击，也不启动网络请求。
- 保留凭据显示分支、发现/测试按钮顺序和请求处理。Pi Agent 导入控件后来按 #223 移除；本次 tooltip rebase 未恢复该 UI/API。没有配置格式、持久化、密钥存储或更新协议变化。
- 影响模块：共享图标、Provider Settings 渲染/焦点句柄和对应 GPUI 测试。
- 失败路径与状态切换通过帮助提示、stored/missing 状态断言及 Provider 操作测试覆盖。Pi 导入成功/失败/进行中测试属于 #223 前历史测试，已由 #223 删除。
- 非目标和回滚方式：不改测试请求/Provider 网络行为；如需回滚，只需撤销本实现变更，无数据迁移。

## 实施步骤

1. 复核 #82 冻结规格、Provider 详情和现有 GPUI fixture。
2. 实现图标/提示并保持凭据显示、发现和测试行为稳定。
3. 添加 hover/focus、凭据状态及窄窗布局回归；收小旧滚动回归的窗口以保留真实 overflow 情形和原断言。
4. 仅运行当前 #82 tooltip、Provider 设置与详情布局相关的定向 nextest；本地不跑全量测试。
5. 检查 rustfmt 与 diff，提交本卡并创建关联 PR；云端完整 PR gate 待 PR 页面结果。

## Results（#223 合并后的当前基线）

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
|---|---|---|---|---|---|
| #223 后 Provider 状态、Pi 导入入口移除、tooltip 窄窗/主题、Provider 操作与详情布局 | GPUI targeted | `cargo nextest run -p vega_ui -E 'test(issue81_provider_settings_remove_pi_import_and_keep_credential_states) | test(issue82_provider_tooltip_remains_visible_in_narrow_windows_and_themes) | test(pointer_settings_uses_real_service_config_and_mock_transport) | test(small_provider_detail_retains_url_height_with_multiple_models)'` | exit 0；4 passed，509 skipped | 0.240s 测试运行时间 | Nextest `e9383739-0e7c-4670-8f38-9e7d06e1018d` |
| 格式 | formatter | `cargo fmt --all -- --check` | exit 0；stdout 空 | — | — |
| 空白/冲突标记 | git | `git diff --check` | exit 0；stdout 空 | — | — |

当前基线定向 nextest 原始输出：

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 23.82s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID e9383739-0e7c-4670-8f38-9e7d06e1018d with nextest profile: default
    Starting 4 tests across 1 binary (509 tests skipped)
        PASS [   0.071s] (1/4) vega_ui settings::provider_management::tests::small_provider_detail_retains_url_height_with_multiple_models
        PASS [   0.122s] (2/4) vega_ui settings::provider_management::tests::issue81_provider_settings_remove_pi_import_and_keep_credential_states
        PASS [   0.162s] (3/4) vega_ui settings::provider_management::tests::pointer_settings_uses_real_service_config_and_mock_transport
        PASS [   0.239s] (4/4) vega_ui settings::provider_management::tests::issue82_provider_tooltip_remains_visible_in_narrow_windows_and_themes
────────────
     Summary [   0.240s] 4 tests run: 4 passed, 509 skipped
```

Pre-#223 Pi import test commands and their output have been removed from the current results because #223 deleted those tests and the corresponding UI/API. Their former existence is recorded only as a historical baseline in H3.

### 失败复现与修复记录

- 首次 `cargo fmt --all -- --check` 返回 1，仅报告实现文件的格式化差异；运行 `cargo fmt --all` 后复查通过。
- 新增边界断言首次运行旧实现时返回 100：`tooltip=Bounds { origin: Point { x: 422px, y: 300.5px }, size: Size { 184px × 28.5px } }`，详情视口从 x=524 开始，证明左侧文案遭裁切。
- 首轮布局修复后增加 680px 文案换行断言，定向回归返回 100：外框已受限到 132px，但文本仍保持 28.5px 单行高度。给文本 flex 子项添加 `min_w_0` 与 normal wrapping 后，680px 换行断言通过。
- 首次定向编译因 gpui-kit 0.6.0 未提供 `IconName::CircleHelp` 而返回 101：`error[E0599]: no variant, associated function, or constant named 'CircleHelp' found for enum 'IconName'`。改为项目既有 Lucide SVG 约定的内联问号圆形图标，定向编译通过。
- 首次运行 `small_provider_detail_retains_url_height_with_multiple_models` 失败：`overflow must expand the scrollable flow: flow=Size { 412px × 475px }, viewport=Size { 412px × 475px }`。说明移除常驻段落后，该 600px 窗口的内容不再发生滚动溢出。将测试窗口高度减至 560px，保留原来的完整 URL 行和 overflow 断言；复测通过。
- 流程偏离：新增 GPUI 回归用例是在实现改动后补入，而非先运行失败用例再实现；最终定向回归集全部通过，未对断言放宽。
- `cargo clippy --workspace`、workspace 全量 nextest、真实 Provider 网络和安装后桌面 Computer Use 未在本地运行；云端 PR gate 和 master 集成后的真实桌面验收分别依既有流程完成。

## Residuals

- `PASS (pre-rebase only)`：PR #224 原 head 的云端 Clippy、Nextest 与 required check；rebase 后的云端 check 尚待 push 并重跑。
- `PASS`：#223 后 rebase head 的 GPUI 定向测试、格式和 diff 检查。
- `NOT RUN`：master 集成后的桌面 Computer Use 和真实服务验收；依仓库验收规程由主控/用户在 master 完成。
- `NOT RUN`：workspace 全量测试和全量 Clippy；本地按规程只执行本卡定向 nextest，PR gate 承担完整门禁。
- `NOT MERGED`：PR #224 仍开放；rebase head 的云端 check、master 集成和桌面复验仍待完成；当前 Issue 开放且看板为 In progress。
- spec 偏离：无。
