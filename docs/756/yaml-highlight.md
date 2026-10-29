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

## 开发验证与审查证据

- 开发提交：`091ca9d456dc5d36a675b922a37ebbeda88ce8ce`；[Actions 40848147142](https://github.com/puzige/vega/actions/runs/40848147142) 成功，fmt/Clippy、Python 三组（5+15+40）成功，Nextest 3 发现/3 执行/3 通过/0 跳过。既有语言用例只证明加载与非空高亮，不冒充全量语义覆盖。
- [原始开发测试源码](https://github.com/puzige/vega/blob/091ca9d456dc5d36a675b922a37ebbeda88ce8ce/crates/vega_markdown/tests/yaml_highlight.rs) 已通过 GitHub API 核验（blob `c6b137e5292b14e316e176fbed1b727a2d5b6e5f`），随后删除，未引入独占支持/fixture/dev dependency。
- 非实现者独立只读审查绑定该开发提交：无发现，核对 YAML query 优先级、别名与标题、capture 映射、唯一 core、pending 与降级契约；未运行代码或验证像素。最终增量只删开发测试及补充本节，生产代码不变。
- 本地 `cargo fmt --all`、`git diff --check` 通过，未本地运行/构建/测试。最终 head required CI 仍待运行；用户手测未进行，规格偏离无。
