mod metadata;
mod ports;

use std::{
    collections::{HashMap, VecDeque},
    io::Cursor,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use futures::{FutureExt, StreamExt, channel::oneshot, future::BoxFuture, future::Shared};
use image::{ImageFormat, ImageReader, Limits};
use reqwest::Url;
use tokio_util::sync::CancellationToken;

use crate::types::{
    LinkPreviewData, LinkPreviewFailure, LinkPreviewImage, LinkPreviewTarget, LinkSiteKind,
};
use ports::{PreparedRequest, PreviewClock, PreviewResolver, PreviewTransport};

type PreviewResult = Result<LinkPreviewData, LinkPreviewFailure>;
type SharedLoad = Shared<BoxFuture<'static, PreviewResult>>;

pub const MAX_PREVIEW_URL_BYTES: usize = 8192;
const HTML_LIMIT: usize = 2 * 1024 * 1024;
const IMAGE_LIMIT: usize = 5 * 1024 * 1024;
const CACHE_BYTES: usize = 32 * 1024 * 1024;
const SUCCESS_TTL: Duration = Duration::from_secs(1800);
const FAILURE_TTL: Duration = Duration::from_secs(30);
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const DNS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct LinkPreviewService(Arc<ServiceInner>);

struct ServiceInner {
    resolver: Arc<dyn PreviewResolver>,
    transport: Arc<dyn PreviewTransport>,
    clock: Arc<dyn PreviewClock>,
    state: Mutex<ServiceState>,
}

#[derive(Default)]
struct ServiceState {
    cache: HashMap<String, Cached>,
    flights: HashMap<String, Flight>,
    queue: VecDeque<(String, oneshot::Sender<()>)>,
    active: usize,
    sequence: u64,
    weight: usize,
}

struct Cached {
    result: PreviewResult,
    expires: Duration,
    touched: u64,
    weight: usize,
}

struct Flight {
    future: SharedLoad,
    cancellation: CancellationToken,
    waiters: usize,
    active: bool,
    id: u64,
}

struct Waiter {
    service: LinkPreviewService,
    key: String,
    id: u64,
}

impl Drop for Waiter {
    fn drop(&mut self) {
        let mut state = self.service.state();
        let remove = state.flights.get_mut(&self.key).is_some_and(|flight| {
            if flight.id != self.id {
                return false;
            }
            flight.waiters = flight.waiters.saturating_sub(1);
            flight.waiters == 0
        });
        if remove && let Some(flight) = state.flights.remove(&self.key) {
            flight.cancellation.cancel();
            if flight.active {
                state.active = state.active.saturating_sub(1);
            }
            state.queue.retain(|(key, _)| key != &self.key);
            state.start_queued();
        }
    }
}

impl ServiceState {
    fn start_queued(&mut self) {
        while self.active < 2 {
            let Some((key, ready)) = self.queue.pop_front() else {
                break;
            };
            if let Some(flight) = self.flights.get_mut(&key) {
                flight.active = true;
                self.active += 1;
                let _ = ready.send(());
            }
        }
    }

    fn prune_cache(&mut self, now: Duration) {
        self.cache.retain(|_, item| item.expires > now);
        self.weight = self.cache.values().map(|item| item.weight).sum();
        while self.cache.len() > 64 || self.weight > CACHE_BYTES {
            let oldest = self
                .cache
                .iter()
                .min_by_key(|(_, item)| item.touched)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            if let Some(item) = self.cache.remove(&oldest) {
                self.weight = self.weight.saturating_sub(item.weight);
            }
        }
    }
}

impl Default for LinkPreviewService {
    fn default() -> Self {
        Self::with_ports(
            Arc::new(ports::SystemResolver),
            Arc::new(ports::SystemTransport),
            Arc::new(ports::SystemClock),
        )
    }
}

impl LinkPreviewService {
    fn with_ports(
        resolver: Arc<dyn PreviewResolver>,
        transport: Arc<dyn PreviewTransport>,
        clock: Arc<dyn PreviewClock>,
    ) -> Self {
        Self(Arc::new(ServiceInner {
            resolver,
            transport,
            clock,
            state: Mutex::new(ServiceState::default()),
        }))
    }

    fn state(&self) -> MutexGuard<'_, ServiceState> {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub async fn load(
        &self,
        original: &str,
        retry: bool,
        cancellation: CancellationToken,
    ) -> PreviewResult {
        if cancellation.is_cancelled() {
            return Err(LinkPreviewFailure::Cancelled);
        }
        let target = classify_target(original)?;
        if target.site == LinkSiteKind::Mail {
            return Ok(LinkPreviewData {
                original_target: original.into(),
                effective_target: original.into(),
                host: original
                    .split_once(':')
                    .map_or(original, |(_, value)| value)
                    .into(),
                site: LinkSiteKind::Mail,
                title: None,
                description: None,
                image: None,
            });
        }
        let mut url = preview_url(original)?;
        url.set_fragment(None);
        let key = url.to_string();
        let (future, flight_id) = {
            let mut state = self.state();
            state.sequence = state.sequence.wrapping_add(1);
            let sequence = state.sequence;
            state.prune_cache(self.0.clock.now());
            if let Some(cached) = state.cache.get_mut(&key)
                && (!retry || cached.result.is_ok())
            {
                cached.touched = sequence;
                return retarget(cached.result.clone(), original);
            }
            if let Some(flight) = state.flights.get_mut(&key) {
                flight.waiters += 1;
                (flight.future.clone(), flight.id)
            } else {
                if state.active >= 2 && state.queue.len() >= 8 {
                    return Err(LinkPreviewFailure::Busy);
                }
                let active = state.active < 2;
                let queue_ready = if active {
                    state.active += 1;
                    None
                } else {
                    let (sender, receiver) = oneshot::channel();
                    state.queue.push_back((key.clone(), sender));
                    Some(receiver)
                };
                let service = self.clone();
                let flight_key = key.clone();
                let token = CancellationToken::new();
                let task_token = token.clone();
                let deadline = self.0.clock.now() + LOAD_TIMEOUT;
                let future = async move {
                    let work = async {
                        if let Some(ready) = queue_ready { tokio::select! { _ = service.0.clock.sleep_until(deadline) => return Err(LinkPreviewFailure::Timeout), result = ready => result.map_err(|_| LinkPreviewFailure::Cancelled)?, } }
                        service.fetch_page(url, task_token.clone(), deadline).await
                    };
                    let result = tokio::select! {
                        biased;
                        _ = task_token.cancelled() => Err(LinkPreviewFailure::Cancelled),
                        result = work => result,
                    };
                    service.complete(&flight_key, sequence, &result);
                    result
                }.boxed().shared();
                state.flights.insert(
                    key.clone(),
                    Flight {
                        future: future.clone(),
                        cancellation: token,
                        waiters: 1,
                        active,
                        id: sequence,
                    },
                );
                (future, sequence)
            }
        };
        let _waiter = Waiter {
            service: self.clone(),
            key,
            id: flight_id,
        };
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(LinkPreviewFailure::Cancelled),
            result = future => result,
        };
        retarget(result, original)
    }

    fn complete(&self, key: &str, id: u64, result: &PreviewResult) {
        let mut state = self.state();
        if state.flights.get(key).is_none_or(|flight| flight.id != id) {
            return;
        }
        let Some(flight) = state.flights.remove(key) else {
            return;
        };
        if flight.active {
            state.active = state.active.saturating_sub(1);
        }
        state.queue.retain(|(queued, _)| queued != key);
        if flight.waiters > 0
            && !flight.cancellation.is_cancelled()
            && !matches!(
                result,
                Err(LinkPreviewFailure::Cancelled | LinkPreviewFailure::Busy)
            )
        {
            let weight = result_weight(result).saturating_add(key.len());
            if weight <= CACHE_BYTES {
                state.sequence = state.sequence.wrapping_add(1);
                let item = Cached {
                    result: result.clone(),
                    expires: self.0.clock.now()
                        + if result.is_ok() {
                            SUCCESS_TTL
                        } else {
                            FAILURE_TTL
                        },
                    touched: state.sequence,
                    weight,
                };
                state.cache.insert(key.into(), item);
                state.prune_cache(self.0.clock.now());
            }
        }
        flight.cancellation.cancel();
        state.start_queued();
    }

    async fn fetch_page(
        &self,
        url: Url,
        cancellation: CancellationToken,
        deadline: Duration,
    ) -> PreviewResult {
        let (effective, headers, bytes) = tokio::select! { biased; _ = self.0.clock.sleep_until(deadline) => return Err(LinkPreviewFailure::Timeout), result = self.resource(url.clone(), HTML_LIMIT, false, cancellation.clone()) => result?, };
        let content_type =
            header(&headers, "content-type").ok_or(LinkPreviewFailure::Unsupported)?;
        let mime = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if !matches!(mime.as_str(), "text/html" | "application/xhtml+xml") {
            return Err(LinkPreviewFailure::Unsupported);
        }
        for part in content_type.split(';').skip(1) {
            if let Some((name, value)) = part.split_once('=')
                && name.trim().eq_ignore_ascii_case("charset")
                && !metadata::supported_encoding(value)
            {
                return Err(LinkPreviewFailure::Malformed);
            }
        }
        let html = std::str::from_utf8(&bytes).map_err(|_| LinkPreviewFailure::Malformed)?;
        let metadata = metadata::parse(html.trim_start_matches('\u{feff}'), &effective)?;
        let mut result = LinkPreviewData {
            original_target: url.to_string(),
            effective_target: effective.to_string(),
            host: host_label(&effective),
            site: site_for_url(&effective),
            title: metadata.title,
            description: metadata.description,
            image: None,
        };
        if let Some(image_url) = metadata.image {
            let image_cancellation = cancellation.child_token();
            let image = tokio::select! { biased; _ = self.0.clock.sleep_until(deadline) => None, result = self.resource(image_url, IMAGE_LIMIT, true, image_cancellation.clone()) => result.ok(), };
            image_cancellation.cancel();
            if let Some((_, image_headers, image_bytes)) = image {
                result.image = sanitize_image(&image_headers, &image_bytes);
            }
        }
        Ok(result)
    }

    async fn resource(
        &self,
        mut url: Url,
        limit: usize,
        image: bool,
        cancellation: CancellationToken,
    ) -> Result<(Url, Vec<(String, String)>, Vec<u8>), LinkPreviewFailure> {
        for hop in 0..=3 {
            url = preview_url(url.as_str())?;
            url.set_fragment(None);
            let host = url
                .host_str()
                .ok_or(LinkPreviewFailure::Blocked)?
                .trim_matches(['[', ']'])
                .to_owned();
            let port = url
                .port_or_known_default()
                .ok_or(LinkPreviewFailure::Blocked)?;
            let addresses = if let Ok(address) = host.parse::<IpAddr>() {
                vec![SocketAddr::new(canonical_ip(address), port)]
            } else {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(LinkPreviewFailure::Cancelled),
                    _ = self.0.clock.sleep_until(self.0.clock.now() + DNS_TIMEOUT) => return Err(LinkPreviewFailure::Timeout),
                    addresses = self.0.resolver.resolve(host, port, cancellation.clone()) => addresses?,
                }
            };
            if addresses.is_empty()
                || addresses
                    .iter()
                    .any(|address| address.port() != port || !public_ip(address.ip()))
            {
                return Err(LinkPreviewFailure::Blocked);
            }
            let addresses = addresses
                .into_iter()
                .map(|address| SocketAddr::new(canonical_ip(address.ip()), port))
                .collect::<Vec<_>>();
            let request = PreparedRequest {
                url: url.to_string(),
                pinned_addresses: addresses.clone(),
                headers: vec![
                    (
                        "accept".into(),
                        if image {
                            "image/png,image/jpeg,image/webp"
                        } else {
                            "text/html,application/xhtml+xml"
                        }
                        .into(),
                    ),
                    ("accept-encoding".into(), "identity".into()),
                    ("user-agent".into(), "Vega-Link-Preview/1.0".into()),
                ],
            };
            let mut response = self.0.transport.get(request, cancellation.clone()).await?;
            let peer = SocketAddr::new(canonical_ip(response.peer.ip()), response.peer.port());
            if !addresses.contains(&peer) {
                return Err(LinkPreviewFailure::Blocked);
            }
            if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                if hop == 3 {
                    return Err(LinkPreviewFailure::Unsupported);
                }
                let location =
                    header(&response.headers, "location").ok_or(LinkPreviewFailure::Malformed)?;
                url = url
                    .join(location)
                    .map_err(|_| LinkPreviewFailure::Malformed)?;
                continue;
            }
            if matches!(response.status, 401 | 403) {
                return Err(LinkPreviewFailure::RequiresLogin);
            }
            if !(200..300).contains(&response.status) {
                return Err(LinkPreviewFailure::Network);
            }
            if header(&response.headers, "content-length")
                .and_then(|length| length.parse::<u64>().ok())
                .is_some_and(|length| length > limit as u64)
            {
                return Err(LinkPreviewFailure::TooLarge);
            }
            if header(&response.headers, "content-encoding")
                .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("identity"))
            {
                return Err(LinkPreviewFailure::Unsupported);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.body.next().await {
                let chunk = chunk?;
                if bytes.len().saturating_add(chunk.len()) > limit {
                    return Err(LinkPreviewFailure::TooLarge);
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok((url, response.headers, bytes));
        }
        Err(LinkPreviewFailure::Unsupported)
    }
}

