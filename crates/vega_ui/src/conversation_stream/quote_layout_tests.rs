use std::cell::RefCell;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, AvailableSpace, Bounds, Context, Element, ElementId, GlobalElementId,
    InspectorElementId, LayoutId, Pixels, Point, Size, StyledText, TestAppContext, TextLayout,
    VisualTestContext, Window, canvas, point, px, size,
};
use vega_markdown::MarkdownStream;
use vega_theme::{Layout, Theme};

use super::{MessageCopy, StreamCounters, StreamModel, render_rows};

pub(super) const LONG_QUOTES: [(&str, &str); 2] = [
    (
        "> Natural wrapping keeps this English quotation readable when the message column becomes narrower. 中文引用也应该根据正文的实际可用宽度自然折行，并在调整窗口后重新排版。 The original words and punctuation remain complete without inserting soft newlines into copied text.",
        "Natural wrapping keeps this English quotation readable when the message column becomes narrower. 中文引用也应该根据正文的实际可用宽度自然折行，并在调整窗口后重新排版。 The original words and punctuation remain complete without inserting soft newlines into copied text.",
    ),
    (
        "> Read this **important English and 中文说明** alongside `inline_code` and the [linked reading guide](https://example.com/guide). 较窄的正文列应让整段引用自然折行，保留粗体、链接和行内代码的完整文字。 Widening the column should reduce its height without changing the copied content.",
        "Read this important English and 中文说明 alongside inline_code and the linked reading guide. 较窄的正文列应让整段引用自然折行，保留粗体、链接和行内代码的完整文字。 Widening the column should reduce its height without changing the copied content.",
    ),
];

#[derive(Default)]
pub(super) struct Observation {
    pub(super) size: Size<Pixels>,
    text: Vec<TextGeometry>,
    bars: Vec<Bounds<Pixels>>,
}

struct TextGeometry {
    text: String,
    bounds: Bounds<Pixels>,
    positions: Vec<Point<Pixels>>,
    line_height: Pixels,
}

thread_local! {
    static RECORDING: RefCell<Option<Observation>> = const { RefCell::new(None) };
}

pub(super) fn record_text(text: &str, layout: &TextLayout) {
    RECORDING.with_borrow_mut(|recording| {
        if let Some(recording) = recording {
            let actual_text = layout.text();
            assert_eq!(actual_text.as_str(), text, "layout retains complete text");
            recording.text.push(TextGeometry {
                text: actual_text,
                bounds: layout.bounds(),
                positions: text
                    .char_indices()
                    .map(|(index, _)| index)
                    .chain([text.len()])
                    .map(|index| {
                        layout
                            .position_for_index(index)
                            .expect("laid-out character")
                    })
                    .collect(),
                line_height: layout.line_height(),
            });
        }
    });
}

pub(super) fn record_bar(bounds: Bounds<Pixels>) {
    RECORDING.with_borrow_mut(|recording| {
        if let Some(recording) = recording {
            recording.bars.push(bounds);
        }
    });
}

pub(super) struct Observed<E> {
    element: E,
    text: Option<(String, TextLayout)>,
}

pub(super) fn observe_text(element: StyledText, content: String) -> Observed<StyledText> {
    let layout = element.layout().clone();
    Observed {
        element,
        text: Some((content, layout)),
    }
}

pub(super) fn observe_bar<E: Element>(element: E) -> Observed<E> {
    Observed {
        element,
        text: None,
    }
}

impl<E: Element> IntoElement for Observed<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for Observed<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.element.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.element.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.element.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let result = self
            .element
            .prepaint(id, inspector, bounds, state, window, cx);
        if let Some((text, layout)) = &self.text {
            record_text(text, layout);
        } else {
            record_bar(bounds);
        }
        result
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element
            .paint(id, inspector, bounds, state, prepaint, window, cx);
    }
}

type ContentBuilder = Box<dyn FnOnce(&mut Window, &mut App) -> AnyElement>;

struct LayoutView {
    width: f32,
    build: Option<ContentBuilder>,
}

impl Render for LayoutView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let width = self.width;
        let build = self.build.take().expect("one layout per observation");
        canvas(
            move |bounds, window, cx| {
                let mut element = build(window, cx);
                let measured = element.layout_as_root(
                    size(
                        AvailableSpace::Definite(px(width)),
                        AvailableSpace::MaxContent,
                    ),
                    window,
                    cx,
                );
                element.prepaint_at(bounds.origin, window, cx);
                RECORDING.with_borrow_mut(|recording| {
                    recording.as_mut().expect("active observation").size = measured;
                });
                element
            },
            |_, mut element, window, cx| element.paint(window, cx),
        )
        .w(px(width))
        .h(px(1024.))
    }
}

pub(super) fn draw(
    visual: &mut VisualTestContext,
    width: f32,
    build: impl FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
) -> Observation {
    RECORDING.with_borrow_mut(|recording| *recording = Some(Observation::default()));
    visual.draw(
        point(px(24.), px(32.)),
        size(
            AvailableSpace::Definite(px(width)),
            AvailableSpace::MaxContent,
        ),
        |_, cx| {
            cx.new(|_| LayoutView {
                width,
                build: Some(Box::new(build)),
            })
            .into_any_element()
        },
    );
    RECORDING.with_borrow_mut(|recording| recording.take().expect("completed observation"))
}

