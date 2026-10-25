#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkSiteKind {
    Github,
    Web,
    Mail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkPreviewFailure {
    #[error("{}", vega_i18n::t("domain.link.blocked"))]
    Blocked,
    #[error("{}", vega_i18n::t("domain.link.unsupported"))]
    Unsupported,
    #[error("{}", vega_i18n::t("domain.link.requireslogin"))]
    RequiresLogin,
    #[error("{}", vega_i18n::t("domain.link.network"))]
    Network,
    #[error("{}", vega_i18n::t("domain.link.timeout"))]
    Timeout,
    #[error("{}", vega_i18n::t("domain.link.toolarge"))]
    TooLarge,
    #[error("{}", vega_i18n::t("domain.link.malformed"))]
    Malformed,
    #[error("{}", vega_i18n::t("domain.link.cancelled"))]
    Cancelled,
    #[error("{}", vega_i18n::t("domain.link.busy"))]
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
