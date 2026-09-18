# Issues 58 / 59 / 61 · 验收记录

## 契约

- [#58](vega-issue-58-full-access.md)：新增独立 Full access，不改变 Auto 的沙箱语义；保留危险确认、取消、超时和审计。
- [#59](vega-issue-59-markdown.md)：修复把表格转换成填充空格文本的渲染路径，保留单元格样式、列对齐与窄窗局部滚动。
- [#61](vega-issue-61-thinking.md)：不再丢弃真实 ThinkingDelta，按事件顺序展示有界、默认折叠的思考块。

## Freeze

- 平台：macOS 15.8 / arm64；rustc 1.98.0；Git 2.55.0。
- 实现与测试均在 feature worktree；生产代码由专用子 agent 实现，主 agent 审查并集成。
- 自动化验证：2026-09-18 UTC / 2026-09-19 Asia/Shanghai。
- 受测 `crates/` 相对本轮基线的 tracked diff SHA-256：`7e034c117d59064ba23870668bc1126c6b02f98dea78f12b1840e6319ed04734`；后续只有文档更新。

## 专项证据

| requirement | evidence class | command | result |
|---|---|---|---|
| Full access 真实 shell、取消和超时 | E2E-REAL | `scripts/cargo-lock.sh test -p vega_tools full_access -- --nocapture` | 4 passed |
| 模式落库到真实 dispatcher、线程隔离、恢复沙箱 | E2E-REAL | `scripts/cargo-lock.sh test -p vega_conversation --test s5_acceptance full_access -- --nocapture` | 1 passed |
| 权限决策与审计 | UNIT/PROPERTY | `scripts/cargo-lock.sh test -p vega_runtime permission -- --nocapture` | 21 passed |
| 权限 UI 选择与重开 | E2E-REAL | `scripts/cargo-lock.sh test -p vega i58_full_access` | 1 passed |
| 表格语义、布局与独立滚动 | production UI integration | `scripts/cargo-lock.sh test -p vega_ui conversation_stream::tests::model_markdown -- --nocapture` | 16 passed |
| Thinking UI、顺序、终态、上下限、UTF-8 | production UI integration | `scripts/cargo-lock.sh test -p vega_ui i61_` | 4 passed |
| Thinking provider 到 controller 与 UI | E2E-REAL / MockProvider | `scripts/cargo-lock.sh test -p vega i61_` | 1 passed |
| Thinking 不增加正文 token 估算 | production UI integration | `scripts/cargo-lock.sh test -p vega_ui composer_counter_projects_estimate_calibration_and_fences` | 1 passed |

## 首次失败与审查修正

- #58 E2E 测试最初调用私有方法导致编译失败；改走已有公开 store 查询后通过，没有扩大生产 API。
- 权限 UI 套件首轮 26 passed / 1 failed：R64 基线固定三项高度。本次契约明确新增第四行，更新断言以继续固定左界、宽度、下界，并独立验证增加 48.5px 向上生长；专项重跑通过。
- #59 独立 worktree 编译误用了共享 target 的 FullAccess 依赖，造成旧源码非穷尽匹配；合到统一源码后不再发生，不清除用户缓存。
- #59 初版几何测试直接 draw 导致 GPUI current_view panic；改用真实 Render root，15 项专项通过。
- 独立审查发现 #61 UTF-8 预算尾部可能产生空块；现分配前检查可接受前缀，新增接近 1 MiB 上限时的汉字回归通过。
- 独立审查发现 #59 嵌套表格可能共用元素 ID；现 ID 包含表格序号，真实 GPUI 滚轮事件验证两个表格独立横向移动，16 项专项通过。
- workspace 首轮在既有 R44 终端测试失败（148 passed / 1 failed）：marker 文件存在后立即读到空字符串；原断言未修改，单独复跑 1 passed。源码中「文件创建」与 shell 完成写入之间存在竞态窗口，但未把源码推断当成唯一已证实根因。
- workspace 并发复跑在既有 diff refresh retry 测试失败（148 passed / 1 failed，GitFailed），R44 本轮通过。随后完整套件改为 `--test-threads=1` 串行验证，不跳过测试；结果见最终门禁。

## Residuals

- LIMIT：Thinking 只保留在当前 ConversationStream，不增加数据库存储；历史重开不能恢复此前未保存的思考内容。
- LIMIT：没有 reasoning 字段时不得制造空思考块；本次已额外验证当前配置模型的真实返回，但不代表所有服务商/模型均会返回。
- LIMIT：Full access 本次改变 bash 的 OS 启动策略，直接 read/write/edit/glob/grep 的原路径边界不变；不提升为 root。
- LIMIT：默认并发 workspace 测试存在上述两次既有用例失败；不能把串行通过写成默认并发通过。
- LIMIT：上游 `block v0.1.6` 有 future-incompatibility 提示；当前严格 Clippy 无警告错误，本轮未升级依赖。

## 最终门禁

| command | result |
|---|---|
| `scripts/cargo-lock.sh test --workspace -- --test-threads=1` | 1287 passed / 0 failed / 9 原有 ignored；退出 0 |
| `scripts/cargo-lock.sh test --workspace -- --ignored --test-threads=1` | 9 passed / 0 failed；退出 0；合计 1296 passed |
| `scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings` | 退出 0，15.45s |
| `scripts/cargo-lock.sh fmt --all -- --check` | 退出 0 |
| `scripts/cargo-lock.sh build --workspace` | 退出 0，6.58s |
| `scripts/cargo-lock.sh xtask package` | 退出 0，release build 28.25s；签名与 plist 校验通过 |

无新增依赖，Cargo manifests/lock 未变；runtime 未增加 UI 依赖，未修改数据库 schema。

## 原生应用验证

- 安装前确认空闲且无草稿；旧应用已备份，未改凭据、默认权限和会话数据。
- 安装后二进制与打包产物 SHA-256 一致：`61dde581201da918a2ddb4f050422f030f1560a340659c3293fc9995dc69f83e`。安装后的严格 codesign 校验通过。
- 原生权限菜单显示四项；「自动」说明为使用沙箱，独立「完全访问」说明无沙箱且危险操作仍需确认。未在用户真实会话中主动打开 Full access；启用与执行语义由生产 controller/真实 shell 测试覆盖。
- 新建独立显示测试任务，仅要求计算与输出三列表格，明确禁止工具和文件读写；通过 composer 实际发送，当前配置的 `deepseek-v4.1-flash` 返回成功。
- 原生截图确认三列表格列对齐、中文、粗体和行内代码显示正确；真实返回产生默认折叠的「思考过程」，点击展开后内容与正文分离。截图证据在本次任务的原生 UI 工具输出中，未公开上传用户界面。
- 粘贴接口曾报告等待剪贴板读取超时，但随后的截图确认文本已完整进入 composer；只提交一次，没有重复发送。

## Raw 日志校验

原始日志保留于本次临时证据目录；以下只登记文件名和 SHA-256，不将用户本地路径或完整日志写入仓库。

| file | SHA-256 |
|---|---|
| workspace-test-serial.log | `29b02ebdbf83ad4655f1804ca1939ffc473c60a068bf57ba7c0a78ffa637e660` |
| ignored-tests.log | `9d69e5a72837dc71daaadeb1829c3819678f7994c0310475d4a4935e1a0266aa` |
| clippy.log | `5535c4901544df3345619efc43620e451349cbadbebc0af3c4bd325163ade8d8` |
| build.log | `8668a766dea07a58e33cf8d615360ec7a7f49cc7627600613843a95bacff56ae` |
| package.log | `4b4a465da2f415ef2ab322df8e595b1056f172be81de2ffa90e8099af2d83b85` |
| fmt.log | `570382c15df180acd1c7b8254395686a4b4d39e9889146de08499b0f7ee03b9b` |
