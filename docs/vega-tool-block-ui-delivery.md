# 工具块 UI 交付 · #381

契约：`docs/vega-tool-block-codex-parity.md`。2026-10-15。

## 变更

- `vega_ui/src/tool_card.rs`：紧凑行显示安全投影中的实际命令与读取文件名；搜索保持动作类别，不展示原始模式/目录；沿用单行省略、图标槽与页面同底。失败、拒绝、取消仍有明确状态。完整命令保留原始空白，输出按需展开。
- 默认命令行不再重复 exit/耗时/截断/复用元信息；详情底部保留真实终态信息，运行时原独立时钟转在详情底部显示。运行总计时设计与数据来源未修改。
- `conversation_stream/run_activity.rs`：阶段类别改为中文活动描述，单类“运行命令”、混合类“读取文件、运行命令…”；原工具子组已有状态加动作的摘要继续保留。原思考正文/标题不改写。
- `vega_conversation/src/types/tool_calls.rs`：ReadOnly 增加可选 display_target，仅读取已有安全 ToolCall 的字符串 file_path 并提取 basename；Glob/Grep 的 pattern/path 不进入展示投影；权限路径独立不变。历史仍从安全记录重建投影，旧/缺失对象回退类别文案；无存储迁移。
- 回归新增对象投影与可达状态矩阵；迁移旧泛称/紧凑时间断言到新展示契约。GPUI 回归仍检查真实 0/1/2/65 秒、独立工具 5/3 秒与终态耗时，改为展开详情断言；保留图标颜色、焦点/折叠、历史恢复与布局边界覆盖。相关 fixture 仅补可选字段。

## 本地验证

先添加对象投影/工具行回归再实现；按用户限制未执行红/绿测试或任何编译。

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check`（首次） | exit 1：新增代码需格式化，未编译 |
| `cargo fmt --all` | exit 0 |
| `cargo fmt --all -- --check`（格式化后） | exit 0，无输出 |
| `git diff --check` | exit 0，无输出 |

## 待验证

CI 执行仓库 pr-check 的 fmt/Clippy/workspace nextest，尤其 tool_card、issue70/issue381、issue296 阶段与类型投影回归；本地未运行，不标记通过。无新依赖、全局主题数值、存储迁移或应用重启。

用户手测：浅深主题与窄窗口中核对长命令省略；Enter/Space 展开、完整命令/输出、失败原因及底部耗时；历史重新打开后对象和阅读位置；最终运行时间与收起行为。

残余：截图对齐依据可见布局，不声称原生像素验收；目录列举仍使用实际 Shell 命令（无独立目录工具投影）；缺少历史对象字段时保留真实类别回退，不推断对象。Glob/Grep 原始查询和目录受既有安全展示边界保护，继续使用动作类别，未实现参考图中的搜索对象展示。

## 首次 CI 反馈

CI run `38671407198` 的 Clippy 首次失败：`issue70_tool_activity.rs` 的长命令布局回归向 `body_text_width` 传入 `String` 后再次 `.into()`，触发 `clippy::useless_conversion`。已移除冗余转换，不改变断言。修复后本地仅运行格式与差异检查，均 exit 0；未编译，待后续 CI 验证。此次修复时该 run 的 Nextest 仍在执行，未取消或标记通过。


首次 run `38671407198` Nextest 完成：2680 passed、2 failed、5 skipped。`issue70_t70_5_lifecycle_updates_keep_entities_and_truthful_failure` 仍在折叠行寻找 exit 1；迁移为折叠行明确断言“运行失败 false”，再展开断言原 exit/耗时，保留危险色和实体/行数覆盖。`issue70_t70_7_redaction` 检出本轮新增 Glob/Grep 对象泄露原始 pattern/path；撤回这两种展示投影，保留原安全断言，并新增投影中 query/path 缺失断言。Read 安全 basename 仍保留。修复后仅运行 fmt 与 diff 检查，均 exit 0；修复效果待后续 CI，未本地编译。
