use super::{
    Capability, ComponentState, ComputerUseControl, ComputerUseError as Error, DirectCapture,
    ExtensionState, PermissionState,
    control::OwnershipAuthority,
    preparation_from_check_permissions,
    probe::{PreparationSession, PreparationTransport, PreparationTransportFactory},
};
use std::{
    collections::VecDeque,
    future::{Future, pending},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Ledger {
    opens: usize,
    live: usize,
    drops: usize,
    cleanup_attempts: usize,
    cleanup_started: Arc<Notify>,
    calls: Vec<(bool, bool)>,
}

enum Reply {
    Status,
    Error,
    Pending,
}

struct Factory {
    ledger: Arc<Mutex<Ledger>>,
    replies: VecDeque<Reply>,
    error: Option<Error>,
    cleanup: Option<mpsc::UnboundedReceiver<Result<(), Error>>>,
}

struct Transport {
    cleanup: mpsc::UnboundedReceiver<Result<(), Error>>,
    ledger: Arc<Mutex<Ledger>>,
    replies: VecDeque<Reply>,
}

impl PreparationTransportFactory for Factory {
    type Transport = Transport;

    fn open(&mut self) -> Result<Transport, Error> {
        self.ledger.lock().unwrap().opens += 1;
        if let Some(error) = self.error {
            return Err(error);
        }
        self.ledger.lock().unwrap().live += 1;
        Ok(Transport {
            cleanup: self.cleanup.take().unwrap(),
            ledger: self.ledger.clone(),
            replies: std::mem::take(&mut self.replies),
        })
    }
}

impl PreparationTransport for Transport {
    fn check_permissions(
        &mut self,
        prompt: bool,
        probe_direct_capture: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Option<serde_json::Value>, Error>> + Send + '_>> {
        self.ledger
            .lock()
            .unwrap()
            .calls
            .push((prompt, probe_direct_capture));
        let reply = self.replies.pop_front().expect("unexpected permission RPC");
        Box::pin(async move {
            match reply {
                Reply::Status => Ok(Some(passive_status())),
                Reply::Error => Err(Error::Unavailable),
                Reply::Pending => pending().await,
            }
        })
    }

    fn cleanup(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>> {
        Box::pin(async move {
            {
                let mut ledger = self.ledger.lock().unwrap();
                ledger.cleanup_attempts += 1;
                ledger.cleanup_started.notify_one();
            }
            self.cleanup
                .recv()
                .await
                .unwrap_or(Err(Error::Unavailable))?;
            self.ledger.lock().unwrap().live -= 1;
            Ok(())
        })
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let mut ledger = self.ledger.lock().unwrap();
        ledger.drops += 1;
    }
}

fn passive_status() -> serde_json::Value {
    serde_json::json!({
        "accessibility": true,
        "screen_recording": true,
        "screen_recording_capturable": null,
        "direct_capture_status": "not_checked"
    })
}

fn fixture(replies: impl IntoIterator<Item = Reply>) -> (Factory, Arc<Mutex<Ledger>>) {
    let ledger = Arc::new(Mutex::new(Ledger::default()));
    let (complete, cleanup) = mpsc::unbounded_channel();
    complete.send(Ok(())).unwrap();
    (
        Factory {
            ledger: ledger.clone(),
            replies: replies.into_iter().collect(),
            error: None,
            cleanup: Some(cleanup),
        },
        ledger,
    )
}

fn controlled_cleanup(factory: &mut Factory) -> mpsc::UnboundedSender<Result<(), Error>> {
    let (complete, cleanup) = mpsc::unbounded_channel();
    factory.cleanup = Some(cleanup);
    complete
}

fn assert_busy(authority: &Arc<OwnershipAuthority>) {
    let run = ComputerUseControl::with_authority(authority.clone());
    assert_eq!(run.begin(Capability::Desktop), Err(Error::Busy));
    let (factory, ledger) = fixture([]);
    assert!(matches!(
        PreparationSession::open(factory, authority.clone(), CancellationToken::new()),
        Err(Error::Busy)
    ));
    assert_eq!(ledger.lock().unwrap().opens, 0);
}

fn assert_released(authority: &Arc<OwnershipAuthority>) {
    let run = ComputerUseControl::with_authority(authority.clone());
    assert!(run.begin(Capability::Desktop).is_ok());
    run.stop();
    run.finish_cleanup();
}

fn assert_pending<F: Future>(future: Pin<&mut F>) {
    let waker = Waker::noop();
    assert!(matches!(
        future.poll(&mut Context::from_waker(waker)),
        Poll::Pending
    ));
}

#[tokio::test]
async fn repeated_manual_checks_reuse_one_session_and_never_prompt_or_verify_capture() {
    let authority = Arc::new(OwnershipAuthority::default());
    let cancel = CancellationToken::new();
    let (factory, ledger) = fixture([Reply::Status, Reply::Status, Reply::Status]);
    let mut session = PreparationSession::open(factory, authority.clone(), cancel).unwrap();
    for _ in 0..3 {
        let state = session.check().await.unwrap();
        assert_eq!(state.component, ComponentState::Available);
        assert_eq!(state.accessibility, PermissionState::Granted);
        assert_eq!(state.screen_recording, PermissionState::Granted);
        assert_eq!(state.direct_capture, DirectCapture::NotChecked);
        assert_eq!(state.browser_extension, ExtensionState::NotEvaluated);
        assert!(!state.desktop_ready());
        assert!(!state.needs_screen_recording_settings());
        assert_eq!(ledger.lock().unwrap().live, 1);
        assert_eq!(ledger.lock().unwrap().drops, 0);
    }
    assert_eq!(ledger.lock().unwrap().opens, 1);
    assert_eq!(ledger.lock().unwrap().calls, vec![(false, false); 3]);
    session.close().await.unwrap();
    session.close().await.unwrap();
    assert_eq!(session.check().await, Err(Error::Cancelled));
    assert_eq!(ledger.lock().unwrap().calls.len(), 3);
    assert_eq!(ledger.lock().unwrap().live, 0);
    assert_eq!(ledger.lock().unwrap().drops, 1);
    assert_released(&authority);
}

#[test]
fn run_first_rejects_preparation_before_spawning_transport() {
    let authority = Arc::new(OwnershipAuthority::default());
    let run = ComputerUseControl::with_authority(authority.clone());
    run.begin(Capability::Desktop).unwrap();
    let (factory, ledger) = fixture([]);
    assert!(matches!(
        PreparationSession::open(factory, authority.clone(), CancellationToken::new()),
        Err(Error::Busy)
    ));
    assert_eq!(ledger.lock().unwrap().opens, 0);
    run.stop();
    run.finish_cleanup();
    assert_released(&authority);
}

#[tokio::test]
async fn close_pending_blocks_run_and_other_probe_until_cleanup_completes() {
    let authority = Arc::new(OwnershipAuthority::default());
    let (mut factory, ledger) = fixture([]);
    let complete = controlled_cleanup(&mut factory);
    let mut session =
        PreparationSession::open(factory, authority.clone(), CancellationToken::new()).unwrap();
    assert_busy(&authority);
    let mut close = Box::pin(session.close());
    assert_pending(close.as_mut());
    assert_eq!(ledger.lock().unwrap().cleanup_attempts, 1);
    assert_eq!(ledger.lock().unwrap().live, 1);
    assert_busy(&authority);
    complete.send(Ok(())).unwrap();
    close.await.unwrap();
    assert_eq!(ledger.lock().unwrap().live, 0);
    assert_eq!(ledger.lock().unwrap().drops, 1);
    session.close().await.unwrap();
    assert_eq!(ledger.lock().unwrap().cleanup_attempts, 1);
    assert_released(&authority);
}

#[tokio::test]
async fn stale_probe_and_run_cleanup_cannot_release_a_new_owner() {
    let authority = Arc::new(OwnershipAuthority::default());
    let old_run = ComputerUseControl::with_authority(authority.clone());
    old_run.begin(Capability::Desktop).unwrap();
    old_run.stop();
    old_run.finish_cleanup();
    let (factory, ledger) = fixture([]);
    let mut old_probe =
        PreparationSession::open(factory, authority.clone(), CancellationToken::new()).unwrap();
    old_run.release();
    let contender = ComputerUseControl::with_authority(authority.clone());
    assert_eq!(contender.begin(Capability::Desktop), Err(Error::Busy));
    old_probe.close().await.unwrap();
    let current = ComputerUseControl::with_authority(authority.clone());
    current.begin(Capability::Desktop).unwrap();
    old_probe.close().await.unwrap();
    drop(old_probe);
    old_run.finish_cleanup();
    old_run.release();
    assert_eq!(contender.begin(Capability::Desktop), Err(Error::Busy));
    assert_eq!(ledger.lock().unwrap().drops, 1);
    current.stop();
    current.finish_cleanup();
    assert_released(&authority);
}

#[test]
fn component_open_errors_release_admission_without_becoming_busy_or_denied() {
    for error in [
        Error::MissingComponents,
        Error::IncompatibleComponents,
        Error::UnsupportedPlatform,
        Error::Unavailable,
    ] {
        let authority = Arc::new(OwnershipAuthority::default());
        let (mut factory, ledger) = fixture([]);
        factory.error = Some(error);
        assert!(
            matches!(PreparationSession::open(factory, authority.clone(), CancellationToken::new()), Err(actual) if actual == error)
        );
        assert_eq!(ledger.lock().unwrap().opens, 1);
        assert_eq!(ledger.lock().unwrap().live, 0);
        assert_released(&authority);
    }
}

#[tokio::test]
async fn transport_error_waits_for_cleanup_preserving_original_result() {
    let authority = Arc::new(OwnershipAuthority::default());
    let (mut factory, ledger) = fixture([Reply::Error]);
    let complete = controlled_cleanup(&mut factory);
    let mut session =
        PreparationSession::open(factory, authority.clone(), CancellationToken::new()).unwrap();
    let mut check = Box::pin(session.check());
    assert_pending(check.as_mut());
    assert_eq!(ledger.lock().unwrap().cleanup_attempts, 1);
    assert_eq!(ledger.lock().unwrap().live, 1);
    assert_busy(&authority);
    complete.send(Ok(())).unwrap();
    assert_eq!(check.await, Err(Error::Unavailable));
    assert_eq!(ledger.lock().unwrap().live, 0);
    assert_eq!(session.check().await, Err(Error::Cancelled));
    assert_eq!(ledger.lock().unwrap().calls, vec![(false, false)]);
    let (retry, _) = fixture([Reply::Status]);
    let mut retry =
        PreparationSession::open(retry, authority.clone(), CancellationToken::new()).unwrap();
    drop(session);
    assert_busy(&authority);
    assert_eq!(
        retry.check().await.unwrap().direct_capture,
        DirectCapture::NotChecked
    );
    retry.close().await.unwrap();
    assert_released(&authority);
}

#[test]
fn cancellation_before_open_does_not_spawn_or_acquire() {
    let authority = Arc::new(OwnershipAuthority::default());
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (factory, ledger) = fixture([]);
    assert!(matches!(
        PreparationSession::open(factory, authority.clone(), cancel),
        Err(Error::Cancelled)
    ));
    assert_eq!(ledger.lock().unwrap().opens, 0);
    assert_released(&authority);
}

#[tokio::test]
async fn cancellation_waits_for_cleanup_preserving_cancelled_result() {
    let authority = Arc::new(OwnershipAuthority::default());
    let cancel = CancellationToken::new();
    let (mut factory, ledger) = fixture([Reply::Pending]);
    let complete = controlled_cleanup(&mut factory);
    let mut session = PreparationSession::open(factory, authority.clone(), cancel.clone()).unwrap();
    let mut check = Box::pin(session.check());
    assert_pending(check.as_mut());
    cancel.cancel();
    assert_pending(check.as_mut());
    assert_eq!(ledger.lock().unwrap().calls, vec![(false, false)]);
    assert_eq!(ledger.lock().unwrap().cleanup_attempts, 1);
    assert_eq!(ledger.lock().unwrap().live, 1);
    assert_busy(&authority);
    complete.send(Ok(())).unwrap();
    assert_eq!(check.await, Err(Error::Cancelled));
    assert_eq!(ledger.lock().unwrap().live, 0);
    assert_eq!(session.check().await, Err(Error::Cancelled));
    assert_released(&authority);
}

#[tokio::test]
async fn rpc_error_and_cancellation_survive_cleanup_failure_without_releasing_admission() {
    for cancelled in [false, true] {
        let authority = Arc::new(OwnershipAuthority::default());
        let cancel = CancellationToken::new();
        let (mut factory, ledger) = fixture([if cancelled {
            Reply::Pending
        } else {
            Reply::Error
        }]);
        let complete = controlled_cleanup(&mut factory);
        let mut session =
            PreparationSession::open(factory, authority.clone(), cancel.clone()).unwrap();
        let mut check = Box::pin(session.check());
        assert_pending(check.as_mut());
        if cancelled {
            cancel.cancel();
            assert_pending(check.as_mut());
        }
        complete.send(Err(Error::NotReady)).unwrap();
        assert_eq!(
            check.await,
            Err(if cancelled {
                Error::Cancelled
            } else {
                Error::Unavailable
            })
        );
        assert_eq!(ledger.lock().unwrap().live, 1);
        assert_eq!(ledger.lock().unwrap().drops, 0);
        assert_busy(&authority);
        complete.send(Ok(())).unwrap();
        session.close().await.unwrap();
        assert_eq!(ledger.lock().unwrap().cleanup_attempts, 2);
        assert_eq!(ledger.lock().unwrap().live, 0);
        assert_released(&authority);
    }
}

#[tokio::test]
async fn abandoned_check_and_session_transfer_lease_until_background_cleanup_finishes() {
    let authority = Arc::new(OwnershipAuthority::default());
    let (mut factory, ledger) = fixture([Reply::Pending]);
    let complete = controlled_cleanup(&mut factory);
    let started = ledger.lock().unwrap().cleanup_started.clone();
    let mut session =
        PreparationSession::open(factory, authority.clone(), CancellationToken::new()).unwrap();
    let mut check = Box::pin(session.check());
    assert_pending(check.as_mut());
    drop(check);
    drop(session);
    started.notified().await;
    assert_eq!(ledger.lock().unwrap().live, 1);
    assert_eq!(ledger.lock().unwrap().drops, 0);
    assert_busy(&authority);
    complete.send(Ok(())).unwrap();
    authority.cleanup_pending().await.unwrap();
    assert_eq!(ledger.lock().unwrap().live, 0);
    assert_eq!(ledger.lock().unwrap().drops, 1);
    let current = ComputerUseControl::with_authority(authority.clone());
    current.begin(Capability::Desktop).unwrap();
    authority.cleanup_pending().await.unwrap();
    assert_busy(&authority);
    current.stop();
    current.finish_cleanup();
    assert_released(&authority);
}

#[tokio::test]
async fn failed_close_keeps_lease_and_transport_for_explicit_retry() {
    let authority = Arc::new(OwnershipAuthority::default());
    let (mut factory, ledger) = fixture([]);
    let complete = controlled_cleanup(&mut factory);
    let mut session =
        PreparationSession::open(factory, authority.clone(), CancellationToken::new()).unwrap();
    complete.send(Err(Error::Unavailable)).unwrap();
    assert_eq!(session.close().await, Err(Error::Unavailable));
    assert_eq!(ledger.lock().unwrap().live, 1);
    assert_eq!(ledger.lock().unwrap().drops, 0);
    assert_busy(&authority);
    assert_eq!(session.check().await, Err(Error::Cancelled));
    complete.send(Ok(())).unwrap();
    session.close().await.unwrap();
    assert_eq!(ledger.lock().unwrap().cleanup_attempts, 2);
    assert_eq!(ledger.lock().unwrap().live, 0);
    assert_released(&authority);
}

#[tokio::test]
async fn abandoned_close_failure_is_retained_by_cleanup_manager_until_retry_succeeds() {
    let authority = Arc::new(OwnershipAuthority::default());
    let (mut factory, ledger) = fixture([]);
    let complete = controlled_cleanup(&mut factory);
    let mut session =
        PreparationSession::open(factory, authority.clone(), CancellationToken::new()).unwrap();
    let mut close = Box::pin(session.close());
    assert_pending(close.as_mut());
    drop(close);
    drop(session);
    assert_busy(&authority);
    complete.send(Err(Error::Unavailable)).unwrap();
    assert_eq!(authority.cleanup_pending().await, Err(Error::Unavailable));
    assert_eq!(ledger.lock().unwrap().live, 1);
    assert_eq!(ledger.lock().unwrap().drops, 0);
    assert_busy(&authority);
    complete.send(Ok(())).unwrap();
    authority.cleanup_pending().await.unwrap();
    assert_eq!(ledger.lock().unwrap().live, 0);
    assert_eq!(ledger.lock().unwrap().drops, 1);
    assert_released(&authority);
}

#[test]
fn not_checked_is_unverified_not_a_request_to_reauthorize_granted_tcc() {
    let state =
        preparation_from_check_permissions(ComponentState::Available, Some(&passive_status()));
    assert_eq!(state.direct_capture, DirectCapture::NotChecked);
    assert!(!state.desktop_ready());
    assert!(!state.needs_accessibility_settings());
    assert!(!state.needs_screen_recording_settings());
}
