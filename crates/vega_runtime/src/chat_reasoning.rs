use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatReasoningSource {
    Reasoning,
    ReasoningText,
}

impl ChatReasoningSource {
    pub fn field(self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
            Self::ReasoningText => "reasoning_text",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Default)]
pub struct ChatRawState {
    pub text: Option<(ChatReasoningSource, String)>,
    pub details: Option<Vec<Value>>,
}

impl ChatRawState {
    pub fn byte_len(&self) -> usize {
        self.text
            .as_ref()
            .map_or(0, |(source, text)| {
                source.field().len().saturating_add(text.len())
            })
            .saturating_add(self.detail_bytes())
    }

    pub fn detail_bytes(&self) -> usize {
        self.details.as_ref().map_or(0, |details| {
            serde_json::to_string(details).map_or(usize::MAX, |text| text.len())
        })
    }
}

impl std::fmt::Debug for ChatRawState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatRawState")
            .field("has_text", &self.text.is_some())
            .field("detail_count", &self.details.as_ref().map(Vec::len))
            .field("bytes", &self.byte_len())
            .finish()
    }
}
