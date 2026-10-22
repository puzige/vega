use super::MdStream;
use super::boundary_detector::{BoundaryDetector, is_table_delimiter};
use super::engine::AppendCtx;
use super::footnotes::is_footnote_definition_start;
use super::html::{html_block_start_state, update_html_block_state};
use super::mode::BlockMode;
use crate::boundary::BoundaryUpdate;
use crate::options::FootnotesMode;
use crate::syntax::facts::{
    count_double_dollars, fence_end, fence_start, is_atx_heading_start as is_heading,
    is_blank_line as is_empty_line, is_blockquote_start, is_list_item_start, is_thematic_break,
    setext_underline_char,
};
use crate::types::{Block, BlockStatus};

impl MdStream {
    pub(super) fn start_mode_for_line(&self, line: &str) -> BlockMode {
        if let Some(idx) = self.boundaries.start_index(line) {
            return BlockMode::CustomBoundary {
                plugin_index: idx,
                started: false,
            };
        }
        if is_heading(line) {
            return BlockMode::Heading;
        }
        if is_thematic_break(line) {
            return BlockMode::ThematicBreak;
        }
        if let Some((ch, len)) = fence_start(line) {
            return BlockMode::CodeFence {
                fence_char: ch,
                fence_len: len,
            };
        }
        if is_footnote_definition_start(line) {
            return BlockMode::FootnoteDefinition;
        }
        if is_blockquote_start(line) {
            return BlockMode::BlockQuote;
        }
        if is_list_item_start(line) {
            return BlockMode::List;
        }
        if let Some((stack, in_comment)) = html_block_start_state(line) {
            return BlockMode::HtmlBlock { stack, in_comment };
        }
        let dollars = count_double_dollars(line);
        if dollars % 2 == 1 && line.trim_start().starts_with("$$") {
            return BlockMode::MathBlock { open_count: 0 };
        }
        BlockMode::Paragraph
    }

    pub(super) fn commit_block(&mut self, end_line_inclusive: usize, ctx: &mut AppendCtx<'_>) {
        if self.block_machine.current_block_start_line >= self.input.line_count() {
            return;
        }
        if end_line_inclusive < self.block_machine.current_block_start_line {
            return;
        }
        let Some(start_off) = self
            .input
            .line_start(self.block_machine.current_block_start_line)
        else {
            return;
        };
        let Some(end_off) = self.input.line_end_with_newline(end_line_inclusive) else {
            return;
        };
        if end_off <= start_off {
            return;
        }

        let raw = self.input.as_str()[start_off..end_off].to_string();
        if raw.trim().is_empty() {
            self.block_machine
                .start_next_block_after(end_line_inclusive);
            self.boundaries.clear_active();
            self.pending_display.clear();
            return;
        }
        let block = Block {
            id: self.block_machine.current_block_id,
            status: BlockStatus::Committed,
            kind: self.block_machine.current_mode.kind(),
            raw,
            display: None,
        };
        self.push_committed_block(block, ctx);

        self.block_machine
            .start_next_block_after(end_line_inclusive);
        self.boundaries.clear_active();
        self.pending_display.clear();
    }

    pub(super) fn push_committed_block(&mut self, block: Block, ctx: &mut AppendCtx<'_>) {
        let effects = self
            .semantics
            .observe_committed_block(&block, self.opts.reference_definitions);
        ctx.extend_invalidated(effects.invalidated);

        ctx.push_committed_clone(&block);
        self.committed.push(block);
    }

    fn maybe_commit_single_line(&mut self, line_index: usize, ctx: &mut AppendCtx<'_>) {
        match self.block_machine.current_mode {
            BlockMode::Heading | BlockMode::ThematicBreak => {
                self.commit_block(line_index, ctx);
            }
            _ => {}
        }
    }

    pub(super) fn line_str(&self, line_index: usize) -> &str {
        self.input.line_str(line_index)
    }

    pub(super) fn process_line(&mut self, line_index: usize, ctx: &mut AppendCtx<'_>) {
        if !self.input.line_has_newline(line_index) {
            return;
        }

        if self.opts.footnotes == FootnotesMode::SingleBlock && self.semantics.footnotes_detected()
        {
            return;
        }

        if line_index == self.block_machine.current_block_start_line {
            self.block_machine.current_mode = self.start_mode_for_line(self.line_str(line_index));
            self.maybe_commit_single_line(line_index, ctx);

            self.update_mode_with_line(line_index, ctx);
            return;
        }

        let (boundary, next_mode) = {
            let prev = self.line_str(line_index - 1);
            let curr = self.line_str(line_index);
            let boundary = self.is_new_block_boundary(prev, curr, line_index);
            let next_mode = if boundary {
                Some(self.start_mode_for_line(curr))
            } else {
                None
            };
            (boundary, next_mode)
        };

        if boundary {
            self.commit_block(line_index - 1, ctx);
            if let Some(m) = next_mode {
                self.block_machine.current_mode = m;
            }
            self.maybe_commit_single_line(line_index, ctx);

            self.update_mode_with_line(line_index, ctx);
            return;
        }

        self.update_mode_with_line(line_index, ctx);
    }

