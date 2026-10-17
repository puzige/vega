pub use vega_runtime::{
    RuntimeUserQuestionRequest, USER_QUESTION_CANCELLED, USER_QUESTION_TOOL,
    USER_QUESTION_UNAVAILABLE, USER_QUESTION_UNSAFE_RESPONSE, UserQuestion, UserQuestionAnswer,
    UserQuestionAnswers, UserQuestionInput, UserQuestionOption,
};

#[derive(Clone, PartialEq, Eq)]
pub struct UserQuestionHistoryText(pub(crate) String);

impl UserQuestionHistoryText {
    pub fn text(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for UserQuestionHistoryText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UserQuestionHistoryText")
            .field("bytes", &self.0.len())
            .finish()
    }
}
