use std::collections::{BTreeMap, HashSet};
use std::fmt;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

pub const USER_QUESTION_TOOL: &str = "ask_user_question";
pub const USER_QUESTION_MAX_BYTES: usize = 64 * 1024;
pub const USER_QUESTION_UNAVAILABLE: &str = "Tool error: question interface unavailable";
pub const USER_QUESTION_BATCH_REJECTED: &str = "Tool error: ask_user_question must be called alone. Resend one question call and read its answers before choosing other tools.";
pub const USER_QUESTION_CANCELLED: &str = "Question interrupted.";
pub const USER_QUESTION_UNSAFE_RESPONSE: &str =
    "Tool error: question response could not be recorded safely";
const VALIDATION_CODES: &[&str] = &[
    "input_size",
    "input_shape",
    "questions_count",
    "question_id",
    "question_header",
    "question_text",
    "option_count",
    "option_label",
    "credentials",
];

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserQuestionInput {
    pub questions: Vec<UserQuestion>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<UserQuestionOption>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserQuestionAnswers {
    #[serde(deserialize_with = "unique_answers")]
    pub answers: BTreeMap<String, UserQuestionAnswer>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserQuestionAnswer {
    pub answers: Vec<String>,
}

impl fmt::Debug for UserQuestionInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserQuestionInput")
            .field("questions", &self.questions.len())
            .finish()
    }
}

impl fmt::Debug for UserQuestionAnswers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserQuestionAnswers")
            .field("answers", &self.answers.len())
            .finish()
    }
}

impl UserQuestionInput {
    pub fn from_json(raw: &str) -> Result<Self, &'static str> {
        if raw.len() > USER_QUESTION_MAX_BYTES {
            return Err("input_size");
        }
        let input: Self = serde_json::from_str(raw).map_err(|_| "input_shape")?;
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=3).contains(&self.questions.len()) {
            return Err("questions_count");
        }
        let mut ids = HashSet::new();
        let mut texts = HashSet::new();
        for question in &self.questions {
            let id = question.id.as_bytes();
            if id.is_empty()
                || id.len() > 64
                || !id[0].is_ascii_lowercase()
                || !id
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
                || id.last() == Some(&b'_')
                || question.id.contains("__")
                || !ids.insert(&question.id)
            {
                return Err("question_id");
            }
            if question.header.trim().is_empty() || question.header.chars().count() > 12 {
                return Err("question_header");
            }
            if question.question.trim().is_empty() || !texts.insert(question.question.trim()) {
                return Err("question_text");
            }
            if !(2..=4).contains(&question.options.len()) {
                return Err("option_count");
            }
            let mut labels = HashSet::new();
            for option in &question.options {
                if option.label.trim().is_empty()
                    || !labels.insert(option.label.trim())
                    || option
                        .label
                        .trim()
                        .strip_suffix(" (Recommended)")
                        .unwrap_or(option.label.trim())
                        .eq_ignore_ascii_case("other")
                {
                    return Err("option_label");
                }
            }
        }
        let encoded = serde_json::to_vec(self).map_err(|_| "input_shape")?;
        if encoded.len() > USER_QUESTION_MAX_BYTES {
            return Err("input_size");
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, &'static str> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| "input_shape")
    }
}

impl UserQuestionAnswers {
    pub fn from_json(raw: &str, input: &UserQuestionInput) -> Result<Self, &'static str> {
        if raw.len() > USER_QUESTION_MAX_BYTES {
            return Err("answer_size");
        }
        let answers: Self = serde_json::from_str(raw).map_err(|_| "answer_shape")?;
        answers.validate(input)?;
        Ok(answers)
    }

    pub fn validate(&self, input: &UserQuestionInput) -> Result<(), &'static str> {
        if self.answers.len() != input.questions.len() {
            return Err("answer_ids");
        }
        for question in &input.questions {
            let answer = self.answers.get(&question.id).ok_or("answer_ids")?;
            let mut selected = false;
            let mut note = false;
            for value in &answer.answers {
                if question.options.iter().any(|option| option.label == *value) {
                    if selected || note {
                        return Err("answer_selection");
                    }
                    selected = true;
                } else if let Some(text) = value.strip_prefix("user_note: ") {
                    if note || text.trim().is_empty() {
                        return Err("answer_note");
                    }
                    note = true;
                } else {
                    return Err("answer_selection");
                }
            }
        }
        if serde_json::to_vec(self).map_err(|_| "answer_shape")?.len() > USER_QUESTION_MAX_BYTES {
            return Err("answer_size");
        }
        Ok(())
    }

    pub fn to_json(&self, input: &UserQuestionInput) -> Result<String, &'static str> {
        self.validate(input)?;
        serde_json::to_string(self).map_err(|_| "answer_shape")
    }
}

