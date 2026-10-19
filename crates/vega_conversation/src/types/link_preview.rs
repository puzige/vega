#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkSiteKind {
    Github,
    Web,
    Mail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkPreviewFailure {
    #[error("This address cannot be previewed")]
    Blocked,
    #[error("This target is not a web page")]
    Unsupported,
    #[error("This page requires access")]
    RequiresLogin,
    #[error("Unable to load preview")]
    Network,
    #[error("Preview request timed out")]
    Timeout,
    #[error("Preview is too large")]
    TooLarge,
    #[error("Unable to read this preview")]
    Malformed,
    #[error("Preview request cancelled")]
    Cancelled,
    #[error("Preview service is busy")]
    Busy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkPreviewImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkPreviewData {
    pub original_target: String,
    pub effective_target: String,
    pub host: String,
    pub site: LinkSiteKind,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<LinkPreviewImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkPreviewTarget {
    pub original_target: String,
    pub site: LinkSiteKind,
}
