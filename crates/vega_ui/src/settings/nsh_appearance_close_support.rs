use super::*;
use std::sync::Mutex;

static QUEUES: Mutex<Vec<Queue>> = Mutex::new(Vec::new());

#[derive(Clone)]
pub(crate) struct Queue {
    snapshot: Snapshot,
    commands: mpsc::Sender<Request>,
}

#[derive(Clone, Copy)]
pub(crate) enum Choice {
    Theme,
    Language,
    Sidebar,
}

impl Queue {
    pub(crate) fn send(&self, choice: Choice) -> oneshot::Receiver<bool> {
        let action = match choice {
            Choice::Theme => Action::Theme("dark"),
            Choice::Language => Action::Language("zh-CN"),
            Choice::Sidebar => Action::Sidebar,
        };
        let (reply, response) = oneshot::channel();
        assert!(
            self.commands
                .try_send(Request::Decide(Box::new(Decision {
                    snapshot: self.snapshot.clone(),
                    action,
                    reply,
                })))
                .is_ok(),
            "actual appearance receiver queue must remain available"
        );
        response
    }
}

pub(crate) fn current(settings: &SettingsView, cx: &App) -> Option<Queue> {
    let page = settings.appearance_presentation.as_ref()?;
    QUEUES
        .lock()
        .unwrap()
        .iter()
        .find(|queue| {
            let current = settings.appearance_snapshot(cx, page.id.clone());
            queue.snapshot.id.matches(&page.id)
                && queue.snapshot.theme == current.theme
                && queue.snapshot.language == current.language
                && queue.snapshot.sidebar == current.sidebar
                && queue.snapshot.enabled == current.enabled
        })
        .cloned()
}

pub(super) fn observe(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) {
    effect(move || {
        let snapshot = state.get();
        let mut queues = QUEUES.lock().unwrap();
        queues.retain(|queue| !queue.commands.same_channel(&commands));
        queues.push(Queue {
            snapshot,
            commands: commands.clone(),
        });
    });
}
