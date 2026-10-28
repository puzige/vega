# 产物引用恢复

## 根因与范围

基线 821182b1。成功 Write/Edit 的卡片仅通过 Git status 的变更条目查找 WorkspaceFileId。commit 后 clean 文件不在列表，reconcile 将 stale_disabled 永久置位且以后跳过；文件在磁盘上存在并不等于卡片仍有能力。另一路径为设置/导航关闭 route 后保留的会话卡片不再属于新 route。

用户样本文件有后续 commit 记录；这证明发生过 commit，不证明截图错误的精确时间或最初触发者。源码可确认 commit→clean→匹配失败链路，不将静态分析称为运行时复现。

## 验收用例

| 前置/操作 | 预期 | 验证层 |
| --- | --- | --- |
| 成功 Write/Edit 后预览与 Open in | 保留原有文本 allowlist、大小及安全围栏 | 静态审查 + 用户手测 |
| 新建或编辑产物 commit 后变为 clean，再点击 | 重新刷新，按原授权路径建立当前 generation 文件能力；预览/Open in 可用 | 静态审查 + 用户手测 |
| 自动 commit 与读取并发 | race 可失败但不永久禁用；后续点击重新验证，不自动重复外部打开 | 静态审查 + 用户手测 |
| 暂时缺失后恢复同一路径 | 缺失不打开，后续工作区 reconcile 可恢复；来源降级不可逆 | 静态审查 + 用户手测 |
| 打开/关闭设置，返回仍缓存的会话 | 复用其原 route，刷新后可继续操作 | 静态审查 + 用户手测 |
| 文件变成 symlink/硬链接、目录、越界路径/.git | 原有安全围栏拒绝预览与打开 | 静态审查 + 用户手测 |
| provenance 不确定、内容被替换 | 降级 workspace change，不恢复 agent provenance | 静态审查 |
| 旧 route/sequence completion 或取消 | 不覆盖当前卡片，不重复 external launch | 静态审查 |
| terminal reconcile 与交互 refresh 交叠、不同卡片 completion 延迟 | 所有权串行，延迟回调读取当前 metadata，不回退文件能力或 provenance | 独立静态审查 |

## 实现计划

1. GitWorkspaceService 内单独维护有界、generation-bound 的产物文件能力。仅由已验证卡片的原路径解析，clean 文件无需混入公开 Git diff 变更列表。能力注册前执行现有 fence，刷新/失效时清空。
2. reconcile 不再永久跳过暂时失效文件，缺失与 provenance 单向降级分离；rename 歧义保持拒绝。
3. 每次预览/Open in 的后台 worker 先 refresh/reconcile。产物 capture/reconcile/refresh 使用同一异步互斥所有权，refresh 与 reconcile 连续持有所有权，旧任务不能覆盖后续恢复。UI 完成回调读取服务当前 metadata，不套用延迟任务捕获的整组快照；terminal 返回的 captured 卡片也须替换为当前 projection。仍只做一次外部打开，不自动重放。
4. 设置期间保留缓存卡片的 route；返回缓存实体时复用并 reconcile，清理不再缓存且无 live owner 的 route，防止无界保留。

## 验证约束

本地仅差异、格式和静态检查，不运行 Vega/构建/测试。不新增需要合入 master 的 Rust 测试或真实外部进程测试门禁；required Actions 验证编译、Clippy、现存脚本回归及 no Rust tests（0 发现/0 执行），不将空集称为行为回归通过。提交版本须独立审查；用户手测未完成前不关闭 Issue。