fn draw_markdown(
    visual: &mut VisualTestContext,
    source: &str,
    finished: bool,
    selectable: bool,
    width: f32,
) -> (Observation, Option<String>) {
    let mut stream = MarkdownStream::new();
    stream.append(source);
    if finished {
        stream.finish();
    }
    let mut model = StreamModel::default();
    model.sync(&stream.snapshot(), &StreamCounters::default());
    let copy = MessageCopy::new(source);
    let rendered_copy = copy.clone();
    let observation = draw(visual, width, move |window, cx| {
        let colors = Theme::light().colors;
        if selectable {
            let focus = cx.focus_handle();
            render_rows::selectable_markdown_content(
                &model,
                &rendered_copy,
                &focus,
                &colors,
                false,
                window,
                cx,
            )
        } else {
            render_rows::markdown_item(&model, None, &colors, "quote-layout")
        }
    });
    (observation, copy.visible_text())
}

impl Observation {
    pub(super) fn assert_quote(&self, expected: &str, width: f32) {
        assert_eq!(
            self.text
                .iter()
                .map(|text| text.text.as_str())
                .collect::<String>(),
            expected
        );
        assert_eq!(self.bars.len(), 1, "quote retains one visible bar");
        let bar = self.bars[0];
        assert_eq!(bar.size.width, px(2.));
        assert!(bar.size.height > px(0.));
        let left = bar.right() + px(Layout::MARKDOWN_CODE_PADDING);
        let right = px(24. + width);
        let tolerance = px(0.5);
        for text in &self.text {
            assert!(
                text.bounds.left() >= left - tolerance,
                "text overlaps quote gutter: {:?}",
                text.bounds
            );
            assert!(
                text.bounds.right() <= right + tolerance,
                "text exceeds quote right boundary: {:?}",
                text.bounds
            );
            assert!(text.bounds.top() >= bar.top() - tolerance);
            assert!(text.bounds.bottom() <= bar.bottom() + tolerance);
            for position in &text.positions {
                assert!(
                    position.x >= left - tolerance && position.x <= right + tolerance,
                    "laid-out character outside quote: {position:?}"
                );
            }
        }
    }

    pub(super) fn text_height(&self) -> Pixels {
        let top = self
            .text
            .iter()
            .map(|text| text.bounds.top())
            .min()
            .expect("visible text");
        let bottom = self
            .text
            .iter()
            .map(|text| text.bounds.bottom())
            .max()
            .expect("visible text");
        bottom - top
    }

    pub(super) fn line_height(&self) -> Pixels {
        self.text.first().expect("visible text").line_height
    }

    pub(super) fn assert_multiline_positions(&self) {
        let mut positions = self
            .text
            .iter()
            .flat_map(|text| &text.positions)
            .map(|position| position.y);
        let first = positions.next().expect("laid-out characters");
        let (min_y, max_y) = positions.fold((first, first), |(min_y, max_y), y| {
            (min_y.min(y), max_y.max(y))
        });
        assert!(
            max_y - min_y >= self.line_height() - px(0.5),
            "quote characters must occupy multiple visual lines: {min_y:?}..{max_y:?}"
        );
    }
}

fn assert_reflow(visual: &mut VisualTestContext, selectable: bool) {
    for (source, expected) in LONG_QUOTES {
        for finished in [false, true] {
            let mut observations = Vec::new();
            for width in [240., 480., 240.] {
                let (observation, copied) =
                    draw_markdown(visual, source, finished, selectable, width);
                observation.assert_quote(expected, width);
                if selectable {
                    assert_eq!(copied.as_deref(), Some(expected));
                    assert!(!copied.expect("visible copy projection").contains('\n'));
                }
                observations.push(observation);
            }
            assert!(observations[0].text_height() > observations[0].line_height());
            observations[0].assert_multiline_positions();
            assert!(
                observations[0].size.height > observations[1].size.height,
                "wider quote must need less height"
            );
            assert_eq!(
                observations[0].size, observations[2].size,
                "narrow width restores natural layout"
            );
        }
    }
}

#[gpui_kit::test]
fn markdown_quotes_reflow_within_the_quote_gutter(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| cx.set_global(Theme::light()));
    assert_reflow(cx.add_empty_window(), false);
}

#[gpui_kit::test]
fn selectable_quotes_reflow_without_copying_soft_newlines(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| cx.set_global(Theme::light()));
    assert_reflow(cx.add_empty_window(), true);
}

#[gpui_kit::test]
fn short_quotes_stay_single_line_and_code_keeps_horizontal_layout(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| cx.set_global(Theme::light()));
    let visual = cx.add_empty_window();
    let code = "let message = \"A long English code line stays horizontally scrollable; 中文代码内容也不应因为引用折行修复而换行。 Preserve every character in this deliberately wide code line.\";";
    let source = format!("```\n{code}\n```\n");
    for selectable in [false, true] {
        let mut code_sizes = Vec::new();
        for width in [240., 480., 240.] {
            let (quote, copied) =
                draw_markdown(visual, "> Short 中文 quote.", true, selectable, width);
            quote.assert_quote("Short 中文 quote.", width);
            assert_eq!(quote.text_height(), quote.line_height());
            if selectable {
                assert_eq!(copied.as_deref(), Some("Short 中文 quote."));
            }
            let (code_layout, copied) = draw_markdown(visual, &source, true, selectable, width);
            assert!(code_layout.bars.is_empty());
            assert_eq!(
                code_layout
                    .text
                    .iter()
                    .map(|text| text.text.as_str())
                    .collect::<String>(),
                code
            );
            assert_eq!(code_layout.text_height(), code_layout.line_height());
            assert!(
                code_layout
                    .text
                    .iter()
                    .flat_map(|text| &text.positions)
                    .any(|position| position.x > px(24. + width)),
                "wide code remains wider than the viewport"
            );
            if selectable {
                assert_eq!(copied.as_deref(), Some(code));
            }
            code_sizes.push(code_layout.size.height);
        }
        assert_eq!(code_sizes[0], code_sizes[1]);
        assert_eq!(code_sizes[0], code_sizes[2]);
    }
}
