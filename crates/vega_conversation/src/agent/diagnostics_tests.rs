use super::*;
use crate::database_writer::{DatabaseCommand, Sealed};
use std::path::PathBuf;
use std::time::Duration;
use vega_store::Store;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
    thread_id: String,
    run_id: String,
    admission: vega_store::thread_admission::ThreadAdmission,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("vega-diagnostics-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        let thread_id = ulid::Ulid::generate().to_string();
        store.conn().execute("INSERT INTO threads(id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,'model','execute','confirm',1,1)", [&thread_id])?;
        let admission = crate::thread_access::acquire(&store, &thread_id)?;
        Ok(Self {
            root,
            store,
            thread_id,
            run_id: ulid::Ulid::generate().to_string(),
            admission,
        })
    }

    fn buffered(&self, terminal: DiagnosticState) -> DiagnosticsWriter {
        let writer = DiagnosticsWriter::buffered();
        let started = Instant::now();
        for state in [DiagnosticState::Started, terminal] {
            writer.root_event(
                &self.thread_id,
                &self.run_id,
                "root",
                state,
                (
                    (state == DiagnosticState::Failed)
                        .then_some(DiagnosticFailureCode::RuntimeFailure),
                    Default::default(),
                ),
                started,
            );
        }
        writer
    }

    async fn flush(&self, writer: &DiagnosticsWriter) -> Result<(), DiagnosticsFlushError> {
        writer
            .flush_to(
                &self.store,
                self.admission.clone(),
                &self.thread_id,
                &self.run_id,
            )
            .await
    }

    fn events(
        &self,
    ) -> Result<
        Vec<vega_store::run_diagnostics::DiagnosticEvent>,
        vega_store::run_diagnostics::DiagnosticError,
    > {
        vega_store::run_diagnostics::read_by_run(self.store.conn(), &self.thread_id, &self.run_id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn buffered_diagnostics_commit_in_recorded_order() -> TestResult {
    let fixture = Fixture::new()?;
    let writer = fixture.buffered(DiagnosticState::Succeeded);
    fixture.flush(&writer).await?;
    let events = fixture.events()?;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event.state, DiagnosticState::Started);
    assert_eq!(events[1].event.state, DiagnosticState::Succeeded);
    assert!(events[0].id < events[1].id);
    assert!(matches!(
        fixture.flush(&writer).await,
        Err(DiagnosticsFlushError::BufferUnavailable)
    ));
    assert_eq!(fixture.events()?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn buffered_diagnostics_sql_failure_is_reported_and_rolls_back_entire_batch() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_diagnostic_terminal BEFORE INSERT ON run_diagnostic_events WHEN NEW.state <> 'started' BEGIN SELECT RAISE(ABORT, 'diagnostic terminal rejected'); END;")?;
    let writer = fixture.buffered(DiagnosticState::Succeeded);
    let error = fixture
        .flush(&writer)
        .await
        .expect_err("SQL failure must reach the caller");
    assert!(matches!(
        error,
        DiagnosticsFlushError::Diagnostic(vega_store::run_diagnostics::DiagnosticError::Sql(_))
    ));
    assert!(fixture.events()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn buffered_diagnostics_reject_cross_run_batch_without_writes() -> TestResult {
    let fixture = Fixture::new()?;
    let writer = fixture.buffered(DiagnosticState::Succeeded);
    let error = writer
        .flush_to(
            &fixture.store,
            fixture.admission.clone(),
            &fixture.thread_id,
            "other-run",
        )
        .await
        .expect_err("cross-run batch must fail");
    assert!(matches!(error, DiagnosticsFlushError::OwnershipChanged));
    assert!(fixture.events()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn buffered_diagnostics_flush_cancelled_and_failed_terminals() -> TestResult {
    for terminal in [DiagnosticState::Cancelled, DiagnosticState::Failed] {
        let fixture = Fixture::new()?;
        fixture.flush(&fixture.buffered(terminal)).await?;
        let events = fixture.events()?;
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].event.state, terminal);
        assert_eq!(events[0].event.failure_code, None);
        assert_eq!(
            events[1].event.failure_code,
            Some(match terminal {
                DiagnosticState::Cancelled => DiagnosticFailureCode::Cancelled,
                DiagnosticState::Failed => DiagnosticFailureCode::RuntimeFailure,
                _ => unreachable!(),
            })
        );
    }
    Ok(())
}

struct HoldActor {
    started: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

impl Sealed for HoldActor {}
impl DatabaseCommand for HoldActor {
    type Output = Result<(), String>;

    fn apply(self, _store: &Store) -> Self::Output {
        self.started
            .send(())
            .map_err(|_| "gate observer dropped".to_string())?;
        self.release
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())
    }
}

struct ReleaseActor(Option<std::sync::mpsc::Sender<()>>);

impl Drop for ReleaseActor {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test]
async fn buffered_diagnostics_wait_releases_mutex_and_other_database_can_commit() -> TestResult {
    let fixture = Fixture::new()?;
    let other = Fixture::new()?;
    let path = fixture.root.join("fixture.db");
    let identity = fixture.store.database_identity();
    let (started, observed) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let release = ReleaseActor(Some(release));
    let gate = tokio::task::spawn_blocking(move || {
        crate::database_writer::commit(
            &path,
            identity,
            HoldActor {
                started,
                release: wait,
            },
        )
    });
    tokio::time::timeout(Duration::from_secs(2), observed).await??;
    let writer = fixture.buffered(DiagnosticState::Succeeded);
    let mut flush = Box::pin(fixture.flush(&writer));
    tokio::select! {
        result = &mut flush => return Err(format!("flush bypassed blocked actor: {result:?}").into()),
        _ = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    {
        let sender = writer
            .sender
            .try_lock()
            .expect("flush must release diagnostics mutex before waiting");
        assert!(sender.is_none());
    }
    writer.root_event(
        &fixture.thread_id,
        &fixture.run_id,
        "late",
        DiagnosticState::Started,
        (None, Default::default()),
        Instant::now(),
    );
    assert!(fixture.events()?.is_empty());
    let other_writer = other.buffered(DiagnosticState::Succeeded);
    tokio::time::timeout(Duration::from_secs(2), other.flush(&other_writer)).await??;
    assert_eq!(other.events()?.len(), 2);
    drop(release);
    gate.await?
        .map_err(std::io::Error::other)?
        .map_err(std::io::Error::other)?;
    tokio::time::timeout(Duration::from_secs(2), &mut flush).await??;
    assert_eq!(fixture.events()?.len(), 2);
    Ok(())
}
