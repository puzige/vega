use std::{
    collections::{HashMap, VecDeque},
    io::Cursor,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{FutureExt, StreamExt, channel::oneshot, future::BoxFuture, stream};
use tokio_util::sync::CancellationToken;

use super::{
    LinkPreviewService,
    ports::{PreparedRequest, PreviewClock, PreviewResolver, PreviewTransport, TransportResponse},
};
use crate::types::LinkPreviewFailure;

type Timer = (Duration, oneshot::Sender<()>);

#[derive(Clone, Default)]
pub struct ManualClock(Arc<Mutex<(Duration, Vec<Timer>)>>);

impl ManualClock {
    pub fn advance(&self, duration: Duration) {
        let mut state = self.0.lock().unwrap();
        state.0 += duration;
        let now = state.0;
        let timers = std::mem::take(&mut state.1);
        for (deadline, sender) in timers {
            if deadline <= now {
                let _ = sender.send(());
            } else {
                state.1.push((deadline, sender));
            }
        }
    }
}

impl PreviewClock for ManualClock {
    fn now(&self) -> Duration {
        self.0.lock().unwrap().0
    }

    fn sleep_until(&self, deadline: Duration) -> BoxFuture<'static, ()> {
        let mut state = self.0.lock().unwrap();
        if deadline <= state.0 {
            return futures::future::ready(()).boxed();
        }
        let (sender, receiver) = oneshot::channel();
        state.1.push((deadline, sender));
        async move {
            let _ = receiver.await;
        }
        .boxed()
    }
}

pub struct ResponseGate(oneshot::Sender<()>);

impl ResponseGate {
    pub fn release(self) {
        let _ = self.0.send(());
    }
}

struct Packet {
    mime: String,
    bytes: Vec<u8>,
    ready: oneshot::Receiver<()>,
}

#[derive(Default)]
struct NetworkState {
    routes: HashMap<String, VecDeque<Packet>>,
    requests: Vec<String>,
    cancellations: Vec<CancellationToken>,
    resolutions: usize,
}

#[derive(Clone, Default)]
pub struct ScriptedNetwork(Arc<Mutex<NetworkState>>);

impl ScriptedNetwork {
    pub fn enqueue(&self, url: &str, mime: &str, bytes: &[u8]) -> ResponseGate {
        let (sender, ready) = oneshot::channel();
        self.0
            .lock()
            .unwrap()
            .routes
            .entry(url.into())
            .or_default()
            .push_back(Packet {
                mime: mime.into(),
                bytes: bytes.into(),
                ready,
            });
        ResponseGate(sender)
    }

    pub fn requests(&self) -> Vec<String> {
        self.0.lock().unwrap().requests.clone()
    }

    pub fn resolutions(&self) -> usize {
        self.0.lock().unwrap().resolutions
    }

    pub fn request_cancelled(&self, index: usize) -> bool {
        self.0.lock().unwrap().cancellations[index].is_cancelled()
    }

    pub fn service(&self, clock: &ManualClock) -> LinkPreviewService {
        LinkPreviewService::with_ports(
            Arc::new(self.clone()),
            Arc::new(self.clone()),
            Arc::new(clock.clone()),
        )
    }
}

impl PreviewResolver for ScriptedNetwork {
    fn resolve(
        &self,
        _: String,
        port: u16,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, Result<Vec<SocketAddr>, LinkPreviewFailure>> {
        self.0.lock().unwrap().resolutions += 1;
        async move {
            if cancellation.is_cancelled() {
                return Err(LinkPreviewFailure::Cancelled);
            }
            Ok(vec![SocketAddr::from(([93, 184, 216, 34], port))])
        }
        .boxed()
    }
}

impl PreviewTransport for ScriptedNetwork {
    fn get(
        &self,
        request: PreparedRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, Result<TransportResponse, LinkPreviewFailure>> {
        let packet = {
            let mut state = self.0.lock().unwrap();
            let packet = state
                .routes
                .get_mut(&request.url)
                .and_then(VecDeque::pop_front);
            state.requests.push(request.url);
            state.cancellations.push(cancellation.clone());
            packet
        };
        async move {
            let packet = packet.ok_or(LinkPreviewFailure::Network)?;
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(LinkPreviewFailure::Cancelled),
                _ = packet.ready => {}
            }
            Ok(TransportResponse {
                status: 200,
                headers: vec![("content-type".into(), packet.mime)],
                peer: SocketAddr::from(([93, 184, 216, 34], 443)),
                body: stream::iter([Ok(packet.bytes)]).boxed(),
            })
        }
        .boxed()
    }
}

pub fn thumbnail_png() -> Vec<u8> {
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        4,
        2,
        image::Rgba([40, 90, 150, 255]),
    ))
    .write_to(&mut encoded, image::ImageFormat::Png)
    .unwrap();
    encoded.into_inner()
}
