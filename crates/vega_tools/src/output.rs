//! Bounded rendered output shared by built-in tools.

use std::collections::VecDeque;

/// Hard cap on glob/grep result entries (tech-spec §4.4: 结果上限 500 条).
pub const MAX_RESULTS: usize = 500;

/// Per-line character cap for the read tool (tech-spec §4.4: 单行 >2k 截断).
pub const MAX_LINE_CHARS: usize = 2000;

/// Marker appended to a read line cut at [`MAX_LINE_CHARS`].
pub const LINE_TRUNCATION_MARKER: &str = "…[截断]";

/// Marker line appended when glob/grep results exceed [`MAX_RESULTS`]
/// (kept in sync with the 500-entry cap).
pub const RESULT_TRUNCATION_MARKER: &str = "…[截断：结果超过上限 500 条]";

/// Fixed async read chunk for bash output.
pub const BASH_READ_CHUNK_BYTES: usize = 16 * 1024;
/// Maximum rendered bytes retained for one logical bash line.
pub const BASH_MAX_LINE_BYTES: usize = 64 * 1024;
/// Maximum payload lines retained at each end of bash output.
pub const BASH_MAX_LINES_PER_SIDE: usize = 2_000;
/// Maximum rendered bytes retained at each end of bash output.
pub const BASH_MAX_BYTES_PER_SIDE: usize = 4 * 1024 * 1024;
/// Stable marker inserted into an oversized logical line.
pub const BASH_LINE_MIDDLE_MARKER: &str = "…[line middle truncated]…";
/// Stable marker inserted between retained output head and tail.
pub const BASH_OUTPUT_MIDDLE_MARKER: &str = "…[output middle truncated]…";

/// What a tool returns on success: rendered text plus a truncation flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// Rendered text payload (numbered lines, path list, match list).
    pub text: String,
    /// True when any content was cut: read lines past [`MAX_LINE_CHARS`]
    /// or glob/grep entries past [`MAX_RESULTS`].
    pub truncated: bool,
}

impl ToolOutput {
    /// A clean (untruncated) output with the given text.
    pub fn clean(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            truncated: false,
        }
    }
}

/// Completed bash output and execution metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashOutput {
    /// Bounded UTF-8 rendering of merged stdout/stderr.
    pub text: String,
    /// Process exit code, or `-1` when termination was signal-only.
    pub exit_code: i32,
    /// Monotonic wall duration measured by the tool.
    pub duration_ms: u64,
    /// True when a line or the overall stream was truncated.
    pub truncated: bool,
    #[cfg(test)]
    pub(crate) high_water_bytes: usize,
}

#[derive(Debug)]
pub(crate) struct BashOutputCollector {
    current: Utf8Line,
    head: String,
    head_lines: usize,
    head_closed: bool,
    tail: VecDeque<IndexedLine>,
    tail_bytes: usize,
    tail_owned: usize,
    next_line: u64,
    any_line_truncated: bool,
    high_water_bytes: usize,
}

#[derive(Debug)]
struct IndexedLine {
    index: u64,
    text: String,
    cost: usize,
    owned: usize,
}

impl BashOutputCollector {
    pub(crate) fn new() -> Self {
        let mut collector = Self {
            current: Utf8Line::new(),
            head: String::new(),
            head_lines: 0,
            head_closed: false,
            tail: VecDeque::new(),
            tail_bytes: 0,
            tail_owned: 0,
            next_line: 0,
            any_line_truncated: false,
            high_water_bytes: 0,
        };
        collector.observe_high_water();
        collector
    }

    pub(crate) fn push(&mut self, mut bytes: &[u8]) {
        while let Some(position) = bytes.iter().position(|byte| *byte == b'\n') {
            self.current.push(&bytes[..position]);
            self.finish_current_line();
            bytes = &bytes[position + 1..];
        }
        self.current.push(bytes);
        self.observe_high_water();
    }