#[derive(Clone)]
pub struct RuntimeUserQuestionRequest {
    pub call_id: String,
    pub input: UserQuestionInput,
}

impl fmt::Debug for RuntimeUserQuestionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeUserQuestionRequest")
            .field("input", &self.input)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeUserQuestionResponse {
    Answered(UserQuestionAnswers),
    Cancelled,
    Unavailable,
}

pub trait RuntimeUserQuestionHook: Send + Sync {
    fn request(
        &self,
        request: RuntimeUserQuestionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, RuntimeUserQuestionResponse>;
}

pub fn user_question_validation_audit(code: &str, raw: &str) -> String {
    serde_json::json!({"validation_error":code, "input_sha256":format!("{:x}", Sha256::digest(raw.as_bytes()))}).to_string()
}

pub fn user_question_validation_error(raw: &str) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Audit {
        validation_error: String,
        input_sha256: String,
    }
    if raw.len() > 256 {
        return None;
    }
    let audit: Audit = serde_json::from_str(raw).ok()?;
    (VALIDATION_CODES.contains(&audit.validation_error.as_str())
        && audit.input_sha256.len() == 64
        && audit
            .input_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(audit.validation_error)
}

fn unique_answers<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, UserQuestionAnswer>, D::Error> {
    struct AnswersVisitor;
    impl<'de> serde::de::Visitor<'de> for AnswersVisitor {
        type Value = BTreeMap<String, UserQuestionAnswer>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("unique question answers")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut answers = BTreeMap::new();
            while let Some((id, answer)) = map.next_entry::<String, UserQuestionAnswer>()? {
                if answers.insert(id, answer).is_some() {
                    return Err(serde::de::Error::custom("duplicate question answer"));
                }
            }
            Ok(answers)
        }
    }
    deserializer.deserialize_map(AnswersVisitor)
}

pub fn user_question_validation_output(code: &str) -> String {
    format!("Tool error: invalid ask_user_question input ({code})")
}

pub(crate) fn definition() -> crate::ToolDefinition {
    crate::ToolDefinition {
        name: USER_QUESTION_TOOL.into(),
        description: "Ask one to three concise single-choice questions when user input materially affects the task. Call this tool alone; read its answers before choosing any other tools. Supply two to four options per question; the client adds Other. Use a short header and unique snake_case id. A recommended option may end in ' (Recommended)'. Wait for the user's answer; answers never approve tool permissions. Do not request secrets or credentials.".into(),
        input_schema: serde_json::json!({"type":"object","properties":{"questions":{"type":"array","minItems":1,"maxItems":3,"items":{"type":"object","properties":{"id":{"type":"string","maxLength":64},"header":{"type":"string","maxLength":12},"question":{"type":"string"},"options":{"type":"array","minItems":2,"maxItems":4,"items":{"type":"object","properties":{"label":{"type":"string"},"description":{"type":"string"}},"required":["label","description"],"additionalProperties":false}}},"required":["id","header","question","options"],"additionalProperties":false}}},"required":["questions"],"additionalProperties":false}),
        strict: true,
    }
}

pub(crate) fn validate_redacted_result(
    call: &super::RuntimeToolCall,
    result: &mut super::RuntimeToolResult,
) {
    if call.name == USER_QUESTION_TOOL
        && ((result.status == super::RuntimeToolStatus::Failed
            && result.output == "Tool error: credential safety check unavailable")
            || (result.status == super::RuntimeToolStatus::Success
                && !UserQuestionInput::from_json(&call.input_json).is_ok_and(|input| {
                    UserQuestionAnswers::from_json(&result.output, &input).is_ok()
                })))
    {
        result.status = super::RuntimeToolStatus::Failed;
        result.output = USER_QUESTION_UNSAFE_RESPONSE.into();
        result.truncated = None;
    }
}
