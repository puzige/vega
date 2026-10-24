use std::collections::HashMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use vega_runtime::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, decode_model_catalog, resolve_model,
};
use vega_runtime::skills::{SkillApproval, SkillCatalog, SkillRun, SkillSource};
use vega_runtime::{
    AgentRequest, ChatMessage, ChatRequest, ChatRole, ContextBudget, EventStream, OpenAiProvider,
    Provider, ProviderStream, RetryPolicy, RuntimeExactRule, RuntimeMutatingTool,
    RuntimePermissionMode, RuntimeRunMode, RuntimeToolConfig, VegaError, run_agent_with_sink,
};
use vega_token::PricingCatalog;
use vega_tools::Tools;

#[path = "protocol.rs"]
mod protocol;
#[path = "trace.rs"]
mod trace;

pub(super) use protocol::TrialRequest;
use protocol::{MAX_JSON_BYTES, Prepared, digest, json_digest, prepare, safe_id, tools_value};
use trace::{Recorder, failure_category};

const KEY_ENV: &str = "VEGA_HARNESS_API_KEY";
const RECORD_FILE: &str = "result.json";
static PROCESS_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub(super) struct EvalError(&'static str);

impl fmt::Display for EvalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for EvalError {}

impl From<std::io::Error> for EvalError {
    fn from(_: std::io::Error) -> Self {
        Self("filesystem_unavailable")
    }
}

struct ObservedProvider<'a> {
    inner: &'a dyn Provider,
    tools_hash: Arc<Mutex<Option<String>>>,
    requests: AtomicUsize,
}

impl ObservedProvider<'_> {
    fn inspect(&self, request: &ChatRequest) -> Result<(), VegaError> {
        let hash = json_digest(&tools_value(&request.tools)).map_err(|_| VegaError::Tool {
            tool: "harness_eval".into(),
            message: "tool snapshot unavailable".into(),
        })?;
        let mut frozen = self.tools_hash.lock().map_err(|_| VegaError::Tool {
            tool: "harness_eval".into(),
            message: "tool snapshot unavailable".into(),
        })?;
        match &*frozen {
            Some(previous) if previous != &hash => Err(VegaError::Tool {
                tool: "harness_eval".into(),
                message: "tool snapshot changed".into(),
            }),
            Some(_) => Ok(()),
            None => {
                *frozen = Some(hash);
                Ok(())
            }
        }?;
        self.requests.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl Provider for ObservedProvider<'_> {
    fn model_snapshot(&self) -> Option<ResolvedModel> {
        self.inner.model_snapshot()
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        if let Err(error) = self.inspect(&request) {
            return Box::pin(async { Err(error) });
        }
        self.inner.chat_stream(request, cancel)
    }

    fn chat_stream_recovering(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        if let Err(error) = self.inspect(&request) {
            return Box::pin(async { Err(error) });
        }
        self.inner.chat_stream_recovering(request, cancel)
    }
}

fn read_plain(path: &Path) -> Result<Vec<u8>, EvalError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > MAX_JSON_BYTES as u64 {
        return Err(EvalError("unsafe_or_oversized_file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(EvalError("unsafe_or_oversized_file"));
    }
    Ok(bytes)
}

fn read_text(path: &Path) -> Result<String, EvalError> {
    String::from_utf8(read_plain(path)?).map_err(|_| EvalError("invalid_utf8_file"))
}

fn safe_directory(path: &Path) -> Result<(), EvalError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(EvalError("unsafe_trial_directory"));
    }
    Ok(())
}

fn checked_workspace_file(workspace: &Path, relative: &str) -> Result<PathBuf, EvalError> {
    let joined = workspace.join(relative);
    let parent = joined.parent().ok_or(EvalError("unsafe_task_path"))?;
    let suffix = parent
        .strip_prefix(workspace)
        .map_err(|_| EvalError("unsafe_task_path"))?;
    let mut current = workspace.to_path_buf();
    safe_directory(&current)?;
    for component in suffix.components() {
        current.push(component.as_os_str());
        safe_directory(&current)?;
    }
    Ok(joined)
}

