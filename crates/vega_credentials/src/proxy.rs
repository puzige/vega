use crate::{CredentialBroker, CredentialError};
use base64::Engine;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use reqwest::{
    Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

const HEADER_LIMIT: usize = 32 * 1024;
const BODY_LIMIT: usize = 8 * 1024 * 1024;

pub struct ProxyLease {
    pub port: u16,
    pub ca_pem: String,
    broker: Arc<CredentialBroker>,
    cancel: CancellationToken,
}

impl ProxyLease {
    pub async fn start(
        broker: Arc<CredentialBroker>,
        parent: CancellationToken,
    ) -> Result<Self, CredentialError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| CredentialError::ProxyUnavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| CredentialError::ProxyUnavailable)?
            .port();
        let key = KeyPair::generate().map_err(|_| CredentialError::ProxyUnavailable)?;
        let mut params = CertificateParams::new(Vec::<String>::new())
            .map_err(|_| CredentialError::ProxyUnavailable)?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = params
            .self_signed(&key)
            .map_err(|_| CredentialError::ProxyUnavailable)?;
        let ca_pem = ca.pem();
        let issuer = Arc::new(Issuer::new(params, key));
        let cancel = parent.child_token();
        let listener_cancel = cancel.clone();
        let listener_broker = broker.clone();
        tokio::spawn(async move {
            let permits = Arc::new(tokio::sync::Semaphore::new(16));
            loop {
                let accepted = tokio::select! {
                    _ = listener_cancel.cancelled() => break,
                    result = listener.accept() => result,
                };
                let Ok((stream, _)) = accepted else {
                    break;
                };
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    continue;
                };
                let broker = listener_broker.clone();
                let issuer = issuer.clone();
                let stop = listener_cancel.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    tokio::select! {
                        _ = stop.cancelled() => {},
                        _ = tokio::time::timeout(Duration::from_secs(120), serve(stream, broker, issuer)) => {},
                    }
                });
            }
        });
        Ok(Self {
            port,
            ca_pem,
            broker,
            cancel,
        })
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
        self.broker.revoke();
    }

    pub fn environment(&self, ca_path: &str) -> Result<BTreeMap<String, String>, CredentialError> {
        let mut environment = self.broker.environment()?;
        let proxy = format!(
            "http://vega:{}@127.0.0.1:{}",
            self.broker.lease_token, self.port
        );
        for name in [
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            environment.insert(name.into(), proxy.clone());
        }
        for name in [
            "SSL_CERT_FILE",
            "REQUESTS_CA_BUNDLE",
            "CURL_CA_BUNDLE",
            "NODE_EXTRA_CA_CERTS",
        ] {
            environment.insert(name.into(), ca_path.into());
        }
        environment.insert("NO_PROXY".into(), String::new());
        environment.insert("no_proxy".into(), String::new());
        Ok(environment)
    }
}

impl Drop for ProxyLease {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.broker.revoke();
    }
}

struct RequestHead {
    method: String,
    target: String,
    headers: HeaderMap,
    length: usize,
}

fn parse_head(bytes: &[u8]) -> Result<RequestHead, CredentialError> {
    let mut slots = [httparse::EMPTY_HEADER; 96];
    let mut parsed = httparse::Request::new(&mut slots);
    if !matches!(parsed.parse(bytes), Ok(httparse::Status::Complete(end)) if end == bytes.len())
        || parsed.version != Some(1)
    {
        return Err(CredentialError::Protocol);
    }
    let mut headers = HeaderMap::new();
    let mut length = 0;
    for header in parsed.headers.iter() {
        let name = HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|_| CredentialError::Protocol)?;
        if headers.contains_key(&name) {
            return Err(CredentialError::Protocol);
        }
        let value = HeaderValue::from_bytes(header.value).map_err(|_| CredentialError::Protocol)?;
        if matches!(
            name.as_str(),
            "transfer-encoding" | "upgrade" | "expect" | "trailer"
        ) {
            return Err(CredentialError::Protocol);
        }
        if name == "content-length" {
            let text = value.to_str().map_err(|_| CredentialError::Protocol)?;
            if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(CredentialError::Protocol);
            }
            length = text
                .parse::<usize>()
                .map_err(|_| CredentialError::Protocol)?;
            if length > BODY_LIMIT {
                return Err(CredentialError::Protocol);
            }
        }
        if name == "connection" && !matches!(value.to_str(), Ok("close" | "keep-alive")) {
            return Err(CredentialError::Protocol);
        }
        headers.insert(name, value);
    }
    Ok(RequestHead {
        method: parsed.method.ok_or(CredentialError::Protocol)?.into(),
        target: parsed.path.ok_or(CredentialError::Protocol)?.into(),
        headers,
        length,
    })
}

