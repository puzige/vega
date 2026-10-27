# #624 交付验证

完全访问、受限模式和单次扩围已统一到版本化权限契约；线程、未来默认和自动化独立确认。reset 持有原数据库暂停令牌，完成撤权、回执核对及当前/缓存界面刷新后才释放。未知清理结果继续阻塞，不通过改历史状态制造完成。

## 开发行为验证

完整开发版本：[`78ae9db5d101c9645f3ed9e8c2f852752d6a4ebe`](https://github.com/puzige/vega/tree/78ae9db5d101c9645f3ed9e8c2f852752d6a4ebe)，源码 Git tree `1d27c4103e7933ddaec4572695841eb968ed2756`。

[Actions 40479328746](https://github.com/puzige/vega/actions/runs/40479328746) 的 Clippy、Nextest 和 required check 全部成功。Rust 127 发现、127 执行、127 通过、0 失败、0 跳过，覆盖完整权限实现及审查修正；Python 三组分别 5、15、40 项通过。先前批次与失败记录见 [权限 UI 收尾](permission-ui-completion.md) 及本目录各规格，不用修正前 123/123 代替本次结果。

原始 127 项 Rust 测试保存在该开发提交历史中，不进入最终 master 文件树。31 个测试源码文件的 SHA-256、Git blob、固定 GitHub 源码链接和精确 run/job/结果记录于 [源码清单](development-validation-sources.json)，已逐个比对远端 blob 与本地内容。完整开发提交同时保留独占 fixture/helper 与当时接线。PR 历史保留此提交；清理/合并后再次核对 PR head 对其可达性，不只依赖本地副本或临时 Actions artifact。

## 独立审查

两位非实现写入者在 `8f6794da8608345740749a89716f052ef987bdce` 对本 PR 完整差异分别核对后端契约、应用/UI；发现 P1 旧队列/已批准计划可能继承后来权限，及 P2 长自动化确认指令溢出。两项均已修正，并在 `f78b75f225be6d6dcc52aa064f70b0d0d07dbcab` 获原审查者增量确认。核心审查者继续确认 `1ca814444233ea96eba0341ccbddca87a5c535a9` 结构化引用集成及 `2f60e13f6a3673ef494e19c243f0747f93b423e2` 借用修正；UI 审查者确认最终开发 head 的公开像素转换和恢复草稿无授权增量。原 P1/P2 已解决，无未决可行动发现；未改范围复用原结论。审查均只读，不代替云端结果或真实平台手测。

## 最终树清理与门禁

删除 127 个测试定义及其 31 个独占源码文件，移除所有对应模块接线、启动/config fixture、GPUI test-support dev dependency 和只为关闭服务的构造入口。正常生产构造仍启动原服务。保留生产权限验证/安全断言、benchmark、Python 测试/fixture、迁移和完整 CI；67 个受保护文件的原始内容及清理后逐字相同，见 [清理清点](test-cleanup-audit.json)。静态清点为 0 Rust 测试定义、0 独占测试接线，无可执行 Rust doctest；仅保留既有 Markdown 流模块的 text 示例。

本地仅执行固定工具链 rustfmt/格式检查、差异、TOML/参数与源码/哈希静态检查，没有本地运行、编译、构建或测试 Vega。最终清理提交须由原有完整 required CI 确认 Rust 0 发现/0 执行；准确 head、run 和合并结果记录在 [PR #653](https://github.com/puzige/vega/pull/653)。该空集门禁只证明最终交付树符合仓库规则，不能冒充行为回归通过。

## 用户手测与状态

CI、独立审查和合并不等于原生平台/Brave、TCC、Seatbelt 拒绝强度、网络/Broker 或外部进程清理已获验证。历史不明清理继续阻塞 reset，不宣称可以安全降级。

用户在云端发布版本上手测：旧完全访问会话需重新确认，取消不授权；当前运行与下一轮设置分别生效；线程/默认/自动化确认各自独立；旧队列/批准在来源变化后不执行，原内容可取回重发；长指令可通过键盘完整滚动；reset 的取消、退出等待、阻塞定位、重试和完成后权限刷新符合实际结果。原生模式矩阵/浏览器链路按 [主规格](vega-permission-sandbox-alignment.md) 验收。

合并后 Issue #624 保持开放、看板 In review，等待用户明确手测通过后再收尾/Done；未收到用户结论不关闭。
