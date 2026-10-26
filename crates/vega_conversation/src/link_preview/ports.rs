use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::OnceLock,
    time::{Duration, Instant},
};

use futures::{FutureExt, StreamExt, channel::oneshot, future::BoxFuture, stream::BoxStream};
use tokio_util::sync::CancellationToken;

use crate::types::LinkPreviewFailure;

#[derive(Clone)]
pub struct PreparedRequest {
    pub url: String,
    pub pinned_addresses: Vec<SocketAddr>,
    pub headers: Vec<(String, String)>,
}

pub struct TransportResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub peer: SocketAddr,
    pub body: BoxStream<'static, Result<Vec<u8>, LinkPreviewFailure>>,
}

pub trait PreviewResolver: Send + Sync {
    fn resolve(
        &self,
        host: String,
        port: u16,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, Result<Vec<SocketAddr>, LinkPreviewFailure>>;
}

pub trait PreviewTransport: Send + Sync {
    fn get(
        &self,
        request: PreparedRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, Result<TransportResponse, LinkPreviewFailure>>;
}

pub trait PreviewClock: Send + Sync {
    fn now(&self) -> Duration;
    fn sleep_until(&self, deadline: Duration) -> BoxFuture<'static, ()>;
}

fn runtime() -> Result<&'static tokio::runtime::Runtime, LinkPreviewFailure> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, LinkPreviewFailure>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .max_blocking_threads(2)
                .thread_name("vega-link-preview")
                .enable_all()
                .build()
                .map_err(|_| LinkPreviewFailure::Network)
        })
        .as_ref()
        .map_err(|failure| *failure)
}

#[derive(Default)]
pub(super) struct SystemClock;

impl PreviewClock for SystemClock {
    fn now(&self) -> Duration {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed()
    }

    fn sleep_until(&self, deadline: Duration) -> BoxFuture<'static, ()> {
        let wait = deadline.saturating_sub(self.now());
        let (sender, receiver) = oneshot::channel();
        let token = CancellationToken::new();
        let sleeping = token.clone();
        if let Ok(runtime) = runtime() {
            runtime.spawn(async move { tokio::select! { _ = sleeping.cancelled() => {}, _ = tokio::time::sleep(wait) => { let _ = sender.send(()); } } });
        }
        let guard = CancelOnDrop(token);
        async move {
            let _guard = guard;
            let _ = receiver.await;
        }
        .boxed()
    }
}

pub(super) struct SystemResolver;

impl PreviewResolver for SystemResolver {
    fn resolve(
        &self,
        host: String,
        port: u16,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, Result<Vec<SocketAddr>, LinkPreviewFailure>> {
        static DNS_WORK: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
        let (sender, receiver) = oneshot::channel();
        let cancellation = cancellation.child_token();
        let working = cancellation.clone();
        let started = runtime().map(|runtime| {
            runtime.spawn(async move {
                let result = async {
                    let permit = tokio::select! {
                        biased;
                        _ = working.cancelled() => return Err(LinkPreviewFailure::Cancelled),
                        permit = DNS_WORK.acquire() => permit.map_err(|_| LinkPreviewFailure::Network)?,
                    };
                    let native_cancellation = working.clone();
                    let mut resolver = tokio::task::spawn_blocking(move || {
                        let _permit = permit;
                        if native_cancellation.is_cancelled() {
                            return Err(LinkPreviewFailure::Cancelled);
                        }
                        (host.as_str(), port)
                            .to_socket_addrs()
                            .map(|addresses| addresses.collect::<Vec<_>>())
                            .map_err(|_| LinkPreviewFailure::Network)
                    });
                    tokio::select! {
                        biased;
                        _ = working.cancelled() => {
                            resolver.abort();
                            Err(LinkPreviewFailure::Cancelled)
                        },
                        result = &mut resolver => result.map_err(|_| LinkPreviewFailure::Network)?,
                    }
                }.await;
                let _ = sender.send(result);
            })
        });
        let guard = CancelOnDrop(cancellation);
        async move {
            let _guard = guard;
            started?;
            receiver.await.map_err(|_| LinkPreviewFailure::Network)?
        }
        .boxed()
    }
}

pub(super) struct SystemTransport;

impl PreviewTransport for SystemTransport {
    fn get(
        &self,
        request: PreparedRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, Result<TransportResponse, LinkPreviewFailure>> {
        let (sender, receiver) = oneshot::channel();
        let started = runtime().map(|runtime| runtime.spawn(async move {
            let response = async {
                let url = reqwest::Url::parse(&request.url).map_err(|_| LinkPreviewFailure::Unsupported)?;
                let host = url.host_str().ok_or(LinkPreviewFailure::Blocked)?;
                let client = reqwest::Client::builder().no_proxy().no_gzip().no_brotli().no_deflate().no_zstd().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(3)).resolve_to_addrs(host, &request.pinned_addresses).build().map_err(|_| LinkPreviewFailure::Network)?;
                let mut builder = client.get(url);
                for (name, value) in request.headers { builder = builder.header(name, value); }
                builder.send().await.map_err(|error| if error.is_timeout() { LinkPreviewFailure::Timeout } else { LinkPreviewFailure::Network })
            };
            let response = tokio::select! { biased; _ = cancellation.cancelled() => Err(LinkPreviewFailure::Cancelled), result = response => result };
            let response = match response { Ok(response) => response, Err(failure) => { let _ = sender.send(Err(failure)); return; } };
            let Some(peer) = response.remote_addr() else { let _ = sender.send(Err(LinkPreviewFailure::Blocked)); return; };
            let status = response.status().as_u16();
            let headers = response.headers().iter().filter_map(|(name, value)| value.to_str().ok().map(|value| (name.as_str().into(), value.into()))).collect();
            let (chunks, received) = tokio::sync::mpsc::channel(2);
            let body = futures::stream::unfold(received, |mut receiver| async move { receiver.recv().await.map(|chunk| (chunk, receiver)) }).boxed();
            if sender.send(Ok(TransportResponse { status, headers, peer, body })).is_err() { return; }
            let mut stream = response.bytes_stream();
            loop {
                let next = tokio::select! { biased; _ = cancellation.cancelled() => break, chunk = stream.next() => chunk };
                let Some(next) = next else { break };
                match next {
                    Ok(bytes) => {
                        for chunk in bytes.chunks(64 * 1024) {
                            let sent = tokio::select! { biased; _ = cancellation.cancelled() => return, result = chunks.send(Ok(chunk.to_vec())) => result };
                            if sent.is_err() { return; }
                        }
                    }
                    Err(_) => { let _ = chunks.send(Err(LinkPreviewFailure::Network)).await; break; }
                }
            }
        }));
        async move {
            started?;
            receiver.await.map_err(|_| LinkPreviewFailure::Network)?
        }
        .boxed()
    }
}

struct CancelOnDrop(CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
