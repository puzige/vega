#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedNoticeKind {
    TurnLimit { limit: u32 },
    ComputerStopped { delivered_images: bool },
}

impl GeneratedNoticeKind {
    pub fn canonical_text(self) -> String {
        match self {
            Self::TurnLimit { limit } => {
                vega_runtime::RuntimeGeneratedNoticeKind::TurnLimit { limit }.canonical_text()
            }
            Self::ComputerStopped { delivered_images } => {
                vega_runtime::RuntimeGeneratedNoticeKind::ComputerStopped { delivered_images }
                    .canonical_text()
            }
        }
    }

    pub fn message(self) -> String {
        match self {
            Self::TurnLimit { limit } => vega_i18n::t_args(
                "domain.runtime.turn_limit",
                &[("limit", &limit.to_string())],
            ),
            Self::ComputerStopped {
                delivered_images: true,
            } => vega_i18n::t("domain.computer.stopped_with_images").to_string(),
            Self::ComputerStopped {
                delivered_images: false,
            } => vega_i18n::t("domain.computer.stopped_without_images").to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedNoticeMetadata {
    pub kind: GeneratedNoticeKind,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
}

impl GeneratedNoticeMetadata {
    pub fn canonical_text(&self) -> String {
        self.kind.canonical_text()
    }

    pub fn message(&self) -> String {
        self.kind.message()
    }

    pub fn is_valid_for(&self, content: &str) -> bool {
        !matches!(self.kind, GeneratedNoticeKind::TurnLimit { limit: 0 })
            && self.text_start_bytes < self.text_end_bytes
            && content.get(self.text_start_bytes..self.text_end_bytes)
                == Some(self.canonical_text().as_str())
    }

    pub(crate) fn from_runtime(
        kind: vega_runtime::RuntimeGeneratedNoticeKind,
        text_start_bytes: usize,
        text_end_bytes: usize,
    ) -> Self {
        Self {
            kind: match kind {
                vega_runtime::RuntimeGeneratedNoticeKind::TurnLimit { limit } => {
                    GeneratedNoticeKind::TurnLimit { limit }
                }
                vega_runtime::RuntimeGeneratedNoticeKind::ComputerStopped { delivered_images } => {
                    GeneratedNoticeKind::ComputerStopped { delivered_images }
                }
            },
            text_start_bytes,
            text_end_bytes,
        }
    }

    pub(crate) fn from_store(row: &vega_store::messages::GeneratedNoticeRow) -> Option<Self> {
        let kind = match (row.kind.as_str(), row.turn_limit, row.delivered_images) {
            ("turn_limit", Some(limit), None) if limit > 0 => {
                GeneratedNoticeKind::TurnLimit { limit }
            }
            ("computer_stopped", None, Some(delivered_images)) => {
                GeneratedNoticeKind::ComputerStopped { delivered_images }
            }
            _ => return None,
        };
        Some(Self {
            kind,
            text_start_bytes: row.text_start_bytes,
            text_end_bytes: row.text_end_bytes,
        })
    }
}
