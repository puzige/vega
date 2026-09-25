# Issue #181 follow-up: secure update staging directory permissions

来源：[Issue #181](https://github.com/puzige/vega/issues/181)、[自动更新规格](vega-issue-181-auto-update.md)、[独立更新签名规格](vega-issue-181-independent-update-signing.md)。本 follow-up 只修复桌面端真实更新时的暂存目录权限错误，不改变更新协议与既有安装安全边界。

## 事实与根因

- 本机 Vega v0.1.9 检查到 v0.1.10 后，下载阶段在 UI 报“更新暂存目录权限无效”，新版本尚未安装。
- `Worker::check_release` 当前使用未显式设置权限的 `tempfile::Builder::tempdir_in(parent)` 创建下载目录，随后调用 `platform::private_dir`。
- `private_dir` 要求目录属于当前 effective user，且 `mode & 0o077 == 0`。创建出来的目录因而必须明确为 owner-only；不显式控制 `tempfile` 权限会使此校验失败。
- 安装 helper 在通过 `private_dir` 校验的下载 staging 内创建 `authenticated-*` 解包目录。该父目录已经不可由 group/world 遍历；follow-up 仍统一显式设定嵌套目录为 owner-only，以保持解包区自身的私有权限契约。

## 冻结范围

- 为 updater 提供唯一私有临时目录构造路径，调用 `tempfile::Builder` 受支持的权限 API 明确设置 Unix mode `0o700`，然后继续执行现有 owner/mode 安全校验。
- 下载暂存目录和安装 helper 的认证解包目录都使用该构造路径。其他 `tempfile` 用途（例如临时文件）不变。
- 保留对符号链接、所有者和 group/world 权限的现有拒绝行为；不放宽 `private_dir`，不提权，不改下载、签名、解包、替换、回滚、启动确认或恢复协议。
- 不新增依赖、公开 API、数据库状态或配置项。

## 验收矩阵

| ID | 前置状态 | 操作 | 预期 | 测试 |
|---|---|---|---|---|
| STG01 | 当前用户拥有的临时父目录 | 经 production staging helper 创建目录 | effective UID 为当前用户，mode 恰为 `0o700`，既有 `private_dir` 校验通过 | updater 定向 Nextest |
| STG02 | 上述私有目录 | 将 mode 改为 `0o755` 后调用 `private_dir` | 保持拒绝，错误为“更新暂存目录权限无效” | updater 定向 Nextest |
| STG03 | 下载与安装流程 | 检查两处临时目录构造调用 | 下载 staging 与认证解包目录均调用 production 私有目录 helper；其余验签、恢复流程不变 | 静态审查 |
| STG04 | 真实桌面端 v0.1.9 → v0.1.10 更新 | 在修复版本发布后用 Vega 设置触发检查/下载 | 不再出现权限错误，下载到 Ready；原安装在用户明确重启前保持不变 | 合并发布后的 Computer Use 验收；本实现阶段 NOT RUN |

## 实现与验证计划

1. 先在 production helper 的回归测试中复现默认权限未满足私有目录契约的失败。
2. 用 `tempfile::Builder` 的权限设置 API 明确请求 `0o700`；下载与嵌套解包目录统一使用 helper。
3. 运行仅匹配 STG01/STG02 的 `cargo nextest run -p vega -E 'test(updater_private_tempdir_)'`；运行 `git diff --check`。
4. 在交付记录保留最初失败和修复后原始测试输出；不运行 workspace 全量测试，不在此 follow-up 提交 push、PR 或 merge。

## 偏离与恢复

- 与父规格无偏离。新增显式 `0o700` 是使现有 owner-only 校验与目录创建契约一致。
- 失败时仍按原路径返回错误并 fail closed；不继续下载或触及已安装 app。代码恢复可通过回退此 follow-up commit 完成。
