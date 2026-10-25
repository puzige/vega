use super::{ComputerUseError, ComputerUseRun, ControlPhase, DesktopTarget, Target};
use crate::{ChatMessage, ChatRole, ImageAttachment};
use serde_json::Value;
use std::sync::Arc;

const MAX_IMAGES: usize = 2;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 2 * (65536 * 4 + 16 * 1024 + 8 * 1024);

#[derive(Clone)]
pub(super) struct ReadProjection {
    pub call_id: String,
    pub content: Arc<str>,
    pub images: Vec<ImageAttachment>,
    target: DesktopTarget,
    epoch: u64,
    observed_at_unix_ms: u128,
    preserve_text: bool,
}

#[derive(Default)]
pub(super) struct ModelBatch {
    id: String,
    run_id: String,
    epoch: u64,
    reads: Vec<ReadProjection>,
    evidence: Vec<ReadProjection>,
    bridge: bool,
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
    pub(crate) fn begin_model_batch(&self, id: &str, screenshots_only: bool) {
        let status = self.control.status();
        if let Ok(mut batch) = self.model_batch.lock() {
            if screenshots_only
                && batch.run_id == self.control.run_id()
                && batch.epoch == status.epoch
                && status.phase == ControlPhase::Running
            {
                batch.id = id.into();
                batch.reads.clear();
                batch.bridge = true;
            } else {
                *batch = ModelBatch {
                    id: id.into(),
                    run_id: self.control.run_id().into(),
                    epoch: status.epoch,
                    ..ModelBatch::default()
                };
            }
        }
    }

    pub(crate) fn clear_model_batch(&self) {
        if let Ok(mut batch) = self.model_batch.lock() {
            *batch = ModelBatch::default();
        }
    }

    pub(super) fn batch_reads(&self) -> Vec<ReadProjection> {
        self.model_reads(false)
    }

    pub(super) fn text_evidence(&self) -> Vec<ReadProjection> {
        self.model_reads(true)
    }

    fn model_reads(&self, evidence: bool) -> Vec<ReadProjection> {
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
        if evidence {
            if batch.bridge {
                batch.evidence.clone()
            } else {
                Vec::new()
            }
        } else {
            batch.reads.clone()
        }
    }

    pub(super) fn read_model_text(&self, read: &ReadProjection, prior: bool) -> String {
        let mut content = read.content.to_string();
        if prior {
            content.push_str(&format!(
                "\nPrior read-only evidence; this earlier snapshot may differ from later screenshots. It does not describe a verified current state.\n{}",
                serde_json::json!({"source_call_id":read.call_id,"observed_at_unix_ms":read.observed_at_unix_ms,"epoch":read.epoch})
            ));
        }
        if self.model_receipt(&read.call_id).is_none() {
            content.push_str(&super::screenshot_target_text(
                self.control.screenshot_target_at(&read.target, read.epoch),
            ));
        }
        content
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
        let Target::Desktop(target) = &observation.target else {
            return Ok(());
        };
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
                ..ModelBatch::default()
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
            .chain(batch.evidence.iter())
            .map(|read| read.content.len())
            .sum::<usize>()
            + content.len()
            > MAX_TEXT_BYTES
        {
            return Err(ComputerUseError::ObservationBudget { executed: true });
        }
        batch.reads.push(ReadProjection {
            call_id: call_id.into(),
            content: content.into(),
            target: target.clone(),
            epoch: observation.epoch,
            observed_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            preserve_text: !observation.read_only,
            images: if observation.model_image {
                observation.image.into_iter().collect()
            } else {
                Vec::new()
            },
        });
        Ok(())
    }

    pub(crate) fn consume_model_batch(&self, messages: &mut [ChatMessage]) {
        if let Ok(mut batch) = self.model_batch.lock() {
            let reads = std::mem::take(&mut batch.reads);
            if reads.iter().any(|read| read.preserve_text) {
                batch.evidence.clear();
                batch.bridge = false;
                for mut read in reads.into_iter().filter(|read| read.preserve_text) {
                    read.images.clear();
                    batch.evidence.retain(|old| {
                        old.target.pid != read.target.pid
                            || old.target.window_id != read.target.window_id
                            || old.target.bundle_id != read.target.bundle_id
                    });
                    if batch.evidence.len() == 2 {
                        batch.evidence.remove(0);
                    }
                    batch.evidence.push(read);
                }
            }
        }
        self.trim_messages(messages);
    }

    pub(super) fn consume_observations(&self) {
        self.clear_model_batch();
        self.control.consume_observations();
    }
}