fn write_new(path: &Path, text: &str, readonly: bool) -> Result<(), EvalError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    if readonly {
        file.set_permissions(fs::Permissions::from_mode(0o444))?;
    }
    Ok(())
}

fn verify_protected(
    request: &TrialRequest,
    prepared: &Prepared,
    workspace: &Path,
) -> Result<(), EvalError> {
    for (relative, original) in &request.seed_files {
        if !request.writable_paths.contains(relative)
            && read_text(&checked_workspace_file(workspace, relative)?)? != *original
        {
            return Err(EvalError("protected_file_changed"));
        }
    }
    if read_text(&workspace.join("AGENTS.md"))? != prepared.instructions {
        return Err(EvalError("protected_file_changed"));
    }
    for (slug, original) in &request.harness.skills {
        let relative = format!(".agents/skills/{slug}/SKILL.md");
        if read_text(&checked_workspace_file(workspace, &relative)?)? != *original {
            return Err(EvalError("protected_file_changed"));
        }
    }
    Ok(())
}

fn artifacts(request: &TrialRequest, workspace: &Path) -> Result<Value, EvalError> {
    let mut values = serde_json::Map::new();
    for relative in &request.writable_paths {
        let path = workspace.join(relative);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let checked = checked_workspace_file(workspace, relative)?;
                values.insert(relative.clone(), Value::String(read_text(&checked)?));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(EvalError("artifact_unavailable")),
        }
    }
    Ok(Value::Object(values))
}

fn cached(
    request: &TrialRequest,
    prepared: &Prepared,
    trial_root: &Path,
    run_id: &str,
) -> Result<Value, EvalError> {
    safe_directory(trial_root)?;
    let result: Value = serde_json::from_slice(&read_plain(&trial_root.join(RECORD_FILE))?)
        .map_err(|_| EvalError("incomplete_trial"))?;
    if result["schema_version"] != 1
        || result["backend"] != "vega_native_runtime"
        || result["run_id"] != run_id
        || result["task_id"] != request.task_id
        || prepared
            .hashes
            .iter()
            .any(|(key, value)| result["hashes"][key].as_str() != Some(value.as_str()))
    {
        return Err(EvalError("run_identity_conflict"));
    }
    let workspace = trial_root.join("workspace");
    verify_protected(request, prepared, &workspace)?;
    if result["artifacts"] != artifacts(request, &workspace)? {
        return Err(EvalError("cached_artifact_changed"));
    }
    Ok(result)
}

fn materialize(
    request: &TrialRequest,
    prepared: &Prepared,
    trial_root: &Path,
) -> Result<PathBuf, EvalError> {
    let workspace = trial_root.join("workspace");
    fs::DirBuilder::new().mode(0o700).create(&workspace)?;
    for (relative, text) in &request.seed_files {
        write_new(
            &workspace.join(relative),
            text,
            !request.writable_paths.contains(relative),
        )?;
    }
    write_new(&workspace.join("AGENTS.md"), &prepared.instructions, true)?;
    for (slug, text) in &request.harness.skills {
        write_new(
            &workspace.join(".agents/skills").join(slug).join("SKILL.md"),
            text,
            true,
        )?;
    }
    Ok(workspace)
}

