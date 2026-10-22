use super::{
    ComponentState, ComputerUseError as Error, Preparation, control::OwnershipAuthority,
    preparation_from_check_permissions,
};
use std::{future::Future, pin::Pin, sync::Arc};
use tokio_util::sync::CancellationToken;

pub(super) trait PreparationTransportFactory {
    type Transport: PreparationTransport + Send + 'static;
    fn open(&mut self) -> Result<Self::Transport, Error>;
}

pub(super) trait PreparationTransport {
    fn check_permissions(
        &mut self,
        prompt: bool,
        probe_direct_capture: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Option<serde_json::Value>, Error>> + Send + '_>>;

    fn cleanup(&mut self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>>;
}

pub(super) struct PreparationSession<F: PreparationTransportFactory> {
    transport: Option<F::Transport>,
    authority: Arc<OwnershipAuthority>,
    id: String,
    cancel: CancellationToken,
    terminal: bool,
}

impl<F: PreparationTransportFactory> PreparationSession<F> {
    pub(super) fn open(
        mut factory: F,
        authority: Arc<OwnershipAuthority>,
        cancel: CancellationToken,
    ) -> Result<Self, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let id = ulid::Ulid::generate().to_string();
        authority.acquire(&id)?;
        let transport = match factory.open() {
            Ok(transport) => transport,
            Err(error) => {
                authority.release(&id);
                return Err(error);
            }
        };
        Ok(Self {
            transport: Some(transport),
            authority,
            id,
            cancel,
            terminal: false,
        })
    }

    pub(super) async fn check(&mut self) -> Result<Preparation, Error> {
        if self.terminal {
            return Err(Error::Cancelled);
        }
        self.terminal = true;
        let transport = self.transport.as_mut().ok_or(Error::Cancelled)?;
        let result = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Error::Cancelled),
            result = transport.check_permissions(false, false) => result,
        };
        match result {
            Ok(value) => {
                self.terminal = false;
                Ok(preparation_from_check_permissions(
                    ComponentState::Available,
                    value.as_ref(),
                ))
            }
            Err(error) => {
                let _ = self.close().await;
                Err(error)
            }
        }
    }

    pub(super) async fn close(&mut self) -> Result<(), Error> {
        self.terminal = true;
        if let Some(transport) = self.transport.as_mut() {
            transport.cleanup().await?;
            self.transport = None;
            self.authority.release(&self.id);
        }
        Ok(())
    }
}

impl<F: PreparationTransportFactory> Drop for PreparationSession<F> {
    fn drop(&mut self) {
        if let Some(transport) = self.transport.take() {
            self.authority.retain(self.id.clone(), transport);
        }
    }
}
