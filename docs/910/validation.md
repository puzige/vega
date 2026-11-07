# Skill 展示验证记录

## 版本与范围

- 基线：`d4beacac2f01d4c09af7545163393e9d43753fe9`。
- 开发验证版本：`19d8ac1f9b3fabffea77704f2ae8a1de68320b6d`。
- PR：[#911](https://github.com/puzige/vega/pull/911)，对应 [#910](https://github.com/puzige/vega/issues/910)。
- 仅改变 UI 展示：过滤纯历史审计，移除独立审计行、占位接线与激活通知合成行；保留真实工具活动。
- runtime、conversation、Store、设置、权限和用户 Skill 文件未修改。

## 已执行

本地仅执行 `cargo fmt --all -- --check`、`git diff --check`，均成功；没有运行 Vega、编译、Clippy 或测试。

开发版 [Actions 42324472886](https://github.com/puzige/vega/actions/runs/42324472886) 绑定上述完整 SHA：

- 格式、全目标 Clippy、required check `check (fmt, clippy, test)` 成功。
- Rust 5 发现 / 5 执行 / 5 通过 / 0 跳过。
- 三组 Python 检查分别 5、15、40 项，全部通过。
- 回归通过真实 `PreparedHistoryEntries` 及 `ToolCard::hydrated`：审计独占历史无 UI 项；混合历史顺序与助手分段保持；真实成功、失败、拒绝、取消记录保留为工具行；资源读取保留；损坏结果保留失败状态。
- 未执行原生 App 像素、键盘或真实会话手测；实时事件不合成展示及底层不变为静态差异结论，不冒充动态集成验证。

## 开发源码留痕与清理

- 远端标签：`evidence/910-skill-presentation-tests-19d8ac1f`，直接指向开发验证完整 SHA。
- [原始源码](https://github.com/puzige/vega/blob/evidence/910-skill-presentation-tests-19d8ac1f/crates/vega_ui/src/conversation_stream/skill_presentation_development_tests.rs)。
- 266 行 / 8527 字节，SHA256：`fb959a1770ba4774e3cae70c712511fef3026dcabaebd64e25a3ad0b97da2aed`。
- 已用 GitHub contents API 独立读回原始字节核对哈希，标签远端 SHA 与开发版本匹配。
- 清理仅删除开发测试文件及 `mod.rs` 的两行独占接线；不引入依赖、feature、manifest 或 workflow 修改。
- 最终无 Rust 测试版本仍须完整 required CI；空集仅表示 0 发现 / 0 执行，不是行为测试通过。

## 独立审查与合并边界

用户最新要求暂不允许子 Agent，本轮未调用或续派任何子 Agent。独立审查尚未执行，主 Agent 静态核对不代替非实现者审查。PR 保持 draft、Issue 保持 OPEN / In progress；即使最终 CI 成功，也不在独立审查缺失时合并。恢复条件为允许非实现者对停写的精确最终版本进行独立审查，或提供符合仓库规则的外部审查结果。
