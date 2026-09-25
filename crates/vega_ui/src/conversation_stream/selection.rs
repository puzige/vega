use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui_kit::base::{
    TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun,
    TextSelectionScopeId,
};
use gpui_kit::{
    App, BorderStyle, Bounds, Corners, Edges, Element, ElementId, FocusHandle, GlobalElementId,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels,
    Point, SharedString, StyledText, TextLayout, Window, transparent_black,
};

use super::model::MessageCopy;

#[cfg(test)]
thread_local! {
    pub(super) static SELECTION_RUN_LAYOUTS: RefCell<Vec<(String, TextLayout, Bounds<Pixels>)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
struct LaidOutRun {
    text: SharedString,
    layout: TextLayout,
    bounds: Bounds<Pixels>,
    order: u64,
    content_range: Range<usize>,
}

pub(crate) struct SelectionDocumentBuilder {
    copy: MessageCopy,
    focus: FocusHandle,
    content: String,
    next_run_order: u64,
    runs: Rc<RefCell<Vec<LaidOutRun>>>,
    projected_ranges: Rc<RefCell<Vec<Option<Range<usize>>>>>,
    selection_color: Hsla,
}

impl SelectionDocumentBuilder {
    pub(crate) fn new(copy: MessageCopy, focus: FocusHandle, selection_color: Hsla) -> Self {
        Self {
            copy,
            focus,
            content: String::new(),
            next_run_order: 0,
            runs: Rc::new(RefCell::new(Vec::new())),
            projected_ranges: Rc::new(RefCell::new(Vec::new())),
            selection_color,
        }
    }

    pub(crate) fn append_literal(&mut self, text: &str) {
        self.content.push_str(text);
    }

    pub(crate) fn append_styled(
        &mut self,
        text: &str,
        styled: StyledText,
        id_prefix: &str,
    ) -> gpui_kit::AnyElement {
        let start = self.content.len();
        self.content.push_str(text);
        let end = self.content.len();
        if text.is_empty() {
            return styled.into_any_element();
        }
        let order = self.next_run_order;
        self.next_run_order += 1;
        SelectableStyledText {
            id: ElementId::Name(format!("{id_prefix}-selection-run-{order}").into()),
            text: styled,
            content: text.into(),
            content_range: start..end,
            order,
            runs: self.runs.clone(),
            projected_ranges: self.projected_ranges.clone(),
            selection_color: self.selection_color,
        }
        .into_any_element()
    }

    pub(crate) fn wrap(
        self,
        body: gpui_kit::AnyElement,
        window: &Window,
        cx: &mut App,
    ) -> gpui_kit::AnyElement {
        let id = self.copy.id;
        let generation = self.copy.selection_generation();
        let previous = self.copy.visible_text();
        SelectableMessage {
            id: ElementId::Name(format!("selectable-message-{id}").into()),
            body,
            copy: self.copy.clone(),
            handle: self.copy.ensure_selection_handle(&self.focus, window, cx),
            scope: self.copy.selection_scope(),
            content: self.content,
            generation,
            previous_visible_text: previous,
            runs: self.runs,
            projected_ranges: self.projected_ranges,
        }
        .into_any_element()
    }
}

struct SelectableStyledText {
    id: ElementId,
    text: StyledText,
    content: SharedString,
    content_range: Range<usize>,
    order: u64,
    runs: Rc<RefCell<Vec<LaidOutRun>>>,
    projected_ranges: Rc<RefCell<Vec<Option<Range<usize>>>>>,
    selection_color: Hsla,
}

impl IntoElement for SelectableStyledText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableStyledText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (layout, ()) = self.text.request_layout(id, inspector_id, window, cx);
        (layout, ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.text
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);
        self.runs.borrow_mut().push(LaidOutRun {
            text: self.content.clone(),
            layout: self.text.layout().clone(),
            bounds,
            order: self.order,
            content_range: self.content_range.clone(),
        });
        #[cfg(test)]
        SELECTION_RUN_LAYOUTS.with_borrow_mut(|runs| {
            if self.order == 0 {
                runs.clear();
            }
            runs.push((self.content.to_string(), self.text.layout().clone(), bounds));
        });
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(Some(range)) = self.projected_ranges.borrow().get(self.order as usize) {
            paint_selection(
                self.text.layout(),
                range.clone(),
                self.selection_color,
                window,
            );
        }
        self.text
            .paint(id, inspector_id, bounds, &mut (), &mut (), window, cx);
    }
}

