mod external_grep;
use super::*;
use crate::{MockProvider, ScriptStep};
use std::fs;
use std::sync::Mutex;
use std::time::Duration;
use tempfile::tempdir;

fn tool_config(
    run_mode: RuntimeRunMode,
    permission_mode: RuntimePermissionMode,
    checkpoint_root: PathBuf,
) -> RuntimeToolConfig {
    RuntimeToolConfig::new(
        run_mode,
        permission_mode,
        "project-1".to_string(),
        "thread-1".to_string(),
        checkpoint_root,
        Vec::new(),
    )
}

fn request(history: Vec<ChatMessage>) -> AgentRequest {
    AgentRequest {
        model: "mock".to_string(),
        system_prompt: "Be precise.".to_string(),
        history,
        max_tokens: None,
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::default(),
        pricing_catalog: None,
        reasoning: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}
