use super::*;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> (Self, Store, String) {
        let root =
            std::env::temp_dir().join(format!("vega-preferences-{}", ulid::Ulid::generate()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let root = root.canonicalize().unwrap();
        let store = Store::open(root.join("state.db")).unwrap();
        store.migrate().unwrap();
        let thread =
            crate::threads::create_standalone_thread(&store, "synthetic", "confirm").unwrap();
        initialize(
            &store,
            &thread.id,
            &[SessionReasoningPreference {
                provider: "synthetic".into(),
                model: "synthetic".into(),
                preference: "provider_default".into(),
            }],
        )
        .unwrap();
        (Self(root), store, thread.id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn defaults_save_reads_committed_preferences_and_writes_yaml_without_database_writer() {
    let (fixture, store, thread) = Fixture::new();
    let writer = Store::open_existing(fixture.0.join("state.db")).unwrap();
    let held = writer.immediate_transaction().unwrap();
    let before = vega_store::write_coordination::recent_observations(store.conn());
    let config = fixture.0.join("config.yaml");
    with_current_preferences(&store, &thread, |preferences| {
        assert_eq!(
            preferences.as_ref().unwrap()[0].preference,
            "provider_default"
        );
        vega_store::yaml_config::begin(&config)
            .unwrap()
            .save_fields(
                &serde_json::json!({"reasoning": {"version": 1, "preferences": preferences}}),
            )
            .unwrap();
    })
    .unwrap();
    assert_eq!(
        vega_store::write_coordination::recent_observations(store.conn()),
        before
    );
    assert!(
        vega_store::reasoning::read_snapshot_from(&config)
            .unwrap()
            .exists
    );
    held.rollback().unwrap();
}

#[test]
fn preference_guard_serializes_edit_with_defaults_operation_without_reserving_writer() {
    let (fixture, store, thread) = Fixture::new();
    let path = fixture.0.join("state.db");
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker_thread = thread.clone();
    let worker_path = path.clone();
    let worker = std::thread::spawn(move || {
        let store = Store::open_existing(worker_path).unwrap();
        with_current_preferences(&store, &worker_thread, |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        })
        .unwrap();
    });
    entered_rx.recv().unwrap();
    let writer = Store::open_existing(&path).unwrap();
    let held = writer.immediate_transaction().unwrap();
    held.rollback().unwrap();
    let worker_thread = thread.clone();
    let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
    let (saved_tx, saved_rx) = std::sync::mpsc::channel();
    let editor = std::thread::spawn(move || {
        let store = Store::open_existing(path).unwrap();
        attempt_tx.send(()).unwrap();
        let mut candidate = ReasoningProfileProjection::unknown("synthetic", "synthetic");
        candidate.protocol = crate::types::ReasoningProtocol::OpenAiChatCompletions;
        let saved = save(&store, &worker_thread, &candidate).unwrap();
        saved_tx.send(saved).unwrap();
    });
    attempt_rx.recv().unwrap();
    assert!(matches!(
        saved_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    let saved = saved_rx.recv().unwrap();
    editor.join().unwrap();
    assert_eq!(
        with_current_preferences(&store, &thread, |preferences| preferences.unwrap()).unwrap(),
        saved
    );
}