async fn read_head(stream: &mut (impl AsyncRead + Unpin)) -> Result<RequestHead, CredentialError> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= HEADER_LIMIT {
            return Err(CredentialError::Protocol);
        }
        bytes.push(
            stream
                .read_u8()
                .await
                .map_err(|_| CredentialError::Protocol)?,
        );
    }
    parse_head(&bytes)
}

fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_documentation()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && ip.octets()[0] != 0
                && ip.octets()[0] < 224
                && !(ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
                && !(ip.octets()[0] == 198 && (18..=19).contains(&ip.octets()[1]))
                && !(ip.octets()[0] == 192 && ip.octets()[1] == 0 && ip.octets()[2] == 0)
                && !(ip.octets()[0] == 192 && ip.octets()[1] == 88 && ip.octets()[2] == 99)
        }
        IpAddr::V6(ip) => {
            ip.to_ipv4_mapped().is_none()
                && !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && (ip.segments()[0] & 0xe000) == 0x2000
                && !(ip.segments()[0] == 0x2001
                    && (ip.segments()[1] < 0x0200 || ip.segments()[1] == 0xdb8))
                && ip.segments()[0] != 0x2002
                && ip.segments()[0] != 0x3fff
        }
    }
}

fn target_url(authority: &str, target: &str, host: &str) -> Result<Url, CredentialError> {
    if authority != host
        || !target.starts_with('/')
        || target.starts_with("//")
        || target.contains(['\\', '#'])
        || target.to_ascii_lowercase().contains("%2f")
        || target.to_ascii_lowercase().contains("%2e")
        || target.to_ascii_lowercase().contains("%5c")
        || target.split('/').any(|part| part == "." || part == "..")
    {
        return Err(CredentialError::DestinationDenied);
    }
    let url = Url::parse(&format!("https://{authority}{target}"))
        .map_err(|_| CredentialError::DestinationDenied)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CredentialError::DestinationDenied);
    }
    Ok(url)
}

