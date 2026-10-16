use super::*;
use crate::types::{
    ContextAccountingRecord, ContextAccountingSource, ContextAccountingStage,
    ContextCompactionStatus,
};
use futures::StreamExt;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use vega_runtime::{ChatMessage, ChatRequest, ChatRole, EventStream, McpReadyServer, Provider};
use vega_store::context_compaction::{
    self as context, ContextCheckpoint, ContextCheckpointInstall, ModelContextPolicy,
    NewContextCheckpoint,
};

const PROVIDER: &str = "s18-owned-provider";
const MODEL: &str = "mock-model";
const TASK: &str = "Review the current owned change using its approved Skill.";
const BODY_MARKER: &str = "S18_PRIVATE_FROZEN_GUIDANCE";
const CHANGED_MARKER: &str = "S18_CHANGED_EXTERNAL_GUIDANCE";
const REFERENCE_MARKER: &str = "S18_PRIVATE_LOWER_TRUST_REFERENCE";
const INPUT_BUDGET: u64 = 300_000;
const OUTPUT_RESERVE: u64 = 128_000;
const TRIGGER: u64 = 240_000;

type DurableRows = BTreeMap<String, BTreeMap<i64, String>>;

struct Fixture {
    store: Option<Store>,
    project: tempfile::TempDir,
    project_id: String,
    tools: vega_tools::Tools,
    permission: FixedPermissionHook,
    dispatches: Arc<AtomicUsize>,
    _endpoint: vega_mcp::mock::Endpoint,
    mcp: McpReadyServer,
    methods: Arc<Mutex<Vec<String>>>,
    discovery_methods: Vec<String>,
    predecessor: ContextCheckpoint,
    old_rows: DurableRows,
    catalog: String,
    envelope: String,
    reference: String,
    approved_sha: String,
}

fn saved_policy(input: u64, output: u64) -> ModelContextPolicy {
    ModelContextPolicy {
        provider: PROVIDER.into(),
        model: MODEL.into(),
        input_limit: Some(input),
        output_reserve: Some(output),
        automatic_compaction: true,
        updated_at: 10,
    }
}

fn wire_tokens(request: &ChatRequest) -> u64 {
    vega_runtime::estimate_wire_context(&request.messages, &request.tools)
        .unwrap()
        .input_tokens
}