    pub(crate) fn finish(mut self) -> CollectedBashOutput {
        if !self.current.is_empty() {
            self.finish_current_line();
        }

        let last_head_index = self.head_lines.checked_sub(1).map(|value| value as u64);
        while self
            .tail
            .front()
            .is_some_and(|line| last_head_index.is_some_and(|head| line.index <= head))
        {
            if let Some(line) = self.tail.pop_front() {
                self.tail_bytes = self.tail_bytes.saturating_sub(line.cost);
                self.tail_owned = self.tail_owned.saturating_sub(line.owned);
            }
        }

        self.current.release();
        self.observe_high_water();

        let omitted = self.head_closed || !self.tail.is_empty();
        let marker_needed = omitted
            && self.tail.front().is_some_and(|line| {
                last_head_index.map_or(line.index > 0, |head| line.index > head + 1)
            });
        if marker_needed {
            append_line(&mut self.head, BASH_OUTPUT_MIDDLE_MARKER);
            self.observe_high_water();
        }
        while let Some(line) = self.tail.pop_front() {
            self.tail_bytes = self.tail_bytes.saturating_sub(line.cost);
            self.tail_owned = self.tail_owned.saturating_sub(line.owned);
            self.observe_high_water_with_extra(line.owned);
            append_line(&mut self.head, &line.text);
            self.observe_high_water_with_extra(line.owned);
        }
        if self.head.ends_with('\n') {
            self.head.pop();
        }

        CollectedBashOutput {
            text: self.head,
            truncated: self.any_line_truncated || marker_needed,
            #[cfg(test)]
            high_water_bytes: self.high_water_bytes,
        }
    }

    fn finish_current_line(&mut self) {
        let current = std::mem::replace(&mut self.current, Utf8Line::new());
        let rendered = current.finish();
        self.observe_high_water_with_extra(rendered.peak_owned);
        self.any_line_truncated |= rendered.truncated;
        let index = self.next_line;
        self.next_line = self.next_line.saturating_add(1);
        let cost = rendered.text.len().saturating_add(1);

        let marker_reserve = BASH_OUTPUT_MIDDLE_MARKER.len().saturating_add(1);
        let head_budget = BASH_MAX_BYTES_PER_SIDE.saturating_sub(marker_reserve);
        if !self.head_closed
            && self.head_lines < BASH_MAX_LINES_PER_SIDE
            && self.head.len().saturating_add(cost) <= head_budget
        {
            append_line(&mut self.head, &rendered.text);
            self.head_lines += 1;
        } else {
            self.head_closed = true;
        }

        let owned = rendered.text.capacity();
        self.tail.push_back(IndexedLine {
            index,
            text: rendered.text,
            cost,
            owned,
        });
        self.tail_bytes = self.tail_bytes.saturating_add(cost);
        self.tail_owned = self.tail_owned.saturating_add(owned);
        while self.tail.len() > BASH_MAX_LINES_PER_SIDE || self.tail_bytes > BASH_MAX_BYTES_PER_SIDE
        {
            if let Some(line) = self.tail.pop_front() {
                self.tail_bytes = self.tail_bytes.saturating_sub(line.cost);
                self.tail_owned = self.tail_owned.saturating_sub(line.owned);
            } else {
                break;
            }
        }
        self.observe_high_water();
    }

    fn observe_high_water(&mut self) {
        self.observe_high_water_with_extra(0);
    }

    fn observe_high_water_with_extra(&mut self, extra: usize) {
        let owned = self
            .head
            .capacity()
            .saturating_add(self.tail_owned)
            .saturating_add(self.current.owned_bytes())
            .saturating_add(BASH_READ_CHUNK_BYTES)
            .saturating_add(extra);
        self.high_water_bytes = self.high_water_bytes.max(owned);
        debug_assert!(
            owned <= 2 * BASH_MAX_BYTES_PER_SIDE + BASH_MAX_LINE_BYTES + BASH_READ_CHUNK_BYTES + 8
        );
    }
}

pub(crate) struct CollectedBashOutput {
    pub(crate) text: String,
    pub(crate) truncated: bool,
    #[cfg(test)]
    pub(crate) high_water_bytes: usize,
}

#[derive(Debug)]
struct RenderedLine {
    text: String,
    truncated: bool,
    peak_owned: usize,
}

#[derive(Debug)]
struct Utf8Line {
    storage: Vec<u8>,
    head_budget: usize,
    tail_budget: usize,
    tail_base: Option<usize>,
    tail_start: usize,
    tail_len: usize,
    pending: Vec<u8>,
    tail_evicted: bool,
}

impl Utf8Line {
    fn new() -> Self {
        let content_budget = BASH_MAX_LINE_BYTES.saturating_sub(BASH_LINE_MIDDLE_MARKER.len());
        let tail_budget = content_budget / 2;
        Self {
            storage: Vec::new(),
            head_budget: content_budget - tail_budget,
            tail_budget,
            tail_base: None,
            tail_start: 0,
            tail_len: 0,
            pending: Vec::with_capacity(4),
            tail_evicted: false,
        }
    }

