# 代码块标题与语法配色

## 用户目标与本轮验收

参考用户提供的 Codex 截图：标题使用代码图标与规范语言名称，字号 13.5px、medium 字重、16px 图标；保留 24px 标题高度、复制按钮及代码正文布局。

用户追加确认：支持 JSON、Bash、HTML、CSS、SQL；浅色与深色主题分别配色；不做流式高亮。用户反馈 Rust 高亮过少，本轮必须同时改善既有语言，而非只增加语言数量。

- 语言别名大小写不敏感，展示规范名称；空语言与 text/plaintext 显示 Text；未知语言保持原标签且不猜测语法。
- Tree-sitter 保留现有 Rust、JavaScript、TypeScript、TSX、Python 支持；新增 json、bash/sh/shell、html、css、sql。
- 新增依赖限 tree-sitter-json 0.24.8、tree-sitter-bash 0.25.1、tree-sitter-html 0.23.2、tree-sitter-css 0.25.0、tree-sitter-sequel 0.3.11；用户授权新增五种语言，本轮依赖仅服务对应语法。上游导出已静态核对，ABI 为 14 或 15，沿用现有 core 0.26.13，无第二套运行时。
- 用独立语法色而非成功/警告状态色：关键字与运算符玫红、函数紫、类型与标签蓝、字符串绿、数字与常量棕橙、属性/变量/标点保留适当层级。普通标点与普通变量无需强行染色。
- 新增捕获覆盖 JSON 键、HTML 标签、SQL boolean/conditional/field/float/function.call/keyword.operator/parameter/storageclass/type.qualifier 等；不改变 UTF-8 span 边界、排序与覆盖优先级。
- Text、未知语言、解析失败仍安全退回单色；代码原文、复制、选择、横向滚动不变；pending 不调用高亮，committed 才高亮。
- HTML 仅本体标签/属性高亮，不新增 script/style 语言注入或 SQL 方言选择器。

## 方案与验证

修改 markdown 语法注册/显示名称与捕获映射、UI 标题、主题语法 token、UI token 样式。无新增运行设置或无关提示文案。

本地仅格式、差异、依赖与静态核对，不运行 Vega、不构建、不执行测试。通过独立审查及最终 head required Actions 验证编译/格式/lint/既有 Python 回归；最终 head 无 Rust 测试。新语法 query 接线与跨度行为风险通过临时开发 Rust 测试在 Actions 验证并留痕，合并前移除并重新验证最终 head。

用户手测：浅/深色下检查 Rust 示例的 fn/let、i32、main/map/sum、数字与字符串均可区分；检查五种新增语言关键语法，Text 不着色；标题与复制按钮在窄窗口不重叠；复制原文、拖选与横向滚动保持正常。CI 与合并不冒充像素验收或已安装。
