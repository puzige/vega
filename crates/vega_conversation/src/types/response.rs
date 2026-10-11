#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantResponseKind {
    Commentary,
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantResponseMetadata {
    pub response_id: String,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
    pub kind: AssistantResponseKind,
}

impl AssistantResponseMetadata {
    pub fn is_valid_for(&self, content: &str) -> bool {
        !self.response_id.is_empty()
            && self.response_id.len() <= 128
            && !self.response_id.chars().any(char::is_control)
            && self.text_start_bytes < self.text_end_bytes
            && content.is_char_boundary(self.text_start_bytes)
            && content.is_char_boundary(self.text_end_bytes)
            && (self.kind != AssistantResponseKind::Final
                || (self.text_end_bytes == content.len()
                    && content
                        .get(self.text_start_bytes..self.text_end_bytes)
                        .is_some_and(|text| !text.trim().is_empty())))
    }
}
