use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, Weak};

use sha2::{Digest, Sha256};
use vega_runtime::{ChatMessage, ChatRole, ReplayFrame};

const MAX_IDLE_SESSIONS: usize = 4;
const MAX_SESSION_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct ReplaySessionRegistry {
    inner: Arc<Mutex<ReplaySessionState>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReplayCheckpointFence {
    checkpoint_id: Option<i64>,
    covered_through_seq: Option<u64>,
    covered_assistant_text_bytes: Option<u64>,
    covered_assistant_tool_seq: Option<u64>,
    digest: [u8; 32],
}

impl ReplayCheckpointFence {
    pub(crate) fn from_checkpoint(
        checkpoint: Option<&vega_store::context_compaction::ContextCheckpoint>,
    ) -> Self {
        fn update_string(digest: &mut Sha256, value: Option<&str>) {
            if let Some(value) = value {
                digest.update([1]);
                digest.update((value.len() as u64).to_le_bytes());
                digest.update(value.as_bytes());
            } else {
                digest.update([0]);
            }
        }

        let mut digest = Sha256::new();
        if let Some(checkpoint) = checkpoint {
            digest.update([1]);
            digest.update(checkpoint.id.to_le_bytes());
            update_string(&mut digest, Some(checkpoint.thread_id.as_str()));
            update_string(&mut digest, Some(checkpoint.model.as_str()));
            digest.update(checkpoint.source_version.to_le_bytes());
            digest.update(checkpoint.covered_through_seq.to_le_bytes());
            update_string(&mut digest, Some(checkpoint.source_fingerprint.as_str()));
            update_string(&mut digest, Some(checkpoint.summary.as_str()));
            update_string(&mut digest, Some(checkpoint.estimator_version.as_str()));
            if let Some(previous) = checkpoint.expected_previous_id {
                digest.update([1]);
                digest.update(previous.to_le_bytes());
            } else {
                digest.update([0]);
            }
            digest.update(checkpoint.created_at.to_le_bytes());
            update_string(&mut digest, checkpoint.covered_assistant_id.as_deref());
            digest.update(checkpoint.covered_assistant_text_bytes.to_le_bytes());
            digest.update(checkpoint.covered_assistant_tool_seq.to_le_bytes());
        } else {
            digest.update([0]);
        }
        Self {
            checkpoint_id: checkpoint.map(|checkpoint| checkpoint.id),
            covered_through_seq: checkpoint.map(|checkpoint| checkpoint.covered_through_seq),
            covered_assistant_text_bytes: checkpoint
                .map(|checkpoint| checkpoint.covered_assistant_text_bytes),
            covered_assistant_tool_seq: checkpoint
                .map(|checkpoint| checkpoint.covered_assistant_tool_seq),
            digest: digest.finalize().into(),
        }
    }
}

impl fmt::Debug for ReplaySessionRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (sessions, bytes) = self
            .inner
            .lock()
            .map(|state| (state.sessions.len(), state.idle_bytes))
            .unwrap_or_default();
        formatter
            .debug_struct("ReplaySessionRegistry")
            .field("idle_session_count", &sessions)
            .field("resident_bytes", &bytes)
            .finish()
    }
}

#[derive(Default)]
struct ReplaySessionState {
    sessions: HashMap<String, ReplaySession>,
    active: HashMap<u64, ActiveReplayLease>,
    current_generation: HashMap<String, u64>,
    next_generation: u64,
    clock: u64,
    idle_bytes: usize,
}

struct ActiveReplayLease {
    thread_id: String,
}

pub(crate) struct ReplaySession {
    source_version: u64,
    source_fingerprint: String,
    checkpoint_fence: ReplayCheckpointFence,
    environment_fingerprint: [u8; 32],
    owner_message_id: String,
    messages: Vec<ChatMessage>,
    byte_size: usize,
    last_used: u64,
}

pub(crate) struct ReplaySessionLease {
    inner: Weak<Mutex<ReplaySessionState>>,
    thread_id: String,
    generation: u64,
    candidate: Option<ReplaySession>,
    active: bool,
}

impl fmt::Debug for ReplaySessionLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplaySessionLease")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("generation", &self.generation)
            .field("has_candidate", &self.candidate.is_some())
            .finish()
    }
}

