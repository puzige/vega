mod context;
mod emphasis;
mod inline;
mod links;
mod setext;

use crate::stream::fence_context::FenceContext;
use crate::syntax::facts::tail_window;

use super::TerminatorOptions;
use emphasis::{
    handle_incomplete_bold, handle_incomplete_bold_italic,
    handle_incomplete_double_underscore_italic, handle_incomplete_single_asterisk_italic,
    handle_incomplete_single_underscore_italic,
};
use inline::{balance_inline_code, balance_katex_block, balance_strikethrough};
pub(crate) use links::fix_incomplete_link_or_image;
use setext::{apply_setext_heading_protection, trim_trailing_single_space};

#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingRepair<'a> {
    opts: &'a TerminatorOptions,
}

impl<'a> PendingRepair<'a> {
    pub(crate) fn new(opts: &'a TerminatorOptions) -> Self {
        Self { opts }
    }

    pub(crate) fn apply(&self, text: &str) -> String {
        self.apply_with_fences(text, &FenceContext::for_text(text), 0)
    }

    pub(crate) fn apply_with_fences(
        &self,
        text: &str,
        context: &FenceContext,
        raw_start: usize,
    ) -> String {
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0;
        for range in context.protected_ranges() {
            let start = range.start.saturating_sub(raw_start).min(text.len());
            let end = range.end.saturating_sub(raw_start).min(text.len());
            if end <= cursor {
                continue;
            }
            if start > cursor {
                out.push_str(&self.apply_prose(&text[cursor..start]));
            }
            out.push_str(&text[start.max(cursor)..end]);
            cursor = end;
        }
        out.push_str(&self.apply_prose(&text[cursor..]));
        out
    }

    fn apply_prose(&self, text: &str) -> String {
        let opts = self.opts;
        if text.is_empty() {
            return String::new();
        }

        let text = trim_trailing_single_space(text);
        let (window, offset) = tail_window(text, opts.window_bytes);

        let prefix = &text[..offset];
        let mut tail = window.to_string();

        if opts.setext_headings {
            tail = apply_setext_heading_protection(&tail);
        }

        if opts.links || opts.images {
            if let Some(processed) = fix_incomplete_link_or_image(
                &tail,
                &opts.incomplete_link_url,
                opts.links,
                opts.images,
            ) {
                if processed.ends_with(&format!("]({})", opts.incomplete_link_url)) {
                    let mut out = String::with_capacity(prefix.len() + processed.len());
                    out.push_str(prefix);
                    out.push_str(&processed);
                    return out;
                }
                tail = processed;
            }
        }

        if opts.emphasis {
            tail = handle_incomplete_bold_italic(&tail);
            tail = handle_incomplete_bold(&tail);
            tail = handle_incomplete_double_underscore_italic(&tail);
            tail = handle_incomplete_single_asterisk_italic(&tail);
            tail = handle_incomplete_single_underscore_italic(&tail);
        }
        if opts.inline_code {
            tail = balance_inline_code(&tail);
        }
        if opts.strikethrough {
            tail = balance_strikethrough(&tail);
        }
        if opts.katex_block {
            tail = balance_katex_block(&tail);
        }

        let mut out = String::with_capacity(prefix.len() + tail.len());
        out.push_str(prefix);
        out.push_str(&tail);
        out
    }
}
