# #494 Chat 目录来源与导入

本文记录 v3（[固定提交 8c57f7ef](https://github.com/puzige/vega/tree/8c57f7efb3d886c1c10429edb2bc1219a7b7a753)）的导入结果。v4 将两种现有 API 的共同能力移入 `model_compat`，删除下述旧目录资产；当前范围与验证见[模型兼容基线](vega-issue-494-model-compat-baseline.md)和[v4 验证记录](vega-issue-494-v4-verification.md)。本文的数量、范围和结论仅对应 v3。

本轮内置的是固定版本的兼容元数据和 Vega 的 Rust codec，不增加 npm 运行时、在线目录依赖或构建期下载。目录记录不是逐模型真实服务验证结果；工具接续、请求和解析行为由本卡冻结测试另行验证。

## 固定来源

| 项目 | 固定值 |
| --- | --- |
| 包 | `@earendil-works/pi-ai@0.84.4` |
| 发布包 | <https://registry.npmjs.org/@earendil-works/pi-ai/-/pi-ai-0.84.4.tgz> |
| npm gitHead | `b79e4cc834970cca69daebffab7df1da7d1e52c4` |
| tarball SHA1 | `348f1be5c2a0f4d17cc167fe1e5a7cabb191d079` |
| tarball integrity | `sha512-AClAZxf5+c4RRu44NJPS6wyQy+Nmq+Mzyyrdvm4ZVMNuixelO02RZX4G4Aq1F145Yzp43wnM5S+hLlSI7ypfVw==` |
| 投影 SHA256 | `3784adbfdfcebe3317bb7c52d262577991a82cb613b32711218b14e8a169f6f6` |
| MIT LICENSE SHA256 | `0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48` |

生成的 provider JSON 位于发布包的 `dist/providers/data/`，不在该 gitHead 的 Git 源码树中。[固定 Chat codec](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/ai/src/api/openai-completions.ts) 提供默认值与编码行为参考，[固定 LICENSE](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/LICENSE) 提供 MIT 许可。该 tarball 不包含 LICENSE，仓库中的副本取自上述固定 Git 提交；其 Git blob 为 `b0a8e9b81083294360c69b4ec45d3d39a2b28197`。

v3 固定提交保存三个文件：

- [Chat 投影](https://github.com/puzige/vega/blob/8c57f7efb3d886c1c10429edb2bc1219a7b7a753/crates/vega_runtime/src/chat_compat/pi-ai-0.84.4.json)：version 1 投影，保留 653 条源记录。
- [Chat 来源清单](https://github.com/puzige/vega/blob/8c57f7efb3d886c1c10429edb2bc1219a7b7a753/crates/vega_runtime/src/chat_compat/pi-ai-0.84.4.sources.json)：26 个源文件的 SHA256、条数、发布 integrity、投影摘要和消费字段清单。
- [PI-LICENSE](https://github.com/puzige/vega/blob/8c57f7efb3d886c1c10429edb2bc1219a7b7a753/crates/vega_runtime/src/chat_compat/PI-LICENSE)：MIT 原文与版权声明，v4 继续保留。

26 个源 provider 共有 653 条 `openai-completions` 记录、645 个规范化 endpoint/model 身份。8 条重复来自 Qwen plan 与 individual 共用入口，其 reasoning、thinkingLevelMap、compat 完全相同。导入保留源记录；运行时 decoder 仅允许行为等价的重复身份，冲突明确失败。以上数字用于来源清点，不表示 653 个不同模型或服务已经实测。

## 投影范围

每条保留 `api`、`baseUrl`、`id`、`provider`、`reasoning`，以及原来存在的 `thinkingLevelMap`。缺键与显式 null 不互换。`compat` 仅保留实际消费的以下 9 个字段；过滤后为空时省略该对象。

| 消费字段 | Vega 用途 |
| --- | --- |
| `thinkingFormat` | 八种请求方言：默认 openai、deepseek、zai、qwen、together、baseten、openrouter、ant-ling |
| `supportsDeveloperRole` | 与模型 reasoning 事实共同决定 system/developer |
| `supportsReasoningEffort` | 区分档位和布尔控制，已知模型不虚构选择 |
| `supportsUsageInStreaming` | 是否请求流用量 |
| `maxTokensField` | 输出上限字段选择 |
| `requiresReasoningContentOnAssistantMessages` | 已声明来源字段的服务内部回传映射 |
| `zaiToolStream` | 工具请求的 `tool_stream` |
| `supportsStrictMode` | wire strict 开关，本地 schema 校验保留 |
| `chatTemplateArgs` | 该版本实际使用的 `enable_thinking: {"$var":"thinking.enabled"}` |

源目录另有 5 个 compat 字段未消费：`supportsStore`、`cacheControlFormat`、`supportsLongCacheRetention`、`sendSessionAffinityHeaders`、`deferredToolsMode`。Vega 当前 ChatRequest 没有对应的存储、缓存寿命/亲和、延迟工具产品输入；不添加这些产品功能，也不在 decoder 中静默接收这些字段。费用、上下文/输出额度、采样、名称等目录信息同样未导入，本卡不改其现有权威来源。

该固定目录未注册 `requiresToolResultName`、`requiresAssistantAfterToolResult`、`requiresThinkingAsText` 或 `supportsFinishReason`，也未使用 pi 的其他思考格式。它们不作为本轮支持承诺。未知字段、格式、映射、版本或冲突身份会被 decoder 拒绝；发送入口传播该错误，不把坏目录当未知模型继续发送。

## Vega 的解析边界

服务通过规范化 HTTPS endpoint 的完整身份识别，拒绝凭主机子串或 provider 显示名称猜测。Cloudflare 仅对源路径中的账户/网关占位段做单段匹配。除源 URL 外，显式入口别名为 Together 的 `https://api.together.xyz/v1`、DeepSeek 的 `/v1`，以及既有 Zai/智谱标准与 coding v4 入口。别名和服务默认值集中在 [v3 catalog.rs](https://github.com/puzige/vega/blob/8c57f7efb3d886c1c10429edb2bc1219a7b7a753/crates/vega_runtime/src/chat_compat/catalog.rs)，不分散到 App、Agent 或 UI。

服务 transport 默认值、该服务一致的目录覆写、精确模型覆写依次合成。未知代理即使同名也不借用原厂模型能力。完全未知模型显示默认加六档，选择通过实际方言发送；已知服务未知模型使用服务的默认控制方言。默认选择不主动开关思考；不支持的选择不降档或吞掉。Responses 保持独立编码与 replay 类型。

Vega 的已有公开覆盖保留在 resolver：Step `step-5-preview` 的 low/medium/high（不推测关闭），DeepSeek 已有 `deepseek-flash` 控制与严格工具回传，以及 GLM 已有保留思考策略。Step 覆盖不重置已识别服务的 transport/方言。未知服务的 Step 仍保留已工作的通用 `max_tokens`。这些覆盖与目录事实共同进入有效 profile 身份；规则版本为 `vega.chat.v3.pi-ai-0.84.4.1`，参与 warm replay 绑定。

本轮不照搬 pi 的 URL 子串启发式，不暴露 compat override UI，也不读取或迁移旧思考配置。新偏好文件继续只保存 provider/model/preference。新增 raw/source/details 仅用于受身份绑定的内存回放，受预算和凭据护栏约束，不新增持久化或展示。

## 可复现投影

以下 Python 只适用于检出固定提交 `8c57f7efb3d886c1c10429edb2bc1219a7b7a753` 的独立目录，在其仓库根目录运行；当前 v4 工作树已移除旧资产。它仅下载固定资料并写入 `/tmp` 的复核产物，不启动 Vega、不编译、不运行测试，也不修改仓库文件。它逐文件核对 manifest，再按源文件名顺序及文件内原记录顺序投影；JSON 使用 UTF-8、递归排序对象键、紧凑分隔符和单个末尾换行。

```python
import base64
import hashlib
import io
import json
import tarfile
import urllib.request
from pathlib import Path

directory = Path("crates/vega_runtime/src/chat_compat")
manifest = json.loads((directory / "pi-ai-0.84.4.sources.json").read_text())
archive = urllib.request.urlopen(manifest["tarball"]).read()
algorithm, expected = manifest["integrity"].split("-", 1)
assert algorithm == "sha512"
assert base64.b64encode(hashlib.sha512(archive).digest()).decode() == expected
assert hashlib.sha1(archive).hexdigest() == manifest["tarball_sha1"]
allowed = set(manifest["consumed_compat_fields"])
records = []
with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
    actual = set()
    for member in package.getmembers():
        if member.name.startswith("package/dist/providers/data/") and member.name.endswith(".json"):
            data = json.load(package.extractfile(member))
            if data.get("openai-completions"):
                actual.add(member.name.removeprefix("package/"))
    assert actual == {source["path"] for source in manifest["sources"]}
    for source in sorted(manifest["sources"], key=lambda source: source["path"]):
        raw = package.extractfile("package/" + source["path"]).read()
        assert hashlib.sha256(raw).hexdigest() == source["sha256"]
        models = json.loads(raw)["openai-completions"]
        assert len(models) == source["chat_records"]
        for model in models.values():
            item = {key: model[key] for key in ("api", "baseUrl", "id", "provider", "reasoning")}
            if "thinkingLevelMap" in model:
                item["thinkingLevelMap"] = model["thinkingLevelMap"]
            compat = {key: value for key, value in model.get("compat", {}).items() if key in allowed}
            if compat:
                item["compat"] = compat
            records.append(item)
projection = (json.dumps({"version": 1, "models": records}, ensure_ascii=False,
                         sort_keys=True, separators=(",", ":")) + "\n").encode()
assert len(records) == manifest["source_records"]
assert hashlib.sha256(projection).hexdigest() == manifest["projection_sha256"]
assert projection == (directory / "pi-ai-0.84.4.json").read_bytes()
license_url = "https://raw.githubusercontent.com/earendil-works/pi/" + manifest["gitHead"] + "/LICENSE"
license_bytes = urllib.request.urlopen(license_url).read()
assert hashlib.sha256(license_bytes).hexdigest() == "0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48"
assert license_bytes == (directory / "PI-LICENSE").read_bytes()
Path("/tmp/vega-pi-ai-0.84.4.projected.json").write_bytes(projection)
print(len(manifest["sources"]), len(records), hashlib.sha256(projection).hexdigest())
```

升级时应固定新的发布包及 integrity，重新审查 compat 字段和 codec 默认值，保留缺键/null 语义，更新来源 manifest、投影摘要、规则版本与验收。不能仅替换 JSON 后宣称新增服务已被真实验证。
