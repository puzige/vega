# R14-B — 打开文件夹自动显示当前分支

Type: User-directed integration fix
Status: Delivered locally — [root acceptance](vega-r14-acceptance.md)
2026-09-06 用户指出：打开文件夹应默认加载当前分支，不应要求「选择分支」。主Agent已在原生R14中看到侧栏正确显示main，而输入区仍显示「选择分支」。源码BranchSelector的current_label默认None，只在打开selector请求列表后才更新，是展示/初始化缺口。

1. 打开文件夹/创建任务/恢复该项目任务时，输入区自动显示该真实目录当前Git分支。无需点按钮、选择分支或触发checkout。分支名本身仍可点击进入现有可选切换，不自动选main/master、不改用户Git状态。
2. 复用已有生产只读Git服务或R12实时项目分支projection。后台IO；render只读cache。普通仓库、linked worktree显示各自真实HEAD；非Git目录不出现误导选择按钮；detached/未诞生/读取错误使用已有真实状态词或明确未就绪文案。不要从任务缓存或分支清单第一行猜current。加载中可以显示「读取分支…」，不能长期停留在需用户主动选择的占位。
3. 项目/任务切换时旧异步结果须校验project/path/generation，不能显示上个目录的分支。当前工作区重新激活/现有刷新周期以及实际切换分支成功后更新，侧栏隐藏仍能显示当前任务的真实branch。保留原branch selector安全快照、切换许可、dirty/终端/运行保护及用户草稿/焦点，不扩大写Git权限、不改runner超时。
4. 空页面项目分组与输入区的分支名字保持一致；不新增第二套强制选择步骤，不调整无关外观/性能/模型逻辑。

专用executor Astra medium；在既有S sibling上从当前已集成基线后续提交，root最后cherry-pick。读AGENTS.md与exec-guide，先fetch/rebase，代码必须晚于本spec提交。新增/调整最小branchselector、项目branchprojection与app接线；不得直接编辑integration工作树。根窗口生产E2E：真实owned normal repo/worktree、打开任务不点branchselector即可显示actualHEAD，切项目/外部切分支/隐藏侧栏刷新、非Git正确状态；未执行checkout证据从真实HEAD验证。只允许少量补充竞态seam；不调用真实provider。

主Agent正在旧联合head1d17b90执行首轮门禁，保留其完整结果；本补丁集成后再针对新增改动运行必要检查和最终门禁。验收日志与边界附docs/vega-r14-sidebar-delivery.md，不另建用户任务。本卡属于用户本轮明确追加要求，允许对应实施提交，不受前卡提交上限阻挡。
