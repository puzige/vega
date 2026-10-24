use super::*;
use vega_store::subagents::DelegationRow;

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) struct Fixture {
    pub store: Store,
    pub database: PathBuf,
    tools: vega_tools::Tools,
    _directory: Directory,
}

impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-delegation-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let directory = Directory(root);
        let database = directory.0.join("isolated.sqlite");
        let fixture = Self {
            store: Store::open(&database).unwrap(),
            database,
            tools: vega_tools::Tools::new(&directory.0).unwrap(),
            _directory: directory,
        };
        fixture.store.migrate().unwrap();
        fixture.parent("parent");
        fixture
    }

    fn thread(&self, id: &str) {
        vega_store::threads::create_standalone(
            self.store.conn(),
            vega_store::threads::NewThread {
                id,
                project_id: "",
                title: id,
                mode: "ask",
                permission_mode: "readonly",
                model: "isolated-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
    }

    pub fn message(&self, thread: &str, id: &str, role: &str, status: &str) {
        messages::insert(
            self.store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: thread.into(),
                seq: messages::next_seq(self.store.conn(), thread).unwrap(),
                role: role.into(),
                kind: "text".into(),
                content: format!("Saved content for {id}"),
                status: status.into(),
                created_at: 2,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }

    pub fn parent(&self, id: &str) {
        self.thread(id);
        self.message(id, &format!("{id}-user"), "user", "done");
        self.message(id, &format!("{id}-run"), "assistant", "done");
    }

    pub fn child(&self, parent: &str, id: &str, state: &str) -> DelegationRow {
        self.thread(id);
        self.message(id, &format!("{id}-instruction"), "user", "done");
        let message = format!("{id}-result");
        let status = match state {
            "completed" => "done",
            "failed" => "failed",
            "interrupted" => "interrupted",
            _ => "streaming",
        };
        self.message(id, &message, "assistant", status);
        let row = DelegationRow {
            child_thread_id: id.into(),
            parent_thread_id: parent.into(),
            parent_message_id: format!("{parent}-run"),
            spawn_call_id: format!("spawn-{id}"),
            child_message_id: message,
            name: id.into(),
            task: format!("Task for {id}"),
            state: state.into(),
            result: format!("Original {state} result for {id}"),
            truncated: state == "failed",
            created_at: 3,
        };
        let tx = self.store.immediate_transaction().unwrap();
        vega_store::subagents::insert(&tx, &row).unwrap();
        let usage = token_usage::insert(
            &tx,
            token_usage::NewTokenUsage {
                thread_id: id,
                message_id: Some(&row.child_message_id),
                model: "isolated-model",
                input_tokens: 123,
                output_tokens: 45,
                cache_read_tokens: 6,
                cache_write_tokens: 7,
                cost_microcents: 89,
                created_at: 4,
                pricing_version: Some(token_usage::PRICED_VERSION),
                pricing_profile: Some("base"),
                call_started_at: Some(1),
            },
        )
        .unwrap();
        vega_store::subagents::attribute_usage(
            &tx,
            usage,
            id,
            parent,
            &row.parent_message_id,
            state == "completed",
        )
        .unwrap();
        vega_store::subagents::mark_usage_complete(
            &tx,
            id,
            &row.child_message_id,
            state == "completed",
        )
        .unwrap();
        tx.commit().unwrap();
        row
    }

    pub fn launch(&self, run: &str) -> ChildLaunch<'_> {
        ChildLaunch {
            provider: &NoExecution,
            tools: &self.tools,
            database: self.database.clone(),
            parent_thread: "parent".into(),
            parent_message: run.into(),
            settings: ChildSettings {
                model: "isolated-model".into(),
                system_prompt: String::new(),
                authority: DelegatedAuthority {
                    run_mode: RuntimeRunMode::Ask,
                    permission_mode: RuntimePermissionMode::ReadOnly,
                    exact_rules: Vec::new(),
                },
                pricing_catalog: None,
                reasoning: None,
                context_budget: None,
                automation_claim: None,
                automation_hook: None,
                owner_credential_reader: None,
                turn_limit: 1,
                steering_source: None,
            },
            mcp_servers: Vec::new(),
            host: Arc::new(NoExecution),
            cancel: CancellationToken::new(),
        }
    }
}

struct NoExecution;

impl Provider for NoExecution {
    fn chat_stream(
        &self,
        _: vega_runtime::ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        panic!("recovery must not invoke a provider")
    }
}

impl SubagentHost for NoExecution {
    fn admit(&self, _: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
        panic!("reservation must not launch a child")
    }

    fn event(&self, _: &str, _: &str, _: &ConversationEvent) -> Result<(), VegaError> {
        panic!("recovery must not emit execution events")
    }

    fn updated(&self, _: SubagentSnapshot) -> Result<(), VegaError> {
        panic!("terminal cancellation must not update the host")
    }

    fn finished(&self, _: SubagentSnapshot, _: bool) -> Result<(), VegaError> {
        panic!("recovery must not finish a live host")
    }
}

pub(super) fn rows(store: &Store, table: &str) -> Vec<Vec<String>> {
    let mut query = store
        .conn()
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .unwrap();
    let columns = query.column_count();
    query
        .query_map([], |row| {
            (0..columns)
                .map(|index| row.get_ref(index).map(|value| format!("{value:?}")))
                .collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

pub(super) fn history(store: &Store) -> Vec<Vec<Vec<String>>> {
    [
        "threads",
        "messages",
        "delegated_runs",
        "delegated_executions",
        "token_usage",
        "tool_calls",
    ]
    .map(|table| rows(store, table))
    .into()
}

pub(super) fn recovery_rows(store: &Store) -> Vec<Vec<Vec<String>>> {
    [
        "delegated_execution_recovery",
        "delegated_execution_messages",
        "delegated_stage_checkpoints",
        "delegated_read_receipts",
    ]
    .map(|table| rows(store, table))
    .into()
}

pub(super) fn assert_failure<T>(result: Result<T, VegaError>, expected: &str) {
    match result {
        Err(VegaError::Tool { tool, message }) => {
            assert_eq!(tool, "subagent");
            assert_eq!(message, expected);
        }
        _ => panic!("expected subagent failure: {expected}"),
    }
}

pub(super) fn spawn(
    supervisor: &Supervisor,
    launch: &ChildLaunch<'_>,
    name: &str,
) -> Result<ReservedExecution, VegaError> {
    supervisor.reserve(
        launch,
        &format!("call-{name}"),
        "spawn_agent",
        &json!({"name": name, "task": "Read only"}),
    )
}
