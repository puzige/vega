use crate::{
    Connection, Error, Event, LaunchConfig, MAX_BATCH_ELEMENTS, MAX_EVENT_BYTES, MAX_EVENT_COUNT,
    MAX_FRAME_BYTES, MAX_PENDING_INBOUND, MAX_PENDING_OUTBOUND, MAX_SEEN_INBOUND_REQUEST_ID_BYTES,
    MAX_SEEN_INBOUND_REQUEST_IDS, PermissionOption, PermissionOutcome, PromptResult, RequestId,
};
use serde_json::{Value, json};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, duplex, split},
    task::JoinHandle,
    time::timeout,
};

struct FakeChild(Arc<AtomicBool>);

impl crate::ChildControl for FakeChild {
    fn force_terminate(self: Box<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn graceful_shutdown(self: Box<Self>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            self.0.store(true, Ordering::SeqCst);
        })
    }
}