    pub(super) fn process_incomplete_tail_boundary(&mut self, ctx: &mut AppendCtx<'_>) {
        if self.input.line_count() < 2 {
            return;
        }
        let last = self.input.line_count() - 1;
        if self.input.line_has_newline(last) {
            return;
        }
        if !self.input.line_has_newline(last - 1) {
            return;
        }

        if self.opts.footnotes == FootnotesMode::SingleBlock && self.semantics.footnotes_detected()
        {
            return;
        }

        let boundary = {
            let prev = self.line_str(last - 1);
            let curr = self.line_str(last);
            self.is_new_block_boundary(prev, curr, last)
        };

        if boundary {
            self.commit_block(last - 1, ctx);
            self.block_machine.current_mode = self.start_mode_for_line(self.line_str(last));
        }
    }

    fn is_new_block_boundary(&self, prev: &str, curr: &str, curr_line_index: usize) -> bool {
        let block_start_mode =
            if self.block_machine.current_block_start_line < self.input.line_count() {
                self.start_mode_for_line(self.line_str(self.block_machine.current_block_start_line))
            } else {
                BlockMode::Unknown
            };
        let detector = BoundaryDetector::new(
            &self.block_machine.current_mode,
            block_start_mode,
            self.block_machine.current_block_start_line,
            self.input.line_has_newline(curr_line_index),
            self.boundaries.matches_start(curr),
            matches!(
                self.block_machine.current_mode,
                BlockMode::List | BlockMode::BlockQuote
            ) && self.block_machine.fence_context.protects_line(curr),
        );
        detector
            .detect(prev, curr, curr_line_index)
            .starts_new_block()
    }

    fn update_mode_with_line(&mut self, line_index: usize, ctx: &mut AppendCtx<'_>) {
        if line_index < self.block_machine.current_block_start_line {
            return;
        }
        let (start, end) = {
            let Some(line) = self.input.line(line_index) else {
                return;
            };
            (line.start(), line.end())
        };
        let line = &self.input.as_str()[start..end];
        if matches!(
            self.block_machine.current_mode,
            BlockMode::List | BlockMode::BlockQuote | BlockMode::CodeFence { .. }
        ) {
            self.block_machine
                .fence_context
                .observe_line(line, start, end + 1, true);
        }
        match &mut self.block_machine.current_mode {
            BlockMode::Unknown => {
                self.block_machine.current_mode = self.start_mode_for_line(line);
                self.maybe_commit_single_line(line_index, ctx);
            }
            BlockMode::CodeFence {
                fence_char,
                fence_len,
            } => {
                if line_index > self.block_machine.current_block_start_line
                    && fence_end(line, *fence_char, *fence_len)
                {
                    self.commit_block(line_index, ctx);
                }
            }
            BlockMode::CustomBoundary {
                plugin_index,
                started,
            } => {
                let idx = *plugin_index;
                if !self.boundaries.contains(idx) {
                    return;
                }
                self.boundaries.set_active(idx);
                if !*started {
                    self.boundaries.start(idx, line);
                    *started = true;
                }
                if self.boundaries.update(idx, line) == BoundaryUpdate::Close {
                    self.boundaries.clear_active();
                    self.commit_block(line_index, ctx);
                }
            }
            BlockMode::MathBlock { open_count } => {
                *open_count += count_double_dollars(line);
                if *open_count % 2 == 0 {
                    self.commit_block(line_index, ctx);
                }
            }
            BlockMode::Paragraph => {
                if setext_underline_char(line).is_some()
                    && self.block_machine.current_block_start_line + 1 == line_index
                    && line_index > 0
                {
                    let prev = self.input.line_str(line_index - 1);
                    if !is_empty_line(prev) {
                        self.block_machine.current_mode = BlockMode::Heading;
                        self.commit_block(line_index, ctx);
                        return;
                    }
                }

                if is_table_delimiter(line) && line_index > 0 {
                    let prev = self.input.line_str(line_index - 1);
                    if prev.contains('|') {
                        self.block_machine.current_mode = BlockMode::Table;
                    }
                }
            }
            BlockMode::Table => {}
            BlockMode::HtmlBlock { stack, in_comment } => {
                update_html_block_state(line, stack, in_comment);
                if !*in_comment && stack.is_empty() {
                    self.commit_block(line_index, ctx);
                }
            }
            BlockMode::FootnoteDefinition => {}
            BlockMode::List | BlockMode::BlockQuote => {}
            BlockMode::Heading | BlockMode::ThematicBreak => {}
        }
    }
}
