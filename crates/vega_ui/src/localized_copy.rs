#[derive(Clone, Debug)]
pub enum LocalizedCopy {
    Text(String),
    Key(&'static str),
    Args(&'static str, Vec<(&'static str, LocalizedCopy)>),
    Run(vega_conversation::types::RunFailureKind),
    Preflight(vega_conversation::types::ProviderPreflightFailure),
    Reference(vega_conversation::types::FileReferenceFailureCode),
    Conversation(std::sync::Arc<vega_conversation::types::ConversationError>),
    Organization(std::sync::Arc<vega_conversation::types::SidebarOrganizationError>),
}

impl LocalizedCopy {
    pub fn new(key: &'static str) -> Self {
        Self::Key(key)
    }

    pub fn with_args(key: &'static str, args: Vec<(&'static str, Self)>) -> Self {
        Self::Args(key, args)
    }

    pub(crate) fn storage(error: impl std::fmt::Display) -> Self {
        Self::with_args(
            "ui.storage_operation_failed_error",
            vec![("error", error.to_string().into())],
        )
    }

    pub fn render(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Key(key) => vega_i18n::t(key).to_owned(),
            Self::Run(failure) => failure.message(),
            Self::Preflight(failure) => failure.message(),
            Self::Reference(failure) => failure.message().to_owned(),
            Self::Conversation(error) => error.to_string(),
            Self::Organization(error) => error.to_string(),
            Self::Args(key, args) => {
                let values = args
                    .iter()
                    .map(|(name, value)| (*name, value.render()))
                    .collect::<Vec<_>>();
                let borrowed = values
                    .iter()
                    .map(|(name, value)| (*name, value.as_str()))
                    .collect::<Vec<_>>();
                vega_i18n::t_args(key, &borrowed)
            }
        }
    }
}

impl From<vega_conversation::types::ConversationError> for LocalizedCopy {
    fn from(value: vega_conversation::types::ConversationError) -> Self {
        Self::Conversation(std::sync::Arc::new(value))
    }
}

impl From<vega_conversation::types::SidebarOrganizationError> for LocalizedCopy {
    fn from(value: vega_conversation::types::SidebarOrganizationError) -> Self {
        Self::Organization(std::sync::Arc::new(value))
    }
}

impl From<String> for LocalizedCopy {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for LocalizedCopy {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl std::fmt::Display for LocalizedCopy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render())
    }
}
