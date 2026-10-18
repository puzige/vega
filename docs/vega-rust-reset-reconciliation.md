# Rust 清零结果的合并纠偏

## 背景与范围

用户发现清零后又出现 1700 多个 Rust 测试，要求补充 PR。初始基线为 `db1116439750fe1c262cfdceadb8b33959402761`；清零结果参考 `9ac5b79b5762f8b6b57e470132034d45ef5a328b`，清零前参考 `a608ffa2f1f3d82d60cc2a83a2ed0fe17133d552`。

- #473 合并时 Rust 测试定义为 0，已删除 1,614 个历史定义及独占支持。
- #472 合并结果恢复了被删除的全部 147 个文件，测试定义变为 1,653；#471 后变为 1,729。
- 这是合并恢复历史测试的纠偏，不是再次无限期清零或禁止后续有效测试，也不是两个功能的回滚。

## 冻结的验收

1. 删除已在 #473 清除、又从旧分支恢复的历史 Rust 测试定义。按清零前源码的定义路径、模块、函数和内容核对，不仅凭文件名或总数删除。
2. 保留输入预测与外部 grep 的生产功能，以及两个任务真正新增的测试数据和断言。新增与历史测试混合的文件按语法边界处理；新测试依赖的共享 fixture、mock、模块和依赖只保留必要部分，不能为了维持历史大集合原样恢复全 workspace 支持。
3. 不改生产协议、安全决策、控件 ID、无障碍、焦点/事件/滚动/绘制逻辑，不降低断言或 lint。保留生产自检/连接检查、Python 测试与 fixture、benchmark、发布与打包能力。
4. 校验最终历史测试残留、新增测试数量、模块/文件引用和生产差异。依赖变更只能删除已无调用者的测试接线；不升级依赖。
5. 补齐各规则入口的合并边界：保留当前有效测试不包括恢复 master 已删除的历史测试；必须先同步并审查与最新 master 的差异，禁止从旧 ref 整目录/整仓覆盖清零结果。需要新回归时按现行标准另开明确的先测后修周期，不能借“解决冲突”追认旧测试。

## 测试来源分类

以下为 `db111643` 的源码定义清单，不是 Nextest 发现或执行数量。按定义路径、模块、函数、内容和原提交差异交叉核对，目标为移除 1,614 个历史定义、保留 115 个真正新增定义；实现后的数量另行验证。

| Workspace 成员 | 基线测试定义 | 历史定义（移除） | 新增定义（保留） |
|---|---:|---:|---:|
| vega | 155 | 145 | 10 |
| vega_acp | 28 | 28 | 0 |
| vega_conversation | 588 | 555 | 33 |
| vega_credentials | 7 | 7 | 0 |
| vega_markdown | 32 | 32 | 0 |
| vega_mcp | 54 | 54 | 0 |
| vega_runtime | 223 | 202 | 21 |
| vega_store | 163 | 162 | 1 |
| vega_theme | 5 | 5 | 0 |
| vega_token | 26 | 26 | 0 |
| vega_tools | 126 | 97 | 29 |
| vega_ui | 286 | 265 | 21 |
| xtask | 36 | 36 | 0 |
| **合计** | **1,729** | **1,614** | **115** |

历史集合中有 1,610 个定义原样恢复，3 个同名用例改动了 fixture，另有 1 个 grep 用例改名并改写预期。最后一项仍来自历史定义：`rejects_invalid_regex_and_all_path_escape_shapes` 变为 `rejects_invalid_regex_and_missing_targets_but_accepts_explicit_external_aliases`，不因改名自动算新增。独立新增的 `absolute_project_paths_match_relative_paths_and_explicit_external_targets` 不在删除范围。