fn durable_rows(store: &Store, include_run: bool) -> DurableRows {
    let mut tables = vec![
        "messages",
        "tool_calls",
        "image_attachments",
        "context_checkpoints",
    ];
    if include_run {
        tables.extend([
            "context_compaction_status",
            "token_usage",
            "skill_run_snapshots",
            "skill_activation_audits",
        ]);
    }
    tables
        .into_iter()
        .map(|table| {
            let columns = {
                let mut statement = store
                    .conn()
                    .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
                    .unwrap();
                statement
                    .query_map([table], |row| row.get::<_, String>(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            assert!(!columns.is_empty(), "owned table exists");
            let fields = columns
                .iter()
                .map(|column| {
                    let identifier = format!("\"{}\"", column.replace('"', "\"\""));
                    let key = column.replace('\'', "''");
                    format!(
                        "'{key}', CASE WHEN typeof({identifier}) = 'blob' THEN json_object('blob', hex({identifier})) ELSE {identifier} END"
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let scope = if table == "image_attachments" {
                "message_id IN (SELECT id FROM messages WHERE thread_id = 'thread-1')"
            } else {
                "thread_id = 'thread-1'"
            };
            let query = format!(
                "SELECT rowid, json_object({fields}) FROM {table} WHERE {scope} ORDER BY rowid"
            );
            let mut statement = store.conn().prepare(&query).unwrap();
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
                .unwrap()
                .collect::<Result<BTreeMap<_, _>, _>>()
                .unwrap();
            (table.to_owned(), rows)
        })
        .collect()
}

fn insert_history(store: &Store, seq: i64, content: &str) {
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: format!("s18-history-{seq}"),
            thread_id: "thread-1".into(),
            seq,
            role: if seq % 2 == 1 { "user" } else { "assistant" }.into(),
            kind: "text".into(),
            content: content.into(),
            status: "done".into(),
            created_at: seq,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
}

fn create_thread(store: &Store, project_id: &str, thread_id: &str) {
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: thread_id,
            project_id,
            title: "",
            mode: "execute",
            permission_mode: "confirm",
            model: MODEL,
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
}

fn schema_endpoint() -> (vega_mcp::mock::Endpoint, Arc<Mutex<Vec<String>>>) {
    let tools = serde_json::json!([{
        "name": "owned_lookup",
        "description": "S18 owned schema contribution. ".repeat(48),
        "inputSchema": {
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"],
            "additionalProperties": false
        }
    }]);
    let methods = Arc::new(Mutex::new(Vec::new()));
    let captured = methods.clone();
    let endpoint = vega_mcp::mock::Endpoint::new("/mcp", move |_| {
        Arc::new(move |request| {
            let body = request.body().and_then(|body| body.as_bytes()).unwrap();
            let request: serde_json::Value = serde_json::from_slice(body).unwrap();
            let method = request["method"].as_str().unwrap();
            assert!(matches!(method, "server/discover" | "tools/list"));
            captured.lock().unwrap().push(method.to_owned());
            let reply = vega_mcp::mock::catalog_reply(&request, &tools, "unused")
                .unwrap()
                .to_string();
            Box::pin(async move {
                Ok(vega_mcp::mock::response(
                    200,
                    "application/json",
                    reply,
                    Vec::new(),
                ))
            })
        })
    });
    (endpoint, methods)
}

impl Fixture {
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }

    async fn new() -> Self {
        let (store, project, project_id) = setup();
        let root = project.path().join(".agents/skills");
        let body = format!("{BODY_MARKER}\n").repeat(3072);
        add_skill(&root, "reviewer", &body);
        fs::create_dir_all(root.join("reviewer/references")).unwrap();
        fs::write(
            root.join("reviewer/references/notes.md"),
            format!("{REFERENCE_MARKER}\n").repeat(64),
        )
        .unwrap();
        let source = SkillSource::project_approved(project.path())
            .unwrap()
            .unwrap();
        let settings = skills::read_settings(store.conn()).unwrap();
        assert!(!settings.global_enabled);
        skills::set_project_settings(
            store.conn(),
            settings.consent_generation,
            &project_id,
            true,
            true,
        )
        .unwrap();
        let (_, approved_sha) = approve(
            &store,
            &source,
            "s18-owned-project",
            Some(&project_id),
            &root,
            true,
        );
        context::save_model_policy(store.conn(), &saved_policy(INPUT_BUDGET, OUTPUT_RESERVE))
            .unwrap();
        let mut prepared = crate::agent::skills::prepare_skill_run_with_config_dir(
            &store,
            &project_id,
            "thread-1",
            "s18-owned-envelope-probe",
            true,
            Some(project.path()),
        )
        .unwrap()
        .unwrap();
        let catalog = prepared.run.model_catalog().to_owned();
        assert_eq!(
            prepared.run.load_model("reviewer", |_| true).audit.status,
            "loaded"
        );
        let envelope = prepared.run.render_skill_envelope().unwrap();
        let reference = prepared
            .run
            .read_reference("reviewer", "references/notes.md", |_| true)
            .unwrap()
            .to_json()
            .unwrap();
        drop(prepared);

        insert_history(&store, 1, "S18 small historical user image evidence.");
        insert_history(&store, 2, "S18 completed historical read.");
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: "s18-historical-read",
                thread_id: "thread-1",
                message_id: "s18-history-2",
                seq: 1,
                tool: "read",
                input_json: r#"{"path":"lib.rs"}"#,
                status: "success",
                created_at: 2,
            },
        )
        .unwrap();
        let approval = ApprovalAudit {
            decision: Approval::Once,
            note: None,
            source: ApprovalSource::ReadonlyTool,
            danger: None,
        }
        .to_json()
        .unwrap();
        tool_calls::update(
            store.conn(),
            "s18-historical-read",
            "success",
            Some(&approval),
            Some("S18 historical read bytes."),
            Some(3),
        )
        .unwrap();
        let mut png = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(4, 3, image::Rgb([20, 40, 60]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        vega_store::image_attachments::insert(store.conn(), "s18-history-1", 0, png.get_ref())
            .unwrap();
        let source = context::load_source(store.conn(), "thread-1").unwrap();
        let predecessor = match context::install_checkpoint(
            store.conn(),
            &NewContextCheckpoint {
                thread_id: "thread-1".into(),
                model: MODEL.into(),
                source_version: source.source_version,
                covered_through_seq: 2,
                covered_assistant_id: None,
                covered_assistant_text_bytes: 0,
                covered_assistant_tool_seq: 0,
                source_fingerprint: source.fingerprint,
                summary: "S18 prior checkpoint fixture.".into(),
                estimator_version: vega_runtime::CONTEXT_ESTIMATOR_VERSION.into(),
                expected_previous_id: None,
                created_at: 4,
            },
        )
        .unwrap()
        {
            ContextCheckpointInstall::Applied(checkpoint) => checkpoint,
            _ => panic!("owned predecessor installation"),
        };
        insert_history(&store, 3, "S18 completed earlier user task.");
        insert_history(&store, 4, "");

        let dispatches = Arc::new(AtomicUsize::new(0));
        let capture_dispatch = dispatches.clone();
        let tools = vega_tools::Tools::new(project.path())
            .unwrap()
            .with_bash_test_executor(Arc::new(move |_, _, _| {
                capture_dispatch.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { panic!("S18 has no Bash execution") })
            }));
        let permission = FixedPermissionHook {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: PermissionDecision::Deny { note: None },
        };
        let (endpoint, methods) = schema_endpoint();
        let client = vega_mcp::HttpClient::connect(&endpoint, true)
            .await
            .unwrap();
        let mcp = McpReadyServer::connect_http("01K5KK7PZ5J8V2GSBMQKS8W71B".into(), 17, client)
            .await
            .unwrap()
            .with_server_display_name("S18 owned schema".into())
            .unwrap();
        let discovery_methods = methods.lock().unwrap().clone();
        assert_eq!(discovery_methods, ["server/discover", "tools/list"]);
        create_thread(&store, &project_id, "thread-s18-calibration");
        let calibration = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let run = run_thread_task_with_images_reasoning_and_mcp(
            &store,
            &calibration,
            &tools,
            "thread-s18-calibration",
            "S18 calibration only.",
            "System",
            CancellationToken::new(),
            &permission,
            |_| Ok(()),
            PersistenceActorConfig::default(),
            None,
            None,
            Some(FrozenReasoning::unknown(PROVIDER, MODEL)),
            Vec::new(),
            vec![mcp.clone()],
        )
        .await
        .unwrap();
        assert!(
            !run.failed && !run.interrupted,
            "owned calibration succeeds"
        );
        let requests = calibration.requests();
        assert_eq!(requests.len(), 1, "owned calibration request count");
        let template = &requests[0];
        assert!(template.messages[0].content.contains(&catalog));
        assert!(!template.messages[0].content.contains(BODY_MARKER));
        assert_eq!(
            template
                .tools
                .iter()
                .filter(|tool| tool.name.starts_with("mcp_"))
                .count(),
            1
        );
        let original = context::load_source(store.conn(), "thread-1").unwrap();
        let estimate_length = |length: usize| {
            let mut source = original.clone();
            source
                .messages
                .iter_mut()
                .find(|row| row.seq == 4)
                .unwrap()
                .content = "h".repeat(length);
            let history =
                crate::agent::pipeline::primary_history_from_context_source_with_checkpoint(
                    &source,
                    Some(&predecessor),
                    "s18-calibration-unpersisted-owner",
                )
                .unwrap();
            let mut request = template.clone();
            request.messages = std::iter::once(template.messages[0].clone())
                .chain(history)
                .chain(std::iter::once(ChatMessage::new(ChatRole::User, TASK)))
                .collect();
            wire_tokens(&request)
        };
        let mut lower = 0;
        let mut upper = 1_200_000;
        assert!(estimate_length(upper) > 225_000, "calibration upper bound");
        while lower < upper {
            let middle = lower + (upper - lower) / 2;
            if estimate_length(middle) < 225_000 {
                lower = middle + 1;
            } else {
                upper = middle;
            }
        }
        assert!(lower > 0, "calibration archive must be nonempty");
        assert!(
            estimate_length(lower) < TRIGGER,
            "calibration starts below trigger"
        );
        let archive = "h".repeat(lower);
        store
            .conn()
            .execute(
                "UPDATE messages SET content = ?1 WHERE id = 's18-history-4'",
                [&archive],
            )
            .unwrap();
        let old_rows = durable_rows(&store, false);
        Self {
            store: Some(store),
            project,
            project_id,
            tools,
            permission,
            dispatches,
            _endpoint: endpoint,
            mcp,
            methods,
            discovery_methods,
            predecessor,
            old_rows,
            catalog,
            envelope,
            reference,
            approved_sha,
        }
    }

    async fn run(
        &self,
        provider: &dyn Provider,
        cancel: CancellationToken,
        change_policy: bool,
    ) -> ConversationRun {
        let skill_file = self.project.path().join(".agents/skills/reviewer/SKILL.md");
        run_thread_task_with_images_reasoning_and_mcp(
            self.store(),
            provider,
            &self.tools,
            "thread-1",
            TASK,
            "System",
            cancel,
            &self.permission,
            |event| {
                if matches!(event, ConversationEvent::SkillActivated { .. }) {
                    fs::write(&skill_file, CHANGED_MARKER).unwrap();
                    if change_policy {
                        context::save_model_policy(
                            self.store().conn(),
                            &saved_policy(400_000, 96_000),
                        )
                        .unwrap();
                    }
                }
                Ok(())
            },
            PersistenceActorConfig::default(),
            None,
            None,
            Some(FrozenReasoning::unknown(PROVIDER, MODEL)),
            Vec::new(),
            vec![self.mcp.clone()],
        )
        .await
        .unwrap()
    }

    fn assert_no_dispatch(&self) {
        assert_eq!(self.permission.calls.load(Ordering::SeqCst), 0);
        assert_eq!(self.dispatches.load(Ordering::SeqCst), 0);
        assert_eq!(*self.methods.lock().unwrap(), self.discovery_methods);
    }

    fn reopen(&mut self) {
        let database = self.store().database_path().unwrap().to_path_buf();
        drop(self.store.take().unwrap());
        fs::remove_file(self.project.path().join(".agents/skills/reviewer/SKILL.md")).unwrap();
        fs::remove_file(
            self.project
                .path()
                .join(".agents/skills/reviewer/references/notes.md"),
        )
        .unwrap();
        self.store = Some(Store::open(database).unwrap());
    }
}

fn accounting(run: &ConversationRun) -> Vec<ContextAccountingRecord> {
    run.events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ContextAccounting { record, .. } => Some(*record),
            _ => None,
        })
        .collect()
}

struct CancelSummaryProvider {
    inner: MockProvider,
    waiting: CancellationToken,
}

impl Provider for CancelSummaryProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let summary = request.tools.is_empty();
        let future = self.inner.chat_stream(request, cancel.clone());
        let waiting = self.waiting.clone();
        Box::pin(async move {
            let stream = future.await?;
            if !summary {
                return Ok(stream);
            }
            let gate = futures::stream::once(async move {
                waiting.cancel();
                cancel.cancelled().await;
                Err(VegaError::Cancelled)
            });
            Ok(Box::pin(stream.chain(gate)) as EventStream)
        })
    }
}
