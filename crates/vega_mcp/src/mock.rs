use futures::future::BoxFuture;
pub use reqwest::{Request, Response};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type Handler =
    Arc<dyn Fn(Request) -> BoxFuture<'static, Result<Response, reqwest::Error>> + Send + Sync>;
static HANDLERS: OnceLock<Mutex<HashMap<String, Handler>>> = OnceLock::new();
static NEXT_PORT: AtomicUsize = AtomicUsize::new(20000);
fn handlers() -> &'static Mutex<HashMap<String, Handler>> {
    HANDLERS.get_or_init(Mutex::default)
}
struct Registration(String);
impl Drop for Registration {
    fn drop(&mut self) {
        handlers().lock().expect("fixture registry").remove(&self.0);
    }
}
#[derive(Clone)]
pub struct Endpoint {
    url: String,
    registration: Arc<Registration>,
}
impl std::ops::Deref for Endpoint {
    type Target = str;
    fn deref(&self) -> &str {
        &self.url
    }
}
impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.url)
    }
}
impl Endpoint {
    pub fn new(path: &str, factory: impl FnOnce(&str) -> Handler) -> Self {
        let origin = format!(
            "http://127.0.0.1:{}",
            NEXT_PORT.fetch_add(1, Ordering::Relaxed)
        );
        let handler = factory(&origin);
        handlers()
            .lock()
            .expect("fixture registry")
            .insert(origin.clone(), handler);
        Self {
            url: format!("{origin}{path}"),
            registration: Arc::new(Registration(origin)),
        }
    }
    pub fn origin(&self) -> &str {
        &self.registration.0
    }
}
pub(crate) async fn send(request: Request) -> Result<Response, reqwest::Error> {
    let key = request.url().origin().ascii_serialization();
    let handler = handlers()
        .lock()
        .expect("fixture registry")
        .get(&key)
        .cloned();
    match handler {
        Some(handler) => handler(request).await,
        None => Err(transport_error()),
    }
}
pub fn transport_error() -> reqwest::Error {
    reqwest::Client::new()
        .get("invalid url")
        .build()
        .expect_err("invalid fixture URL")
}
pub fn response(status: u16, kind: &str, body: String, headers: Vec<(&str, String)>) -> Response {
    let mut response = ::http::Response::builder()
        .status(status)
        .header("content-type", kind);
    for (key, value) in headers {
        response = response.header(key, value);
    }
    response.body(body).expect("fixture response").into()
}
pub fn stream_response<S>(stream: S) -> Response
where
    S: futures::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send + 'static,
{
    ::http::Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .body(reqwest::Body::wrap_stream(stream))
        .expect("fixture stream")
        .into()
}

pub fn raw_response(raw: String) -> Response {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .expect("fixture response boundary");
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .expect("fixture status")
        .split_whitespace()
        .nth(1)
        .expect("status code")
        .parse()
        .expect("numeric status");
    let mut response = ::http::Response::builder().status(status);
    for line in lines {
        let (key, value) = line.split_once(':').expect("fixture header");
        response = response.header(key, value.trim());
    }
    response.body(body.to_owned()).expect("fixture body").into()
}

type StdioHandler = Arc<
    dyn Fn(crate::LocalServer, tokio::io::DuplexStream) -> BoxFuture<'static, ()> + Send + Sync,
>;
static STDIO: OnceLock<Mutex<HashMap<std::path::PathBuf, StdioHandler>>> = OnceLock::new();
fn stdio_handlers() -> &'static Mutex<HashMap<std::path::PathBuf, StdioHandler>> {
    STDIO.get_or_init(Mutex::default)
}
pub struct StdioFixture(std::path::PathBuf);
impl StdioFixture {
    pub fn new(path: impl Into<std::path::PathBuf>, handler: StdioHandler) -> Self {
        let path = path.into();
        assert!(
            stdio_handlers()
                .lock()
                .expect("stdio fixtures")
                .insert(path.clone(), handler)
                .is_none()
        );
        Self(path)
    }
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for StdioFixture {
    fn drop(&mut self) {
        stdio_handlers()
            .lock()
            .expect("stdio fixtures")
            .remove(&self.0);
    }
}
pub(crate) struct Child {
    pub stdin: Option<tokio::io::WriteHalf<tokio::io::DuplexStream>>,
    pub stdout: Option<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    task: tokio::task::JoinHandle<()>,
    cancel: tokio_util::sync::CancellationToken,
    waited: bool,
}
impl Child {
    pub async fn wait(&mut self) -> std::io::Result<()> {
        if self.waited {
            return Ok(());
        }
        let result = (&mut self.task).await;
        self.waited = true;
        result.map_err(std::io::Error::other)
    }
    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.cancel.cancel();
        Ok(())
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
pub(crate) fn stdio_connect(server: &crate::LocalServer) -> Result<Child, crate::McpError> {
    let handler = {
        let registry = stdio_handlers().lock().expect("stdio fixtures");
        registry
            .get(&server.executable)
            .or_else(|| {
                server
                    .args
                    .first()
                    .and_then(|arg| registry.get(std::path::Path::new(arg)))
            })
            .cloned()
    }
    .ok_or(crate::McpError::Transport)?;
    let (client, peer) = tokio::io::duplex(65536);
    let (stdout, stdin) = tokio::io::split(client);
    let cancel = tokio_util::sync::CancellationToken::new();
    let signal = cancel.clone();
    let response = handler(server.clone(), peer);
    let task = tokio::spawn(async move {
        tokio::select! { biased; _ = signal.cancelled() => {}, _ = response => {} }
    });
    Ok(Child {
        stdin: Some(stdin),
        stdout: Some(stdout),
        task,
        cancel,
        waited: false,
    })
}

pub async fn serve_json<F, Fut>(stream: tokio::io::DuplexStream, mut respond: F)
where
    F: FnMut(serde_json::Value) -> Fut,
    Fut: std::future::Future<Output = Option<serde_json::Value>>,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let request = serde_json::from_str(&line).expect("fixture JSON request");
        if let Some(response) = respond(request).await {
            if writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    }
}
pub fn catalog_reply(
    request: &serde_json::Value,
    tools: &serde_json::Value,
    answer: &str,
) -> Option<serde_json::Value> {
    use serde_json::json;
    let id = request.get("id")?;
    let result = match request["method"].as_str()? {
        "server/discover" => {
            json!({"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}})
        }
        "tools/list" => {
            json!({"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":tools})
        }
        "tools/call" => {
            json!({"resultType":"complete","content":[{"type":"text","text":answer}],"isError":false})
        }
        _ => return None,
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
pub fn catalog_stdio(
    path: impl Into<std::path::PathBuf>,
    tools: serde_json::Value,
    answer: String,
    started: Arc<dyn Fn(&crate::LocalServer) + Send + Sync>,
) -> StdioFixture {
    StdioFixture::new(
        path,
        Arc::new(move |server, stream| {
            started(&server);
            let tools = tools.clone();
            let answer = answer.clone();
            Box::pin(serve_json(stream, move |request| {
                std::future::ready(catalog_reply(&request, &tools, &answer))
            }))
        }),
    )
}
