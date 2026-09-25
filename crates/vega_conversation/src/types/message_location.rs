#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageLocationStatus {
    Searching,
    Deferred,
    Located,
    NotFound,
    Failed,
}
