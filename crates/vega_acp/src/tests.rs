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

async fn read_line<R: AsyncRead + Unpin>(reader: &mut R) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        assert_eq!(reader.read(&mut byte).await.unwrap(), 1);
        if byte[0] == b'\n' {
            return bytes;
        }
        bytes.push(byte[0]);
    }
}

async fn read_json<R: AsyncRead + Unpin>(reader: &mut R) -> Value {
    serde_json::from_slice(&read_line(reader).await).unwrap()
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) {
    writer
        .write_all(&serde_json::to_vec(value).unwrap())
        .await
        .unwrap();
    writer.write_all(b"\n").await.unwrap();
}

fn permission_request(id: Value) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":id,
        "method":"session/request_permission",
        "params":{"sessionId":"s","toolCall":{"toolCallId":format!("call-{id}")},"options":[{"optionId":"allow","name":"Allow"}]}
    })
}

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
