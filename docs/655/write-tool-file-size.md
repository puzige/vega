# 写入工具文件大小单位

## 范围与根因

Issue：<https://github.com/puzige/vega/issues/655>。基线：`e47afcba`。

`ToolCard::build_summary` 的 Write 输入和 WriteSuccess 结果使用原始数字，英中文模板固定追加 bytes / 字节。文件卡片已有按 1024 换算的 B、KB、MB、GB、TB、PB、EB 显示规则；本卡将该函数抽为 crate 内共享函数并用于写入摘要。

仅改变显示：不修改原始字节计数、工具协议、持久化、文件写入、权限或失败处理。不扩展到 Edit、Skill 或其他工具。

## 验收

| 场景 | 预期 | 验证 |
| --- | --- | --- |
| 0、1、1023 字节 | 0 B、1 B、1023 B | 静态审查；用户手测 |
| 1024、1536、4013 字节 | 1.0 KB、1.5 KB、3.9 KB | 静态审查；用户手测 |
| 1024²、1024³ 字节 | 1.0 MB、1.0 GB | 静态审查 |
| 更大计数直至 u64 最大值 | 继续换算至 EB，不越界 | 静态审查 |
| Write 运行中与成功、英文与中文 | 数值自带单位，无重复 bytes / 字节；动作与路径保留 | 独立审查；用户手测 |
| 写入失败/取消、Edit 摘要、文件卡片 | 既有行为不变 | 差异审查 |

沿用现有换算与一位小数规则，不改变舍入边界规则或引入新依赖。

## 实现与验证计划

1. 将 artifact_card 的 format_size 原样抽到私有 file_size 模块，提供 crate 内 format_file_size。
2. Write 输入与成功摘要调用共享函数；英中文既有模板移除固定单位，保留键和参数兼容性。
3. 本地仅格式及差异静态检查，不运行 Vega、构建或测试。此为局部显示复用，不新增 Rust 测试；最终 required Actions 执行 fmt、Clippy、Python 回归及 Rust 测试清点。
4. 非实现者独立审查与最新 head required CI 成功后合并；Issue 保持开放，等待用户手测。回滚可还原本卡差异。
