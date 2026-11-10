use crate::AppView;
use std::{cell::Cell, time::Instant};

#[path = "scroll_integration_support.rs"]
mod scroll_integration_support;
#[path = "text_integration_support.rs"]
mod text_integration_support;

pub use scroll_integration_support::IntegrationScrollObservation;
pub use text_integration_support::IntegrationTextObservation;

thread_local! {
    static BATCH: Cell<usize> = const { Cell::new(0) };
    static DRAWS: Cell<usize> = const { Cell::new(0) };
    static REPAINTS: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn batch_started(jobs: usize) -> (usize, Instant, usize, usize) {
    let batch = BATCH.with(|count| {
        let next = count.get() + 1;
        count.set(next);
        next
    });
    if batch <= 25 || batch % 100 == 0 {
        eprintln!("[nsh-runtime] BEGIN batch={batch} jobs={jobs}");
    }
    (
        batch,
        Instant::now(),
        DRAWS.with(Cell::get),
        REPAINTS.with(Cell::get),
    )
}

pub(crate) fn batch_finished((batch, start, draws, repaints): (usize, Instant, usize, usize)) {
    let elapsed = start.elapsed();
    if batch <= 25 || batch % 100 == 0 || elapsed.as_millis() > 100 {
        eprintln!(
            "[nsh-runtime] END batch={batch} elapsed={elapsed:?} draws={} repaints={}",
            DRAWS.with(Cell::get) - draws,
            REPAINTS.with(Cell::get) - repaints
        );
    }
}

pub(crate) fn drawing(surface: u64) {
    let n = DRAWS.with(|count| {
        let n = count.get() + 1;
        count.set(n);
        n
    });
    if n <= 30 || n % 100 == 0 {
        eprintln!("[nsh-runtime] draw={n} surface={surface}");
    }
}

pub(crate) fn repaint(surface: u64) {
    let n = REPAINTS.with(|count| {
        let n = count.get() + 1;
        count.set(n);
        n
    });
    if n <= 30 || n % 100 == 0 {
        eprintln!("[nsh-runtime] repaint={n} surface={surface}");
    }
}

impl AppView {
    #[allow(missing_docs)]
    pub fn integration_scroll_alpha(&self) -> Option<f32> {
        let mut pending = vec![self.surface().root.clone()];
        let mut found = None;
        while let Some(view) = pending.pop() {
            let state = view.try_state()?;
            if let Some(scroll) = &state.scroll {
                if found.is_some() || !scroll.live.alpha.is_finite() {
                    return None;
                }
                found = Some(scroll.live.alpha);
            }
            drop(state);
            pending.extend(view.with_children(|children| children.to_vec()));
        }
        found
    }
}
