use super::*;

#[test]
fn approval_commit_returns_one_durable_runner_capability() {
    let (store, thread_id) = pending_plan();
    let request = PlanReviewRequested {
        thread_id: thread_id.clone(),
        plan_id: "plan".into(),
        action: PlanReviewAction::Approve,
    };
    let refresh = persist_review(&store, &request).expect("approval refresh");
    assert_eq!(refresh.thread.mode, ThreadMode::Execute);
    assert_eq!(refresh.plans[0].status, PlanStatus::Approved);
    let instruction_id = refresh
        .approved_instruction_id
        .expect("approval runner capability");
    let instruction = vega_store::messages::find(store.conn(), &instruction_id)
        .expect("instruction query")
        .expect("durable instruction");
    assert_eq!(instruction.thread_id, thread_id);
    assert_eq!(instruction.role, "user");
    assert_eq!(instruction.kind, "text");
    assert_eq!(instruction.status, "done");

    let replay = persist_review(&store, &request).expect("stale review reload");
    assert_eq!(replay.approved_instruction_id, None);
}

#[test]
fn change_and_abandon_never_schedule_execute_turn() {
    for action in [
        PlanReviewAction::RequestChanges { note: None },
        PlanReviewAction::Abandon { note: None },
    ] {
        let (store, thread_id) = pending_plan();
        let request = PlanReviewRequested {
            thread_id,
            plan_id: "plan".into(),
            action,
        };
        let refresh = persist_review(&store, &request).expect("non-approval refresh");
        assert_eq!(refresh.approved_instruction_id, None);
        assert_eq!(refresh.thread.mode, ThreadMode::Plan);
    }
}

#[test]
fn provider_model_resolution_is_exact_and_unique() {
    let provider = |name: &str, models: &[&str]| vega_store::config::ProviderConfig {
        name: name.into(),
        base_url: "https://provider.invalid/v1".into(),
        models: models.iter().map(|model| (*model).to_string()).collect(),
        key_ref: name.into(),
    };
    let mut config = vega_store::config::AppConfig {
        providers: vec![provider("one", &["model"]), provider("two", &["other"])],
        ..Default::default()
    };
    assert_eq!(
        unique_provider_for_model(&config, "model").map(|provider| provider.name),
        Some("one".into())
    );
    assert!(unique_provider_for_model(&config, "missing").is_none());
    config.providers.push(provider("duplicate", &["model"]));
    assert!(unique_provider_for_model(&config, "model").is_none());
}

#[gpui::test]
async fn active_plan_review_is_deferred_and_cancels_exactly_once(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let (store, thread_id) = pending_plan();
    let thread =
        vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
    let rebuilt_thread = thread.clone();
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let rebuilt_stream = cx.new(|cx| ConversationStream::new(rebuilt_thread, cx));
    let mut controller = AppAgentController::default();
    let (_, cancel) = controller.begin(thread_id.clone(), stream.clone(), None, None);
    let request = PlanReviewRequested {
        thread_id: thread_id.clone(),
        plan_id: "plan".into(),
        action: PlanReviewAction::Approve,
    };
    assert!(controller.queue_review(&rebuilt_stream, &request));
    assert!(cancel.is_cancelled());
    assert!(controller.active.is_some());
    assert_eq!(
        vega_conversation::plans::list_plans(&store, &thread_id).expect("plans before terminal")[0]
            .status,
        PlanStatus::Pending
    );
    assert_eq!(
        controller
            .pending_review
            .as_ref()
            .map(|pending| (pending.stream.clone(), pending.request.clone())),
        Some((rebuilt_stream, request.clone()))
    );
    assert!(!controller.queue_review(&stream, &request));
    controller.active = None;
    let pending = controller.pending_review.take().expect("deferred review");
    assert_eq!(pending.request, request);
    assert!(controller.pending_review.is_none());
    let refresh = persist_review(&store, &pending.request).expect("deferred review commit");
    assert!(refresh.approved_instruction_id.is_some());
    let replay = persist_review(&store, &pending.request).expect("stale replay");
    assert!(replay.approved_instruction_id.is_none());
}

#[test]
fn completion_first_makes_deferred_old_review_stale() {
    let (store, thread_id) = pending_plan();
    insert(
        store.conn(),
        &MessageRow {
            id: "new-plan".into(),
            thread_id: thread_id.clone(),
            seq: 2,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 3,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("new streaming plan");
    complete_plan(store.conn(), &thread_id, "new-plan", "new", 4).expect("new completion wins");
    let request = PlanReviewRequested {
        thread_id,
        plan_id: "plan".into(),
        action: PlanReviewAction::Approve,
    };
    let refresh = persist_review(&store, &request).expect("stale deferred review");
    assert!(refresh.approved_instruction_id.is_none());
    assert_eq!(refresh.plans[0].status, PlanStatus::Abandoned);
    assert_eq!(refresh.plans[1].status, PlanStatus::Pending);
}