    fn is_empty(&self) -> bool {
        self.storage.is_empty() && self.pending.is_empty()
    }

    fn owned_bytes(&self) -> usize {
        self.storage
            .capacity()
            .saturating_add(self.pending.capacity())
    }

    fn release(&mut self) {
        self.storage = Vec::new();
        self.head_budget = 0;
        self.tail_budget = 0;
        self.tail_base = None;
        self.tail_start = 0;
        self.tail_len = 0;
        self.pending = Vec::new();
        self.tail_evicted = false;
    }

    fn push(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if !self.pending.is_empty() {
            let needed = 4_usize.saturating_sub(self.pending.len()).min(bytes.len());
            self.pending.extend_from_slice(&bytes[..needed]);
            let pending = std::mem::take(&mut self.pending);
            self.decode(&pending);
            if needed == bytes.len() {
                return;
            }
            self.decode(&bytes[needed..]);
        } else {
            self.decode(bytes);
        }
    }

    fn decode(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            match std::str::from_utf8(bytes) {
                Ok(valid) => {
                    self.push_valid(valid);
                    return;
                }
                Err(error) => {
                    let valid_end = error.valid_up_to();
                    if valid_end != 0
                        && let Ok(valid) = std::str::from_utf8(&bytes[..valid_end])
                    {
                        self.push_valid(valid);
                    }
                    match error.error_len() {
                        Some(length) => {
                            self.push_char('\u{fffd}');
                            bytes = &bytes[valid_end + length..];
                        }
                        None => {
                            self.pending.extend_from_slice(&bytes[valid_end..]);
                            return;
                        }
                    }
                }
            }
        }
    }

    fn push_valid(&mut self, value: &str) {
        for character in value.chars() {
            self.push_char(character);
        }
    }

    fn push_char(&mut self, character: char) {
        let mut encoded = [0_u8; 4];
        let value = character.encode_utf8(&mut encoded);
        if self.tail_base.is_none()
            && self.storage.len().saturating_add(value.len()) <= self.head_budget
        {
            self.storage.extend_from_slice(value.as_bytes());
            return;
        }
        if self.tail_base.is_none() {
            self.tail_base = Some(self.storage.len());
            let additional = BASH_MAX_LINE_BYTES.saturating_sub(self.storage.len());
            self.storage.reserve_exact(additional);
        }
        self.push_tail(value.as_bytes());
    }

    fn push_tail(&mut self, bytes: &[u8]) {
        while self.tail_len.saturating_add(bytes.len()) > self.tail_budget && self.tail_len != 0 {
            self.evict_tail_character();
            self.tail_evicted = true;
        }
        let Some(base) = self.tail_base else {
            return;
        };
        for byte in bytes {
            let physical_len = self.storage.len().saturating_sub(base);
            if physical_len < self.tail_budget {
                self.storage.push(*byte);
            } else {
                let index = base + (self.tail_start + self.tail_len) % self.tail_budget;
                self.storage[index] = *byte;
            }
            self.tail_len += 1;
        }
    }

    fn evict_tail_character(&mut self) {
        let Some(base) = self.tail_base else {
            return;
        };
        if self.tail_len == 0 || self.tail_budget == 0 {
            return;
        }
        let first = self.storage[base + self.tail_start];
        let width = utf8_width(first).min(self.tail_len);
        self.tail_start = (self.tail_start + width) % self.tail_budget;
        self.tail_len -= width;
    }

    fn finish(mut self) -> RenderedLine {
        if !self.pending.is_empty() {
            self.push_char('\u{fffd}');
            self.pending.clear();
        }
        if let Some(base) = self.tail_base {
            if self.tail_start != 0 {
                self.storage[base..].rotate_left(self.tail_start);
            }
            self.storage.truncate(base.saturating_add(self.tail_len));
            if self.tail_evicted {
                let original_len = self.storage.len();
                let marker_len = BASH_LINE_MIDDLE_MARKER.len();
                self.storage
                    .resize(original_len.saturating_add(marker_len), 0);
                self.storage
                    .copy_within(base..original_len, base.saturating_add(marker_len));
                self.storage[base..base + marker_len]
                    .copy_from_slice(BASH_LINE_MIDDLE_MARKER.as_bytes());
            }
        }
        let peak_owned = self.owned_bytes();
        self.storage.shrink_to_fit();
        let text = match String::from_utf8(self.storage) {
            Ok(value) => value,
            Err(_) => "[invalid utf-8 output]".to_string(),
        };
        RenderedLine {
            text,
            truncated: self.tail_evicted,
            peak_owned,
        }
    }
}

fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn append_line(output: &mut String, line: &str) {
    output.reserve_exact(line.len().saturating_add(1));
    output.push_str(line);
    output.push('\n');
}

/// Render a result list while keeping at most [`MAX_RESULTS`] entries.
///
/// Callers collect at most `MAX_RESULTS + 1`, so the extra entry is only a
/// truncation sentinel and never reaches the output.
pub(crate) fn capped_results(mut entries: Vec<String>) -> ToolOutput {
    let truncated = entries.len() > MAX_RESULTS;
    entries.truncate(MAX_RESULTS);
    if truncated {
        entries.push(RESULT_TRUNCATION_MARKER.to_string());
    }
    ToolOutput {
        text: entries.join("\n"),
        truncated,
    }
}

#[cfg(test)]
mod bash_output_tests {
    use super::{
        BASH_LINE_MIDDLE_MARKER, BASH_MAX_BYTES_PER_SIDE, BASH_MAX_LINE_BYTES,
        BASH_READ_CHUNK_BYTES, BashOutputCollector,
    };

    #[test]
    fn bash_output_invalid_utf8_is_stable_and_never_splits_rendering() {
        let mut collector = BashOutputCollector::new();
        collector.push(b"a\xffb\xe2\x82");
        let output = collector.finish();
        assert_eq!(output.text, "a\u{fffd}b\u{fffd}");
        assert!(!output.truncated);
    }

    #[test]
    fn bash_output_utf8_scalar_roundtrips_across_every_chunk_split() {
        let scalar = "🦀".as_bytes();
        for split in 1..scalar.len() {
            let prefix_len = BASH_READ_CHUNK_BYTES - split;
            let mut first = vec![b'a'; prefix_len];
            first.extend_from_slice(&scalar[..split]);
            assert_eq!(first.len(), BASH_READ_CHUNK_BYTES);

            let mut collector = BashOutputCollector::new();
            collector.push(&first);
            collector.push(&scalar[split..]);
            collector.push(b"z");
            let output = collector.finish();
            let expected = format!("{}🦀z", "a".repeat(prefix_len));
            assert_eq!(output.text, expected, "split {split}");
            assert!(!output.truncated, "split {split}");
        }
    }

    #[test]
    fn bash_output_one_huge_line_keeps_both_ends() {
        let mut collector = BashOutputCollector::new();
        collector.push(&vec![b'a'; BASH_MAX_LINE_BYTES]);
        collector.push(&vec![b'z'; BASH_MAX_LINE_BYTES]);
        let output = collector.finish();
        assert!(output.truncated);
        assert!(output.text.starts_with('a'));
        assert!(output.text.contains(BASH_LINE_MIDDLE_MARKER));
        assert!(output.text.ends_with('z'));
        assert_eq!(output.text.len(), BASH_MAX_LINE_BYTES);
    }

    #[test]
    fn bash_output_forty_kib_line_is_not_truncated() {
        let line = vec![b'x'; 40 * 1024];
        let mut collector = BashOutputCollector::new();
        collector.push(&line);
        let output = collector.finish();
        assert!(!output.truncated);
        assert_eq!(output.text.as_bytes(), line);
    }

    #[test]
    fn bash_output_near_full_head_tail_and_current_line_stays_within_owned_budget() {
        let mut collector = BashOutputCollector::new();
        let mut retained_line = vec![b'x'; 2_048];
        retained_line.push(b'\n');
        for _ in 0..4_001 {
            collector.push(&retained_line);
        }
        collector.push(&vec![b'z'; 2 * BASH_MAX_LINE_BYTES]);
        let output = collector.finish();
        let strict_limit =
            2 * BASH_MAX_BYTES_PER_SIDE + BASH_MAX_LINE_BYTES + BASH_READ_CHUNK_BYTES + 8;
        assert!(output.truncated);
        assert!(output.high_water_bytes > 2 * BASH_MAX_BYTES_PER_SIDE - 256 * 1024);
        assert!(output.high_water_bytes <= strict_limit);
    }
}
