use super::{Duration, HashMap, Instant, StreamModel, StreamSpan};
use gpui_kit::base::animation::cubic_bezier;
use gpui_kit::{TextRun, Window};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum TextPart {
    Body,
    Marker,
    Checkbox,
    Cell(usize),
}

type PartKey = (u64, usize, usize, TextPart);

struct Fade {
    part: PartKey,
    range: Range<usize>,
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
        if now < self.start {
            return self.initial;
        }
        let progress =
            (now.duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32()).min(1.);
        self.initial + (1. - self.initial) * cubic_bezier(0.37, 0.55, 0.86, 0.88)(progress)
    }
}

#[derive(Default)]
pub(super) struct StreamFade {
    previous_text: HashMap<PartKey, String>,
    active: HashMap<u64, Fade>,
    next_segment_id: u64,
    cursor: Option<Instant>,
    tail_start: usize,
    finished: bool,
}

impl StreamFade {
    fn register(&mut self, part: PartKey, text: String, now: Instant) {
        let previous = self.previous_text.entry(part).or_default();
        if *previous == text {
            return;
        }
        let start_byte = previous.len();
        let appended = text.starts_with(previous.as_str());
        *previous = text;
        if !appended {
            self.active.retain(|_, fade| fade.part != part);
            return;
        }
        let end_byte = previous.len();
        let start = if self.finished {
            now
        } else {
            self.cursor
                .unwrap_or(now)
                .max(now)
                .min(now + Duration::from_millis(800))
        };
        self.cursor = Some(start + Duration::from_millis(24));
        let key = self.next_segment_id;
        self.next_segment_id += 1;
        self.active.insert(
            key,
            Fade {
                part,
                range: start_byte..end_byte,
                appeared: now,
                start,
                duration: Duration::from_millis(if self.finished { 150 } else { 700 }),
                initial: 0.,
            },
        );
    }

    pub(super) fn sync(&mut self, model: &StreamModel) {
        let now = Instant::now();
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
            if let Some(table) = &line.table {
                for (row, cells) in table.rows.iter().enumerate() {
                    for (column, spans) in cells.iter().enumerate() {
                        self.register(
                            (line.block_id, index, row, TextPart::Cell(column)),
                            span_text(spans),
                            now,
                        );
                    }
                }
            } else {
                if line.starts_list_item() {
                    self.register(
                        (line.block_id, index, 0, TextPart::Marker),
                        format!("{} ", line.marker),
                        now,
                    );
                    if let Some(checked) = line.checked {
                        self.register(
                            (line.block_id, index, 0, TextPart::Checkbox),
                            if checked { "[x]" } else { "[ ]" }.to_owned(),
                            now,
                        );
                    }
                }
                self.register(
                    (line.block_id, index, 0, TextPart::Body),
                    span_text(&line.spans),
                    now,
                );
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
        self.cursor = None;
        self.finished = true;
    }

    pub(super) fn frame(&mut self, window: &Window, reduced: bool) -> FadeFrame {
        if reduced || !window.is_window_active() {
            self.active.clear();
            self.cursor = None;
            return FadeFrame::default();
        }
        let now = Instant::now();
        let mut parts: HashMap<PartKey, Vec<RunOpacity>> = HashMap::new();
        self.active.retain(|_, fade| {
            let opacity = fade.opacity(now);
            if opacity >= 1. {
                return false;
            }
            parts.entry(fade.part).or_default().push(RunOpacity {
                range: fade.range.clone(),
                opacity,
            });
            true
        });
        for fades in parts.values_mut() {
            fades.sort_unstable_by_key(|fade| fade.range.start);
        }
        if !parts.is_empty() {
            window.request_animation_frame();
        }
        FadeFrame { parts }
    }
}

fn span_text(spans: &[StreamSpan]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

struct RunOpacity {
    range: Range<usize>,
    opacity: f32,
}

#[derive(Default)]
pub(super) struct FadeFrame {
    parts: HashMap<PartKey, Vec<RunOpacity>>,
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
    pub(super) fn runs(&self, row: usize, part: TextPart, runs: Vec<TextRun>) -> Vec<TextRun> {
        let Some(fades) = self.frame.parts.get(&(self.block, self.line, row, part)) else {
            return runs;
        };
        let mut output = Vec::new();
        let mut offset = 0;
        for run in runs {
            let end = offset + run.len;
            while offset < end {
                let next = fades.iter().find(|fade| fade.range.end > offset);
                let (boundary, opacity) = match next {
                    Some(fade) if fade.range.start <= offset => {
                        (end.min(fade.range.end), fade.opacity)
                    }
                    Some(fade) => (end.min(fade.range.start), 1.),
                    None => (end, 1.),
                };
                let mut fragment = run.clone();
                fragment.len = boundary - offset;
                fragment.color.a *= opacity;
                if let Some(color) = &mut fragment.background_color {
                    color.a *= opacity;
                }
                if let Some(underline) = &mut fragment.underline
                    && let Some(color) = &mut underline.color
                {
                    color.a *= opacity;
                }
                if let Some(strikethrough) = &mut fragment.strikethrough
                    && let Some(color) = &mut strikethrough.color
                {
                    color.a *= opacity;
                }
                output.push(fragment);
                offset = boundary;
            }
        }
        output
    }
}