impl ReplaySessionLease {
    pub(crate) fn candidate(&self) -> Option<&ReplaySession> {
        self.candidate.as_ref()
    }
}

impl Drop for ReplaySessionLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let Ok(mut state) = inner.lock() else {
            return;
        };
        if let Some(active) = state.active.remove(&self.generation)
            && state.current_generation.get(&active.thread_id) == Some(&self.generation)
        {
            state.current_generation.remove(&active.thread_id);
        }
        self.active = false;
    }
}

impl ReplaySessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn begin(
        &self,
        thread_id: &str,
        source_version: u64,
        source_fingerprint: &str,
        checkpoint_fence: ReplayCheckpointFence,
    ) -> ReplaySessionLease {
        let mut lease = ReplaySessionLease {
            inner: Arc::downgrade(&self.inner),
            thread_id: thread_id.to_owned(),
            generation: 0,
            candidate: None,
            active: false,
        };
        let Ok(mut state) = self.inner.lock() else {
            return lease;
        };
        state.next_generation = state.next_generation.saturating_add(1);
        if state.next_generation == 0 {
            state.next_generation = 1;
        }
        let generation = state.next_generation;
        if let Some(session) = state.sessions.remove(thread_id) {
            state.idle_bytes = state.idle_bytes.saturating_sub(session.byte_size);
            if session.source_version == source_version
                && session.source_fingerprint == source_fingerprint
                && session.checkpoint_fence == checkpoint_fence
            {
                state.active.insert(
                    generation,
                    ActiveReplayLease {
                        thread_id: thread_id.to_owned(),
                    },
                );
                lease.candidate = Some(session);
            } else {
                state.active.insert(
                    generation,
                    ActiveReplayLease {
                        thread_id: thread_id.to_owned(),
                    },
                );
            }
        } else {
            state.active.insert(
                generation,
                ActiveReplayLease {
                    thread_id: thread_id.to_owned(),
                },
            );
        }
        state
            .current_generation
            .insert(thread_id.to_owned(), generation);
        lease.generation = generation;
        lease.active = true;
        lease
    }

    pub(crate) fn seal(
        &self,
        mut lease: ReplaySessionLease,
        source_version: u64,
        source_fingerprint: String,
        checkpoint_fence: ReplayCheckpointFence,
        owner_message_id: String,
        messages: Vec<ChatMessage>,
    ) -> bool {
        let Some(system) = messages
            .first()
            .filter(|message| message.role == ChatRole::System)
        else {
            return false;
        };
        let Some(frame) = system.replay_frame() else {
            return false;
        };
        let Some(environment_fingerprint) = frame.environment_fingerprint() else {
            return false;
        };
        if !frame.seal_eligible() || owner_message_id.trim().is_empty() {
            return false;
        }
        let thread_id = lease.thread_id.clone();
        let byte_size = replay_session_resident_size(
            thread_id.capacity(),
            source_fingerprint.capacity(),
            owner_message_id.capacity(),
            messages.capacity(),
            &messages,
        );
        if byte_size > MAX_SESSION_BYTES {
            return false;
        }
        let Ok(mut state) = self.inner.lock() else {
            return false;
        };
        let active = state.active.remove(&lease.generation);
        let had_active = active.is_some();
        let current = state.current_generation.get(&lease.thread_id) == Some(&lease.generation);
        drop(active);
        if current {
            state.current_generation.remove(&lease.thread_id);
        }
        lease.active = false;
        if !current || !had_active {
            return false;
        }
        if let Some(old) = state.sessions.remove(&lease.thread_id) {
            state.idle_bytes = state.idle_bytes.saturating_sub(old.byte_size);
        }
        while state.sessions.len() >= MAX_IDLE_SESSIONS
            || state.idle_bytes.saturating_add(byte_size) > MAX_SESSION_BYTES
        {
            let Some(oldest) = state
                .sessions
                .iter()
                .min_by_key(|(_, session)| session.last_used)
                .map(|(thread_id, _)| thread_id.clone())
            else {
                return false;
            };
            if let Some(old) = state.sessions.remove(&oldest) {
                state.idle_bytes = state.idle_bytes.saturating_sub(old.byte_size);
            }
        }
        state.clock = state.clock.saturating_add(1);
        let last_used = state.clock;
        state.idle_bytes = state.idle_bytes.saturating_add(byte_size);
        state.sessions.insert(
            thread_id,
            ReplaySession {
                source_version,
                source_fingerprint,
                checkpoint_fence,
                environment_fingerprint,
                owner_message_id,
                messages,
                byte_size,
                last_used,
            },
        );
        true
    }

    pub fn invalidate_thread(&self, thread_id: &str) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if let Some(session) = state.sessions.remove(thread_id) {
            state.idle_bytes = state.idle_bytes.saturating_sub(session.byte_size);
        }
        state.current_generation.remove(thread_id);
    }

    pub fn invalidate_all(&self) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        let idle_bytes = state.sessions.values().fold(0usize, |total, session| {
            total.saturating_add(session.byte_size)
        });
        state.sessions.clear();
        state.idle_bytes = state.idle_bytes.saturating_sub(idle_bytes);
        state.current_generation.clear();
    }

    pub fn invalidate_active(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.current_generation.clear();
        }
    }
}

