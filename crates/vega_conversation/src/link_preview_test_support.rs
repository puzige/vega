use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{FutureExt, StreamExt, channel::oneshot, future::BoxFuture, stream};
use tokio_util::sync::CancellationToken;

use crate::{
    link_preview::{
        LinkPreviewService,
        test_support::{
            PreparedRequest, PreviewClock, PreviewResolver, PreviewTransport, TransportResponse,
            service_with_ports,
        },
    },
    types::LinkPreviewFailure,
};

type Timer = (Duration, oneshot::Sender<()>);

#[derive(Clone, Default)]
pub struct ManualClock {
    state: Arc<Mutex<(Duration, Vec<Timer>)>>,
}

impl ManualClock {
    pub fn advance(&self, duration: Duration) {
        let ready = {
            let mut state = self.state.lock().unwrap();
            state.0 += duration;
            let now = state.0;
            let mut ready = Vec::new();
            let mut waiting = Vec::new();
            for timer in state.1.drain(..) {
                if timer.0 <= now {
                    ready.push(timer.1);
                } else {
                    waiting.push(timer);
                }
            }
            state.1 = waiting;
            ready
        };
        for sender in ready {
            let _ = sender.send(());
        }
    }
}

impl PreviewClock for ManualClock {
    fn now(&self) -> Duration {
        self.state.lock().unwrap().0
    }

    fn sleep_until(&self, deadline: Duration) -> BoxFuture<'static, ()> {
        let mut state = self.state.lock().unwrap();
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

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub chunks: Vec<Result<Vec<u8>, LinkPreviewFailure>>,
    pub peer: SocketAddr,
}

impl Reply {
    pub fn html(body: impl Into<String>) -> Self {
        Self::bytes("text/html; charset=utf-8", body.into().into_bytes())
    }

    pub fn bytes(mime: &str, bytes: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".into(), mime.into())],
            chunks: vec![Ok(bytes)],
            peer: "93.184.216.34:443".parse().unwrap(),
        }
    }

    pub fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            headers: vec![("location".into(), location.into())],
            chunks: Vec::new(),
            peer: "93.184.216.34:443".parse().unwrap(),
        }
    }

    pub fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

struct Script {
    reply: Result<Reply, LinkPreviewFailure>,
    gate: Option<oneshot::Receiver<()>>,
}

#[derive(Default)]
struct NetworkState {
    dns: HashMap<String, VecDeque<Vec<IpAddr>>>,
    dns_calls: Vec<(String, u16)>,
    dns_gates: HashMap<String, oneshot::Receiver<()>>,
    routes: HashMap<String, VecDeque<Script>>,
    requests: Vec<PreparedRequest>,
    request_tokens: Vec<CancellationToken>,
    cancellations: usize,
}

#[derive(Clone, Default)]
pub struct ScriptedNetwork {
    state: Arc<Mutex<NetworkState>>,
}

impl ScriptedNetwork {
    pub fn answer(&self, host: &str, addresses: &[&str]) {
        self.state
            .lock()
            .unwrap()
            .dns
            .entry(host.into())
            .or_default()
            .push_back(
                addresses
                    .iter()
                    .map(|address| address.parse().unwrap())
                    .collect(),
            );
    }

    pub fn gate_dns(&self, host: &str) -> oneshot::Sender<()> {
        let (sender, receiver) = oneshot::channel();
        self.state
            .lock()
            .unwrap()
            .dns_gates
            .insert(host.into(), receiver);
        sender
    }

    pub fn respond(&self, url: &str, reply: Reply) {
        self.enqueue(url, Ok(reply), None);
    }

    pub fn fail(&self, url: &str, failure: LinkPreviewFailure) {
        self.enqueue(url, Err(failure), None);
    }

    pub fn gated(&self, url: &str, reply: Reply) -> oneshot::Sender<()> {
        let (sender, receiver) = oneshot::channel();
        self.enqueue(url, Ok(reply), Some(receiver));
        sender
    }

    fn enqueue(
        &self,
        url: &str,
        reply: Result<Reply, LinkPreviewFailure>,
        gate: Option<oneshot::Receiver<()>>,
    ) {
        self.state
            .lock()
            .unwrap()
            .routes
            .entry(url.into())
            .or_default()
            .push_back(Script { reply, gate });
    }

    pub fn requests(&self) -> Vec<PreparedRequest> {
        self.state.lock().unwrap().requests.clone()
    }

    pub fn dns_calls(&self) -> Vec<(String, u16)> {
        self.state.lock().unwrap().dns_calls.clone()
    }

    pub fn cancellations(&self) -> usize {
        self.state.lock().unwrap().cancellations
    }

    pub fn request_cancelled(&self, index: usize) -> bool {
        self.state.lock().unwrap().request_tokens[index].is_cancelled()
    }

    pub fn service(&self, clock: &ManualClock) -> LinkPreviewService {
        service_with_ports(
            Arc::new(self.clone()),
            Arc::new(self.clone()),
            Arc::new(clock.clone()),
        )
    }
}

impl PreviewResolver for ScriptedNetwork {
    fn resolve(
        &self,
        host: String,
        port: u16,
        cancellation: CancellationToken,
    ) -> BoxFuture<'static, Result<Vec<SocketAddr>, LinkPreviewFailure>> {
        let (addresses, gate) = {
            let mut state = self.state.lock().unwrap();
            state.dns_calls.push((host.clone(), port));
            let answers = state.dns.entry(host.clone()).or_default();
            let addresses = if answers.len() > 1 {
                answers.pop_front().unwrap()
            } else {
                answers
                    .front()
                    .cloned()
                    .unwrap_or_else(|| vec!["93.184.216.34".parse().unwrap()])
            };
            (addresses, state.dns_gates.remove(&host))
        };
        async move {
            if let Some(gate) = gate {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(LinkPreviewFailure::Cancelled),
                    _ = gate => {}
                }
            }
            Ok(addresses
                .into_iter()
                .map(|address| SocketAddr::new(address, port))
                .collect())
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
        let script = {
            let mut state = self.state.lock().unwrap();
            let script = state
                .routes
                .get_mut(&request.url)
                .and_then(VecDeque::pop_front);
            state.requests.push(request);
            state.request_tokens.push(cancellation.clone());
            script
        };
        let state = self.state.clone();
        async move {
            let Some(script) = script else {
                return Err(LinkPreviewFailure::Network);
            };
            if let Some(gate) = script.gate {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        state.lock().unwrap().cancellations += 1;
                        return Err(LinkPreviewFailure::Cancelled);
                    },
                    _ = gate => {}
                }
            }
            let reply = script.reply?;
            Ok(TransportResponse {
                status: reply.status,
                headers: reply.headers,
                peer: reply.peer,
                body: stream::iter(reply.chunks).boxed(),
            })
        }
        .boxed()
    }
}
