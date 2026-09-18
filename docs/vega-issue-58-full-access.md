# Issue 58 · Full access 的真实执行语义

关联：[GitHub #58](https://github.com/puzige/vega/issues/58)。2026-09-18 用户要求开始实现该 Issue：选择 Full access 时直接绕过 Vega 的 OS 沙箱。

## 已确认问题与契约修订

Composer 目前将 Auto 描述为「不受限访问」，但 Auto 仅免除普通确认，runtime 不把权限模式传到 bash launcher，所有命令仍由 Seatbelt 启动。2026-09-18 用户进一步明确：**新增独立 Full access，保留自动模式**。

本任务对 tech-spec §4.4.2 / S5 T24「所有 bash 必须套用 Seatbelt」作明确例外：**Execute + FullAccess 使用直接 shell 启动；其余模式保持原契约**。这是用户要求的产品能力，不是 sandbox-exec 故障时自动降级。

## I58 规则

1. I58-R1：新增类型化 `PermissionMode::FullAccess` / `RuntimePermissionMode::FullAccess`，持久化值为 `full_access`。旧 `auto` 仍自动批准普通操作且有沙箱，默认仍为 Confirm；不自动升级已有线程或默认设置。复用 TEXT 列，无需 DDL；未知模式继续 fail closed。
2. I58-R2：只有通过现有 runtime 权限门禁的 Execute + FullAccess bash 调用可直接启动 `/bin/zsh -lc`。策略来自本轮冻结的线程权限，不能从 provider 的工具 JSON 或命令内容推导。普通批准使用可区分的 FullAccess 审计来源，沿用既有审计结构并补齐 parser/serialization/recovery 的闭合集合。
3. I58-R3：Full access 不构建/自测 Seatbelt profile，不执行为该沙箱服务的项目 hardlink 扫描，也不依赖 sandbox-exec 是否可用。命令以当前用户权限运行，可写项目外与 `.git`；不会获得 root 或绕过系统自身权限。
4. I58-R4：Auto 和 Confirm（包括用户批准 once/always 的命令）继续走 Seatbelt 和 fail-closed 检查。ReadOnly、Ask、Plan 继续拒绝 bash。FullAccess 仍保留危险命令确认、审计、取消、超时、输出上限、scope binding、私有临时目录与安全清理。
5. I58-R5：只调整 bash 的 OS 启动策略。read/glob/grep/write/edit 的既有路径与 checkpoint 契约不在本次变更范围；Full access 可通过 bash 完成项目外操作。提供给模型的 bash 描述不得再无条件声称总在沙箱内。
6. I58-R6：UI 的模式选择和持久化必须到达真实生产 dispatcher；线程间不得共享或泄漏 Full access。切回 Confirm 后下一轮恢复沙箱。
7. I58-R7：Composer 权限 picker、加号菜单和 Settings 默认权限选项完整支持第四项「完全访问」。FullAccess 使用既有 Warning 图标和 warning 颜色；Auto 的说明改为「自动批准，使用沙箱」，FullAccess 说明为「不使用沙箱，危险操作仍需确认」。复用既有菜单尺寸/键盘路径与持久化 controller，不新增模态确认步骤。历史规格中的三项集合与 Auto 不受限描述由本条取代。

## 实现任务卡与验收

- 范围：`vega_tools` 的 bash 启动边界、`vega_runtime` 的策略接线/工具描述、conversation/store 的模式及审计 codec、UI 选择入口，以及必要的 production-entry 回归。主 agent 负责规格、审查和交付；专用执行 agent 负责代码实现。
- 保留默认沙箱 API 或等价默认行为，显式传递无沙箱策略，复用进程/输出生命周期，不复制整套 runner。不引入新依赖，不修改危险规则。
- E2E-REAL：owned temporary project + sibling directory，真实 bash 在 Full access 写 sibling 标记、在自建仓库执行 git add/commit；Confirm 获准后同类操作受沙箱阻止，普通项目内写入仍成功。禁止对用户仓库或用户文件做这些实验。
- 生产接线：从持久化 FullAccess 的 conversation/controller 入口发起 MockProvider bash 调用，断言真实文件/Git 结果和审计终态；切回 Auto/Confirm 后外部写入失败。MockProvider 只替代网络，不能替换 shell/dispatcher。生产 UI 测试证明第四项可鼠标/键盘选择、通过真实 controller 落库，重开后保持正确模式。
- 既有 Ask/Plan/ReadOnly、危险命令、cancel/timeout、scope mismatch 回归继续通过。Full access 的 cancel/timeout 必须有真实进程收拢证据。
- 第四项使原 R64 三项 picker 的几何基线需要更新：宽 350、左界 445、下界 1014.5 保持不变；高度从 188 增加一行 48.5 得 236.5，上界相应为 1014.5 − 236.5 = 778。测试须保留这些锚点约束，不能简单删除尺寸断言。
- 门禁：`cargo fmt --all -- --check`、`scripts/cargo-lock.sh clippy --all-targets -- -D warnings`、`scripts/cargo-lock.sh test --workspace`、`scripts/cargo-lock.sh build --workspace`。
- 交付报告区分真实 shell/生产链路测试与真实 provider/UI 验证；记录命令、结果、残余。本地合并后更新 Issue/Project 的实际交付状态。

## 变更记录

- 2026-09-18：按 #58 和后续用户裁决新增独立 FullAccess，Auto 保持原语义；保留危险确认等独立权限约束。
