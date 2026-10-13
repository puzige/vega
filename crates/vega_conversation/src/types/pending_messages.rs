#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PendingMessageBehavior {
    #[default]
    Queue,
    Steer,
}

impl PendingMessageBehavior {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queue => "排队",
            Self::Steer => "插话",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Steer => "steer",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PendingMessage {
    pub id: String,
    pub thread_id: String,
    pub generation: String,
    pub behavior: PendingMessageBehavior,
    pub content: String,
    pub images: Vec<super::ImageAttachment>,
}

#[derive(Clone, Default)]
pub struct PendingQueueProjection {
    pub messages: Vec<PendingMessage>,
    pub paused: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct PendingRunContext {
    pub generation: String,
    pub message_id: Option<String>,
}
