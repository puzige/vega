use std::collections::{BTreeMap, VecDeque};

use sha2::{Digest, Sha256};

use super::{RuntimeToolCall, RuntimeToolResult, RuntimeToolStatus};
use crate::provider::RecoveryGuidance;

const WINDOW_BATCHES: u64 = 32;
const MAX_KEYS: usize = 64;
const REPEATED_BATCHES: usize = 3;
const REVIEW_BATCHES: u8 = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Evidence {
    digest: [u8; 32],
    failed: bool,
}

struct Observation {
    evidence: Evidence,
    batches: VecDeque<u64>,
    last_seen: u64,
    notified: bool,
}

#[derive(Default)]
pub(super) struct ProgressRecovery {
    observations: BTreeMap<[u8; 32], Observation>,
    batch: u64,
    review_batches: u8,
}

impl ProgressRecovery {
    pub(super) fn observe(&mut self, call: &RuntimeToolCall, result: &RuntimeToolResult) {
        let oldest = self.batch.saturating_sub(WINDOW_BATCHES - 1);
        self.observations
            .retain(|_, value| value.last_seen >= oldest);
        let key = call_digest(call);
        let evidence = result_evidence(result);
        if !self.observations.contains_key(&key) && self.observations.len() == MAX_KEYS {
            let oldest = self
                .observations
                .iter()
                .min_by_key(|(_, value)| value.last_seen)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                self.observations.remove(&oldest);
            }
        }
        let value = self.observations.entry(key).or_insert_with(|| Observation {
            evidence,
            batches: VecDeque::with_capacity(REPEATED_BATCHES),
            last_seen: self.batch,
            notified: false,
        });
        if value.evidence != evidence {
            value.evidence = evidence;
            value.batches.clear();
            value.notified = false;
        }
        value.last_seen = self.batch;
        value.batches.retain(|batch| *batch >= oldest);
        if value.batches.back() != Some(&self.batch) {
            if value.batches.len() == REPEATED_BATCHES {
                value.batches.pop_front();
            }
            value.batches.push_back(self.batch);
        }
    }

    pub(super) fn finish_batch(&mut self) -> Option<RecoveryGuidance> {
        let ready = |value: &Observation| {
            value.last_seen == self.batch
                && !value.notified
                && value.batches.len() == REPEATED_BATCHES
        };
        let repeated_failure = self
            .observations
            .values()
            .any(|value| ready(value) && value.evidence.failed);
        let repeated_result = self
            .observations
            .values()
            .any(|value| ready(value) && !value.evidence.failed);
        let repeated = if repeated_failure {
            Some(true)
        } else if repeated_result {
            Some(false)
        } else {
            None
        };
        if let Some(failed) = repeated {
            for value in self.observations.values_mut() {
                if value.last_seen == self.batch
                    && value.batches.len() == REPEATED_BATCHES
                    && value.evidence.failed == failed
                {
                    value.notified = true;
                }
            }
        }
        self.batch = self.batch.saturating_add(1);
        let oldest = self.batch.saturating_sub(WINDOW_BATCHES);
        self.observations
            .retain(|_, value| value.last_seen >= oldest);
        self.review_batches += 1;
        let review = self.review_batches == REVIEW_BATCHES;
        if review {
            self.review_batches = 0;
        }
        match repeated {
            Some(true) => Some(RecoveryGuidance::RepeatedFailure),
            Some(false) => Some(RecoveryGuidance::RepeatedResult),
            None if review => Some(RecoveryGuidance::Review),
            None => None,
        }
    }
}

fn call_digest(call: &RuntimeToolCall) -> [u8; 32] {
    crate::tool_context::fingerprint_call(&call.name, &call.input_json)
}

fn result_evidence(result: &RuntimeToolResult) -> Evidence {
    let mut hash = Sha256::new();
    crate::tool_context::hash_field(&mut hash, 0, result.output.as_bytes());
    let status = match result.status {
        RuntimeToolStatus::Success => 0,
        RuntimeToolStatus::Failed => 1,
        RuntimeToolStatus::Rejected => 2,
        RuntimeToolStatus::Cancelled => 3,
    };
    crate::tool_context::hash_field(&mut hash, 1, &[status]);
    if let Some(exit_code) = result.exit_code {
        crate::tool_context::hash_field(&mut hash, 2, &exit_code.to_le_bytes());
    }
    if let Some(truncated) = result.truncated {
        crate::tool_context::hash_field(&mut hash, 3, &[u8::from(truncated)]);
    }
    Evidence {
        digest: hash.finalize().into(),
        failed: result.status != RuntimeToolStatus::Success
            || result.exit_code.is_some_and(|code| code != 0),
    }
}