fn retarget(mut result: PreviewResult, original: &str) -> PreviewResult {
    if let Ok(data) = &mut result {
        data.original_target = original.into();
    }
    result
}

fn result_weight(result: &PreviewResult) -> usize {
    match result {
        Ok(data) => {
            data.original_target.len()
                + data.effective_target.len()
                + data.host.len()
                + data.title.as_ref().map_or(0, String::len)
                + data.description.as_ref().map_or(0, String::len)
                + data.image.as_ref().map_or(0, |image| image.png.len())
                + 128
        }
        Err(_) => 128,
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn preview_url(target: &str) -> Result<Url, LinkPreviewFailure> {
    if target.len() > MAX_PREVIEW_URL_BYTES
        || target
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(LinkPreviewFailure::Blocked);
    }
    let url = Url::parse(target).map_err(|_| LinkPreviewFailure::Unsupported)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(LinkPreviewFailure::Unsupported);
    }
    let host = url
        .host_str()
        .ok_or(LinkPreviewFailure::Blocked)?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !url.username().is_empty()
        || url.password().is_some()
        || host == "localhost"
        || host.ends_with(".localhost")
    {
        return Err(LinkPreviewFailure::Blocked);
    }
    Ok(url)
}

pub fn classify_target(target: &str) -> Result<LinkPreviewTarget, LinkPreviewFailure> {
    if target.len() > MAX_PREVIEW_URL_BYTES
        || target
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(LinkPreviewFailure::Blocked);
    }
    if target
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("mailto:"))
        && target.len() > 7
    {
        return Ok(LinkPreviewTarget {
            original_target: target.into(),
            site: LinkSiteKind::Mail,
        });
    }
    let url = preview_url(target)?;
    Ok(LinkPreviewTarget {
        original_target: target.into(),
        site: site_for_url(&url),
    })
}