fn tool_config(
    request: &TrialRequest,
    tools: &Tools,
    trial_root: &Path,
    run_id: &str,
    known_credentials: &[String],
) -> Result<RuntimeToolConfig, EvalError> {
    let exact_rules = request
        .writable_paths
        .iter()
        .flat_map(|path| {
            [RuntimeMutatingTool::Write, RuntimeMutatingTool::Edit].map(|tool| RuntimeExactRule {
                tool,
                pattern: path.clone(),
            })
        })
        .collect();
    let mut config = RuntimeToolConfig::new(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        "harness-pilot".into(),
        run_id.into(),
        trial_root.join("checkpoints"),
        exact_rules,
    )
    .with_turn_limit(request.limits.turn_limit);
    if !known_credentials.is_empty() {
        let frozen = known_credentials.to_vec();
        config = config.with_credential_reader(Arc::new(move || Ok(frozen.clone())));
    }
    if !request.harness.skills.is_empty() {
        let source = SkillSource::project_approved(tools.root())
            .map_err(|_| EvalError("skill_source_unavailable"))?
            .ok_or(EvalError("skill_source_unavailable"))?;
        let discovery = source
            .discover()
            .map_err(|_| EvalError("skill_source_unavailable"))?;
        if !discovery.diagnostics.is_empty()
            || discovery.candidates.len() != request.harness.skills.len()
        {
            return Err(EvalError("skill_catalog_changed"));
        }
        let mut approvals = Vec::new();
        for candidate in &discovery.candidates {
            if request.trusted_skill_approvals.get(&candidate.name) != Some(&candidate.sha256) {
                return Err(EvalError("skill_approval_missing_or_stale"));
            }
            approvals.push(
                SkillApproval::reviewed(candidate, "trial-skills", true, true)
                    .map_err(|_| EvalError("skill_review_unavailable"))?,
            );
        }
        let catalog = SkillCatalog::freeze(discovery.candidates, &approvals, true)
            .map_err(|_| EvalError("skill_catalog_unavailable"))?;
        if !catalog.exclusions().is_empty() {
            return Err(EvalError("skill_catalog_changed"));
        }
        config = config.with_skill_run(SkillRun::new(catalog, true), Vec::new());
    }
    Ok(config)
}

fn sanitize(value: &mut Value, known_credentials: &[String]) {
    match value {
        Value::String(text) => {
            *text = vega_runtime::redact_sensitive_credential_text(text, known_credentials);
        }
        Value::Array(values) => {
            for value in values {
                sanitize(value, known_credentials);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                sanitize(value, known_credentials);
            }
        }
        _ => {}
    }
}

