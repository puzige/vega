//! Content-free identities used by window navigation and its database service.
/// A logical page, independent of mutable task attributes or settings categories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationRoute {
    /// Settings page.
    Settings,
    /// Project landing page (including the unselected application landing page).
    Project(Option<String>),
    /// A task belonging to a registered project.
    Task { project: String, task: String },
}
/// Navigation failures distinguish skippable missing routes from service failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationError {
    /// Deleted, archived, or removed destination.
    InvalidRoute,
    /// Database or worker unavailable; current page must remain intact.
    Unavailable,
}
