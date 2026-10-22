pub mod bundle;
mod card;
pub mod controller;
pub mod lifecycle;
mod native;
pub mod placement;
pub mod retention;
mod session;

pub use card::{GuidanceCard, OpenError, open};

#[derive(Clone, Debug)]
pub struct PreparationRequested {
    pub thread_id: String,
    pub context: lifecycle::TaskContext,
}

#[derive(Clone, Debug, Default)]
struct CallbackIdentity(std::sync::Arc<()>);

impl PartialEq for CallbackIdentity {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CallbackIdentity {}
