use gpui_kit::{App, BorrowAppContext, Entity, Global, WeakEntity};
use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use vega_conversation::types::*;
use vega_ui::conversation_stream::{AutomationTiming, ConversationStream};

pub(crate) struct AutomationGlobal {
    pub(crate) ready: bool,
    pub(crate) mutation_lane: Arc<Mutex<()>>,
    pub(crate) revoked: HashSet<(String, AutomationKind)>,
    owners: HashMap<String, Weak<AtomicBool>>,
    pub(crate) epochs: HashMap<String, Arc<AtomicU64>>,
    pub(crate) mutations: HashMap<String, u64>,
    pub(crate) runs: HashMap<String, AutomationRunOwner>,
    streams: HashMap<String, Vec<AutomationParticipant>>,
}
impl Default for AutomationGlobal {
    fn default() -> Self {
        Self {
            ready: true,
            mutation_lane: Arc::new(Mutex::new(())),
            revoked: HashSet::new(),
            owners: HashMap::new(),
            epochs: HashMap::new(),
            mutations: HashMap::new(),
            runs: HashMap::new(),
            streams: HashMap::new(),
        }
    }
}
pub(crate) struct AutomationRunOwner {
    pub(crate) generation: u64,
    pub(crate) owner: Weak<AtomicBool>,
    pub(crate) claim: Option<AutomationRunClaim>,
    pub(crate) started_at: Option<std::time::Instant>,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
}

#[derive(Clone)]
pub(crate) struct AutomationClockSnapshot {
    pub(crate) generation: u64,
    pub(crate) started: std::time::Instant,
    pub(crate) claim_id: Option<String>,
}

struct AutomationParticipant {
    owner: Weak<AtomicBool>,
    stream: WeakEntity<ConversationStream>,
}

impl Global for AutomationGlobal {}

impl AutomationGlobal {
    pub(crate) fn clock_is_current(
        &self,
        thread: &str,
        generation: u64,
        started: std::time::Instant,
    ) -> bool {
        self.runs
            .get(thread)
            .is_some_and(|run| run.generation == generation && run.started_at == Some(started))
    }

    pub(crate) fn timing_projection(
        &self,
        thread: &str,
        tasks: &[AutomationTask],
        active_claim: Option<&AutomationRunClaim>,
        clock: Option<&AutomationClockSnapshot>,
        sampled_at: std::time::Instant,
    ) -> Option<HashMap<String, AutomationTiming>> {
        if active_claim.is_some()
            && clock.is_some_and(|clock| {
                !self.clock_is_current(thread, clock.generation, clock.started)
            })
        {
            return None;
        }
        let live = clock.filter(|clock| {
            self.clock_is_current(thread, clock.generation, clock.started)
                && active_claim.is_some_and(|claim| {
                    claim.started
                        && clock
                            .claim_id
                            .as_ref()
                            .map_or(claim.source.is_none(), |id| *id == claim.id)
                })
        });
        Some(
            tasks
                .iter()
                .map(|task| {
                    let running = live.is_some()
                        && active_claim.is_some_and(|claim| {
                            claim
                                .tasks
                                .iter()
                                .any(|snapshot| snapshot.identity == task.identity)
                        });
                    let extra = if running {
                        live.map_or(0, |clock| {
                            u64::try_from(
                                sampled_at
                                    .saturating_duration_since(clock.started)
                                    .as_millis(),
                            )
                            .unwrap_or(u64::MAX)
                        })
                    } else {
                        0
                    };
                    (
                        task.identity.clone(),
                        AutomationTiming {
                            elapsed_ms: task.elapsed_ms.saturating_add(extra),
                            running,
                        },
                    )
                })
                .collect(),
        )
    }

