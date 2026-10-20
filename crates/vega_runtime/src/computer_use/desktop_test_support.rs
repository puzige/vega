use super::{Desktop, Error};
use futures::future::BoxFuture;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use vega_mcp::{ManagedResult, McpError};

pub(crate) trait DesktopTransport: Send {
    fn is_connected(&self) -> bool;
    fn connect<'a>(&'a mut self, cancel: &'a CancellationToken)
    -> BoxFuture<'a, Result<(), Error>>;
    fn call<'a>(
        &'a mut self,
        name: &'a str,
        args: Value,
        cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Result<ManagedResult, McpError>>;
    fn shutdown(&mut self) -> BoxFuture<'_, bool>;
}

pub(super) enum DesktopConnection {
    Process(Box<Desktop>),
    InProcess(Box<dyn DesktopTransport>),
}

impl DesktopConnection {
    pub(super) fn is_connected(&self) -> bool {
        match self {
            Self::Process(desktop) => desktop.is_connected(),
            Self::InProcess(transport) => transport.is_connected(),
        }
    }

    pub(super) async fn connect(&mut self, cancel: &CancellationToken) -> Result<(), Error> {
        match self {
            Self::Process(desktop) => desktop.connect(cancel).await,
            Self::InProcess(transport) => transport.connect(cancel).await,
        }
    }

    pub(super) async fn call(
        &mut self,
        name: &str,
        args: Value,
        cancel: &CancellationToken,
    ) -> Result<ManagedResult, McpError> {
        match self {
            Self::Process(desktop) => desktop.call(name, args, cancel).await,
            Self::InProcess(transport) => transport.call(name, args, cancel).await,
        }
    }

    pub(super) async fn shutdown(&mut self) -> bool {
        match self {
            Self::Process(desktop) => desktop.shutdown().await,
            Self::InProcess(transport) => transport.shutdown().await,
        }
    }
}
