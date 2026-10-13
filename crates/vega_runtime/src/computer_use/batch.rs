use super::{ComputerUseError, ComputerUseRun, ControlPhase, Target};
use crate::{ChatMessage, ChatRole, ImageAttachment};
use serde_json::Value;

const MAX_IMAGES: usize = 2;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 2 * (65536 * 4 + 16 * 1024 + 8 * 1024);

#[derive(Clone)]
pub(super) struct ReadProjection {
    pub call_id: String,
    pub content: String,
    pub images: Vec<ImageAttachment>,
}

#[derive(Default)]
pub(super) struct ModelBatch {
    id: String,
    run_id: String,
    epoch: u64,
    reads: Vec<ReadProjection>,
}

fn image_budget<'a>(messages: impl Iterator<Item = &'a ChatMessage>) -> (usize, usize) {
    messages
        .filter(|message| message.role == ChatRole::Tool)
        .flat_map(|message| &message.images)
        .fold((0, 0), |(count, bytes), image| {
            (count + 1, bytes + image.bytes().len())
        })
}

impl ComputerUseRun {
    pub(crate) fn begin_model_batch(&self, id: &str) {
        if let Ok(mut batch) = self.model_batch.lock() {
            *batch = ModelBatch {
                id: id.into(),
                run_id: self.control.run_id().into(),
                epoch: self.control.status().epoch,
                reads: Vec::new(),
            };
        }
    }

    pub(crate) fn clear_model_batch(&self) {
        if let Ok(mut batch) = self.model_batch.lock() {
            *batch = ModelBatch::default();
        }
    }

    pub(super) fn batch_reads(&self) -> Vec<ReadProjection> {
        let status = self.control.status();
        let Ok(mut batch) = self.model_batch.lock() else {
            return Vec::new();
        };
        if batch.run_id != self.control.run_id()
            || batch.epoch != status.epoch
            || status.phase != ControlPhase::Running
        {
            *batch = ModelBatch::default();
        }
        batch.reads.clone()
    }

    pub(crate) fn defer_model_read(
        &self,
        name: &str,
        input: &str,
        messages: &mut [ChatMessage],
    ) -> bool {
        let Ok(arguments) = serde_json::from_str::<Value>(input) else {
            return false;
        };
        let image = match name {
            "ComputerObserve" | "ComputerScreenshot" => arguments["target_id"].is_string(),
            "BrowserObserve" => arguments["include_image"] == true,
            _ => false,
        };
        if !image {
            return false;
        }
        self.trim_messages(messages);
        let reads = self.batch_reads();
        let current = self.control.snapshot().observations;
        let capability = Self::tool_capability(name);
        let mut projected = messages.to_vec();
        for message in &mut projected {
            if message.tool_call_id.as_ref().is_some_and(|id| {
                current.iter().any(|item| {
                    &item.call_id == id
                        && Some(item.capability) == capability
                        && !reads.iter().any(|read| &read.call_id == id)
                })
            }) {
                message.images.clear();
            }
        }
        let (count, bytes) = image_budget(projected.iter());
        count >= MAX_IMAGES || bytes >= MAX_BYTES
    }

    pub(crate) fn stage_model_observation(
        &self,
        batch_id: &str,
        call_id: &str,
        messages: &mut [ChatMessage],
    ) -> Result<(), ComputerUseError> {
        self.trim_messages(messages);
        let Some(observation) = self
            .control
            .snapshot()
            .observations
            .into_iter()
            .find(|observation| observation.call_id == call_id)
        else {
            return Ok(());
        };
        let mut message = ChatMessage::tool_result(call_id, "");
        self.apply_observation(call_id, &mut message);
        let (count, bytes) = image_budget(messages.iter().chain(std::iter::once(&message)));
        if count > MAX_IMAGES || bytes > MAX_BYTES {
            return Err(ComputerUseError::ObservationBudget { executed: true });
        }
        if !matches!(observation.target, Target::Desktop(_)) {
            return Ok(());
        }
        let content = observation.read_only_model_text();
        let mut batch = self
            .model_batch
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if batch.id.is_empty() {
            *batch = ModelBatch {
                id: batch_id.into(),
                run_id: observation.run_id.clone(),
                epoch: observation.epoch,
                reads: Vec::new(),
            };
        }
        if batch.id != batch_id
            || batch.run_id != observation.run_id
            || batch.epoch != observation.epoch
        {
            return Err(ComputerUseError::Cancelled);
        }
        if batch
            .reads
            .iter()
            .map(|read| read.content.len())
            .sum::<usize>()
            + content.len()
            > MAX_TEXT_BYTES
        {
            return Err(ComputerUseError::ObservationBudget { executed: true });
        }
        batch.reads.push(ReadProjection {
            call_id: call_id.into(),
            content,
            images: if observation.model_image {
                observation.image.into_iter().collect()
            } else {
                Vec::new()
            },
        });
        Ok(())
    }

    pub(crate) fn consume_model_batch(&self, messages: &mut [ChatMessage]) {
        self.clear_model_batch();
        self.trim_messages(messages);
    }

    pub(super) fn consume_observations(&self) {
        self.clear_model_batch();
        self.control.consume_observations();
    }
}