    pub(crate) fn register_stream(
        &mut self,
        thread: &str,
        owner: &Arc<AtomicBool>,
        stream: &Entity<ConversationStream>,
    ) {
        let participants = self.streams.entry(thread.into()).or_default();
        participants.retain(|entry| {
            entry
                .owner
                .upgrade()
                .is_some_and(|alive| alive.load(Ordering::SeqCst))
                && entry.stream.upgrade().is_some()
        });
        if !participants
            .iter()
            .any(|entry| entry.stream.upgrade().as_ref() == Some(stream))
        {
            participants.push(AutomationParticipant {
                owner: Arc::downgrade(owner),
                stream: stream.downgrade(),
            });
        }
    }
    pub(crate) fn streams_ready(&self, thread: &str, cx: &App) -> bool {
        self.streams.get(thread).is_none_or(|participants| {
            participants.iter().all(|entry| {
                entry
                    .owner
                    .upgrade()
                    .is_none_or(|alive| !alive.load(Ordering::SeqCst))
                    || entry
                        .stream
                        .upgrade()
                        .is_none_or(|stream| stream.read(cx).automation_can_run(cx))
            })
        })
    }
    pub(crate) fn finish_run(&mut self, thread: &str, generation: u64, owner: &Arc<AtomicBool>) {
        if self.runs.get(thread).is_some_and(|run| {
            run.generation == generation
                && run
                    .owner
                    .upgrade()
                    .is_some_and(|existing| Arc::ptr_eq(&existing, owner))
        }) {
            self.runs.remove(thread);
        }
    }
    pub(crate) fn close_window(&mut self, owner: &Arc<AtomicBool>) {
        owner.store(false, Ordering::SeqCst);
        self.owners.retain(|_, existing| {
            existing
                .upgrade()
                .is_some_and(|existing| !Arc::ptr_eq(&existing, owner))
        });
        self.runs.retain(|_, run| {
            let same = run
                .owner
                .upgrade()
                .is_some_and(|existing| Arc::ptr_eq(&existing, owner));
            if same {
                run.cancel.cancel();
            }
            !same
        });
    }
    pub(crate) fn acquire(&mut self, thread: &str, owner: &Arc<AtomicBool>) -> bool {
        if let Some(existing) = self.owners.get(thread).and_then(Weak::upgrade)
            && existing.load(Ordering::SeqCst)
            && !Arc::ptr_eq(&existing, owner)
        {
            return false;
        }
        self.owners.insert(thread.into(), Arc::downgrade(owner));
        true
    }
    pub(crate) fn release(&mut self, thread: &str, owner: &Arc<AtomicBool>) {
        if self
            .owners
            .get(thread)
            .and_then(Weak::upgrade)
            .is_some_and(|existing| Arc::ptr_eq(&existing, owner))
        {
            self.owners.remove(thread);
        }
    }
    pub(crate) fn counter(&mut self, thread: &str) -> Arc<AtomicU64> {
        self.epochs.entry(thread.into()).or_default().clone()
    }
    pub(crate) fn epoch(&self, thread: &str) -> u64 {
        self.epochs
            .get(thread)
            .map(|epoch| epoch.load(Ordering::SeqCst))
            .unwrap_or(0)
    }
    pub(crate) fn invalidate(&mut self, thread: &str) -> u64 {
        self.counter(thread)
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1)
    }
}

#[derive(Default)]
pub(crate) struct AutomationController {
    pub(crate) streams: HashMap<String, Entity<ConversationStream>>,
    pub(crate) pending: HashSet<String>,
    pub(crate) dispatch: HashMap<String, AutomationRunClaim>,
    pub(crate) timer_started: bool,
}

pub(crate) fn ensure_global(cx: &mut App) {
    if cx.try_global::<AutomationGlobal>().is_none() {
        cx.set_global(AutomationGlobal::default());
    }
}

pub(crate) fn startup(cx: &mut App) {
    ensure_global(cx);
    let ready = cx
        .global::<vega_ui::sidebar::VegaStore>()
        .0
        .as_ref()
        .ok()
        .is_some_and(|store| vega_conversation::automation::recover_startup(store).is_ok());
    cx.update_global::<AutomationGlobal, _>(|global, _| global.ready = ready);
    if !ready {
        tracing::warn!(
            operation = "automation_recovery",
            "automatic task recovery failed"
        );
    }
}
