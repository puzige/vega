use vega_store::Store;
use vega_store::thread_admission::{self, ThreadAdmission};

use crate::types::ConversationError;

pub(crate) fn acquire(store: &Store, thread: &str) -> Result<ThreadAdmission, ConversationError> {
    thread_admission::try_acquire(store.database_identity(), thread)
        .map_err(|_| ConversationError::Store("Thread access is unavailable".into()))?
        .ok_or_else(|| ConversationError::Store("Stop the current task before changing it".into()))
}

pub(crate) fn verify(
    store: &Store,
    thread: &str,
    admission: &ThreadAdmission,
) -> Result<(), ConversationError> {
    if admission.verifies(store.database_identity(), thread) {
        Ok(())
    } else {
        Err(ConversationError::InvalidTaskIdentity)
    }
}