async fn serve(
    mut stream: TcpStream,
    broker: Arc<CredentialBroker>,
    issuer: Arc<Issuer<'static, KeyPair>>,
) -> Result<(), CredentialError> {
    broker.check_active()?;
    let connect = read_head(&mut stream).await?;
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("vega:{}", broker.lease_token))
    );
    if connect.method != "CONNECT"
        || connect.length != 0
        || connect
            .headers
            .get("proxy-authorization")
            .is_none_or(|value| value.as_bytes() != expected.as_bytes())
    {
        return Err(CredentialError::DestinationDenied);
    }
    let authority = connect.target;
    let connect_host = connect
        .headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .ok_or(CredentialError::Protocol)?;
    let origin = target_url(&authority, "/", connect_host)?;
    if !broker.permits_host(&origin) {
        return Err(CredentialError::DestinationDenied);
    }
    let host = origin
        .host_str()
        .ok_or(CredentialError::DestinationDenied)?
        .to_string();
    let key = KeyPair::generate().map_err(|_| CredentialError::ProxyUnavailable)?;
    let cert = CertificateParams::new(vec![host.clone()])
        .map_err(|_| CredentialError::ProxyUnavailable)?
        .signed_by(&key, issuer.as_ref())
        .map_err(|_| CredentialError::ProxyUnavailable)?;
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| CredentialError::ProxyUnavailable)?
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
    )
    .map_err(|_| CredentialError::ProxyUnavailable)?;
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .map_err(|_| CredentialError::Protocol)?;
    let mut stream = tokio_rustls::TlsAcceptor::from(Arc::new(tls))
        .accept(stream)
        .await
        .map_err(|_| CredentialError::Protocol)?;
    if stream.get_ref().1.server_name() != Some(host.as_str()) {
        return Err(CredentialError::DestinationDenied);
    }
    let mut request = read_head(&mut stream).await?;
    let requested_host = request
        .headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .ok_or(CredentialError::Protocol)?;
    let canonical_host = if origin.port_or_known_default() == Some(443) && requested_host == host {
        authority.as_str()
    } else {
        requested_host
    };
    let url = target_url(&authority, &request.target, canonical_host)?;
    if request.method == "CONNECT" {
        return Err(CredentialError::Protocol);
    }
    let port = url
        .port_or_known_default()
        .ok_or(CredentialError::DestinationDenied)?;
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| CredentialError::ProxyUnavailable)?
        .collect();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| !public_address(address.ip()))
    {
        return Err(CredentialError::DestinationDenied);
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(90))
        .resolve_to_addrs(&host, &addresses)
        .build()
        .map_err(|_| CredentialError::ProxyUnavailable)?;
    for name in [
        "host",
        "connection",
        "proxy-connection",
        "proxy-authorization",
        "content-length",
        "accept-encoding",
        "keep-alive",
        "te",
    ] {
        request.headers.remove(name);
    }
    request
        .headers
        .insert("accept-encoding", HeaderValue::from_static("identity"));
    let mut body = vec![0; request.length];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|_| CredentialError::Protocol)?;
    let method = reqwest::Method::from_bytes(request.method.as_bytes())
        .map_err(|_| CredentialError::Protocol)?;
    broker.inject(&url, &mut request.headers)?;
    let mut response = client
        .request(method, url)
        .headers(request.headers)
        .body(body)
        .send()
        .await
        .map_err(|_| CredentialError::ProxyUnavailable)?;
    if response
        .headers()
        .get("content-encoding")
        .is_some_and(|value| value != "identity")
        || response
            .content_length()
            .is_some_and(|length| length > BODY_LIMIT as u64)
    {
        return Err(CredentialError::Protocol);
    }
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| CredentialError::ProxyUnavailable)?
    {
        if body.len() + chunk.len() > BODY_LIMIT {
            return Err(CredentialError::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    let body = broker.redact(&body);
    let mut head = format!(
        "HTTP/1.1 {status} Response\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )
    .into_bytes();
    for (name, value) in &headers {
        if matches!(
            name.as_str(),
            "content-length"
                | "transfer-encoding"
                | "connection"
                | "keep-alive"
                | "upgrade"
                | "trailer"
                | "proxy-authenticate"
                | "content-encoding"
        ) {
            continue;
        }
        head.extend_from_slice(name.as_str().as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(&broker.redact(value.as_bytes()));
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(b"\r\n");
    stream
        .write_all(&head)
        .await
        .map_err(|_| CredentialError::Protocol)?;
    stream
        .write_all(&body)
        .await
        .map_err(|_| CredentialError::Protocol)?;
    stream
        .shutdown()
        .await
        .map_err(|_| CredentialError::Protocol)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ambiguous_framing_and_unsupported_protocols_are_rejected() {
        for headers in [
            "Content-Length: 1\r\nContent-Length: 1",
            "Content-Length: 1\r\nTransfer-Encoding: chunked",
            "Upgrade: websocket",
            "Content-Length: -1",
            "Connection: authorization",
            "Content-Length: 99999999",
        ] {
            assert!(
                parse_head(
                    format!("POST / HTTP/1.1\r\nHost: api.github.com\r\n{headers}\r\n\r\n")
                        .as_bytes()
                )
                .is_err()
            );
        }
        assert!(parse_head(b"GET / HTTP/1.1\r\nHost: api.github.com\r\n\r\n").is_ok());
    }
    #[test]
    fn authority_and_path_confusion_are_rejected() {
        for target in [
            "https://evil.test/",
            "//evil.test/",
            "/repos/../admin",
            "/repos/%2e%2e/admin",
            "/repos%2fadmin",
            "/repos\\admin",
        ] {
            assert!(target_url("api.github.com:443", target, "api.github.com:443").is_err());
        }
        assert!(target_url("api.github.com:443", "/repos", "evil.test:443").is_err());
        assert!(target_url("api.github.com:443", "/repos", "api.github.com:443").is_ok());
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public_address(ip.parse().unwrap()));
        }
    }
}
