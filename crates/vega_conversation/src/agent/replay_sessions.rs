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
        let Some(generation) = state.next_generation.checked_add(1) else {
            return lease;
        };
        state.next_generation = generation;
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
        let _ = self.invalidate_all_acknowledged();
    }

    pub fn invalidate_all_acknowledged(&self) -> Result<(), crate::types::PermissionResetError> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| crate::types::PermissionResetError::Unavailable)?;
        state.sessions.clear();
        state.idle_bytes = 0;
        state.current_generation.clear();
        Ok(())
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
