use std::{cell::RefCell, ops::Range, rc::Rc, sync::Arc};

use gpui_kit::base::{
    TextSelection, TextSelectionContentKey, TextSelectionCoverage, TextSelectionHandle,
    TextSelectionRegistration, TextSelectionRun, TextSelectionScopeId,
};
use gpui_kit::{
    App, BorderStyle, Bounds, Corners, CursorStyle, Edges, Element, ElementId, FocusHandle,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    PaintQuad, Pixels, Point, SharedString, StyledText, TextLayout, TextRun, Window, WrappedLine,
    transparent_black,
};

use super::code_drag::CodeDragSelection;
use super::links::{LinkRange, fragment_link_ranges, register_link_interactions};
use super::model::MessageCopy;

#[derive(Clone)]
struct LaidOutRun {
    text: SharedString,
    layout: TextLayout,
    bounds: Bounds<Pixels>,
    order: (u64, usize),
    content_range: Range<usize>,
    logical_range: Range<usize>,
    selected: Rc<RefCell<Option<Range<usize>>>>,
}

#[derive(Clone)]
pub(crate) struct SelectionDocumentBuilder {
    copy: MessageCopy,
    focus: FocusHandle,
    content: String,
    projected_content: Option<Arc<str>>,
    projected_identity: u64,
    next_run_order: u64,
    runs: Rc<RefCell<Vec<LaidOutRun>>>,
    selection_color: Hsla,
    punctuation_rules: bool,
}

impl SelectionDocumentBuilder {
    pub(crate) fn new(copy: MessageCopy, focus: FocusHandle, selection_color: Hsla) -> Self {
        Self {
            copy,
            focus,
            content: String::new(),
            projected_content: None,
            projected_identity: 0,
            next_run_order: 0,
            runs: Rc::new(RefCell::new(Vec::new())),
            selection_color,
            punctuation_rules: false,
        }
    }

    pub(super) fn with_punctuation_rules(mut self) -> Self {
        self.punctuation_rules = true;
        self
    }

    pub(super) fn projected(
        copy: MessageCopy,
        focus: FocusHandle,
        selection_color: Hsla,
        content: Arc<str>,
        identity: u64,
    ) -> Self {
        Self {
            projected_content: Some(content),
            projected_identity: identity,
            ..Self::new(copy, focus, selection_color)
        }
    }

    pub(super) fn append_positioned(
        &self,
        text: &str,
        runs: Vec<TextRun>,
        links: Vec<LinkRange>,
        content_range: Range<usize>,
        logical_range: Range<usize>,
    ) -> gpui_kit::AnyElement {
        let paint_runs = runs
            .iter()
            .any(|run| run.background_color.is_some())
            .then(|| runs.clone());
        SelectableStyledText {
            id: ElementId::Name(
                format!(
                    "thinking-{}-selection-{}",
                    self.copy.id, content_range.start
                )
                .into(),
            ),
            text: StyledText::new(text.to_owned()).with_runs(runs),
            content: text.to_owned().into(),
            order: (content_range.start as u64, 0),
            content_range,
            logical_range,
            runs: self.runs.clone(),
            links,
            selected: Rc::new(RefCell::new(None)),
            selection_color: self.selection_color,
            paint_runs,
            assistant_inline: false,
        }
        .into_any_element()
    }

    pub(super) fn code_drag_selection(&self, window: &Window, cx: &mut App) -> CodeDragSelection {
        let runs = self.runs.clone();
        CodeDragSelection::new(
            self.copy.clone(),
            self.focus.clone(),
            self.copy.ensure_selection_handle(&self.focus, window, cx),
            move |position| {
                runs.borrow().iter().find_map(|run| {
                    if !run.bounds.contains(&position) {
                        return None;
                    }
                    let index = run.layout.index_for_position(position).ok()?;
                    (index < run.content_range.len())
                        .then_some((run.content_range.start + index) as u64)
                })
            },
        )
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
        self.append(text, styled, None, Vec::new(), id_prefix)
    }

    pub(super) fn append_code_runs(
        &mut self,
        text: &str,
        runs: Vec<TextRun>,
        links: Vec<LinkRange>,
        id_prefix: &str,
    ) -> gpui_kit::AnyElement {
        let assistant = self.punctuation_rules;
        self.punctuation_rules = false;
        let element = self.append_styled_runs(text, runs, links, id_prefix);
        self.punctuation_rules = assistant;
        element
    }

