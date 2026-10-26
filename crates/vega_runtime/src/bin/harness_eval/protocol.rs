use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use vega_runtime::model_compat::{ModelApi, ResolvedModel};
use vega_runtime::{FrozenReasoning, ReasoningChoice, RuntimeRunMode, ToolDefinition};
use vega_token::PricingCatalog;

use super::EvalError;

pub(super) const WORKER_VERSION: &str = "vega-harness-worker-v1";
pub(super) const MAX_JSON_BYTES: usize = 32 * 1024 * 1024;
const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrialRequest {
    pub(super) schema_version: u32,
    pub(super) task_id: String,
    pub(super) task_prompt: String,
    pub(super) seed_files: BTreeMap<String, String>,
    pub(super) writable_paths: Vec<String>,
    pub(super) fixed_policy: String,
    pub(super) harness: Harness,
    pub(super) trusted_skill_approvals: BTreeMap<String, String>,
    pub(super) model: Model,
    pub(super) limits: Limits,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Harness {
    pub(super) strategy: String,
    pub(super) skills: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Model {
    pub(super) id: String,
    pub(super) reasoning_effort: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Limits {
    pub(super) timeout_ms: u64,
    pub(super) turn_limit: usize,
    pub(super) max_output_tokens: u32,
    pub(super) max_total_tokens: u64,
}

pub(super) struct Prepared {
    pub(super) reasoning: FrozenReasoning,
    pub(super) instructions: String,
    pub(super) hashes: BTreeMap<String, String>,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn json_digest(value: &Value) -> Result<String, EvalError> {
    let mut canonical = value.clone();
    canonical.sort_all_objects();
    serde_json::to_vec(&canonical)
        .map(|bytes| digest(&bytes))
        .map_err(|_| EvalError("json_encoding_failed"))
}

pub(super) fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn safe_path(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 512
        || value.contains(['\\', '\0'])
        || Path::new(value).is_absolute()
    {
        return false;
    }
    let mut normalized = Vec::new();
    for component in Path::new(value).components() {
        let Component::Normal(segment) = component else {
            return false;
        };
        let Some(segment) = segment.to_str() else {
            return false;
        };
        if segment.is_empty()
            || matches!(
                segment.to_ascii_lowercase().as_str(),
                ".git" | ".agents" | ".vega" | ".codex" | "agents.md" | "claude.md"
            )
        {
            return false;
        }
        normalized.push(segment);
    }
    normalized.join("/") == value
}

fn safe_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn text_valid(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.contains('\0')
}

fn validate(request: &TrialRequest) -> Result<(), EvalError> {
    if request.schema_version != 1
        || !safe_id(&request.task_id)
        || !text_valid(&request.task_prompt, 64 * 1024)
        || !text_valid(&request.fixed_policy, 64 * 1024)
        || !text_valid(&request.harness.strategy, 64 * 1024)
        || request.model.id != "gpt-6.1-sol"
        || request.model.reasoning_effort != "max"
        || request.limits.timeout_ms == 0
        || request.limits.timeout_ms > 3_600_000
        || request.limits.turn_limit == 0
        || request.limits.turn_limit > 256
        || request.limits.max_output_tokens == 0
        || request.limits.max_total_tokens == 0
        || request.limits.max_total_tokens > 10_000_000
        || request.seed_files.len() > 256
        || request.harness.skills.len() > 16
        || request.writable_paths.is_empty()
        || request.writable_paths.len() > 64
    {
        return Err(EvalError("invalid_request"));
    }
    let public = serde_json::to_value(request).map_err(|_| EvalError("invalid_request"))?;
    if serde_json::to_vec(&public)
        .map_err(|_| EvalError("invalid_request"))?
        .len()
        > MAX_INPUT_BYTES
    {
        return Err(EvalError("request_too_large"));
    }
    let writable: BTreeSet<_> = request.writable_paths.iter().collect();
    if writable.len() != request.writable_paths.len()
        || writable.iter().any(|path| !safe_path(path))
        || request
            .seed_files
            .iter()
            .any(|(path, text)| !safe_path(path) || text.contains('\0'))
    {
        return Err(EvalError("unsafe_task_path"));
    }
    let paths: BTreeSet<String> = request
        .seed_files
        .keys()
        .chain(request.writable_paths.iter())
        .cloned()
        .collect();
    for path in &paths {
        if Path::new(path)
            .ancestors()
            .skip(1)
            .any(|parent| parent.to_str().is_some_and(|value| paths.contains(value)))
        {
            return Err(EvalError("conflicting_task_path"));
        }
    }
    for (slug, content) in &request.harness.skills {
        if !safe_slug(slug) || !text_valid(content, 64 * 1024) {
            return Err(EvalError("invalid_skill"));
        }
        if request.trusted_skill_approvals.get(slug) != Some(&digest(content.as_bytes())) {
            return Err(EvalError("skill_approval_missing_or_stale"));
        }
        vega_runtime::skills::parse_skill_md(content.as_bytes(), slug)
            .map_err(|_| EvalError("invalid_skill"))?;
    }
    if request.trusted_skill_approvals.iter().any(|(slug, hash)| {
        !safe_slug(slug)
            || hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    }) {
        return Err(EvalError("invalid_skill_approval"));
    }
    Ok(())
}

pub(super) fn tools_value(definitions: &[ToolDefinition]) -> Value {
    Value::Array(
        definitions
            .iter()
            .map(|definition| {
                json!({
                    "name": definition.name,
                    "description": definition.description,
                    "input_schema": definition.input_schema,
                    "strict": definition.strict,
                })
            })
            .collect(),
    )
}

pub(super) fn prepare(
    request: &TrialRequest,
    model: &ResolvedModel,
    pricing: Option<&PricingCatalog>,
) -> Result<Prepared, EvalError> {
    validate(request)?;
    if model.identity.model != request.model.id
        || !model
            .capabilities
            .allowed_efforts
            .iter()
            .any(|effort| effort == "max")
        || model
            .definition
            .max_output_tokens
            .is_none_or(|limit| request.limits.max_output_tokens > limit)
        || model
            .definition
            .context_window
            .is_none_or(|limit| limit <= u64::from(request.limits.max_output_tokens))
    {
        return Err(EvalError("model_profile_unavailable"));
    }
    let reasoning = vega_runtime::model_compat::freeze_choice(
        model,
        ReasoningChoice::Effort(request.model.reasoning_effort.clone()),
    )
    .map_err(|_| EvalError("model_effort_unsupported"))?;
    let instructions = format!(
        "{}\n\nOnly task-declared outputs are writable. Project instructions, approved skills, shell execution, external access, credentials and permission rules are fixed by the evaluator.\n",
        request.fixed_policy
    );
    let pricing = pricing
        .map(|catalog| {
            catalog
                .encode()
                .map_err(|_| EvalError("pricing_invalid"))
                .and_then(|bytes| {
                    serde_json::from_slice::<Value>(&bytes)
                        .map_err(|_| EvalError("pricing_invalid"))
                })
        })
        .transpose()?;
    let config = json!({
        "worker_version": WORKER_VERSION,
        "runtime_version": env!("CARGO_PKG_VERSION"),
        "model_rules_version": vega_runtime::model_compat::RULES_VERSION,
        "model_definition": model.definition,
        "model_capabilities": model.capabilities,
        "model_fingerprint": model.fingerprint,
        "model": request.model,
        "limits": request.limits,
        "pricing": pricing,
        "transport_retry_limit": 0,
        "base_tool_definitions": tools_value(&vega_runtime::tool_definitions(RuntimeRunMode::Execute)),
        "capabilities": {
            "run_mode": "execute",
            "permission_mode": "confirm",
            "skills": !request.harness.skills.is_empty(),
            "bash_approved": false,
            "mcp": false,
            "subagents": false,
            "automation": false,
            "computer_use": false,
            "user_questions": false,
        },
        "api": match model.definition.route.api {
            ModelApi::ChatCompletions => "openai-completions",
            ModelApi::Responses => "openai-responses",
        },
    });
    let hashes = BTreeMap::from([
        (
            "task".into(),
            json_digest(&json!({"task_id": request.task_id, "task_prompt": request.task_prompt}))?,
        ),
        ("seed".into(), json_digest(&json!(request.seed_files))?),
        ("policy".into(), digest(request.fixed_policy.as_bytes())),
        ("harness".into(), json_digest(&json!(request.harness))?),
        ("model".into(), json_digest(&json!(request.model))?),
        ("instructions".into(), digest(instructions.as_bytes())),
        ("request".into(), json_digest(&json!(request))?),
        ("config".into(), json_digest(&config)?),
    ]);
    Ok(Prepared {
        reasoning,
        instructions,
        hashes,
    })
}
