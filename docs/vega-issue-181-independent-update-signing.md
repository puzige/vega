# Issue #181：独立更新签名与真实安装位置

2026-09-25 用户针对 0.1.1 检测到 0.1.2 却只能手动安装，明确批准独立 Ed25519 更新签名与 /Applications 支持。本规格取代 #181 原来 Developer ID/固定 Documents 才能应用内更新的前置条件；其他有界下载、任务保护、显式重启和回滚不变。

## 冻结契约

1. Ed25519 验证使用已在锁文件中的 `ring = =0.17.14`（主架构批准新增 workspace direct dependency），不自写密码学。公钥为仓库 `assets/update-public-key.hex` 的32字节hex，编译进应用；不从网络、用户配置或待更新包获取可信公钥。主 agent 已生成专用密钥，私钥只保存在 owner-only 的本机发布配置目录，并作为 `VEGA_UPDATE_PRIVATE_KEY` GitHub Actions secret（base64 PKCS8 DER）注入签名步骤，不输出/提交私钥。公钥指纹 SHA256 为 2d7368ceca2084b120bb77c0107ada73b6f2ab2f9ae10084460ba7f7c49770d2。
2. 新增两个 release 资产：`Vega-update.json`（最多16KiB）及 `Vega-update.json.sig`（最多256字节，base64编码64字节 Ed25519 签名）。签名消息是字节前缀 `Vega update manifest v1\n` 加原始 manifest UTF8 bytes。manifest JSON仅包含：schema=1、version（不带v的canonical三段版本）、bundle_id=`ai.vega`、target=`aarch64-apple-darwin`、archive=`Vega-macos-arm64.zip`、size（精确ZIP字节数）、sha256（64位小写hex）。拒绝未知字段/重复字段、错误版本/架构/ID/资产、非法数值，先验证签名再解析可信字段。
3. 网络只使用已有 GitHub 来源限制；元数据可用于提示，下载和安装资格由可信manifest决定。校验manifest版本必须匹配release tag且高于当前bundle版本。ZIP仍最大512MiB，验证实际长度和manifest摘要后才能解压。原有 `.sha256` 可继续发布供用户使用，安全信任不依赖独立未签名摘要。
4. ad-hoc 构建只要内置公钥即可走独立签名更新；候选必须满足ai.vega/arm64/manifest版本，并通过本地 codesign 完整性检查。不把ad-hoc误当发布者身份。如果当前应用具有已验证Developer ID，则保留候选同TeamID/公证额外约束，避免静默降低现有Apple身份。不得关闭Gatekeeper/SIP，不移除quarantine，不提权；独立签名不承诺替代macOS首次启动校验。
5. 应用支持 `/Applications/Vega.app`、`~/Applications/Vega.app` 与兼容的 `~/Documents/Vega/Vega.app`，只替换实际运行的那个canonical bundle。拒绝symlink路径、开发checkout、DMG/AppTranslocation/只读安装。可信目录：用户owned且非group/world writable的用户安装父目录；系统 `/Applications` 可root-owned/admin-group-writable但不可world writable，实际暂存与rename权限不足时失败并提示人工安装。不得沿用“parent必须uid=currentuser”导致正常/Applications失效，也不得放宽到任意不可信目录。
6. helper必须独立重新验证manifest签名、版本、archive摘要/长度和候选来源，不能把可写install.json里的new_hash或team当作认证依据。建议在helper内从已认证archive重新解压候选，再验证bundle；staging私有且新建，保留原先父进程身份/目标inode/旧binaryhash/退出等待/回滚/启动确认边界。新应用启动确认及身份核对成功后，若旧的 root-owned bundle 无法由当前用户清理，安装仍算成功，保留剩余备份并写清理说明；不提权，不将清理失败误报为安装失败（实现审查补充）。
7. release保持master自动patch。最终ZIP（Apple步骤如有应先完成）生成签名manifest，再发布四项资产。签名前导出私钥对应公钥并与仓库pin比较，错误或缺secret失败，不能默默发不支持独立更新的新包。已发布的历史版本仅两项资产的幂等重跑仍只读结束；新draft必须四项完整才publish。不覆盖历史正式Release。PR构建不获取签名secret。
8. 设置文案反映真实能力：ad-hoc不再一律“当前构建请手动安装”；显示签名校验、可安装/目录不可写/旧发布缺更新签名等具体原因。自动检查开关继续默认开，自动下载；安装仍必须用户确认且无活动任务。旧0.1.1/0.1.2必须先手动安装含公钥的新版本，不能远程给旧binary添加功能。

## 实现分工

- updater agent：app updater/network/platform/helper/必要UI接线与Rust依赖。与发布agent共同遵循上述manifest字节协议；不修改发布脚本。
- 发布agent：xtask签名命令、release workflow、scripts/release.py资产规则及发布文档；不改app updater。顶层Cargo及Cargo.lock仅updater agent写，发布agent请求依赖合并。
- 主agent：密钥生成/secret配置、规格、审查、PR/CI/合并。签名密钥初始化是用户授权实现的必要发布配置；不配置Apple证书、不安装/强退用户运行应用。

## 验收矩阵（实现前冻结）

| ID | 操作/风险 | 预期与证据 |
|---|---|---|
| SIG01 | 合法签名manifest+ZIP | ad-hoc当前应用识别并准备更新；编译/静态审查，真实安装待用户 |
| SIG02 | 错key/篡改manifest/错误签名 | 解压前拒绝，不触碰当前app |
| SIG03 | signed版本/ID/架构/大小/digest不符或旧版重放 | 拒绝，显示可理解错误 |
| SIG04 | /Applications、用户Applications、Documents | 原位安装；父目录权限规则一致，其他副本不修改 |
| SIG05 | 无写权限、路径别名/链接、DMG | 禁止替换并提供人工下载入口 |
| SIG06 | helper启动前或过程中候选/manifest变化 | 重新从可信archive验证；不能仅靠ad-hoc重签或new_hash伪造 |
| SIG07 | CI私钥缺失/公钥不匹配 | 不publish；日志无私钥 |
| SIG08 | 新Release/历史两资产Release重跑 | 新版四资产完整才公开；历史不覆盖、不重新bump |
| SIG09 | 运行任务、稍后、安装失败、启动确认 | 保留既有保护和备份恢复语义 |

会话更高优先级约束：用户未明确要求测试，因此不新增或运行本地测试；只进行编译、静态审查，真实集成由用户手测，云端既有检查保持不变。不能把编译/静态审查或签名产物生成称为真实安装PASS。
