# YAML 代码块高亮

## 验收与范围

用户要求新增 YAML 高亮，`yaml` / `yml` 大小写不敏感且标题统一为 `YAML`，复用 #750 已交付的浅色/深色 syntax tokens。

- 使用 tree-sitter-yaml =0.7.2，其 LANGUAGE ABI 14 与既有 tree-sitter =0.26.13 兼容，唯一新增依赖用于本轮已授权的 YAML 语法。
- 接入 Grammar::Yaml、语言别名、LANGUAGE、HIGHLIGHTS_QUERY 及标题名称；现有 capture 表已覆盖上游 boolean/constant.builtin/string/number/comment/label/type/attribute/property/punctuation，无需增加映射或主题颜色。
- 普通/引号键名使用 Property；字符串与块字符串使用 String；整数/小数使用 Number；布尔/null 使用 Constant；注释使用 Comment。锚点与别名遵循现有 label → Variable 映射，标签使用 Type。
- 不修改 pending，仅 committed 高亮；未知语言与 Text 保持单色。原文、复制、选择、横向滚动、其余语言和主题色不变。不添加设置、自动检测或嵌入语言注入。

## 最小实现与验证

修改根 Cargo.toml、markdown Cargo.toml、Cargo.lock 和 highlight.rs。本地只做格式/差异/依赖静态检查，不运行 Vega、不编译、不执行测试。

在 Actions 用临时开发 Rust 测试验证两种别名/大小写与显示名称、块/流式映射键、Unicode 值、字符串/数字/布尔/null/注释/块字符串/锚点/标签、span UTF-8 边界/非重叠，以及既有语言与安全降级。测试通过后固定完整 SHA 和可恢复源码，删除开发测试；最终 head 经独立审查及 required CI，Rust 应为 no Rust tests（0 发现/0 执行），不是 Rust 测试通过。

用户手测：更新后检查 yaml/yml 标题、键名和值的色差、浅深色可读性；复制与拖选不受影响。合并不代表已安装或手测通过。