struct SelectableMessage {
    id: ElementId,
    body: gpui_kit::AnyElement,
    copy: MessageCopy,
    handle: TextSelectionHandle,
    scope: TextSelectionScopeId,
    content: String,
    generation: u64,
    previous_visible_text: Option<String>,
    runs: Rc<RefCell<Vec<LaidOutRun>>>,
    projected_ranges: Rc<RefCell<Vec<Option<Range<usize>>>>>,
}

struct MessagePrepaint {
    hitbox: Hitbox,
}

impl IntoElement for SelectableMessage {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableMessage {
    type RequestLayoutState = ();
    type PrepaintState = MessagePrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.body.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.runs.borrow_mut().clear();
        self.body.prepaint(window, cx);
        if self
            .previous_visible_text
            .as_ref()
            .is_some_and(|text| text != &self.content)
            && self.handle.snapshot(cx).is_some()
        {
            self.copy.invalidate_selection();
            TextSelection::clear(window, cx);
        }
        self.copy.set_visible_text(self.content.clone());
        let mut runs = self.runs.borrow().clone();
        runs.sort_by_key(|run| run.order);
        let text_runs = runs
            .iter()
            .map(|run| {
                TextSelectionRun::new(run.text.clone(), run.layout.clone(), run.bounds)
                    .with_document_order(run.order)
            })
            .collect::<Vec<_>>();
        let projection = self.handle.update_runs(&text_runs, cx);
        *self.projected_ranges.borrow_mut() = projection.ranges().to_vec();
        let selected = projection
            .ranges()
            .iter()
            .zip(&runs)
            .filter_map(|(range, run)| {
                range.as_ref().map(|range| {
                    run.content_range.start + range.start..run.content_range.start + range.end
                })
            })
            .fold(None::<Range<usize>>, |combined, range| {
                Some(match combined {
                    Some(current) => current.start.min(range.start)..current.end.max(range.end),
                    None => range,
                })
            });
        let selection_text = selected
            .and_then(|range| self.content.get(range))
            .unwrap_or_default()
            .to_owned();
        self.copy.set_selected_text(self.generation, selection_text);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.handle.register(
            TextSelectionRegistration::new(hitbox.clone(), bounds)
                .with_scope(self.scope)
                .with_text_bounds(runs.iter().map(|run| run.bounds).collect()),
            window,
            cx,
        );
        MessagePrepaint { hitbox }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scope = self.scope;
        let hitbox = prepaint.hitbox.clone();
        window.on_mouse_event(move |event: &gpui_kit::MouseDownEvent, phase, window, cx| {
            if phase.bubble()
                && event.button == gpui_kit::MouseButton::Left
                && hitbox.is_hovered(window)
            {
                TextSelection::activate_scope(scope, window, cx);
            }
        });
        self.body.paint(window, cx);
    }
}

fn paint_selection(layout: &TextLayout, range: Range<usize>, color: Hsla, window: &mut Window) {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return;
    };
    for bounds in selection_quad_bounds(start, end, layout.bounds(), layout.line_height()) {
        window.paint_quad(PaintQuad {
            bounds,
            background: color.into(),
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
            border_color: transparent_black(),
            border_style: BorderStyle::default(),
        });
    }
}

fn selection_quad_bounds(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        )];
    }
    let mut quads = vec![Bounds::from_corners(
        start,
        Point::new(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        quads.push(Bounds::from_corners(
            Point::new(bounds.left(), start.y + line_height),
            Point::new(bounds.right(), end.y),
        ));
    }
    quads.push(Bounds::from_corners(
        Point::new(bounds.left(), end.y),
        Point::new(end.x, end.y + line_height),
    ));
    quads
}
