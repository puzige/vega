//! Window-level navigation actions and shared sidebar/collapsed controls.
use gpui_kit::*;

mod nsh_controls;

actions!(
    navigation,
    [
        NavigateBack,
        NavigateForward,
        EditorNavigateBack,
        EditorNavigateForward
    ]
);
/// Root-published navigation availability. Contains no routes or draft text.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct NavigationState {
    /// Whether a backward traversal is available.
    pub back: bool,
    /// Whether a forward traversal is available.
    pub forward: bool,
    /// Short content-free navigation failure or capacity explanation.
    pub error: Option<&'static str>,
}
impl NavigationState {
    pub fn error_message(&self) -> Option<&'static str> {
        self.error.map(|message| {
            if message == "ui.draft_capacity_reached_send_or_clear_the_current" {
                vega_i18n::t(message)
            } else {
                message
            }
        })
    }
}

impl Global for NavigationState {}
/// Install focus-independent shortcuts with more-local text-editor overrides.
pub fn init(cx: &mut App) {
    cx.set_global(NavigationState::default());
    cx.bind_keys([
        KeyBinding::new("cmd-[", NavigateBack, None),
        KeyBinding::new("cmd-]", NavigateForward, None),
        KeyBinding::new("cmd-[", EditorNavigateBack, Some("TextInput")),
        KeyBinding::new("cmd-]", EditorNavigateForward, Some("TextInput")),
    ]);
}
pub fn controls(window: &mut Window, cx: &mut App, sidebar_visible: bool) -> AnyElement {
    nsh_controls::controls(window, cx, sidebar_visible, false)
}

pub fn sidebar_toggle(window: &mut Window, cx: &mut App, sidebar_visible: bool) -> AnyElement {
    nsh_controls::controls(window, cx, sidebar_visible, true)
}

/// Weak editor identity and scalar cache budget; this never retains a stream or worker.
#[derive(Clone, PartialEq, Eq)]
pub struct DraftNavigationGuard {
    /// Current editor task identity.
    pub task: String,
    /// Weak composer editor, read at the action boundary (including IME edits).
    pub input: WeakEntity<crate::text_input::TextInput>,
    /// Cached nonempty tasks excluding this live task.
    pub cached_tasks: usize,
    /// Cached UTF-8 bytes excluding this live task.
    pub cached_bytes: usize,
}
impl Global for DraftNavigationGuard {}
/// Reject a visit before database side effects if its departing draft cannot fit.
pub fn allow_task_navigation(target: Option<&str>, cx: &mut App) -> bool {
    let allowed = cx.try_global::<DraftNavigationGuard>().is_none_or(|guard| {
        target == Some(guard.task.as_str())
            || guard.input.upgrade().is_none_or(|input| {
                let text = input.read(cx).text();
                text.is_empty()
                    || (guard.cached_tasks < 100
                        && guard.cached_bytes.saturating_add(text.len()) <= 1024 * 1024)
            })
    });
    if !allowed {
        let mut state = cx
            .try_global::<NavigationState>()
            .cloned()
            .unwrap_or_default();
        state.error = Some("ui.draft_capacity_reached_send_or_clear_the_current");
        cx.set_global(state);
        cx.refresh_windows();
    }
    allowed
}

/// UI mutation fence used by asynchronous route resolvers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskMutationState {
    /// Monotonic revision across mutation dispatch and completion.
    pub epoch: u64,
    /// Mutations whose durable result has not yet been acknowledged.
    pub pending: usize,
}
impl Global for TaskMutationState {}
/// Fence route reads before a task mutation starts.
pub fn begin_task_mutation(cx: &mut App) {
    let mut state = cx
        .try_global::<TaskMutationState>()
        .copied()
        .unwrap_or_default();
    state.epoch = state.epoch.wrapping_add(1);
    state.pending = state.pending.saturating_add(1);
    cx.set_global(state);
}
/// Release one started mutation even when its originating UI entity was dropped.
pub fn finish_task_mutation(cx: &mut App) {
    let mut state = cx
        .try_global::<TaskMutationState>()
        .copied()
        .unwrap_or_default();
    state.epoch = state.epoch.wrapping_add(1);
    state.pending = state.pending.saturating_sub(1);
    cx.set_global(state);
}
