# GPUI Kit Markdown Demo

## 范围

用户授权先做简单 demo。独立 example 使用 gpui-kit 0.6.1 的 component::text::TextView 默认 Markdown 样式，不覆盖圆点、字号、颜色、缩进，不接入会话、不访问模型或用户数据。不代表完成整体迁移。

## 运行

云端构建命令：`cargo build -p vega_ui --example markdown_demo`。在具备图形环境的 macOS 上运行产物，查看默认效果。本地 Agent 不构建或运行。

## 验收

固定示例展示中文普通列表、强调、行内代码、链接、长文本换行、嵌套、有序列表、任务列表、引用、表格及代码块。窗口可调整宽度，正文可选择复制。原会话保持不变。实际外观与交互由用户查看，静态检查及 CI 不代表视觉验收。
