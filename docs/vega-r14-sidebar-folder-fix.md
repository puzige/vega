# R14-S — 侧栏恢复与打开文件夹的默认分组

Type: User-directed fix
Status: Delivered locally — [root acceptance](vega-r14-acceptance.md)
Executor: Astra medium
2026-09-06 用户补充：「左边的侧栏收起以后没有按钮再打开了」「理论上是打开一个文件夹默认是开启一个新的分组」。本卡优先插入 R14，由主 Agent 单独审查和原生验收。

主 Agent 在 native-r13 的实际窗口复现了已隐藏侧栏只有后退/前进、没有可见展开按钮；Cmd+B可恢复。展开后当前是R13自定义分组视图。代码 `ProjectsBlock::register_path` 成功后只选中项目，未切换组织视图/展开项目；用户看到的文件夹分组缺失有对应机制。

## 必须实现

1. 工作区顶部始终保留侧栏显隐按钮，隐藏时也可见。与Cmd+B使用同一状态/持久化路径，tooltip明确「显示侧栏」/「隐藏侧栏」及快捷键；鼠标、Tab/Enter/Space可用。包括无任务、已有任务、右栏开启、设置返回、窄宽窗口和浅深色。按钮不能遮挡macOS标题区/后退前进，也不能抢走输入区草稿或触发后方动作。保留原侧栏收起入口或以统一标题入口替换，避免两套不一致逻辑。
2. 「打开文件夹」成功后，以该文件夹的真实项目行作为默认文件夹分组，自动进入项目视图、展开该项目、展开相关分区并显示侧栏，让用户直接看到新文件夹的标题和新建任务入口。不另建一个与项目重复、可漂移的同名自定义分组。空文件夹组也要显示。原自定义分组保留，用户仍可手动切回；打开文件夹覆盖当前视图选择符合本次明确指令。
3. 新任务创建继续用当前打开文件夹的真实 project_id，立即显示在该文件夹组下；从另一个文件夹切换时不能悄悄归入旧项目/自定义组。打开已有文件夹选中并展开原组，不重复插入；常规绝对路径重复打开必须去重，规范化路径别名可以用canonicalize后去重（不迁移旧路径、不破坏原记录）。取消picker/注册失败不产生空分组，不切换原选择/导航，不丢草稿。原来的任务/分组/消息/费用不得重写或丢失。
4. 用现有组织service与共享types实现“显露项目”操作；如果需要一次设置项目视图+清除相关折叠，使用单一事务动作，以确切project_id为参数并校验存在，保留其他排序/组/元数据。后台组织写入遵守当前revision、mutation fence、generation/owner，不用旧快照覆盖新偏好。UI注册后收到项目确认再刷新/展示；失败有可重试提示，不能假显示持久成功。新增IO不放render/上屏关键路径。沿用既有project注册入口，可以窄重构到后台以满足此链路。
5. 使用现有theme/icon/layout tokens；不改变主题或字体以规避布局问题。无新依赖/迁移，不碰模型设置/网络/Keychain/成本UI。

## 归属与验收

专属 sibling `vega-r14-sidebar-fix` / `codex/r14-sidebar-fix`。实现前读AGENTS.md与docs/vega-exec-guide.md并fetch/rebase。拥有 UI sidebar / navigation header、必要app主布局、conversation sidebar_organization 的新项目显露动作与types、相关真实回归。R14-D只拥有provider/runtime/catalog，R14-U拥有Settings及其app接线，若与window/render同文件先协调，只改各自明确区块。主Agent不实现功能，只写规格/独立验证。

生产入口E2E：真实root/Sidebar挂载，通过实际指针点击折叠/恢复与键盘；owned目录经生产注册入口 → owned文件DB验证新组/空组/旧组去重/两文件夹中新任务归属、从Groups/Timeline/折叠状态打开后切回Projects并显露、picker取消/失败和异步晚到守卫。无需无意义的布局常量镜像测试。主Agent再使用原生目录picker和实际窗口验收所有主要路径，重启核对偏好、原数据保留。960×600与1280×750浅深色检查标题栏可操作。性能bench/soak延后。

≤3实现提交，交付 `docs/vega-r14-sidebar-delivery.md`，提供精确head/原始测试日志/范围和未覆盖边界。最终workspace统一门禁由主Agent运行，首次失败保留，禁止删断言或重复求绿。
