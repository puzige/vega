use std::path::Path;

use vega_store::{DatabaseIdentity, Store};

mod sealed {
    pub(crate) trait Sealed {}
}

pub(crate) use sealed::Sealed;

pub(crate) trait DatabaseCommand: Sealed + Send + 'static {
    type Output: Send + 'static;
    fn apply(self, store: &Store) -> Self::Output;
}

pub(crate) trait WriteRequest: Send {
    fn execute(self: Box<Self>, store: &Store);
}

struct Request<C: DatabaseCommand> {
    identity: DatabaseIdentity,
    command: C,
    reply: std::sync::mpsc::SyncSender<Result<C::Output, String>>,
}

impl<C: DatabaseCommand> WriteRequest for Request<C> {
    fn execute(self: Box<Self>, store: &Store) {
        let result = if store.database_identity() == self.identity {
            Ok(self.command.apply(store))
        } else {
            Err("database identity changed before commit".into())
        };
        let _ = self.reply.send(result);
    }
}

pub(crate) fn commit<C: DatabaseCommand>(
    path: &Path,
    identity: DatabaseIdentity,
    command: C,
) -> Result<C::Output, String> {
    let writer = crate::agent::persistence::database_writer::DatabaseWriter::open_blocking(
        path.to_path_buf(),
    )
    .map_err(|_| "database writer unavailable".to_string())?;
    let (reply, receipt) = std::sync::mpsc::sync_channel(1);
    writer
        .submit(Box::new(Request {
            identity,
            command,
            reply,
        }))
        .map_err(|_| "database writer stopped before enqueue".to_string())?;
    receipt
        .recv()
        .map_err(|_| "database commit outcome unknown".to_string())?
}