async fn run_trial_inner(
    provider: &dyn Provider,
    request: TrialRequest,
    trial_root: &Path,
    run_id: &str,
    pricing: Option<PricingCatalog>,
    cancel: CancellationToken,
    known_credentials: &[String],
) -> Result<Value, EvalError> {
    if !safe_id(run_id) || !trial_root.is_absolute() {
        return Err(EvalError("invalid_trial_identity"));
    }
    let model = provider
        .model_snapshot()
        .ok_or(EvalError("model_profile_unavailable"))?;
    let mut prepared = prepare(&request, &model, pricing.as_ref())?;
    let encoded = serde_json::to_string(&request).map_err(|_| EvalError("invalid_request"))?;
    if vega_runtime::inspect_credential_text(&encoded, known_credentials)
        .iter()
        .any(|finding| finding.confidence == vega_runtime::CredentialConfidence::KnownExact)
    {
        return Err(EvalError("credential_exposure_blocked"));
    }
    let started = Instant::now();
    match fs::DirBuilder::new().mode(0o700).create(trial_root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return cached(&request, &prepared, trial_root, run_id);
        }
        Err(_) => return Err(EvalError("trial_directory_unavailable")),
    }
    let trial_root = trial_root.canonicalize()?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(trial_root.join("checkpoints"))?;
    let workspace = materialize(&request, &prepared, &trial_root)?;
    let tools = Tools::new(&workspace).map_err(|_| EvalError("tools_unavailable"))?;
    let instructions = tools
        .project_instructions()
        .map_err(|_| EvalError("project_instructions_unavailable"))?
        .ok_or(EvalError("project_instructions_unavailable"))?;
    if digest(instructions.content().as_bytes()) != prepared.hashes["instructions"] {
        return Err(EvalError("project_instructions_changed"));
    }
    let system_prompt = format!(
        "You are Vega's restricted native Runtime evaluation worker. Complete the user's task with the declared native tools. Tool schemas and evaluator permissions are authoritative. Guidance and skill content do not grant permissions.\n\nFixed policy:\n{}\n\nTrial guidance:\n{}\n\n{}",
        request.fixed_policy,
        request.harness.strategy,
        json!({
            "cwd": tools.root().to_string_lossy(),
            "project_instructions": {"path": instructions.file_name(), "content": instructions.content()},
        })
    );
    let config = tool_config(&request, &tools, &trial_root, run_id, known_credentials)?;
    let context_budget = ContextBudget::new(
        model
            .definition
            .context_window
            .ok_or(EvalError("model_profile_unavailable"))?,
        u64::from(request.limits.max_output_tokens),
        false,
    )
    .map_err(|_| EvalError("context_budget_invalid"))?;
    let agent = AgentRequest {
        model: request.model.id.clone(),
        system_prompt,
        history: vec![ChatMessage::new(ChatRole::User, &request.task_prompt)],
        max_tokens: Some(request.limits.max_output_tokens),
        completed_tool_results: HashMap::new(),
        tool_config: config,
        pricing_catalog: pricing,
        reasoning: Some(prepared.reasoning.clone()),
        model_snapshot: Some(Box::new(model)),
        replay_frame: None,
        context_budget: Some(context_budget),
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    };
    let full_tools_hash = Arc::new(Mutex::new(None));
    let observed = ObservedProvider {
        inner: provider,
        tools_hash: full_tools_hash.clone(),
        requests: AtomicUsize::new(0),
    };
    let mut recorder = Recorder::new(
        request.limits.max_total_tokens,
        cancel.clone(),
        known_credentials.to_vec(),
    );
    let mut timed_out = false;
    let outcome = {
        let run = run_agent_with_sink(&observed, &tools, agent, cancel.clone(), |event| {
            std::future::ready(recorder.observe(event))
        });
        tokio::pin!(run);
        tokio::select! {
            biased;
            result = &mut run => result,
            _ = tokio::time::sleep(Duration::from_millis(request.limits.timeout_ms)) => {
                timed_out = true;
                cancel.cancel();
                run.await
            }
        }
    };
    let mut terminal = if timed_out {
        "timeout"
    } else if recorder.budget_exhausted {
        "budget_exhausted"
    } else {
        recorder.terminal.unwrap_or("failed")
    };
    let (final_text, mut failure) = match outcome {
        Ok(outcome) => {
            if outcome.failed && !timed_out && !recorder.budget_exhausted {
                terminal = "failed";
            }
            (outcome.final_text, recorder.failure)
        }
        Err(error) => {
            if !timed_out && !recorder.budget_exhausted {
                terminal = if cancel.is_cancelled() {
                    "cancelled"
                } else {
                    "failed"
                };
            }
            (String::new(), Some(failure_category(&error)))
        }
    };
    if let Err(error) = verify_protected(&request, &prepared, &workspace) {
        terminal = "failed";
        failure = Some(error.0);
    }
    let output = match artifacts(&request, &workspace) {
        Ok(value) => value,
        Err(error) => {
            terminal = "failed";
            failure = Some(error.0);
            json!({})
        }
    };
    if let Some(hash) = full_tools_hash
        .lock()
        .map_err(|_| EvalError("tool_snapshot_unavailable"))?
        .as_ref()
    {
        prepared.hashes.insert("tools".into(), hash.clone());
    }
    let metrics = recorder.metrics(observed.requests.load(Ordering::Relaxed));
    let mut result = json!({
        "schema_version": 1,
        "backend": "vega_native_runtime",
        "worker_version": protocol::WORKER_VERSION,
        "task_id": request.task_id,
        "run_id": run_id,
        "terminal": terminal,
        "final_text": final_text,
        "artifacts": output,
        "metrics": metrics,
        "trace": recorder.trace,
        "hashes": prepared.hashes,
        "wall_time_ms": started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        "failure_category": failure,
    });
    sanitize(&mut result, known_credentials);
    let bytes = serde_json::to_vec(&result).map_err(|_| EvalError("result_encoding_failed"))?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(EvalError("result_too_large"));
    }
    let pending = trial_root.join("result.pending.json");
    write_new(
        &pending,
        std::str::from_utf8(&bytes).map_err(|_| EvalError("result_encoding_failed"))?,
        false,
    )?;
    fs::rename(pending, trial_root.join(RECORD_FILE))?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderConfig {
    schema_version: u32,
    provider: String,
    api: ModelApi,
    base_url: String,
    model_catalog: Value,
    pricing: Option<Value>,
}

