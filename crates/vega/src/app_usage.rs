//! Settings usage worker ownership. UI never opens or queries SQLite.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::{Context, Entity};
use vega_conversation::{
    UsageDashboardController,
    types::{UsageDashboard, UsageDashboardError},
};
use vega_ui::{
    settings::{SettingsOpen, SettingsView, UsageReloadRequested},
    sidebar::VegaStore,
};

use crate::window::VegaWindow;

/// Bind one Settings entity. Every refresh has a generation, and completion
/// requires the same still-open entity; late results never replace a new view.
pub(crate) fn bind(view: &Entity<SettingsView>, cx: &mut Context<VegaWindow>) {
    let generation = Arc::new(AtomicU64::new(0));
    let reload_generation = generation.clone();
    cx.subscribe(view, move |owner, view, _: &UsageReloadRequested, cx| {
        if !cx.global::<SettingsOpen>().0 || owner.settings_view.as_ref() != Some(&view) {
            return;
        }
        start(view, reload_generation.clone(), cx);
    })
    .detach();
    start(view.clone(), generation, cx);
}

fn start(view: Entity<SettingsView>, generation: Arc<AtomicU64>, cx: &mut Context<VegaWindow>) {
    let Ok(previous) = generation.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
        value.checked_add(1)
    }) else {
        view.update(cx, |view, cx| {
            view.apply_usage_dashboard(Err(UsageDashboardError::Overflow), cx)
        });
        return;
    };
    let request = previous + 1;
    view.update(cx, |view, cx| view.begin_usage_load(cx));
    let path = cx
        .try_global::<VegaStore>()
        .and_then(|global| global.0.as_ref().ok())
        .and_then(|store| store.database_path())
        .map(std::path::Path::to_path_buf);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok());
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("vega-usage".into())
        .spawn(move || {
            let result = match (path, now_ms) {
                (Some(path), Some(now_ms)) => load_on_worker(path, now_ms),
                _ => Err(UsageDashboardError::Unavailable),
            };
            let _ = sender.send(result);
        });
    if worker.is_err() {
        view.update(cx, |view, cx| {
            view.apply_usage_dashboard(Err(UsageDashboardError::Unavailable), cx)
        });
        return;
    }
    let weak_view = view.downgrade();
    cx.spawn(async move |owner, cx| {
        loop {
            if generation.load(Ordering::SeqCst) != request {
                break;
            }
            let result = match receiver.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => {
                    cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await;
                    continue;
                }
                Err(mpsc::TryRecvError::Disconnected) => Err(UsageDashboardError::Unavailable),
            };
            let _ = owner.update(cx, |owner, cx| {
                let Some(view) = weak_view.upgrade() else {
                    return;
                };
                if generation.load(Ordering::SeqCst) != request
                    || !cx.global::<SettingsOpen>().0
                    || owner.settings_view.as_ref() != Some(&view)
                {
                    return;
                }
                view.update(cx, |view, cx| view.apply_usage_dashboard(result, cx));
            });
            break;
        }
    })
    .detach();
}

fn load_on_worker(
    path: std::path::PathBuf,
    now_ms: i64,
) -> Result<UsageDashboard, UsageDashboardError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| UsageDashboardError::Unavailable)?;
    runtime.block_on(UsageDashboardController::new(path).load(now_ms))
}
