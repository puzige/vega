use std::collections::{BTreeMap, VecDeque};

use serde_json::Value;
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

fn add_digest(hash: &mut Sha256, tag: u8, bytes: &[u8]) {
    hash.update([tag]);
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn json_digest(hash: &mut Sha256, value: &Value) {
    match value {
        Value::Null => add_digest(hash, 0, &[]),
        Value::Bool(value) => add_digest(hash, 1, &[u8::from(*value)]),
        Value::Number(value) => add_digest(hash, 2, value.to_string().as_bytes()),
        Value::String(value) => add_digest(hash, 3, value.as_bytes()),
        Value::Array(values) => {
            add_digest(hash, 4, &(values.len() as u64).to_le_bytes());
            for value in values {
                json_digest(hash, value);
            }
        }
        Value::Object(values) => {
            add_digest(hash, 5, &(values.len() as u64).to_le_bytes());
            let sorted = values.iter().collect::<BTreeMap<_, _>>();
            for (key, value) in sorted {
                add_digest(hash, 6, key.as_bytes());
                json_digest(hash, value);
            }
        }
    }
}

fn call_digest(call: &RuntimeToolCall) -> [u8; 32] {
    let mut hash = Sha256::new();
    add_digest(
        &mut hash,
        0,
        super::tools_exec::canonical_tool_name(&call.name).as_bytes(),
    );
    if let Ok(value) = serde_json::from_str::<Value>(&call.input_json) {
        json_digest(&mut hash, &value);
    } else {
        add_digest(&mut hash, 7, call.input_json.as_bytes());
    }
    hash.finalize().into()
}

fn result_evidence(result: &RuntimeToolResult) -> Evidence {
    let mut hash = Sha256::new();
    add_digest(&mut hash, 0, result.output.as_bytes());
    let status = match result.status {
        RuntimeToolStatus::Success => 0,
        RuntimeToolStatus::Failed => 1,
        RuntimeToolStatus::Rejected => 2,
        RuntimeToolStatus::Cancelled => 3,
    };
    add_digest(&mut hash, 1, &[status]);
    if let Some(exit_code) = result.exit_code {
        add_digest(&mut hash, 2, &exit_code.to_le_bytes());
    }
    if let Some(truncated) = result.truncated {
        add_digest(&mut hash, 3, &[u8::from(truncated)]);
    }
    Evidence {
        digest: hash.finalize().into(),
        failed: result.status != RuntimeToolStatus::Success
            || result.exit_code.is_some_and(|code| code != 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, input: &str) -> RuntimeToolCall {
        RuntimeToolCall {
            id: id.into(),
            name: "read".into(),
            input_json: input.into(),
        }
    }

    fn result(output: &str) -> RuntimeToolResult {
        super::super::tools_exec::terminal_result(
            &call("ignored", "{}"),
            output.into(),
            RuntimeToolStatus::Success,
            None,
        )
    }

    #[test]
    fn canonical_keys_ignore_ids_and_object_order_but_preserve_arrays_and_values() {
        let original = call("one", r#"{"x":{"a":1,"b":2},"values":[true,null,"1"]}"#);
        let mut reordered = call("two", r#"{"values":[true,null,"1"],"x":{"b":2,"a":1}}"#);
        reordered.name = "Read".into();
        assert_eq!(call_digest(&original), call_digest(&reordered));
        for input in [
            r#"{"x":{"a":2,"b":1},"values":[true,null,"1"]}"#,
            r#"{"x":{"a":1,"b":2},"values":[null,true,"1"]}"#,
            r#"{"x":{"a":1,"b":2},"values":[true,null,1]}"#,
        ] {
            assert_ne!(call_digest(&original), call_digest(&call("three", input)));
        }
    }

    #[test]
    fn result_change_resets_only_its_key_and_batch_duplicates_cannot_inflate_counts() {
        for change in 0..4 {
            let mut state = ProgressRecovery::default();
            let repeated = call("same", r#"{"path":"source"}"#);
            let mut evidence = result("same");
            for _ in 0..2 {
                for _ in 0..20 {
                    state.observe(&repeated, &evidence);
                }
                assert_eq!(state.finish_batch(), None);
            }
            match change {
                0 => evidence.output.push_str("changed"),
                1 => evidence.status = RuntimeToolStatus::Failed,
                2 => evidence.exit_code = Some(1),
                3 => evidence.truncated = Some(true),
                _ => unreachable!(),
            }
            for index in 0..3 {
                state.observe(&repeated, &evidence);
                let expected = (index == 2).then_some(if change == 1 || change == 2 {
                    RecoveryGuidance::RepeatedFailure
                } else {
                    RecoveryGuidance::RepeatedResult
                });
                assert_eq!(state.finish_batch(), expected);
            }
            state.observe(&repeated, &evidence);
            assert_eq!(state.finish_batch(), None);
        }
    }

    #[test]
    fn evidence_is_bounded_expires_and_repeats_once_per_unchanged_chain() {
        let mut state = ProgressRecovery::default();
        for index in 0..128 {
            state.observe(
                &call("id", &format!("{{\"index\":{index}}}")),
                &result("observed"),
            );
            assert!(state.observations.len() <= MAX_KEYS);
        }
        assert_eq!(state.finish_batch(), None);
        assert_eq!(state.observations.len(), MAX_KEYS);
        for _ in 0..32 {
            state.finish_batch();
        }
        assert!(state.observations.is_empty());
        let repeated = call("same", "{}");
        for index in 0..4 {
            state.observe(&repeated, &result("one"));
            assert_eq!(
                state.finish_batch(),
                (index == 2).then_some(RecoveryGuidance::RepeatedResult)
            );
        }
        state.observe(&repeated, &result("two"));
        state.observe(&repeated, &result("one"));
        assert_eq!(state.finish_batch(), None);
        state.observe(&repeated, &result("one"));
        assert_eq!(state.finish_batch(), None);
        state.observe(&repeated, &result("one"));
        assert_eq!(state.finish_batch(), Some(RecoveryGuidance::RepeatedResult));
    }

    #[test]
    fn failure_priority_consumes_due_review_without_losing_unrelated_failure_evidence() {
        let mut state = ProgressRecovery::default();
        let failed = call("failed", r#"{"path":"missing"}"#);
        let mut failure = result("not found");
        failure.status = RuntimeToolStatus::Failed;
        for index in 0..16 {
            if [0, 8, 15].contains(&index) {
                state.observe(&failed, &failure);
            }
            state.observe(
                &call("unrelated", &format!("{{\"path\":\"{index}\"}}")),
                &result("success"),
            );
            assert_eq!(
                state.finish_batch(),
                (index == 15).then_some(RecoveryGuidance::RepeatedFailure)
            );
        }
        assert_eq!(state.review_batches, 0);
        state.observe(&failed, &failure);
        assert_eq!(state.finish_batch(), None);
    }
}
