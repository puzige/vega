# Draft 默认权限来源的独审修复

## 原始发现与验收

独审基于开发提交 `32af3eb9d5eb19d25d878b495f2b330066a406ea`：DraftCreationGuard 最后一次 YAML 重读后，外部修改不推进 SQL configuration generation，旧默认权限仍可通过 materialize。此为权限来源缺口，不是测试夹具问题。

验收：锁外解析的默认权限必须与 draft 一致，并保存包围读取的文件身份/版本；排队后直接原地编辑、原子替换或删除配置均拒绝旧安装，线程及其相关事务变更不部分提交，intent 不标 committed。正常创建、service generation 撤销及默认值来源清除继续通过。使用真实 actor 命令与临时文件，不写用户库。

## 最小方案

Guard 携带配置原路径、canonical 路径及原路径/解析目标的 device、inode、length、mtime/ctime（含纳秒）元数据。读取前后比较该版本，actor 安装前后比较相同版本；配置正文读取/解析和配置锁仍在 actor 外，无 guard 随命令运输。保持 authority epoch、configuration generation、intent/source SQL CAS。

此方案沿用 Skill/credential 末检的文件身份版本边界，不声称可阻止提交后外部变化，也不声称 metadata 调用耗时有界。异常或来源变化保守拒绝，可重新准备；不盲目重试未知提交。最终版本需中央回归及非实现者增量审查。
