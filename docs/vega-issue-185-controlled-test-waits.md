## 用户要求与基线
用户在nextest耗时分析后要求继续优化一轮。基线为PR #184云端run36074411687：1787通过/5跳过，测试47.647s。

优先范围：当前分支/侧栏刷新测试的真实2秒刷新间隔；7个issue67并发场景mock provider的600/1200ms人为延迟。保留全部业务、安全、取消、过期结果拒绝及UI断言，用可控时钟或事件握手表达原时序。

不调整生产刷新频率，不删除测试/增加ignore/弱化断言，不改CI拓扑和nextest配置。本轮不优化万行解析和目录规模测试。

验收：受影响目标用例通过且真实等待减少；云端完整nextest通过；同名测试逐项对比基线，整体耗时仅作为单次观测，不承诺固定提速。

实现由专用agent在独立worktree进行，先写spec，本地只跑目标测试，PR云端通过后squash merge并进入In review。

## 设计边界与验收矩阵

| 用例 | 基线测试时间 | 优化后必须保留 |
|---|---:|---|
| r14_current_head_loads_without_selector_click_and_refreshes_hidden_sidebar | 5.171s | 自动加载、HEAD外部变更、隐藏侧栏、linked/detached/non-Git、切换后正确owner |
| production_sidebar_refreshes_head_metadata_and_rejects_removed_results | 2.793s | HEAD更新、选择/重载、invalid HEAD、删除后拒绝旧结果 |
| issue67_concurrent_* 7例 | 合计11.663s | 并发进入、各自停止、窗口关闭、preprovider失败重试、设置下完成、两端权限拒绝 |

优先复用现有GPUI虚拟时钟；若生产使用std::time::Instant导致不可控，允许最小内部时钟注入，默认路径仍使用单调真实时间，保持2秒刷新与1秒超时语义。不得引入全局可变时钟或扩展公开产品API。异步worker通过事件/屏障确定进入和释放，取消仍必须能打断等待；每个原有前置状态与结果断言保留。测试辅助同步须有有界失败，不以短sleep猜时序。

实施步骤：读取现有慢测试和生产计时路径→专用agent实现最小注入/握手→运行受影响定向nextest并记录原始输出→主agent审核diff与对照断言→PR云端完整gate→主动合并并等待手测。数据迁移不适用，生产默认行为不变；回滚通过revert本卡提交。

不执行本地workspace全量测试，不增加依赖，不引入Rust注释（unsafe SAFETY例外）。

## 实现审查决策

复用GPUI `BackgroundExecutor::now()`，正常平台仍提供真实单调时间；不调整2秒刷新/1秒超时。保留head先消费完成再判超时、sidebar先判超时再消费完成的原顺序。

navigation自动刷新回归必须由既有100ms生产timer触发。测试仅通过test-support只读查询worker是否仍在执行，在worker等待时冻结虚拟时间，其他阶段推进时钟；禁止用测试直接调用poll替代定时器接线。为跨crate查询完成状态，`vega_ui/test-support`显式转发conversation/test-support，默认发布图不受影响。sidebar测试保留既有poll路径，补1999ms未刷新/再1ms到期断言；旧结果拒绝使用真实worker完成值在移除项目后进入既有消费路径，替代50ms猜时序。

并发场景仅增加测试内Provider包装，在首个delta后发送entered信号并等待release或取消。两端进入、部分文本持久化后才继续操作；完成场景先释放A确认B仍活跃，再释放B；停止/关闭场景保持gate关闭验证取消可收敛。原MockProvider请求记录及业务断言保持。

依赖例外（主agent审查批准）：测试内Provider包装需要Stream组合器，允许在vega的dev-dependencies中引用已存在且锁定的workspace `futures`。不引入新包/版本或生产依赖，避免为测试扩大runtime接口。此明确批准更新上文“无新增依赖”为“无新增包或生产依赖”。

## 本地定向证据

9个目标nextest测试通过，测试阶段0.894s；已启动的重复运行9/9通过0.843s。首轮单例：head0.276s、sidebar0.126s、7个并发场景0.744–0.893s。邻接current_head/sidebar/navigation回归14/14通过，测试阶段2.082s。范围有重叠，不合计为独立测试数；filter跳过其他测试不等于新增ignore。测试使用本工作树独立默认target。

首次编译发现BackgroundExecutor与CancellationToken导入遗漏及vega缺少直接futures测试依赖；补齐导入并按上文批准增加现有workspace dev依赖后通过。无运行时断言失败、无删除或忽略用例。本地时间与云端基线来自不同机器，不作为正式提速比较。

精确命令（均退出0）：

```sh
cargo nextest run -p vega -p vega_ui -E 'test(issue67_concurrent_) | test(r14_current_head_loads_without_selector_click_and_refreshes_hidden_sidebar) | test(production_sidebar_refreshes_head_metadata_and_rejects_removed_results)'
cargo nextest run -p vega -p vega_ui -E 'test(branch_selector::current_head::tests::) | test(sidebar::projects_block::tests::) | test(window::navigation::tests::)'
cargo clippy -p vega -p vega_ui -p vega_conversation --all-targets -- -D warnings
cargo check -p vega -p vega_ui -p vega_conversation
cargo fmt --all -- --check
git diff --check
```

原始测试footer：

```text
Summary [0.894s] 9 tests run: 9 passed, 649 skipped
Summary [2.082s] 14 tests run: 14 passed, 644 skipped
```

受影响包Clippy通过10.83s，默认正常构建检查通过11.24s；正常依赖图没有新增vega_ui/conversation test-support。原有第三方block0.1.6 future-compat提示未由本卡引入。完整云端结果、PR及合并身份以Issue交付记录为准。
