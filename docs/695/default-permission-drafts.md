# 新对话默认权限与草稿权限选择

## 目标与替代边界

用户反馈：默认 Full access 的新对话仍需重新选择才能发送；选择权限会提前生成 Untitled task。本规格补充并取代 `../624/direct-permission-selection.md` 中草稿选择 Full access 立即持久化的时机，保留显式选择即授权、默认授权派生及所有执行安全检查。

## 行为契约

1. 新对话从已保存的系统默认设置取得权限快照。设置保存刷新时，仍继承默认设置且尚未开始首次发送/创建的草稿同步新的权限模式、revision 和默认授权快照；手动选择过权限的草稿不被覆盖。模型默认变化继续独立同步。
2. 草稿选择任意权限，包括 Full access，只更新内存草稿与当前会话投影，不创建 threads 行，不新增侧栏任务。Full access 的显式选择生成绑定当前数据库 authority epoch 与草稿 thread ID 的授权；不更改系统默认设置。
3. 首次发送才物化草稿。继承默认权限继续核对配置身份、默认 revision、授权内容、FutureThreads scope 与数据库 epoch，派生 thread grant；显式选择继续核对 thread scope、epoch、reset 状态。不得仅凭 Full access 字符串绕过授权。
4. 多次切换权限、重新加载、首次发送保持相同草稿 ID、内容、图片和项目绑定；已持久化对话继续使用 revision CAS，不改生命周期。
5. reset、旧 epoch、配置快照冲突、数据库身份变化仍失败关闭。没有有效默认授权的历史配置不自动伪造授权；用户须在设置页明确保存 Full access。不修改用户配置或历史空任务。

## 实现与验证范围

- 默认刷新：window/session；草稿选择投影：window/thread_permissions；授权服务：conversation/thread_permissions；首次物化：conversation/permission_contracts。
- 不增加 UI 文案、弹窗、依赖、schema 或运行时执行特权。
- 本地只做格式、差异和静态安全路径检查，不运行 Vega、构建或测试。最终 head 经非实现者独立审查和现有 required Actions；不新增临时 Rust 测试，CI 空集不称功能测试通过。
- 用户手测：设置默认 Full access 后在现有空草稿直接发送；连续创建新对话直接发送；未发送切换 Full access/Auto/ReadOnly/Confirm 不新增侧栏项；附图片后切换仍保留附件；首次发送只创建一个任务；已存在会话修改权限正常；reset 后旧草稿不保留可执行授权。

## 已定位缺陷

- 配置刷新仅在默认模型变化时同步草稿模型，没有同步仍继承默认设置的权限快照；旧草稿与已保存默认设置不一致时，首次物化拒绝。
- window 的草稿分支排除 Full access，服务的 draft save 又直接物化，因此显式选择提前创建任务。
