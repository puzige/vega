use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub struct Reply {
    pub status: u16,
    pub body: String,
}

impl Reply {
    pub fn events(chunks: &[Value]) -> Self {
        Self {
            status: 200,
            body: chunks
                .iter()
                .map(|chunk| format!("data: {chunk}\n\n"))
                .collect::<String>()
                + "data: [DONE]\n\n",
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            body: "{}".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Received {
    pub path: String,
    pub body: Value,
}

pub struct HttpFixture {
    pub base: String,
    pub received: Arc<Mutex<Vec<Received>>>,
    worker: Option<JoinHandle<()>>,
    stop: tokio_util::sync::CancellationToken,
    expected: usize,
}

impl HttpFixture {
    pub async fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let received = Arc::new(Mutex::new(Vec::new()));
        let captured = received.clone();
        let expected = replies.len();
        let stop = tokio_util::sync::CancellationToken::new();
        let worker_stop = stop.clone();
        let worker = tokio::spawn(async move {
            let mut replies: std::collections::VecDeque<_> = replies.into();
            loop {
                let (mut socket, peer) = tokio::select! {
                    _ = worker_stop.cancelled() => break,
                    accepted = listener.accept() => accepted.unwrap(),
                };
                let reply = replies.pop_front().unwrap_or_else(|| Reply::status(400));
                assert!(peer.ip().is_loopback());
                let captured_index = {
                    let mut requests = captured.lock().unwrap();
                    let index = requests.len();
                    requests.push(Received {
                        path: String::new(),
                        body: Value::Null,
                    });
                    index
                };
                let mut bytes = Vec::new();
                let (header_end, content_length) = loop {
                    assert!(bytes.len() < 1024 * 1024);
                    let mut buffer = [0u8; 4096];
                    let count =
                        tokio::time::timeout(Duration::from_secs(8), socket.read(&mut buffer))
                            .await
                            .unwrap()
                            .unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..index]).unwrap();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        assert!(length <= 1024 * 1024);
                        break (index + 4, length);
                    }
                };
                while bytes.len() < header_end + content_length {
                    let mut buffer = [0u8; 4096];
                    let count =
                        tokio::time::timeout(Duration::from_secs(8), socket.read(&mut buffer))
                            .await
                            .unwrap()
                            .unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    assert!(bytes.len() <= 1024 * 1024 + header_end);
                }
                let path = std::str::from_utf8(&bytes[..header_end])
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_string();
                let body = if content_length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap()
                };
                captured.lock().unwrap()[captured_index] = Received { path, body };
                let response = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.status,
                    reply.body.len(),
                    reply.body
                );
                tokio::time::timeout(
                    Duration::from_secs(8),
                    socket.write_all(response.as_bytes()),
                )
                .await
                .unwrap()
                .unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        Self {
            base,
            received,
            worker: Some(worker),
            stop,
            expected,
        }
    }

    pub async fn finish(&mut self) {
        self.stop.cancel();
        if let Some(mut worker) = self.worker.take() {
            match tokio::time::timeout(Duration::from_secs(9), &mut worker).await {
                Ok(result) => result.unwrap(),
                Err(_) => {
                    worker.abort();
                    let _ = worker.await;
                    panic!("bounded HTTP fixture did not receive its expected requests");
                }
            }
        }
        assert_eq!(
            self.received.lock().unwrap().len(),
            self.expected,
            "HTTP request count"
        );
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}