    pub(crate) fn append_styled_runs(
        &mut self,
        text: &str,
        runs: Vec<TextRun>,
        links: Vec<LinkRange>,
        id_prefix: &str,
    ) -> gpui_kit::AnyElement {
        let styled = StyledText::new(text.to_owned()).with_runs(runs.clone());
        let paint_runs = (self.punctuation_rules
            || runs.iter().any(|run| run.background_color.is_some())
            || !links.is_empty())
        .then_some(runs);
        self.append(text, styled, paint_runs, links, id_prefix)
    }

    fn append(
        &mut self,
        text: &str,
        styled: StyledText,
        paint_runs: Option<Vec<TextRun>>,
        links: Vec<LinkRange>,
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
        let runs = self.runs.clone();
        let selection_color = self.selection_color;
        let mut links = links;
        super::links::own_links(&mut links, &fragment_id_owner(id_prefix, order));
        let fragment_links = links.clone();
        let fragment_id = format!("{id_prefix}-selection-run-{order}");
        let assistant_inline = self.punctuation_rules;
        let fragment =
            move |range: Range<usize>, content: SharedString, paint_runs: Vec<TextRun>| {
                let styled = StyledText::new(content.clone()).with_runs(paint_runs.clone());
                let code = paint_runs
                    .first()
                    .is_some_and(|run| run.background_color.is_some());
                let mut links = fragment_link_ranges(&fragment_links, &range);
                for link in &mut links {
                    link.assistant_code_padding = assistant_inline && code && link.starts_here;
                }
                let paint_runs = paint_runs
                    .iter()
                    .any(|run| run.background_color.is_some())
                    .then_some(paint_runs);
                SelectableStyledText {
                    id: ElementId::Name(format!("{fragment_id}-fragment-{}", range.start).into()),
                    text: styled,
                    content,
                    content_range: start + range.start..start + range.end,
                    logical_range: start..end,
                    order: (order, range.start),
                    runs: runs.clone(),
                    links,
                    selected: Rc::new(RefCell::new(None)),
                    selection_color,
                    paint_runs,
                    assistant_inline,
                }
                .into_any_element()
            };
        if let Some(paint_runs) = paint_runs {
            super::inline_code::flow_with_punctuation(
                text.into(),
                paint_runs,
                links,
                self.punctuation_rules,
                fragment,
            )
        } else {
            SelectableStyledText {
                id: ElementId::Name(format!("{id_prefix}-selection-run-{order}").into()),
                text: styled,
                content: text.into(),
                content_range: start..end,
                logical_range: start..end,
                order: (order, 0),
                runs: self.runs.clone(),
                links: fragment_link_ranges(&links, &(0..text.len())),
                selected: Rc::new(RefCell::new(None)),
                selection_color,
                paint_runs: None,
                assistant_inline: false,
            }
            .into_any_element()
        }
    }

    pub(crate) fn wrap(
        self,
        body: gpui_kit::AnyElement,
        window: &Window,
        cx: &mut App,
    ) -> gpui_kit::AnyElement {
        let id = self.copy.id;
        let generation = self.copy.selection_generation();
        let projected = self.projected_content.is_some();
        let clear_projected_selection = self
            .projected_content
            .as_ref()
            .is_some_and(|_| self.copy.set_projected_identity(self.projected_identity));
        let previous = (!projected).then(|| self.copy.visible_text()).flatten();
        SelectableMessage {
            id: ElementId::Name(format!("selectable-message-{id}").into()),
            body,
            copy: self.copy.clone(),
            handle: self.copy.ensure_selection_handle(&self.focus, window, cx),
            scope: self.copy.selection_scope(),
            focus: self.focus,
            content: self
                .projected_content
                .unwrap_or_else(|| self.content.into()),
            projected,
            clear_projected_selection,
            generation,
            previous_visible_text: previous,
            runs: self.runs,
        }
        .into_any_element()
    }
}

struct SelectableStyledText {
    id: ElementId,
    text: StyledText,
    content: SharedString,
    content_range: Range<usize>,
    logical_range: Range<usize>,
    order: (u64, usize),
    runs: Rc<RefCell<Vec<LaidOutRun>>>,
    links: Vec<LinkRange>,
    selected: Rc<RefCell<Option<Range<usize>>>>,
    selection_color: Hsla,
    paint_runs: Option<Vec<TextRun>>,
    assistant_inline: bool,
}

struct SelectablePrepaintState {
    lines: Option<Vec<WrappedLine>>,
    hitbox: Hitbox,
    targets: Vec<gpui_kit::AnyElement>,
}