impl ReplaySession {
    pub(crate) fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub(crate) fn frame(&self) -> Option<&ReplayFrame> {
        self.messages
            .first()
            .filter(|message| message.role == ChatRole::System)
            .and_then(ChatMessage::replay_frame)
    }

    pub(crate) fn environment_fingerprint(&self) -> [u8; 32] {
        self.environment_fingerprint
    }
}

impl fmt::Debug for ReplaySession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplaySession")
            .field("source_version", &self.source_version)
            .field("source_fingerprint_bytes", &self.source_fingerprint.len())
            .field("checkpoint_id", &self.checkpoint_fence.checkpoint_id)
            .field(
                "checkpoint_covered_through_seq",
                &self.checkpoint_fence.covered_through_seq,
            )
            .field(
                "checkpoint_assistant_text_bytes",
                &self.checkpoint_fence.covered_assistant_text_bytes,
            )
            .field(
                "checkpoint_assistant_tool_seq",
                &self.checkpoint_fence.covered_assistant_tool_seq,
            )
            .field("has_environment_fingerprint", &true)
            .field("owner_message_id_bytes", &self.owner_message_id.len())
            .field("message_count", &self.messages.len())
            .field("frame", &self.frame())
            .field("byte_size", &self.byte_size)
            .finish()
    }
}

fn messages_size(messages: &[ChatMessage]) -> usize {
    messages.iter().fold(0usize, |total, message| {
        total.saturating_add(message.replay_resident_size_bytes())
    })
}

