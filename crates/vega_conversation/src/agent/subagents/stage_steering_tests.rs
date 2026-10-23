use super::*;

struct SteeringHost {
    inner: Arc<TestHost>,
    generation: Mutex<Option<String>>,
}

impl SubagentHost for SteeringHost {
    fn admit(&self, admission: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
        *self.generation.lock().expect("generation") = Some(admission.queue_generation.clone());
        self.inner.admit(admission)
    }

    fn event(
        &self,
        child: &str,
        message: &str,
        event: &ConversationEvent,
    ) -> Result<(), VegaError> {
        self.inner.event(child, message, event)
    }

    fn updated(&self, snapshot: SubagentSnapshot) -> Result<(), VegaError> {
        self.inner.updated(snapshot)
    }

    fn finished(&self, snapshot: SubagentSnapshot, complete: bool) -> Result<(), VegaError> {
        self.inner.finished(snapshot, complete)
    }

    fn finish_and_release(
        &self,
        snapshot: SubagentSnapshot,
        complete: bool,
    ) -> BoxFuture<'static, Result<(), VegaError>> {
        self.inner.finish_and_release(snapshot, complete)
    }
}

#[tokio::test]
async fn real_steering_preserves_execution_bound_read_handoff_and_followup() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.workspace.join("artifact.txt"),
        b"steered artifact\n",
    )
    .expect("artifact");
    let gate = Arc::new(Semaphore::new(0));
    let provider = ScriptedProvider::with(vec![
        read_response("before-steering", "artifact.txt"),
        read_response("after-steering", "artifact.txt"),
        checkpoint_response(
            "steered-handoff",
            checkpoint(true, "artifact.txt", "after-steering"),
        ),
        final_response("Retained steering and reviewed the exact artifact."),
    ]);
    provider
        .gates
        .lock()
        .expect("gates")
        .push_back(Some(gate.clone()));
    let host = Arc::new(SteeringHost {
        inner: TestHost::new(true),
        generation: Mutex::new(None),
    });
    let mut launch = fixture.launch(&provider, host.inner.clone());
    launch.host = host.clone();
    launch.settings.owner_credential_reader = Some(Arc::new(|| Ok(Vec::new())));
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let (child,handed,followed,read_message,request) = supervisor.run(async {
        let child = spawn(&hook,&observer,"Prepare the artifact for review.").await;
        provider.request_count(1).await;
        let generation = host.generation.lock().expect("generation").clone().expect("native admission");
        vega_store::pending_messages::insert(fixture.store.conn(), &vega_store::pending_messages::PendingMessageRow {
            id:"actual-steering".into(), thread_id:child.agent_id.clone(), ordinal:0, behavior:"steer".into(), generation,
            payload:json!({"content":"Keep the new user constraint through handoff.","images":[]}).to_string(), created_at:2,
        }).expect("queued steering");
        gate.add_permits(1);
        host.inner.call_finished("steered-handoff").await;
        let handed = bounded(wait(&hook,&child,None)).await;
        let read_message: String = fixture.store.conn().query_row("SELECT message_id FROM tool_calls WHERE id='after-steering'",[],|row|row.get(0)).expect("real Read message");
        let next = followup(&hook,&observer,&child,"Review the saved artifact.").await;
        let followed = bounded(wait(&hook,&next,None)).await;
        let request = provider.requests.lock().expect("requests").last().cloned().expect("followup input");
        (child,handed,followed,read_message,request)
    }.boxed_local(),launch).await;
    assert_ne!(
        read_message, child.message_id,
        "real steering must create its own assistant segment"
    );
    assert_eq!(handed["agents"][0]["ready_for_review"], true);
    assert_eq!(
        handed["agents"][0]["checkpoint"]["message_id"],
        child.message_id
    );
    assert_eq!(followed["agents"][0]["state"], "completed");
    assert!(
        request
            .messages
            .iter()
            .any(|message| message.role == ChatRole::User
                && message.content == "Keep the new user constraint through handoff.")
    );
    let finals: i64 = fixture
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM assistant_responses WHERE message_id=?1 AND kind='final'",
            [read_message],
            |row| row.get(0),
        )
        .expect("runtime handoff metadata");
    assert_eq!(
        finals, 0,
        "runtime handoff cannot fabricate a model final after steering"
    );
}
