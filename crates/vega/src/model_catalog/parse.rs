use super::{CatalogResult, MAX_BYTES};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use vega_conversation::types::*;

#[derive(Deserialize)]
struct Provider {
    id: String,
    name: String,
    models: BTreeMap<String, Model>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
    name: String,
    canonical_model_id: Option<String>,
    limit: Limit,
    reasoning: Option<bool>,
    tool_call: Option<bool>,
    #[serde(default)]
    reasoning_options: Vec<Value>,
    cost: Option<BTreeMap<String, Value>>,
    modalities: Option<Modalities>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct Modalities {
    input: Vec<String>,
    output: Vec<String>,
}

#[derive(Deserialize)]
struct Limit {
    context: u64,
    input: Option<u64>,
    output: u64,
}

fn safe(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn price(value: Option<&Value>) -> CatalogResult<Option<String>> {
    value
        .map(|v| {
            let n = v.as_number().ok_or("模型目录价格格式无效")?;
            if !n.as_f64().is_some_and(|v| v.is_finite() && v >= 0.) {
                return Err("模型目录价格格式无效".into());
            }
            Ok(n.to_string())
        })
        .transpose()
}

fn option(value: Value) -> CatalogResult<CatalogReasoningOption> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or("模型目录思考资料无效")?;
    Ok(match kind {
        "toggle" => CatalogReasoningOption::Toggle,
        "effort" => {
            let values = value
                .get("values")
                .and_then(Value::as_array)
                .ok_or("模型目录思考档位无效")?;
            if values.len() > 32 {
                return Err("模型目录思考档位超限".into());
            }
            CatalogReasoningOption::Effort(
                values
                    .iter()
                    .map(|v| {
                        if v.is_null() {
                            Ok(None)
                        } else {
                            let value = v
                                .as_str()
                                .filter(|v| safe(v, 64))
                                .ok_or("模型目录思考档位无效")?;
                            Ok(Some(value.to_string()))
                        }
                    })
                    .collect::<CatalogResult<Vec<_>>>()?,
            )
        }
        "budget_tokens" => {
            let min = value
                .get("min")
                .map(|v| {
                    v.as_i64()
                        .filter(|v| *v >= -1)
                        .ok_or("模型目录思考预算无效")
                })
                .transpose()?;
            let max = value
                .get("max")
                .map(|v| v.as_u64().ok_or("模型目录思考预算无效"))
                .transpose()?;
            if min.is_some_and(|min| min >= 0 && max.is_some_and(|max| min as u64 > max)) {
                return Err("模型目录思考预算无效".into());
            }
            CatalogReasoningOption::Budget { min, max }
        }
        unknown if safe(unknown, 64) => CatalogReasoningOption::Unknown(unknown.into()),
        _ => return Err("模型目录思考资料无效".into()),
    })
}

pub(super) fn parse(bytes: &[u8]) -> CatalogResult<Vec<CatalogProvider>> {
    if bytes.len() > MAX_BYTES {
        return Err("模型目录响应超过大小限制".into());
    }
    let providers: BTreeMap<String, Provider> =
        serde_json::from_slice(bytes).map_err(|_| "模型目录格式无效")?;
    if providers.is_empty() || providers.len() > 4096 {
        return Err("模型目录提供方数量无效".into());
    }
    let mut count = 0;
    providers
        .into_iter()
        .map(|(key, p)| {
            if key != p.id || !safe(&p.id, 1024) || !safe(&p.name, 4096) {
                return Err("模型目录提供方身份无效".into());
            }
            let models = p
                .models
                .into_iter()
                .map(|(key, m)| {
                    count += 1;
                    if count > 100_000
                        || key != m.id
                        || !safe(&m.id, 1024)
                        || !safe(&m.name, 4096)
                        || m.canonical_model_id
                            .as_ref()
                            .is_some_and(|s| !safe(s, 1024))
                        || m.status.as_ref().is_some_and(|s| !safe(s, 64))
                        || m.reasoning_options.len() > 32
                    {
                        return Err("模型目录模型身份或数量无效".into());
                    }
                    let cost = m
                        .cost
                        .map(|cost| {
                            Ok::<_, String>(CatalogCost {
                                input: price(cost.get("input"))?,
                                output: price(cost.get("output"))?,
                                cache_read: price(cost.get("cache_read"))?,
                                cache_write: price(cost.get("cache_write"))?,
                                complex: cost.keys().any(|k| {
                                    !matches!(
                                        k.as_str(),
                                        "input" | "output" | "cache_read" | "cache_write"
                                    )
                                }),
                            })
                        })
                        .transpose()?;
                    Ok(CatalogModel {
                        id: m.id,
                        name: m.name,
                        canonical_id: m.canonical_model_id,
                        context: m.limit.context,
                        input: m.limit.input,
                        output: m.limit.output,
                        reasoning: m.reasoning,
                        tool_call: m.tool_call,
                        reasoning_options: m
                            .reasoning_options
                            .into_iter()
                            .map(option)
                            .collect::<CatalogResult<_>>()?,
                        cost,
                        text_pricing: m.modalities.is_some_and(|v| {
                            !v.input.is_empty()
                                && v.input.iter().all(|m| m == "text")
                                && v.output == ["text"]
                        }),
                        status: m.status,
                    })
                })
                .collect::<CatalogResult<Vec<_>>>()?;
            Ok(CatalogProvider {
                id: p.id,
                name: p.name,
                models,
            })
        })
        .collect()
}
