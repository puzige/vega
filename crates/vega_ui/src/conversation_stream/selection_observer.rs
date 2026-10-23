use super::{inline_code, links, model};
use gpui_kit::{AnyElement, App, FocusHandle, Hsla, IntoElement, StyledText, TextRun, Window};
use std::{ops::Range, sync::Arc};

#[path = "selection.rs"]
mod original;

#[derive(Clone)]
pub(crate) struct SelectionDocumentBuilder(original::SelectionDocumentBuilder);

impl SelectionDocumentBuilder {
    pub(crate) fn new(copy: model::MessageCopy, focus: FocusHandle, color: Hsla) -> Self {
        Self(original::SelectionDocumentBuilder::new(copy, focus, color))
    }

    pub(super) fn projected(
        copy: model::MessageCopy,
        focus: FocusHandle,
        color: Hsla,
        content: Arc<str>,
        identity: u64,
    ) -> Self {
        Self(original::SelectionDocumentBuilder::projected(
            copy, focus, color, content, identity,
        ))
    }

    pub(super) fn append_positioned(
        &self,
        text: &str,
        runs: Vec<TextRun>,
        links: Vec<links::LinkRange>,
        content_range: Range<usize>,
        logical_range: Range<usize>,
    ) -> AnyElement {
        crate::test_support::current_element(
            self.0
                .append_positioned(text, runs, links, content_range, logical_range),
            text.to_owned(),
        )
        .into_any_element()
    }

    pub(crate) fn append_literal(&mut self, text: &str) {
        self.0.append_literal(text);
    }

    pub(crate) fn append_styled(
        &mut self,
        text: &str,
        styled: StyledText,
        id_prefix: &str,
    ) -> AnyElement {
        crate::test_support::current_element(
            self.0.append_styled(text, styled, id_prefix),
            text.to_owned(),
        )
        .into_any_element()
    }

    pub(crate) fn append_styled_runs(
        &mut self,
        text: &str,
        runs: Vec<TextRun>,
        links: Vec<links::LinkRange>,
        id_prefix: &str,
    ) -> AnyElement {
        crate::test_support::current_element(
            self.0.append_styled_runs(text, runs, links, id_prefix),
            text.to_owned(),
        )
        .into_any_element()
    }

    pub(crate) fn wrap(self, body: AnyElement, window: &Window, cx: &mut App) -> AnyElement {
        self.0.wrap(body, window, cx)
    }
}
