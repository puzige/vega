#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PendingMessageBehavior {
    #[default]
    Queue,
    Steer,
}

impl PendingMessageBehavior {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queue => vega_i18n::t("domain.pending.queue"),
            Self::Steer => vega_i18n::t("domain.pending.steer"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Steer => "steer",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileReferenceMention {
    pub range: std::ops::Range<usize>,
    pub path: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct PendingMessage {
    pub id: String,
    pub thread_id: String,
    pub generation: String,
    pub behavior: PendingMessageBehavior,
    pub content: String,
    pub images: Vec<super::ImageAttachment>,
    pub references: Vec<String>,
    pub reference_mentions: Vec<FileReferenceMention>,
    pub permission_fence: Option<super::PermissionFence>,
}

#[derive(Clone)]
pub struct PendingInputAuthority {
    pub(crate) database: super::DatabaseIdentity,
    pub(crate) message: PendingMessage,
}

#[derive(Clone, Default)]
pub struct PendingQueueProjection {
    pub messages: Vec<PendingMessage>,
    pub committed_generation: Option<String>,
    pub paused: bool,
    pub busy: bool,
    pub error: Option<String>,
}

impl PendingQueueProjection {
    pub fn committed_message_ids(&self) -> Vec<String> {
        let Some(generation) = self.committed_generation.as_deref() else {
            return Vec::new();
        };
        self.messages
            .iter()
            .filter(|message| {
                message.behavior == PendingMessageBehavior::Steer
                    && message.generation == generation
            })
            .map(|message| message.id.clone())
            .collect()
    }
}

#[derive(Clone)]
pub struct PendingRunContext {
    pub generation: String,
    pub message_id: Option<String>,
}
