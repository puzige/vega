# Issue #79：将提交错误显示在 Composer 上方

## 目标

截图标注的 Composer 上方区域用于显示任务提交与会话操作错误。当前 `controller_error` 在 Composer 卡片之后渲染，用户需要在输入区下方寻找错误；本卡将其移动到 Composer 卡片之前。

## 行为

- 所有既有 `controller_error` 文案紧邻显示在 Composer 卡片上方，沿用正文列宽度、红色语义和原有清除时机。
- 保留 utility bar、输入卡、运行状态及 MCP warning 的既有行为；不把普通 MCP warning 或文件索引下拉错误改成 controller error。
- 错误出现时不清空用户草稿，不改变提交、凭据读取、模型请求或重试逻辑。
- 错误消失后不保留专属空白占位。
- 覆盖 New Task 与会话视图，以及不同 controller error 来源；不增加依赖、持久化字段或公开 API。

## 验收

- GPUI 定向测试验证错误节点存在于 Composer 卡片上方、使用正文列宽且无空错误节点。
- 凭据失败和引用校验失败保留草稿、不创建用户回显，并在 Composer 上方显示原错误。
- 清除错误、MCP warning、运行状态和连续提交相关 UI 回归通过。
- 桌面端验证实际错误落在截图标注的 Composer 上方区域，保留草稿且重试入口可用。
