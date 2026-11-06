use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

#[tokio::test]
async fn actor_failure_keeps_original_busy_and_records_event_run_and_lock_phase() {
    verify_actor_failure(false).await;
    verify_actor_failure(true).await;
}

async fn verify_actor_failure(unsafe_destination: bool) {
    let root = std::env::temp_dir().join(format!("actor-diagnostic-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).expect("root");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let root = root.canonicalize().expect("canonical root");
    let path = root.join("vega.db");
    let thread = ulid::Ulid::generate().to_string();
    let run = ulid::Ulid::generate().to_string();
    let store = Store::open(&path).expect("store");
    store.migrate().expect("migrate");
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            reasoning_preferences: None,
            id: &thread,
            project_id: "",
            title: "fixture",
            mode: "execute",
            permission_mode: "readonly",
            permission_revision: 0,
            full_access_grant: None,
            model: "fixture",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .expect("thread");
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: run.clone(),
            thread_id: thread.clone(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("message");
    let admission = vega_store::thread_admission::try_acquire(store.database_identity(), &thread)
        .expect("admission")
        .expect("owner");
    let actor = PersistenceActor::start(PersistenceActorStart {
        database_path: path.clone(),
        project_id: String::new(),
        thread_id: thread.clone(),
        message_id: run.clone(),
        model: "fixture".into(),
        is_plan: false,
        next_tool_seq: 1,
        config: PersistenceActorConfig::default().with_thread_admission(admission),
    })
    .await
    .expect("actor");
    let target = root.join("do-not-change");
    if unsafe_destination {
        std::fs::create_dir(&target).expect("target");
        symlink(&target, root.join("persistence-failures")).expect("link");
    }
    let (ready, waiting) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let blocker = std::thread::spawn(move || {
        let conn = vega_store::Store::open(&path).expect("blocker");
        conn.conn()
            .execute_batch("BEGIN IMMEDIATE")
            .expect("writer");
        ready.send(()).expect("ready");
        released.recv().expect("release");
        conn.conn().execute_batch("ROLLBACK").expect("rollback");
    });
    waiting.recv().expect("writer held");
    let error = actor
        .event(
            RuntimeEvent::AssistantResponseStarted {
                response_id: "response-fixture".into(),
                text_start_bytes: 0,
            },
            "private-body-marker".into(),
            None,
        )
        .await
        .expect_err("busy event");
    let metrics = error.diagnostic_metrics();
    assert_eq!(metrics.sqlite_code, Some(5));
    assert_eq!(metrics.sqlite_extended_code, Some(5));
    assert_eq!(metrics.runtime_stage, Some("event_pipeline"));
    if unsafe_destination {
        assert!(
            vega_store::persistence_failures::read_by_run(store.conn(), &thread, &run).is_err()
        );
        assert_eq!(std::fs::read_dir(&target).expect("untouched").count(), 0);
    } else {
        let saved = vega_store::persistence_failures::read_by_run(store.conn(), &thread, &run)
            .expect("saved");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].category, PersistenceCategory::ResponseStarted);
        assert_eq!(saved[0].message_id, run);
        let json = serde_json::to_value(&saved[0]).expect("json");
        assert!(
            json["observations"]
                .as_array()
                .expect("observations")
                .iter()
                .any(|event| event["operation"] == "begin" && event["succeeded"] == false)
        );
        assert!(
            !serde_json::to_string(&json)
                .expect("serialized")
                .contains("private-body-marker")
        );
    }
    release.send(()).expect("release");
    blocker.join().expect("blocker join");
    actor.close().await.expect("close");
    assert_eq!(
        messages::find(store.conn(), &run)
            .expect("message lookup")
            .expect("message")
            .content,
        ""
    );
    drop(store);
    std::fs::remove_dir_all(root).expect("cleanup");
}
