use super::*;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("vega-fork-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        let installed: bool = store.conn().query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='trigger' AND name='source_validation_credential_continuations_insert')", [], |row| row.get(0))?;
        if !installed {
            store.conn().execute_batch(include_str!(
                "../../../../docs/919/0065_source_validation.sql"
            ))?;
        }
        store.conn().execute("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/fork-fixture','fixture',1,1)", [])?;
        store.conn().execute("INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('source','project','Task','model',1,1)", [])?;
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('user','source',1,'user','Question','done',1),('assistant','source',2,'assistant','Answer','done',2)", [])?;
        store.conn().execute("INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('assistant','response',0,6,'final')", [])?;
        store.conn().execute("INSERT INTO assistant_response_starts(message_id,response_id,ordinal,text_start_bytes,started_at_ms) VALUES('assistant','response',0,0,2)", [])?;
        seed_terminal(store.conn(), "source", "assistant", "call")?;
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2).write_to(&mut bytes, image::ImageFormat::Png)?;
        store.conn().execute(
            "INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('user',0,?1)",
            [bytes.into_inner()],
        )?;
        Ok(Self { root, store })
    }

    fn path(&self) -> PathBuf {
        self.root.join("fixture.db")
    }

    fn fork_count(&self) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(self.store.conn().query_row(
            "SELECT COUNT(*) FROM threads WHERE forked_from_thread_id='source'",
            [],
            |row| row.get(0),
        )?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn seed_terminal(
    conn: &vega_store::Connection,
    thread: &str,
    message: &str,
    call: &str,
) -> TestResult {
    let approval = r#"{"decision":"once","note":null,"source":"readonly_tool","danger":null}"#;
    conn.execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,approval,created_at,finished_at) VALUES(?1,?2,?3,1,'read_conversation','{}',?4,'cancelled',?5,2,3)", (call, thread, message, vega_store::recovery::RECOVERY_CANCELLED_OUTPUT, approval))?;
    Ok(())
}

#[test]
fn fork_installs_history_terminal_tools_images_and_unique_titles() -> TestResult {
    let fixture = Fixture::new()?;
    let prepared = prepare_fork(&fixture.store, "source", Some("assistant"))?;
    let second_prepared = prepare_fork(&fixture.store, "source", None)?;
    let first = commit_fork(&fixture.store, prepared)?;
    let second = commit_fork(&fixture.store, second_prepared)?;
    assert_eq!(first.title, "Task (1)");
    assert_eq!(second.title, "Task (2)");
    assert_ne!(first.id, second.id);
    let source = context_compaction::load_source(fixture.store.conn(), &first.id)?;
    assert_eq!(source.messages.len(), 2);
    assert_eq!(source.messages[0].content, "Question");
    assert_eq!(source.messages[1].content, "Answer");
    assert_eq!(source.images.len(), 1);
    assert_eq!(source.tool_calls.len(), 1);
    assert_eq!(source.tool_calls[0].status, "cancelled");
    assert_ne!(source.tool_calls[0].id, "call");
    assert_ne!(source.messages[1].id, "assistant");
    assert_eq!(source.tool_calls[0].message_id, source.messages[1].id);
    let terminal =
        crate::agent::recover_terminal_tools(fixture.store.conn(), "project", &first.id)?;
    assert_eq!(terminal.len(), 1);
    assert_eq!(
        terminal[&source.tool_calls[0].id].result.output,
        vega_store::recovery::RECOVERY_CANCELLED_OUTPUT
    );
    assert_eq!(fixture.fork_count()?, 2);
    let foreign_errors: i64 = fixture.store.conn().query_row(
        "SELECT COUNT(*) FROM pragma_foreign_key_check",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(foreign_errors, 0);
    Ok(())
}

#[test]
fn malformed_history_and_images_leave_no_fork() -> TestResult {
    let fixture = Fixture::new()?;
    fixture
        .store
        .conn()
        .execute("UPDATE tool_calls SET approval='{}' WHERE id='call'", [])?;
    assert!(fork_latest_thread(&fixture.store, "source").is_err());
    assert_eq!(fixture.fork_count()?, 0);
    fixture.store.conn().execute("DELETE FROM tool_calls", [])?;
    fixture
        .store
        .conn()
        .execute("UPDATE image_attachments SET encoded=X'010203'", [])?;
    assert!(fork_latest_thread(&fixture.store, "source").is_err());
    assert_eq!(fixture.fork_count()?, 0);
    fixture
        .store
        .conn()
        .execute("DELETE FROM image_attachments", [])?;
    fixture
        .store
        .conn()
        .execute("UPDATE assistant_responses SET text_end_bytes=99", [])?;
    assert!(fork_latest_thread(&fixture.store, "source").is_err());
    assert_eq!(fixture.fork_count()?, 0);
    Ok(())
}

struct SourceChange {
    entered: SyncSender<()>,
    release: Receiver<()>,
    done: SyncSender<Result<(), String>>,
}

impl crate::database_writer::WriteRequest for SourceChange {
    fn execute(self: Box<Self>, store: &Store) {
        let result = (|| -> Result<(), String> {
            self.entered.send(()).map_err(|error| error.to_string())?;
            self.release
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| error.to_string())?;
            let tx = store
                .immediate_transaction()
                .map_err(|error| error.to_string())?;
            tx.execute("UPDATE messages SET content='Changed' WHERE id='user'", [])
                .map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())
        })();
        let _ = self.done.send(result);
    }
}

