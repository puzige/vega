use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

#[tokio::test]
async fn actor_keeps_busy_and_exports_original_observations_when_sidecar_is_unsafe() {
    for unsafe_destination in [false, true] {
        let root =
            std::env::temp_dir().join(format!("actor-availability-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join("vega.db");
        let thread = ulid::Ulid::generate().to_string();
        let run = ulid::Ulid::generate().to_string();
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
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
        .unwrap();
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
        .unwrap();
        let admission =
            vega_store::thread_admission::try_acquire(store.database_identity(), &thread)
                .unwrap()
                .unwrap();
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
        .unwrap();
        actor
            .event(
                RuntimeEvent::AssistantResponseStarted {
                    response_id: "positive-response".into(),
                    text_start_bytes: 0,
                },
                String::new(),
                None,
            )
            .await
            .unwrap();
        assert!(!root.join("persistence-failures").exists());
        assert!(
            vega_store::persistence_failures::process_save_failures(&store, &thread, &run)
                .is_empty()
        );
        let target = root.join("untouched");
        if unsafe_destination {
            std::fs::create_dir(&target).unwrap();
            symlink(&target, root.join("persistence-failures")).unwrap();
        }
        let (ready, waiting) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let blocker = std::thread::spawn(move || {
            let store = Store::open(path).unwrap();
            store.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
            ready.send(()).unwrap();
            released.recv().unwrap();
            store.conn().execute_batch("ROLLBACK").unwrap();
        });
        waiting.recv().unwrap();
        let error = actor
            .event(
                RuntimeEvent::AssistantResponseStarted {
                    response_id: "blocked-response".into(),
                    text_start_bytes: 0,
                },
                "private-body-marker".into(),
                None,
            )
            .await
            .unwrap_err();
        release.send(()).unwrap();
        blocker.join().unwrap();
        let metrics = error.diagnostic_metrics();
        assert_eq!(metrics.sqlite_code, Some(5));
        assert_eq!(metrics.sqlite_extended_code, Some(5));
        assert_eq!(metrics.runtime_stage, Some("event_pipeline"));
        let encoded = vega_store::run_diagnostics::export_run(&store, &thread, &run).unwrap();
        let export: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        let observations = if unsafe_destination {
            assert_eq!(export["persistence_failures_state"], "failed");
            assert_eq!(
                export["current_process_save_failures"][0]["fault"]["stage"],
                "directory_open"
            );
            assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
            &export["current_process_save_failures"][0]["failure"]["observations"]
        } else {
            assert_eq!(export["persistence_failures_state"], "readable");
            assert!(
                export["current_process_save_failures"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            &export["persistence_failures"][0]["observations"]
        };
        assert!(
            observations
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["operation"] == "begin" && event["succeeded"] == false)
        );
        assert!(!encoded.contains("private-body-marker"));
        assert!(!encoded.contains(root.to_str().unwrap()));
        actor.close().await.unwrap();
        assert_eq!(
            messages::find(store.conn(), &run).unwrap().unwrap().content,
            ""
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
