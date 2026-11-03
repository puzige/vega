use super::{Duration, HashMap, HashSet, Instant, StreamModel};
use gpui_kit::Window;
use gpui_kit::base::animation::cubic_bezier;

type Segment = (u64, usize, usize);

struct Fade {
    appeared: Instant,
    start: Instant,
    duration: Duration,
    initial: f32,
}

impl Fade {
    fn opacity(&self, now: Instant) -> f32 {
        if now.saturating_duration_since(self.appeared) >= Duration::from_millis(2500) {
            return 1.;
        }
        let elapsed = now.saturating_duration_since(self.start);
        if now < self.start {
            return self.initial;
        }
        let progress = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).min(1.);
        self.initial + (1. - self.initial) * cubic_bezier(0.37, 0.55, 0.86, 0.88)(progress)
    }
}

#[derive(Default)]
pub(super) struct StreamFade {
    seen: HashSet<Segment>,
    active: HashMap<Segment, Fade>,
    tail_start: usize,
    finished: bool,
}

impl StreamFade {
    pub(super) fn sync(&mut self, model: &StreamModel) {
        let now = Instant::now();
        let mut ordinal = 0;
        self.active.retain(|_, fade| fade.opacity(now) < 1.);
        let mut previous_block = None;
        let mut index = 0;
        for line in model
            .committed_lines
            .iter()
            .chain(&model.pending_lines)
            .skip(self.tail_start.min(model.committed_lines.len()))
        {
            if previous_block == Some(line.block_id) {
                index += 1;
            } else {
                previous_block = Some(line.block_id);
                index = 0;
            }
            let rows = line.table.as_ref().map_or(1, |table| table.rows.len());
            for row in 0..rows {
                let segment = (line.block_id, index, row);
                if !self.seen.insert(segment) {
                    continue;
                }
                let delay = if self.finished {
                    0
                } else {
                    (ordinal * 24).min(800)
                };
                self.active.insert(
                    segment,
                    Fade {
                        appeared: now,
                        start: now + Duration::from_millis(delay),
                        duration: Duration::from_millis(if self.finished { 150 } else { 700 }),
                        initial: 0.,
                    },
                );
                ordinal += 1;
            }
        }
        self.tail_start = model.committed_lines.last().map_or(0, |last| {
            model
                .committed_lines
                .iter()
                .rposition(|line| line.block_id != last.block_id)
                .map_or(0, |index| index + 1)
        });
    }

    pub(super) fn finish(&mut self) {
        if self.finished {
            return;
        }
        let now = Instant::now();
        for fade in self.active.values_mut() {
            fade.initial = fade.opacity(now);
            fade.start = now;
            fade.duration = Duration::from_millis(150);
        }
        self.finished = true;
    }

    pub(super) fn frame(&mut self, window: &Window, reduced: bool) -> FadeFrame {
        if reduced || !window.is_window_active() {
            self.active.clear();
            return FadeFrame::default();
        }
        let now = Instant::now();
        let mut opacities = HashMap::with_capacity(self.active.len());
        self.active.retain(|segment, fade| {
            let opacity = fade.opacity(now);
            if opacity >= 1. {
                return false;
            }
            opacities.insert(*segment, opacity);
            true
        });
        if !opacities.is_empty() {
            window.request_animation_frame();
        }
        FadeFrame { opacities }
    }
}

#[derive(Default)]
pub(super) struct FadeFrame {
    opacities: HashMap<Segment, f32>,
}

impl FadeFrame {
    pub(super) fn line(&self, block: u64, line: usize) -> LineFade<'_> {
        LineFade {
            frame: self,
            block,
            line,
        }
    }
}

pub(super) struct LineFade<'a> {
    frame: &'a FadeFrame,
    block: u64,
    line: usize,
}

impl LineFade<'_> {
    pub(super) fn opacity(&self, row: usize) -> f32 {
        self.frame
            .opacities
            .get(&(self.block, self.line, row))
            .copied()
            .unwrap_or(1.)
    }
}