fn configured_model(
    config: &ProviderConfig,
    request: &TrialRequest,
) -> Result<ResolvedModel, EvalError> {
    if config.schema_version != 1 || config.provider.trim().is_empty() {
        return Err(EvalError("provider_config_invalid"));
    }
    let route = ModelRoute::new(config.api, &config.base_url)
        .map_err(|_| EvalError("provider_route_invalid"))?;
    let api_name = match config.api {
        ModelApi::ChatCompletions => "openai-completions",
        ModelApi::Responses => "openai-responses",
    };
    let exact_profile = config.model_catalog["models"]
        .as_array()
        .is_some_and(|models| {
            models.iter().any(|entry| {
                entry["provider"] == config.provider
                    && entry["id"] == request.model.id
                    && entry["api"] == api_name
                    && entry["baseUrl"].as_str().is_some_and(|base| {
                        ModelRoute::new(config.api, base).is_ok_and(|declared| declared == route)
                    })
            })
        });
    if !exact_profile {
        return Err(EvalError("exact_model_profile_required"));
    }
    let catalog = decode_model_catalog(
        &serde_json::to_vec(&config.model_catalog)
            .map_err(|_| EvalError("provider_profile_invalid"))?,
    )
    .map_err(|_| EvalError("provider_profile_invalid"))?;
    let definition = catalog
        .lookup(
            &route,
            &ModelIdentity {
                provider: config.provider.clone(),
                model: request.model.id.clone(),
            },
        )
        .map_err(|_| EvalError("provider_profile_invalid"))?;
    resolve_model(&definition, None).map_err(|_| EvalError("provider_profile_invalid"))
}

struct Cli {
    preflight: bool,
    provider_config: PathBuf,
    trial_root: Option<PathBuf>,
    run_id: Option<String>,
}

impl Cli {
    fn parse() -> Result<Self, EvalError> {
        let mut arguments = std::env::args().skip(1);
        let mut preflight = false;
        let mut provider_config = None;
        let mut trial_root = None;
        let mut run_id = None;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--preflight" if !preflight => preflight = true,
                "--provider-config" if provider_config.is_none() => {
                    provider_config = Some(PathBuf::from(
                        arguments.next().ok_or(EvalError("invalid_cli"))?,
                    ));
                }
                "--trial-root" if trial_root.is_none() => {
                    trial_root = Some(PathBuf::from(
                        arguments.next().ok_or(EvalError("invalid_cli"))?,
                    ));
                }
                "--run-id" if run_id.is_none() => {
                    run_id = Some(arguments.next().ok_or(EvalError("invalid_cli"))?);
                }
                _ => return Err(EvalError("invalid_cli")),
            }
        }
        if !preflight && (trial_root.is_none() || run_id.is_none()) {
            return Err(EvalError("trial_identity_required"));
        }
        Ok(Self {
            preflight,
            provider_config: provider_config.ok_or(EvalError("provider_config_required"))?,
            trial_root,
            run_id,
        })
    }
}

extern "C" fn signal_cancel(_: libc::c_int) {
    PROCESS_CANCELLED.store(true, Ordering::SeqCst);
}

struct SignalGuard {
    interrupt: libc::sighandler_t,
    terminate: libc::sighandler_t,
}

impl SignalGuard {
    fn install() -> Result<Self, EvalError> {
        // SAFETY: The process-local handlers only store to a static lock-free AtomicBool.
        let interrupt = unsafe {
            libc::signal(
                libc::SIGINT,
                signal_cancel as *const () as libc::sighandler_t,
            )
        };
        if interrupt == libc::SIG_ERR {
            return Err(EvalError("cancellation_handler_unavailable"));
        }
        // SAFETY: The process-local handlers only store to a static lock-free AtomicBool.
        let terminate = unsafe {
            libc::signal(
                libc::SIGTERM,
                signal_cancel as *const () as libc::sighandler_t,
            )
        };
        if terminate == libc::SIG_ERR {
            // SAFETY: Restores the handler value returned by libc::signal in this process.
            unsafe { libc::signal(libc::SIGINT, interrupt) };
            return Err(EvalError("cancellation_handler_unavailable"));
        }
        Ok(Self {
            interrupt,
            terminate,
        })
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        // SAFETY: Restores the exact process-local handler values saved during installation.
        unsafe {
            libc::signal(libc::SIGINT, self.interrupt);
            libc::signal(libc::SIGTERM, self.terminate);
        }
    }
}

