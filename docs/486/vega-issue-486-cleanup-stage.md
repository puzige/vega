# #486 独立 Rust 测试清理阶段

## 清理前证据与基线

主审先核实并公开固定[完整 42 项 GREEN 与源码来源](https://github.com/puzige/vega/pull/491#issuecomment-6261003738)，随后明确授权本次独立清理。

- 带测试提交：`8f53229bc0b7fb349657aabe0e01f2c4b595fbdd`。
- [Actions 39208603579](https://github.com/puzige/vega/actions/runs/39208603579)：[Nextest 117916766750](https://github.com/puzige/vega/actions/runs/39208603579/job/117916766750) 发现、执行、通过均为 **42**，失败/跳过均为 **0**，耗时 **7.450s**；22 项服务测试、20 项 UI/model 测试的实际名称与冻结清单一致。
- 全目标 [Clippy 117916766511](https://github.com/puzige/vega/actions/runs/39208603579/job/117916766511)、fmt、Python 34 项及 [required check 117917764955](https://github.com/puzige/vega/actions/runs/39208603579/job/117917764955) 均成功。
- 清理前 fetch/update 后分支干净，与远端任务分支为 `0/0`；远端 master 仍为 `84baff54f98ffd0888858b447677b637be86c8cf`，与带测试提交为 `0/10`，未出现新的基线变化。此前基线通过普通 merge 集成，未改写 RED/GREEN 历史。
- 原始先测提交为 `5d46a1955d0212182f128e46dec20c67898dce55`；有效接口缺失 RED、原始源码与三次事先批准的 UI 接线/输入坐标修正，保持在[测试阶段](vega-issue-486-test-stage.md)、[实现阶段](vega-issue-486-implementation-stage.md)及 PR 证据中。本次未再修改任何断言或测试输入。

删除前重新从上述 GREEN 提交读取源码，核实以下 SHA-256。文件仍可通过该不可变提交恢复：

| 删除文件 | GREEN 源码 SHA-256 |
|---|---|
| `crates/vega_conversation/src/link_preview_tests.rs` | `cd688119fe429bc05cb592cfbf09164dc472b1b931c76d7dbfbf073efac1909b` |
| `crates/vega_conversation/src/link_preview_test_support.rs` | `b43ae6426450ca724611ef39021b7c70b24ee89fc2b3e56cd7089adaf8eee3be` |
| `crates/vega_ui/src/conversation_stream/link_preview_tests.rs` | `e58d73ee6c783a4a65e88e4f1720334b09d85114f07e068b982ba89d028f387d` |
| `crates/vega_ui/src/conversation_stream/link_preview_thinking_fixture.rs` | `6683f075a1fcfd3185958d60393abc19a9d5e60573a01725513344ba07080ca6` |

## 精确清理范围

删除上述四文件及 conversation `lib.rs`、UI `conversation_stream/mod.rs`、`thinking.rs` 中的独占模块接线。删除 conversation 的临时 `test-support` feature、dev image 边、服务端测试 re-export/factory，以及 UI 的三个 dev 边（GPUI test-support、conversation test-support、image）。

UI 预览模块仅删除测试 observation/install 模块、七处 `.test_support()` 标记、`LinkOpener`/`Rc` 与可选 opener 注入；`open_target` 保留并直接调用 `cx.open_url`。真实 ID、role、可访问名称/描述及其焦点代理消费者全部保留。

生产 ports、私有 `with_ports`、DTO、安全决策、缓存、取消、scope/target 替换抑制、焦点、事件、选择偏移、几何和点击身份围栏全部保留。未改 UI 产品行为、依赖版本、Python/fixture、workflow、Nextest 配置、迁移、资源、benchmark 或发布能力。

## 锁文件

`Cargo.lock` **924 → 914**。离线 `--no-deps` metadata 本身未删除孤立包，因此仅按已审清单精确移除以下十个测试独占身份：

`bit-set 0.8.0`、`bit-vec 0.8.0`、`convert_case 0.11.0`、`proptest 1.11.0`、`proptest-macro 0.5.0`、`quick-error 1.2.3`、`rand_xorshift 0.4.0`、`rusty-fork 0.3.1`、`unarray 0.1.4`、`wait-timeout 0.2.1`。

解析锁文件中依赖引用的真实身份后，保留包仅减少三条边：`gpui-pre 0.3.4 → backtrace 0.3.76`、`gpui-pre 0.3.4 → proptest 1.11.0`、`vega_ui 0.1.0 → image 0.25.10`。`backtrace`、生产 image 及其余生产依赖保留。消除同名旧版本后，`bit-set 0.9.1`、`bit-vec 0.9.1`、`convert_case 0.10.0`、`quick-error 2.0.1` 的九处引用缩为唯一包名，语义边不变。所有 **914** 个保留包的 name/version/source/checksum 与 GREEN 完全一致，新增包/语义边均为 **0**。

## 生产投影与本地静态结果

生产投影逐字节核对：conversation 服务仅去掉末尾测试模块；UI 模块恰为已批准的 observation/opener/七标记删除及直接打开调用；三个模块入口仅去掉对应测试声明。两个 Cargo 清单的正常 dependencies 表完全相同。其余本功能变更的 **18** 个生产 Rust 文件与 GREEN 逐字节一致，覆盖窗口注入、metadata、真实 IO ports、DTO、theme、autolinks、inline flow、links、model、render/rows、selection、thinking body、文件预览、图标、UI 导出、release notes。

关键受保护文件 SHA-256：

| 文件 | SHA-256 |
|---|---|
| `crates/vega_conversation/src/link_preview/ports.rs` | `958c0b7b1c1ae03a86d3e1e1da7a486d90e9f4314f2a3a942c97eddfcff5c387` |
| `crates/vega_conversation/src/link_preview/metadata.rs` | `5f858108da8b640df26d9d8ce5d59537fc369b57518ca51b6a9529911dda2a2e` |
| `crates/vega_conversation/src/types/link_preview.rs` | `8382e509a7402ade8769bd9aa1fcfd6c3d35677e9a26c441a8b06b0eee632748` |
| `crates/vega_ui/src/conversation_stream/links.rs` | `1a0a849b95d5d38fd73d9cc34d9162562f229f70b31357f75b7cf98fe7e3968b` |
| `crates/vega_ui/src/conversation_stream/selection.rs` | `e57e59b17307303b6a754a0841a598bc3ba8d3441a7d99fdf87fbb137067a2e7` |

本地仅执行快速静态检查：`cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --offline --locked --format-version 1 --no-deps` 均退出 **0**。锁文件身份/边审计及精确源码投影通过。主审另行执行完整 `cargo metadata --offline --locked --format-version 1`，退出 **0**，独立确认锁文件依赖图有效，未触发编译。

最终树保留 **376** 个 Rust 文件，静态清点测试/bench/property 属性、测试 cfg、test-support 引用、dev/test/test-feature 清单项均为 **0**；可执行 doctest 为 **0**，保留原有 `text` 文档围栏及 benchmark 的代码字符串数据。新增 Rust 注释为 **0**。该清点不是 Rust 类型、借用或执行验证。

执行方未在本地编译、运行 Vega、Clippy、测试、benchmark 或打包，未 commit/push。主审自己的 README/交付文档修改保持原样。

## 最终门禁

**PENDING**：此为独立清理待审工作树，删测后的提交及最终现有 PR gate 尚待主审执行。需要最新 head 的 fmt、全目标 Clippy、Python 及 Rust **0 发现/0 执行、`no Rust tests`** 和 required check 全部成功；不得把清理前 42 项 GREEN 复用为最终空集门禁结果。
