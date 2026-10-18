use super::*;
use serde_json::json;
use std::sync::mpsc as std_mpsc;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::oneshot;
use vega_acp::RequestId;

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: Value) {
    writer
        .write_all(&serde_json::to_vec(&value).unwrap())
        .await
        .unwrap();
    writer.write_all(b"\n").await.unwrap();
}