115 个新增定义为输入预测 39 个、外部 grep 76 个。保留它们不等于认证测试质量或追认 TDD：输入预测的[原规格](vega-issue-469-composer-predictions.md#8-新-master-集成边界)和[交付记录](vega-issue-469-composer-predictions-delivery.md)记录了测试与实现同批交付及先行证据缺口；外部 grep 的[原规格](vega-issue-426-external-grep.md)也明确记录首批测试与生产实现同提交、没有有效行为 RED，部分后续用例晚于修复。该缺口在本次不通过补测、重排提交或放宽断言消除。后续新行为仍必须遵守 TEST-FIRST。

## 证据与执行限制

- 本次仅维护测试集合与必要接线，不新增用例或人为制造 RED。
- 真正新增测试的 RED/GREEN、测试质量与 TDD 时序分开审查。已有绿色结果或原任务文档中的“保留全部测试”不能授权再次恢复历史集合；缺少有效先行证据明确写缺失，不补测或重排历史追认。
- 仅在独立 worktree 工作；主会话协调、审查、集成，代码修改委派专职子任务。
- 本地只做低 CPU 格式、差异和静态检查，不编译、不执行测试、不运行 Vega/Clippy；完整门禁交 GitHub Actions。
- PR 明确区分源码静态定义数与 Nextest 发现/执行/通过数量，保留首轮失败和修复证据。用户本次要求补充 PR；不自行合并或清理其他任务资源。

## 验证记录

### 源码与接线（静态）

在 `db111643` 基线上逐项复核，最终静态定义为 **115**，历史定义残留 **0**，预期新增缺失 **0**，意外新增和重复定义均为 **0**。保留的 115 个定义正文、数据、断言及测试属性与基线逐字节一致；没有通过改名、ignore 或降低预期减少集合。

| 保留测试的源码 | 定义数 |
|---|---:|
| `crates/vega/src/app_agent.rs` | 1 |
| `crates/vega/src/tests/composer_predictions.rs` | 9 |
| `crates/vega_conversation/src/agent/tests/external_grep.rs` | 16 |
| `crates/vega_conversation/src/composer_predictions.rs` | 13 |
| `crates/vega_conversation/src/types/tool_calls/external_grep_tests.rs` | 4 |
| `crates/vega_runtime/src/agent/tests/external_grep.rs` | 21 |
| `crates/vega_store/src/config.rs` | 1 |
| `crates/vega_tools/src/grep.rs` | 1 |
| `crates/vega_tools/src/grep/tests.rs` | 28 |
| `crates/vega_ui/src/conversation_stream/tests/composer_predictions.rs` | 12 |
| `crates/vega_ui/src/conversation_stream/tests/issue426_external_grep.rs` | 6 |
| `crates/vega_ui/src/settings/tests.rs` | 2 |
| `crates/vega_ui/src/text_input/tests.rs` | 1 |
| **合计** | **115** |

- 历史删除差异以已审查的 #473 结果为参照，混合文件保留后续功能差异。两个功能新增的 12 个 Rust 文件全部与基线逐字节一致。
- 保留实际调用者需要的 `MockProvider` / `ScriptStep`、HTTP 边界替身、GPUI dev-only feature、应用 fixture、只读执行同步屏障和卡片可见文本查询；删除无调用者的旧 fixture、专用模块、debug selector 与 dev dependency 接线。预测提示 selector 保留，不删除生产控件 ID、无障碍属性、事件和焦点行为。
- 源码共 385 个 Rust 文件；371 个显式外部模块引用检查未发现缺失文件，字面量 `include*` 引用未发现缺失。旧版语法解析器有 61 个文件存在语法覆盖限制，不能据此宣称完整语法、类型、借用或运行正确；与基线比较未发现新增解析错误，格式解析与完整编译仍分别以 rustfmt 和云端门禁为准。
- 63 个受保护文件的 SHA-256 均未变，涵盖 Python 脚本与 fixture、发布/PR/master workflows、Nextest 配置、迁移、资源与 benchmark。原云端检查结构、Python 命令、Nextest 版本、默认并发与 `retries=0` 不变。
- `Cargo.lock` 的 924 个包名/版本/来源/checksum 元组完全不变；只删除 11 条 workspace 测试依赖边，不新增依赖边或升级包。运行时生产所用的 `tempfile` 保留，GPUI `test-support` 仅在需要的 dev dependency 开启。

### 本地已执行

| 检查 | 结果 |
|---|---|
| 历史/新增来源清单、定义正文与属性哈希交叉复核 | 115 保留、1,614 移除；正文与属性无变化 |
| `cargo fmt --all -- --check` | 退出码 0 |
| `cargo metadata --no-deps --offline --locked --format-version 1` | 退出码 0；13 个 workspace 成员的 manifest/锁文件静态解析成功，不编译 |
| 模块/include 引用、受保护文件哈希、依赖锁元组检查 | 未发现缺失或受保护文件变更；无包升级 |
| `git diff --check` | 退出码 0 |
| YAML、skill frontmatter 与新增文档链接目标 | 3 个 workflow、2 个 skill 和 8 个链接目标静态检查通过 |

本地未执行 Vega、构建、Clippy、Rust/Python 测试或打包。初次 frontmatter 检查被系统 Ruby 的 US-ASCII 默认编码阻断，显式使用 UTF-8 后解析通过；没有修改被校验文件来规避解析。

### 云端验证

待补充 PR 触发 GitHub Actions。必须取得本次 head 的 fmt、全目标 Clippy、Python 与 Nextest 原始结果及 required check，不能复用 #473 的绿色记录。此处 115 仅为源码定义数，Nextest 发现/执行/通过/失败/忽略/跳过数目前均未取得。
