use super::*;
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};

#[derive(Clone, Default)]
pub struct WriteObservation(Arc<AtomicUsize>);

impl WriteObservation {
    pub fn bytes(&self) -> usize {
        self.0.load(Ordering::Acquire)
    }
}

struct ObservedWriter {
    observation: WriteObservation,
    after_write: Option<Box<dyn FnOnce() + Send>>,
}

impl AsyncWrite for ObservedWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.observation.bytes() > 0 {
            return Poll::Pending;
        }
        let count = buffer.len().min(1);
        self.observation.0.fetch_add(count, Ordering::Release);
        if let Some(after_write) = self.after_write.take() {
            after_write();
        }
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub fn observed_client(
    before_send: Option<Box<dyn FnOnce() + Send>>,
    after_write: Option<Box<dyn FnOnce() + Send>>,
) -> (ManagedClient, WriteObservation) {
    let observation = WriteObservation::default();
    let client = ManagedClient {
        child: None,
        stdin: Box::new(ObservedWriter {
            observation: observation.clone(),
            after_write,
        }),
        stdout: BufReader::new(Box::new(tokio::io::empty())),
        next_id: 1,
        tools: ["click", "browser_click"]
            .into_iter()
            .map(|name| (name.into(), json!({"type":"object"})))
            .collect(),
        closed: false,
        browser_connection_started: false,
        before_send,
    };
    (client, observation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn send_evidence_distinguishes_presend_cancellation_from_partial_write() {
        for partial in [false, true] {
            let cancel = CancellationToken::new();
            let cancelled = cancel.clone();
            let callback: Box<dyn FnOnce() + Send> = Box::new(move || cancelled.cancel());
            let (mut client, writes) = if partial {
                observed_client(None, Some(callback))
            } else {
                observed_client(Some(callback), None)
            };
            let mut send = ManagedSendEvidence::NotStarted;
            assert!(matches!(
                client
                    .call_with_send_evidence("click", json!({}), &cancel, &mut send)
                    .await,
                Err(McpError::Cancelled)
            ));
            assert_eq!(writes.bytes(), usize::from(partial));
            assert_eq!(
                send,
                if partial {
                    ManagedSendEvidence::WriteStarted
                } else {
                    ManagedSendEvidence::NotStarted
                }
            );
            let cancel = CancellationToken::new();
            let cancelled = cancel.clone();
            let (mut ordinary, writes) =
                observed_client(Some(Box::new(move || cancelled.cancel())), None);
            assert!(matches!(
                ordinary.call("click", json!({}), &cancel).await,
                Err(McpError::Cancelled)
            ));
            assert_eq!(writes.bytes(), 0);
        }
    }
}