impl IntoElement for SelectableStyledText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableStyledText {
    type RequestLayoutState = ();
    type PrepaintState = SelectablePrepaintState;

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
            logical_range: self.logical_range.clone(),
            selected: self.selected.clone(),
        });
        let lines = self.paint_runs.as_ref().and_then(|runs| {
            super::inline_code::shape(self.content.clone(), runs, self.text.layout(), window)
        });
        let hitbox = window.insert_hitbox(
            crate::link_preview::interactive_bounds(bounds, &self.links),
            HitboxBehavior::Normal,
        );
        let targets = crate::link_preview::prepaint_link_targets(
            id,
            self.text.layout(),
            &self.links,
            window,
            cx,
        );
        SelectablePrepaintState {
            lines,
            hitbox,
            targets,
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if prepaint.lines.is_some()
            && let Some(runs) = &self.paint_runs
        {
            super::inline_code::paint_inline_backgrounds(
                self.text.layout(),
                runs,
                self.assistant_inline,
                window,
            );
        }
        if let Some(range) = self.selected.borrow().as_ref() {
            paint_selection(
                self.text.layout(),
                range.clone(),
                self.selection_color,
                window,
            );
        }
        if let Some(lines) = prepaint.lines.as_ref() {
            super::inline_code::paint_text(lines, self.text.layout(), window, cx);
        } else {
            self.text
                .paint(id, inspector_id, bounds, &mut (), &mut (), window, cx);
        }
        for target in &mut prepaint.targets {
            target.paint(window, cx);
        }
        window.set_cursor_style(CursorStyle::IBeam, &prepaint.hitbox);
        register_link_interactions(
            id,
            prepaint.hitbox.clone(),
            self.text.layout().clone(),
            self.links.clone(),
            window,
            cx,
        );
    }
}

