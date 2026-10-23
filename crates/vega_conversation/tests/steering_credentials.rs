use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use vega_conversation::pending_messages::QueueSteeringSource;
use vega_conversation::privacy;
use vega_runtime::{CredentialReader, RuntimeSteeringSource, VegaError};
use vega_store::{Store, keystore, pending_messages as rows};

const THREAD: &str = "thread";
const GENERATION: &str = "generation";
const SECRET: &str = "synthetic-owner-value";
const MARKER: &str = "[REDACTED:provider_credential]";

struct Fixture {
    directory: PathBuf,
    database: PathBuf,
    root: PathBuf,
    workspace: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("vega-steering-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self {
            database: directory.join("store.sqlite"),
            root: directory.join("config"),
            workspace: directory.join("workspace"),
            directory,
        };
        std::fs::create_dir(&fixture.workspace).unwrap();
        let store = fixture.store();
        store.migrate().unwrap();
        for id in [THREAD, "other-thread"] {
            vega_store::threads::create_standalone(
                store.conn(),
                vega_store::threads::NewThread {
                    id,
                    project_id: "",
                    title: "Synthetic steering test",
                    mode: "execute",
                    permission_mode: "confirm",
                    model: "synthetic-model",
                    status: "active",
                    pinned: false,
                    unread: false,
                    created_at: 1,
                    updated_at: 1,
                },
            )
            .unwrap();
        }
        keystore::set_key(&fixture.root, "provider", SECRET).unwrap();
        privacy::refresh_authority(&fixture.database, &fixture.root).unwrap();
        fixture
    }

    fn store(&self) -> Store {
        Store::open(&self.database).unwrap()
    }

    fn insert(&self, id: &str, behavior: &str, content: &str, images: Vec<Vec<u8>>) {
        rows::insert(
            self.store().conn(),
            &rows::PendingMessageRow {
                id: id.into(),
                thread_id: THREAD.into(),
                ordinal: 0,
                behavior: behavior.into(),
                generation: GENERATION.into(),
                payload: serde_json::json!({"content":content,"images":images}).to_string(),
                created_at: 1,
            },
        )
        .unwrap();
    }

    fn source(&self, reader: CredentialReader, cancel: CancellationToken) -> QueueSteeringSource {
        QueueSteeringSource::new(
            self.database.clone(),
            self.workspace.clone(),
            THREAD.into(),
            GENERATION.into(),
            reader,
            cancel,
        )
    }

    fn authority_reader(&self, calls: Arc<AtomicUsize>) -> CredentialReader {
        let database = self.database.clone();
        let root = self.root.clone();
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            privacy::refresh_authority(&database, &root).map_err(|_| ())?;
            keystore::get_key(&root, "provider")
                .map(|key| vec![key])
                .map_err(|_| ())
        })
    }

    fn state(&self, id: &str) -> Option<String> {
        self.store()
            .conn()
            .query_row(
                "SELECT (SELECT state FROM pending_messages WHERE id=?1)",
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn epoch(&self) -> String {
        self.store()
            .conn()
            .query_row(
                "SELECT digest FROM credential_privacy_epoch WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[tokio::test]
async fn unchanged_authority_delivers_redacted_content_once() {
    let fixture = Fixture::new();
    fixture.insert(
        "candidate",
        "steer",
        &format!("retain {SECRET} suffix"),
        vec![],
    );
    let epoch = fixture.epoch();
    let calls = Arc::new(AtomicUsize::new(0));
    let source = fixture.source(
        fixture.authority_reader(calls.clone()),
        CancellationToken::new(),
    );

    let delivered = source.take().await.unwrap().unwrap();
    assert_eq!(delivered.id, "candidate");
    assert_eq!(
        delivered.original_content,
        format!("retain {SECRET} suffix")
    );
    assert_eq!(delivered.message.content, format!("retain {MARKER} suffix"));
    assert_eq!(delivered.user_message_seq, 1);
    assert!(!delivered.assistant_message_id.is_empty());
    assert_eq!(fixture.state("candidate").as_deref(), Some("claimed"));
    assert!(source.take().await.unwrap().is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.epoch(), epoch);
}

#[tokio::test]
async fn rotated_authority_commits_refresh_before_claiming() {
    let fixture = Fixture::new();
    let epoch = fixture.epoch();
    let rotated = "synthetic-rotated-owner-value";
    fixture.insert(
        "candidate",
        "steer",
        &format!("retain {rotated} suffix"),
        vec![],
    );
    vega_store::credential_privacy::allow(fixture.store().conn(), THREAD, "rule", "fingerprint")
        .unwrap();
    keystore::set_key(&fixture.root, "provider", rotated).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let source = fixture.source(
        fixture.authority_reader(calls.clone()),
        CancellationToken::new(),
    );

    let delivered = source.take().await.unwrap().unwrap();
    assert_eq!(delivered.message.content, format!("retain {MARKER} suffix"));
    assert_ne!(fixture.epoch(), epoch);
    assert!(
        vega_store::credential_privacy::exceptions(fixture.store().conn(), THREAD)
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.state("candidate").as_deref(), Some("claimed"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stale_candidates_are_not_claimed_or_replaced() {
    let changes = [
        "DELETE FROM pending_messages WHERE id='candidate'",
        "UPDATE pending_messages SET payload='{\"content\":\"replacement\",\"images\":[]}' WHERE id='candidate'",
        "UPDATE pending_messages SET generation='new-generation' WHERE id='candidate'",
        "UPDATE pending_messages SET state='claimed' WHERE id='candidate'",
        "UPDATE pending_messages SET behavior='steer' WHERE id='earlier'",
        "UPDATE pending_messages SET ordinal=10 WHERE id='candidate'",
        "UPDATE pending_messages SET behavior='queue' WHERE id='candidate'",
        "UPDATE pending_messages SET created_at=2 WHERE id='candidate'",
        "UPDATE pending_messages SET thread_id='other-thread' WHERE id='candidate'",
        "UPDATE pending_messages SET id='replacement' WHERE id='candidate'",
    ];
    for sql in changes {
        let fixture = Fixture::new();
        fixture.insert("earlier", "queue", "earlier content", vec![]);
        fixture.insert("candidate", "steer", "original content", vec![]);
        fixture.insert("later", "steer", "later content", vec![]);
        let database = fixture.database.clone();
        let expected = Arc::new(Mutex::new(None));
        let captured = expected.clone();
        let reader = Arc::new(move || {
            let store = Store::open(&database).unwrap();
            let tx = store.immediate_transaction().unwrap();
            tx.execute_batch(sql).unwrap();
            tx.commit().unwrap();
            *captured.lock().unwrap() = Some(queue_snapshot(&store));
            Ok(vec![])
        });
        let source = fixture.source(reader, CancellationToken::new());

        assert!(source.take().await.unwrap().is_none(), "{sql}");
        assert_eq!(
            queue_snapshot(&fixture.store()),
            expected.lock().unwrap().take().unwrap(),
            "{sql}"
        );
        assert_eq!(fixture.state("later").as_deref(), Some("pending"));
    }
}

fn queue_snapshot(store: &Store) -> Vec<String> {
    store
        .conn()
        .prepare("SELECT json_array(id,thread_id,ordinal,behavior,generation,payload,state,created_at) FROM pending_messages ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[tokio::test]
async fn concurrent_consumers_deliver_one_candidate_once() {
    let fixture = Fixture::new();
    fixture.insert("candidate", "steer", "original content", vec![]);
    let (ready_sender, ready_receiver) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let reader_gate = gate.clone();
    let reader = Arc::new(move || {
        ready_sender.send(()).map_err(|_| ())?;
        let (open, changed) = &*reader_gate;
        let (open, _) = changed
            .wait_timeout_while(
                open.lock().map_err(|_| ())?,
                Duration::from_secs(5),
                |open| !*open,
            )
            .map_err(|_| ())?;
        if !*open {
            return Err(());
        }
        Ok(vec![])
    });
    let source = Arc::new(fixture.source(reader, CancellationToken::new()));
    let first_source = source.clone();
    let first = tokio::spawn(async move { first_source.take().await });
    let second = tokio::spawn(async move { source.take().await });
    let both_prepared = tokio::task::spawn_blocking(move || {
        ready_receiver.recv_timeout(Duration::from_secs(5)).is_ok()
            && ready_receiver.recv_timeout(Duration::from_secs(5)).is_ok()
    })
    .await
    .unwrap();
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let results = [
        first.await.unwrap().unwrap(),
        second.await.unwrap().unwrap(),
    ];

    assert!(both_prepared);
    assert_eq!(results.iter().filter(|result| result.is_some()).count(), 1);
    assert_eq!(fixture.state("candidate").as_deref(), Some("claimed"));
}

#[tokio::test]
async fn empty_and_cancelled_sources_skip_credential_reads() {
    let fixture = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let reader_calls = calls.clone();
    let reader: CredentialReader = Arc::new(move || {
        reader_calls.fetch_add(1, Ordering::SeqCst);
        Err(())
    });
    let empty = fixture.source(reader.clone(), CancellationToken::new());
    assert!(empty.take().await.unwrap().is_none());
    fixture.insert("candidate", "steer", "original content", vec![]);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let cancelled = fixture.source(reader, cancel);
    assert!(cancelled.take().await.unwrap().is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.state("candidate").as_deref(), Some("pending"));
}

#[tokio::test]
async fn credential_read_failure_preserves_pending_message() {
    let fixture = Fixture::new();
    fixture.insert("candidate", "steer", "original content", vec![]);
    let root = fixture.root.clone();
    let reader = Arc::new(move || {
        keystore::get_key(&root, "missing-reference")
            .map(|key| vec![key])
            .map_err(|_| ())
    });
    let source = fixture.source(reader, CancellationToken::new());

    assert!(matches!(
        source.take().await,
        Err(VegaError::CredentialCheckUnavailable)
    ));
    assert_eq!(fixture.state("candidate").as_deref(), Some("pending"));
}

#[tokio::test]
async fn cancellation_during_preparation_preserves_pending_message() {
    let fixture = Fixture::new();
    fixture.insert("candidate", "steer", "original content", vec![]);
    let cancel = CancellationToken::new();
    let reader_cancel = cancel.clone();
    let reader = Arc::new(move || {
        reader_cancel.cancel();
        Ok(vec![])
    });
    let source = fixture.source(reader, cancel);

    assert!(source.take().await.unwrap().is_none());
    assert_eq!(fixture.state("candidate").as_deref(), Some("pending"));
}

fn claim_permits(store: &Store) -> usize {
    vega_store::write_coordination::recent_observations(store.conn())
        .iter()
        .filter(|observation| {
            let value = serde_json::to_value(observation).unwrap();
            value["category"] == "runtime_event" && value["operation"] == "permit_acquired"
        })
        .count()
}

#[tokio::test]
async fn cancellation_while_begin_waits_for_sqlite_writer_prevents_claim() {
    let fixture = Fixture::new();
    fixture.insert("candidate", "steer", "original content", vec![]);
    let observer = fixture.store();
    let before = claim_permits(&observer);
    let writer = vega_store::Connection::open(&fixture.database).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let cancel = CancellationToken::new();
    let source = fixture.source(Arc::new(|| Ok(vec![])), cancel.clone());
    let taking = tokio::spawn(async move { source.take().await });
    let observed = tokio::time::timeout(Duration::from_secs(5), async {
        while claim_permits(&observer) == before {
            tokio::task::yield_now().await;
        }
    })
    .await;
    cancel.cancel();
    writer.execute_batch("ROLLBACK").unwrap();
    let result = taking.await.unwrap();

    assert!(observed.is_ok());
    assert!(result.unwrap().is_none());
    assert_eq!(fixture.state("candidate").as_deref(), Some("pending"));
}

#[tokio::test]
async fn references_images_and_sequence_are_preserved_with_redaction() {
    let fixture = Fixture::new();
    std::fs::write(fixture.workspace.join("note.txt"), format!("note {SECRET}")).unwrap();
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let image = bytes.into_inner();
    let content = format!("read @note.txt then {SECRET}");
    fixture.insert("candidate", "steer", &content, vec![image.clone()]);
    fixture.store().conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('previous',?1,7,'user','previous','done',1)", [THREAD]).unwrap();
    let source = fixture.source(
        fixture.authority_reader(Arc::new(AtomicUsize::new(0))),
        CancellationToken::new(),
    );

    let delivered = source.take().await.unwrap().unwrap();
    assert_eq!(delivered.original_content, content);
    assert_eq!(
        delivered.message.content,
        format!("[@note.txt]\nnote {MARKER}\n[/note.txt]\n\nread @note.txt then {MARKER}")
    );
    assert_eq!(delivered.message.images.len(), 1);
    assert_eq!(delivered.message.images[0].bytes(), image);
    assert_eq!(delivered.user_message_seq, 8);
}

#[tokio::test]
async fn invalid_reference_or_image_does_not_claim_or_read_credentials() {
    for (content, images) in [
        ("read @missing.txt", vec![]),
        ("invalid image", vec![vec![1, 2, 3]]),
    ] {
        let fixture = Fixture::new();
        fixture.insert("candidate", "steer", content, images);
        let calls = Arc::new(AtomicUsize::new(0));
        let source = fixture.source(
            fixture.authority_reader(calls.clone()),
            CancellationToken::new(),
        );

        assert!(source.take().await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.state("candidate").as_deref(), Some("pending"));
    }
}