fn replay_session_resident_size(
    thread_id_capacity: usize,
    source_fingerprint_capacity: usize,
    owner_message_id_capacity: usize,
    messages_capacity: usize,
    messages: &[ChatMessage],
) -> usize {
    std::mem::size_of::<(String, ReplaySession)>()
        .saturating_add(2usize.saturating_mul(std::mem::size_of::<usize>()))
        .saturating_add(thread_id_capacity)
        .saturating_add(source_fingerprint_capacity)
        .saturating_add(owner_message_id_capacity)
        .saturating_add(messages_size(messages))
        .saturating_add(
            messages_capacity
                .saturating_sub(messages.len())
                .saturating_mul(std::mem::size_of::<ChatMessage>()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value};
    use vega_runtime::{ResponseReplayLayout, ResponseReplaySlot};

    fn messages_with_content(content: String) -> Vec<ChatMessage> {
        let mut system = ChatMessage::new(ChatRole::System, "system");
        system.attach_replay_frame(ReplayFrame::new(
            1,
            vega_runtime::ReasoningReplayPolicy::StandardChat,
        ));
        if let Some(frame) = system.replay_frame().cloned() {
            let mut frame = frame;
            frame.bind_environment([0; 32]);
            system.attach_replay_frame(frame);
        }
        vec![system, ChatMessage::new(ChatRole::User, content)]
    }

    fn seal_session(
        registry: &ReplaySessionRegistry,
        thread_id: &str,
        source_version: u64,
        source_fingerprint: String,
        content: String,
    ) -> bool {
        let checkpoint_fence = ReplayCheckpointFence::from_checkpoint(None);
        let lease = registry.begin(
            thread_id,
            source_version,
            &source_fingerprint,
            checkpoint_fence,
        );
        registry.seal(
            lease,
            source_version,
            source_fingerprint,
            checkpoint_fence,
            format!("owner-{thread_id}"),
            messages_with_content(content),
        )
    }

    #[test]
    fn automatic_replay_old_window_lease_drop_keeps_newer_owner_sealable() {
        let registry = ReplaySessionRegistry::new();
        let checkpoint_fence = ReplayCheckpointFence::from_checkpoint(None);
        let old_owner = registry.begin("shared-thread", 1, "source", checkpoint_fence);
        let new_owner = registry.begin("shared-thread", 1, "source", checkpoint_fence);
        drop(old_owner);
        assert!(registry.seal(
            new_owner,
            1,
            "source".into(),
            checkpoint_fence,
            "final-owner".into(),
            messages_with_content("second owner remains current".into()),
        ));
    }

    #[test]
    fn automatic_replay_active_candidate_does_not_consume_idle_session_budget() {
        let registry = ReplaySessionRegistry::new();
        let content = "x".repeat(7 * 1024 * 1024);
        for index in 0..4 {
            assert!(seal_session(
                &registry,
                &format!("thread-{index}"),
                1,
                format!("source-{index}"),
                content.clone(),
            ));
        }
        let active = registry.begin(
            "thread-0",
            1,
            "source-0",
            ReplayCheckpointFence::from_checkpoint(None),
        );
        assert!(active.candidate().is_some());
        assert!(seal_session(
            &registry,
            "thread-new",
            1,
            "source-new".into(),
            content,
        ));
        let state = registry.inner.lock().unwrap();
        assert_eq!(state.sessions.len(), MAX_IDLE_SESSIONS);
        assert!(!state.sessions.contains_key("thread-0"));
        assert!(state.sessions.contains_key("thread-new"));
        assert!(state.idle_bytes <= MAX_SESSION_BYTES);
        drop(state);
        drop(active);
    }

    #[test]
    fn automatic_replay_registry_rejects_a_session_above_the_idle_byte_cap() {
        let registry = ReplaySessionRegistry::new();
        assert!(!seal_session(
            &registry,
            "oversized",
            1,
            "source".into(),
            "x".repeat(MAX_SESSION_BYTES + 1),
        ));
        let state = registry.inner.lock().unwrap();
        assert!(state.sessions.is_empty());
        assert_eq!(state.idle_bytes, 0);
    }

    #[test]
    fn automatic_replay_registry_counts_nested_reserved_json_and_session_metadata() {
        let mut nested_values = Vec::with_capacity(25_000);
        for object_index in 0..25_000 {
            let mut object = Map::new();
            for field_index in 0..8 {
                let mut key = String::with_capacity(32);
                key.push_str(&format!("field-{object_index}-{field_index}"));
                let mut text = String::with_capacity(32);
                text.push('v');
                object.insert(key, Value::String(text));
            }
            nested_values.push(Value::Object(object));
        }
        let mut messages = messages_with_content("nested replay payload".into());
        messages[1].response_replay_layout = Some(ResponseReplayLayout {
            response_id: None,
            slots: vec![ResponseReplaySlot::Message(Value::Array(nested_values))],
        });
        assert!(messages_size(&messages) > MAX_SESSION_BYTES);
        let registry = ReplaySessionRegistry::new();
        let checkpoint_fence = ReplayCheckpointFence::from_checkpoint(None);
        let lease = registry.begin("nested", 1, "source", checkpoint_fence);
        assert!(!registry.seal(
            lease,
            1,
            "source".into(),
            checkpoint_fence,
            "owner-nested".into(),
            messages,
        ));
        let state = registry.inner.lock().unwrap();
        assert!(state.sessions.is_empty());
        assert_eq!(state.idle_bytes, 0);

        let metadata_registry = ReplaySessionRegistry::new();
        assert!(seal_session(
            &metadata_registry,
            "metadata",
            1,
            "source-metadata".into(),
            "small replay".into(),
        ));
        let metadata_state = metadata_registry.inner.lock().unwrap();
        let session = metadata_state.sessions.get("metadata").unwrap();
        let minimum_metadata = std::mem::size_of::<(String, ReplaySession)>()
            + 2 * std::mem::size_of::<usize>()
            + "metadata".len()
            + "source-metadata".len()
            + "owner-metadata".len();
        assert!(session.byte_size >= messages_size(&session.messages) + minimum_metadata);
        assert!(session.frame().is_some());
    }
}
