use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use vega_runtime::{CredentialReadError, CredentialReadReason, CredentialReadStage};

use crate::types::PermissionFence;
use vega_runtime::{CredentialExceptionFilter, CredentialReader};

#[derive(Clone)]
pub struct RunCredentialReader {
    inner: Arc<RunCredentials>,
}

struct RunCredentials {
    database: PathBuf,
    thread: String,
    root: Option<PathBuf>,
    binding_refs: Vec<String>,
    required: CredentialReader,
    accepted: OnceLock<AcceptedCredentials>,
    exceptions: Mutex<Option<CredentialExceptionFilter>>,
}

struct AcceptedCredentials {
    fence: PermissionFence,
    message: String,
}

impl AcceptedCredentials {
    fn matches(&self, fence: &PermissionFence, message: &str) -> bool {
        if !fence.is_valid() {
            return false;
        }
        match &fence.delegation {
            Some(source) => {
                source.parent_fence.as_ref() == &self.fence
                    && source.parent_message_id == self.message
                    && source.child_message_id == message
            }
            None => self.fence == *fence && self.message == message,
        }
    }
}

impl RunCredentialReader {
    pub fn new(
        database: PathBuf,
        thread: String,
        root: Option<PathBuf>,
        binding_refs: Vec<String>,
        required: CredentialReader,
    ) -> Self {
        Self {
            inner: Arc::new(RunCredentials {
                database,
                thread,
                root,
                binding_refs,
                required,
                accepted: OnceLock::new(),
                exceptions: Mutex::new(None),
            }),
        }
    }

    pub fn reader(&self) -> CredentialReader {
        let this = self.clone();
        Arc::new(move || {
            let accepted = this.inner.accepted.get().ok_or_else(|| {
                crate::credential_errors::state_error(CredentialReadStage::Authority)
            })?;
            this.read_values(&accepted.fence)
        })
    }

    pub fn exception_filter(&self) -> CredentialExceptionFilter {
        let this = self.clone();
        Arc::new(move |text, finding| {
            this.inner
                .exceptions
                .lock()
                .ok()
                .and_then(|filter| filter.clone())
                .is_some_and(|filter| filter(text, finding))
        })
    }

    fn read_values(&self, fence: &PermissionFence) -> Result<Vec<String>, CredentialReadError> {
        let result = self.read_current_values(fence);
        if result.is_err()
            && let Some(root) = &self.inner.root
        {
            crate::privacy::invalidate_authority(root);
        }
        result
    }

    fn read_current_values(
        &self,
        fence: &PermissionFence,
    ) -> Result<Vec<String>, CredentialReadError> {
        let optional = fence.run_mode == "execute" && fence.permission_mode == "full_access";
        let mut values = Vec::new();
        for reference in &self.inner.binding_refs {
            let root = self.inner.root.as_ref().ok_or_else(|| {
                crate::credential_errors::state_error(CredentialReadStage::Authority)
            })?;
            match vega_store::keystore::lookup_existing_key(root, reference).map_err(|error| {
                match error {
                    vega_store::keystore::LookupError::StoreAbsent => CredentialReadError::new(
                        CredentialReadStage::Bindings,
                        CredentialReadReason::Missing,
                    ),
                    vega_store::keystore::LookupError::Storage(error) => {
                        crate::credential_errors::credential_error(
                            CredentialReadStage::Bindings,
                            error,
                        )
                    }
                }
            })? {
                Some(value) => values.push(value),
                None if optional => {}
                None => {
                    return Err(CredentialReadError::new(
                        CredentialReadStage::Bindings,
                        CredentialReadReason::Missing,
                    ));
                }
            }
        }
        values.extend((self.inner.required)()?);
        if let Some(root) = &self.inner.root {
            values.extend(crate::privacy::refresh_authority_values(
                &self.inner.database,
                root,
            )?);
        }
        Ok(values)
    }

    pub(crate) fn prepare(
        &self,
        fence: &PermissionFence,
        message: &str,
    ) -> Result<Vec<String>, CredentialReadError> {
        if !fence.is_valid()
            || match self.inner.accepted.get() {
                Some(accepted) => !accepted.matches(fence, message),
                None => fence.delegation.is_some() || fence.thread_id != self.inner.thread,
            }
        {
            return Err(crate::credential_errors::state_error(
                CredentialReadStage::Authority,
            ));
        }
        let store = vega_store::Store::open_read_only(&self.inner.database).map_err(|error| {
            CredentialReadError::database(CredentialReadStage::Authority, error)
        })?;
        if store.database_identity().permission_fence_key() != fence.database_identity {
            return Err(crate::credential_errors::state_error(
                CredentialReadStage::Authority,
            ));
        }
        let values = self.read_values(fence)?;
        if fence.delegation.is_none() {
            let filter = self
                .inner
                .root
                .as_ref()
                .map(|root| {
                    crate::privacy::exception_filter(
                        self.inner.database.clone(),
                        self.inner.thread.clone(),
                        root.clone(),
                    )
                })
                .transpose()?;
            *self.inner.exceptions.lock().map_err(|_| {
                crate::credential_errors::state_error(CredentialReadStage::Authority)
            })? = filter;
        }
        Ok(values)
    }

    pub(crate) fn activate(
        &self,
        fence: &PermissionFence,
        message: &str,
    ) -> Result<(), CredentialReadError> {
        if let Some(accepted) = self.inner.accepted.get() {
            return accepted
                .matches(fence, message)
                .then_some(())
                .ok_or_else(|| {
                    crate::credential_errors::state_error(CredentialReadStage::Authority)
                });
        }
        if !fence.is_valid() || fence.delegation.is_some() || fence.thread_id != self.inner.thread {
            return Err(crate::credential_errors::state_error(
                CredentialReadStage::Authority,
            ));
        }
        self.inner
            .accepted
            .set(AcceptedCredentials {
                fence: fence.clone(),
                message: message.to_owned(),
            })
            .map_err(|_| crate::credential_errors::state_error(CredentialReadStage::Authority))
    }
}