struct QueuedFork {
    command: ForkCommand,
    done: SyncSender<Result<Thread, ForkThreadError>>,
}

impl crate::database_writer::WriteRequest for QueuedFork {
    fn execute(self: Box<Self>, store: &Store) {
        let Self { command, done } = *self;
        let _ = done.send(command.install(store, None));
    }
}

#[test]
fn queued_fork_rejects_changed_source_without_partial_install() -> TestResult {
    let fixture = Fixture::new()?;
    let prepared = prepare_fork(&fixture.store, "source", None)?;
    let writer =
        crate::agent::persistence::database_writer::DatabaseWriter::open_blocking(fixture.path())?;
    let (entered, entering) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let (changed, changing) = mpsc::sync_channel(1);
    writer.submit(Box::new(SourceChange {
        entered,
        release: released,
        done: changed,
    }))?;
    entering.recv_timeout(Duration::from_secs(5))?;
    let (done, receipt) = mpsc::sync_channel(1);
    writer.submit(Box::new(QueuedFork {
        command: ForkCommand { prepared },
        done,
    }))?;
    release.send(())?;
    changing.recv_timeout(Duration::from_secs(5))??;
    assert!(receipt.recv_timeout(Duration::from_secs(5))?.is_err());
    assert_eq!(fixture.fork_count()?, 0);
    let messages: i64 =
        fixture
            .store
            .conn()
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
    assert_eq!(messages, 2);
    Ok(())
}

#[test]
fn changed_unversioned_fork_metadata_rejects_without_partial_install() -> TestResult {
    let fixture = Fixture::new()?;
    let prepared = prepare_fork(&fixture.store, "source", None)?;
    fixture.store.conn().execute(
        "INSERT INTO assistant_run_durations(message_id,execution_duration_ms) VALUES('assistant',42)", [],
    )?;
    assert!(commit_fork(&fixture.store, prepared).is_err());
    assert_eq!(fixture.fork_count()?, 0);
    Ok(())
}

#[test]
fn prepared_fork_is_bound_to_physical_database_and_rolls_back_sql_failure() -> TestResult {
    let fixture = Fixture::new()?;
    let other = Fixture::new()?;
    let prepared = prepare_fork(&fixture.store, "source", None)?;
    assert!(commit_fork(&other.store, prepared).is_err());
    assert_eq!(other.fork_count()?, 0);
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_fork_image BEFORE INSERT ON image_attachments WHEN NEW.message_id<>'user' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")?;
    let prepared = prepare_fork(&fixture.store, "source", None)?;
    assert!(commit_fork(&fixture.store, prepared).is_err());
    assert_eq!(fixture.fork_count()?, 0);
    Ok(())
}

struct AppendTerminal {
    thread: String,
}
impl crate::database_writer::Sealed for AppendTerminal {}
impl crate::database_writer::DatabaseCommand for AppendTerminal {
    type Output = Result<(), String>;
    fn apply(self, store: &Store) -> Self::Output {
        (|| -> Result<(), Box<dyn std::error::Error>> {
            let message = format!("{}-message", self.thread);
            let call = format!("{}-call", self.thread);
            let tx = store.immediate_transaction()?;
            tx.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'assistant','Completed','done',3)", (&message, &self.thread))?;
            seed_terminal(&tx, &self.thread, &message, &call)?;
            tx.commit()?;
            Ok(())
        })().map_err(|error| error.to_string())
    }
}

#[test]
fn paused_projection_allows_parent_and_two_children_to_commit_complete_tools() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute("INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('parent','project','Parent','model',1,1)", [])?;
    fixture.store.conn().execute("INSERT INTO threads(id,project_id,title,model,parent_thread_id,created_at,updated_at) VALUES('child-a','project','Child A','model','parent',1,1),('child-b','project','Child B','model','parent',1,1)", [])?;
    let projection =
        vega_store::thread_fork::ForkProjection::capture(fixture.store.conn(), "source", None)?;
    let (release, released) = mpsc::sync_channel(1);
    let (done, receipt) = mpsc::sync_channel(1);
    let preparation = std::thread::spawn(move || {
        let result = released
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())
            .and_then(|_| {
                prepare_fork_projection(projection, "source").map_err(|error| error.to_string())
            });
        let _ = done.send(result);
    });
    for thread in ["parent", "child-a", "child-b"] {
        crate::database_writer::commit(
            &fixture.path(),
            fixture.store.database_identity(),
            AppendTerminal {
                thread: thread.into(),
            },
        )??;
        let calls = crate::agent::recover_terminal_tools(fixture.store.conn(), "project", thread)?;
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[&format!("{thread}-call")].result.output,
            vega_store::recovery::RECOVERY_CANCELLED_OUTPUT
        );
    }
    assert!(matches!(receipt.try_recv(), Err(mpsc::TryRecvError::Empty)));
    release.send(())?;
    let prepared = receipt.recv_timeout(Duration::from_secs(5))??;
    preparation
        .join()
        .map_err(|_| "preparation worker panicked")?;
    commit_fork(&fixture.store, prepared)?;
    assert_eq!(fixture.fork_count()?, 1);
    Ok(())
}