async fn cli() -> Result<Value, EvalError> {
    let _signals = SignalGuard::install()?;
    let cli = Cli::parse()?;
    let config: ProviderConfig = serde_json::from_slice(&read_plain(&cli.provider_config)?)
        .map_err(|_| EvalError("provider_config_invalid"))?;
    let mut stdin = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_JSON_BYTES as u64 + 1)
        .read_to_end(&mut stdin)?;
    if stdin.len() > MAX_JSON_BYTES {
        return Err(EvalError("request_too_large"));
    }
    let request: TrialRequest =
        serde_json::from_slice(&stdin).map_err(|_| EvalError("invalid_request_json"))?;
    let model = configured_model(&config, &request)?;
    let pricing = config
        .pricing
        .as_ref()
        .map(|value| {
            PricingCatalog::decode(
                &serde_json::to_vec(value).map_err(|_| EvalError("pricing_invalid"))?,
            )
            .map_err(|_| EvalError("pricing_invalid"))
        })
        .transpose()?;
    let prepared = prepare(&request, &model, pricing.as_ref())?;
    if PROCESS_CANCELLED.load(Ordering::SeqCst) {
        return Err(EvalError("startup_interrupted"));
    }
    if cli.preflight {
        return Ok(json!({
            "schema_version": 1,
            "backend": "vega_native_runtime",
            "status": "ready",
            "transport_verified": false,
            "live_calls": 0,
            "hashes": prepared.hashes,
            "model": request.model,
            "limits": request.limits,
            "pricing_configured": pricing.is_some(),
        }));
    }
    let key = std::env::var(KEY_ENV)
        .ok()
        .filter(|key| !key.trim().is_empty())
        .ok_or(EvalError("dedicated_provider_credential_required"))?;
    let provider = OpenAiProvider::new(&config.base_url, &key)
        .map_err(|_| EvalError("provider_initialization_failed"))?
        .with_responses_api(config.api == ModelApi::Responses)
        .with_model_snapshot(model)
        .map_err(|_| EvalError("provider_profile_invalid"))?
        .with_retry_policy(RetryPolicy {
            max_retries: 0,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(1),
        });
    let cancel = CancellationToken::new();
    if PROCESS_CANCELLED.load(Ordering::SeqCst) {
        cancel.cancel();
    }
    let monitor_done = CancellationToken::new();
    let monitor_cancel = cancel.clone();
    let monitor_stop = monitor_done.clone();
    let monitor = tokio::spawn(async move {
        loop {
            if PROCESS_CANCELLED.load(Ordering::SeqCst) {
                monitor_cancel.cancel();
                break;
            }
            tokio::select! {
                _ = monitor_stop.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    });
    let result = run_trial_inner(
        &provider,
        request,
        cli.trial_root
            .as_deref()
            .ok_or(EvalError("trial_identity_required"))?,
        cli.run_id
            .as_deref()
            .ok_or(EvalError("trial_identity_required"))?,
        pricing,
        cancel,
        &[key],
    )
    .await;
    monitor_done.cancel();
    monitor
        .await
        .map_err(|_| EvalError("cancellation_monitor_failed"))?;
    result
}

pub(super) async fn run_cli() -> i32 {
    match cli().await {
        Ok(value) => {
            let encoded = match serde_json::to_vec(&value) {
                Ok(bytes) => bytes,
                Err(_) => return 1,
            };
            let mut stdout = std::io::stdout().lock();
            if stdout
                .write_all(&encoded)
                .and_then(|_| stdout.write_all(b"\n"))
                .is_ok()
            {
                0
            } else {
                1
            }
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "{error}");
            1
        }
    }
}