pub fn display_host(target: &str) -> String {
    Url::parse(target)
        .ok()
        .map(|url| host_label(&url))
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| target.strip_prefix("mailto:").unwrap_or(target).to_owned())
}

fn host_label(url: &Url) -> String {
    url.host_str().unwrap_or_default().to_owned()
}

fn site_for_url(url: &Url) -> LinkSiteKind {
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host == "github.com" || host.ends_with(".github.com") {
        LinkSiteKind::Github
    } else {
        LinkSiteKind::Web
    }
}

fn canonical_ip(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map_or(IpAddr::V6(address), IpAddr::V4),
        address => address,
    }
}

fn public_ip(address: IpAddr) -> bool {
    match canonical_ip(address) {
        IpAddr::V4(address) => public_v4(address),
        IpAddr::V6(address) => public_v6(address),
    }
}

fn public_v4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(matches!(a, 0 | 10 | 127)
        || a >= 224
        || a == 100 && (64..128).contains(&b)
        || a == 169 && b == 254
        || a == 172 && (16..32).contains(&b)
        || a == 192 && (b == 168 || b == 0 && matches!(c, 0 | 2) || b == 88 && c == 99)
        || a == 198 && (matches!(b, 18 | 19) || b == 51 && c == 100)
        || a == 203 && b == 0 && c == 113)
}

fn public_v6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    segments[0] & 0xe000 == 0x2000
        && !(segments[0] == 0x2001 && (segments[1] <= 0x01ff || segments[1] == 0x0db8)
            || segments[0] == 0x2002
            || segments[0] == 0x3ffe
            || segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
}

fn sanitize_image(headers: &[(String, String)], bytes: &[u8]) -> Option<LinkPreviewImage> {
    let mime = header(headers, "content-type")?
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase();
    let format = match mime.as_str() {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/webp" => ImageFormat::WebP,
        _ => return None,
    };
    if image::guess_format(bytes).ok()? != format {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(64 * 1024 * 1024);
    let mut probe = ImageReader::with_format(Cursor::new(bytes), format);
    probe.limits(limits.clone());
    let (width, height) = probe.into_dimensions().ok()?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 8_000_000 {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let image = if width > 512 || height > 288 {
        image.thumbnail(512, 288)
    } else {
        image
    };
    let image = image::DynamicImage::ImageRgba8(image.to_rgba8());
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).ok()?;
    Some(LinkPreviewImage {
        png: output.into_inner(),
        width: image.width(),
        height: image.height(),
    })
}
