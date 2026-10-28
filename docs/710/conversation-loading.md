# #710 对话加载与 Logo shimmer

## 目标与边界

打开未复用的已有对话时，后台完成首次历史及恢复数据读取，内容区保持可响应的加载/失败/就绪状态。运行中、准备发送中和已有缓存流继续复用，不重新恢复历史。使用 `assets/logo/vega-symbol-smile-mono.svg`，保持原始图形、透明背景，不旋转、不变形。

当前证据：`window/render.rs` 在创建 ConversationStream 时同步读取 restart_history_page、plans、composer_history、recovery、usage、summary 和 pending_model；侧栏 open_thread 同步 visit_thread。尚无性能采样，不承诺消除所有渲染/布局卡顿。

## 实现契约

1. conversation 层提供一次后台初始数据读取，独立打开现有数据库，不新增数据库、不迁移 schema。保留 restart_history_page 的恢复及线程准入语义；pending_model 和其余恢复字段一起读取。
2. Window 持有路由身份、请求代次和 Loading/Ready/Failed 状态。仅无可复用流的持久对话发起加载，重复 render 不重复请求。后台不访问 GPUI Entity；完成时校验当前路由、代次和 settings 状态。离开、重试或销毁后旧结果不得覆盖当前状态。
3. 数据就绪前不构造可发送的空 ConversationStream，避免用户发送/恢复运行与历史填充竞争。成功后在原创建路径按既有顺序应用历史、计划、输入历史、summary、recovery、usage、pending model，再恢复草稿、队列和滚动锚点。活跃流优先于后台结果。
4. 对话内容区保持原背景，150ms 后才显示居中 Logo；成功立即替换内容，不设置最短等待。shimmer 1800ms 一周期，从左向右，亮带仅在现有 SVG 图形内。使用主题 text_tertiary/text_secondary 色，Reduce Motion 或非活动窗口静态显示；隐藏/完成后不请求动画帧。
5. SVG 使用 128px 画布，原图形约占画布一半，显示宽约 61px；沿用源几何，不裁切或重绘。动效和尺寸进入共享 token。
6. 失败停止动效，显示本地化错误及可键盘操作的重试按钮；不显示内部数据库错误，不无限旋转。加载容器提供本地化无障碍名称，不增加常驻解释文案。
7. 检查侧栏 visit_thread 的同步操作；后台化时只由最新选择发布路由，保留导航权限/草稿保护与访问记录语义。不得削弱任务变更和权限栅栏。

## 风险与验收矩阵

- 慢 A→快 B、A→B→A、重复选择、连续重试、转设置/新草稿：旧成功/失败不污染新路由。
- 快加载不闪 Logo、不人为延迟；空持久对话成功，新草稿绕过历史加载。
- 运行中流、权限与问题队列不重新绑定覆盖；恢复草稿/滚动/待发送队列及用量保持原语义。
- 浅色/深色、Reduce Motion、失焦、窄窗口、重试键盘入口；完成/失败后无持续动画。
- 大历史仍有 UI 元素创建/布局成本；本卡移出数据库读取，不将未测性能报告为已解决。

## 验证与交付

本地只做格式、差异与静态检查，不运行 Vega、构建或测试。required GitHub Actions 验证最终 head，独立非实现者审查状态所有权和受影响调用链；真实交互/性能及像素效果由用户在合并后手测。Rust 测试不进入 master，不为流程强造测试或恢复历史集合。Issue 保持开放，合并后 In review。
