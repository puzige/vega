pub mod bundle;
mod card;
pub mod controller;
pub mod lifecycle;
pub(crate) mod model;
mod native;
pub mod placement;
mod session;

pub use card::{GuidanceCard, OpenError, open};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionPane {
    Accessibility,
    ScreenRecording,
}

impl PermissionPane {
    pub fn url(self) -> &'static str {
        match self {
            Self::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            Self::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreparationRequested {
    pub thread_id: String,
    pub context: lifecycle::TaskContext,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CallbackIdentity(std::sync::Arc<()>);

impl PartialEq for CallbackIdentity {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CallbackIdentity {}