struct SelectableMessage {
    id: ElementId,
    body: gpui_kit::AnyElement,
    copy: MessageCopy,
    handle: TextSelectionHandle,
    scope: TextSelectionScopeId,
    focus: FocusHandle,
    content: Arc<str>,
    projected: bool,
    clear_projected_selection: bool,
    generation: u64,
    previous_visible_text: Option<String>,
    runs: Rc<RefCell<Vec<LaidOutRun>>>,
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
        if (self.clear_projected_selection
            || self
                .previous_visible_text
                .as_ref()
                .is_some_and(|text| text.as_str() != self.content.as_ref()))
            && (self.handle.snapshot(cx).is_some() || self.handle.has_local_selection(cx))
        {
            self.copy.invalidate_selection();
            TextSelection::clear(window, cx);
            self.generation = self.copy.selection_generation();
        }
        self.copy.set_visible_text(self.content.clone());
        let mut runs = self.runs.borrow().clone();
        runs.sort_by_key(|run| run.order);
        let text_runs = runs
            .iter()
            .enumerate()
            .map(|(order, run)| {
                TextSelectionRun::new(run.text.clone(), run.layout.clone(), run.bounds)
                    .with_document_order(order as u64)
            })
            .collect::<Vec<_>>();
        let projection = self.handle.update_runs(&text_runs, cx);
        {
            let resolver_runs = self.runs.clone();
            self.handle.resolve_content_key_with(
                move |point, _| {
                    let position = point + bounds.origin;
                    resolver_runs
                        .borrow()
                        .iter()
                        .min_by(|left, right| {
                            let distance = |run: &LaidOutRun| {
                                let y = if position.y < run.bounds.top() {
                                    run.bounds.top() - position.y
                                } else if position.y > run.bounds.bottom() {
                                    position.y - run.bounds.bottom()
                                } else {
                                    gpui_kit::px(0.)
                                };
                                let x = if position.x < run.bounds.left() {
                                    run.bounds.left() - position.x
                                } else if position.x > run.bounds.right() {
                                    position.x - run.bounds.right()
                                } else {
                                    gpui_kit::px(0.)
                                };
                                (f32::from(y), f32::from(x))
                            };
                            distance(left)
                                .partial_cmp(&distance(right))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|run| {
                            TextSelectionContentKey::new(
                                (run.content_range.start
                                    + run
                                        .layout
                                        .index_for_position(position)
                                        .unwrap_or_else(|index| index)
                                        .min(run.content_range.len()))
                                    as u64,
                            )
                        })
                },
                cx,
            );
        }
        let logical_range = self.handle.snapshot(cx).and_then(|snapshot| {
            let index = |endpoint: gpui_kit::base::TextSelectionEndpoint| {
                (endpoint.entity_id() == Some(self.handle.entity_id()))
                    .then(|| endpoint.content_key().map(|key| key.value() as usize))
                    .flatten()
            };
            let anchor = index(snapshot.anchor());
            let cursor = index(snapshot.cursor());
            match snapshot.coverage() {
                TextSelectionCoverage::Full => Some(0..self.content.len()),
                TextSelectionCoverage::FromStart => Some(0..anchor.or(cursor)?),
                TextSelectionCoverage::ToEnd => Some(anchor.or(cursor)?..self.content.len()),
                TextSelectionCoverage::Bounded => Some(anchor?.min(cursor?)..anchor?.max(cursor?)),
            }
        });
        let local = self.copy.local_selection_range();
        let visible_selection = local.clone().or(logical_range);
        self.handle.set_local_selection(local.is_some(), cx);
        for (run, range) in runs.iter().zip(projection.ranges()) {
            *run.selected.borrow_mut() = if let Some(local) = &visible_selection {
                let start = local.start.max(run.content_range.start);
                let end = local.end.min(run.content_range.end);
                (start < end).then_some(
                    start - run.content_range.start..end.saturating_sub(run.content_range.start),
                )
            } else {
                range.clone()
            };
        }
        let selected = visible_selection.or_else(|| {
            projection
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
                })
        });
        if self.projected {
            self.copy.set_projected_selection(selected.clone());
        }
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
        let runs = self.runs.clone();
        let content = self.content.clone();
        let projected = self.projected;
        let generation = self.copy.content_generation();
        let copy = self.copy.clone();
        let handle = self.handle.clone();
        let focus = self.focus.clone();
        window.on_mouse_event(move |event: &gpui_kit::MouseDownEvent, phase, window, cx| {
            if !phase.bubble()
                || event.button != gpui_kit::MouseButton::Left
                || !hitbox.is_hovered_at(event.position, window)
            {
                return;
            }
            TextSelection::activate_scope(scope, window, cx);
            if event.click_count >= 2 {
                let selected = runs.borrow().iter().find_map(|run| {
                    if !run.bounds.contains(&event.position)
                        || run.content_range == run.logical_range
                    {
                        return None;
                    }
                    let offset = run.layout.index_for_position(event.position).ok()?
                        + run.content_range.start;
                    let text = content.get(run.logical_range.clone())?;
                    let range = multi_click_range(
                        text,
                        offset - run.logical_range.start,
                        event.click_count,
                    )?;
                    Some(run.logical_range.start + range.start..run.logical_range.start + range.end)
                });
                if let Some(range) = selected {
                    let copy = copy.clone();
                    let handle = handle.clone();
                    let focus = focus.clone();
                    let content = content.clone();
                    window.defer(cx, move |window, cx| {
                        if (projected && copy.content_generation() == generation)
                            || (!projected
                                && copy.visible_text().as_deref() == Some(content.as_ref()))
                        {
                            copy.set_local_selection_range(range.clone());
                            copy.set_selected_text(
                                copy.selection_generation(),
                                content[range].to_owned(),
                            );
                            handle.set_local_selection(true, cx);
                            focus.focus(window, cx);
                            window.refresh();
                        }
                    });
                    cx.stop_propagation();
                }
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectionCharacter {
    Word,
    Whitespace,
    Other,
}

fn selection_character(character: char) -> SelectionCharacter {
    if character == '_'
        || character.is_ascii_alphanumeric()
        || matches!(character, '\u{00C0}'..='\u{024F}' | '\u{0400}'..='\u{04FF}' | '\u{1E00}'..='\u{1EFF}' | '\u{0300}'..='\u{036F}')
    {
        SelectionCharacter::Word
    } else if character.is_whitespace() && !matches!(character, '\n' | '\r') {
        SelectionCharacter::Whitespace
    } else {
        SelectionCharacter::Other
    }
}

fn multi_click_range(text: &str, offset: usize, count: usize) -> Option<Range<usize>> {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    if count >= 3 {
        let start = text[..offset].rfind('\n').map_or(0, |newline| newline + 1);
        let end = text[offset..]
            .find('\n')
            .map_or(text.len(), |newline| offset + newline);
        return Some(start..end);
    }
    let character = text[offset..].chars().next()?;
    let kind = selection_character(character);
    let connects = |next| kind != SelectionCharacter::Other && selection_character(next) == kind;
    let start = text[..offset]
        .chars()
        .rev()
        .take(128)
        .take_while(|character| connects(*character))
        .fold(offset, |start, character| start - character.len_utf8());
    let end = text[offset + character.len_utf8()..]
        .chars()
        .take(128)
        .take_while(|character| connects(*character))
        .fold(offset + character.len_utf8(), |end, character| {
            end + character.len_utf8()
        });
    Some(start..end)
}

fn fragment_id_owner(prefix: &str, order: u64) -> String {
    format!("{prefix}-{order}")
}
