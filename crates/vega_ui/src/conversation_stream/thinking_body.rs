use super::super::selection_menu::body::{Content, render_fading as render_native_body};
use super::*;
use gpui_kit::{
    Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels,
    WeakEntity,
};
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
pub(super) struct ThinkingBody {
    stream: MarkdownStream,
    model: StreamModel,
    source: String,
    source_start: usize,
    prefix: String,
    content: Content,
    revision: Option<u64>,
    fade: Rc<std::cell::RefCell<super::super::stream_fade::StreamFade>>,
    pub(super) painted: Rc<Cell<bool>>,
    measured_height: Rc<Cell<f32>>,
}

impl ThinkingBody {
    pub(super) fn replace_source(&mut self) {
        self.stream = MarkdownStream::new();
        self.model = StreamModel::default();
        self.source.clear();
        self.prefix.clear();
        self.content = Content::default();
        self.revision = None;
        self.fade = Default::default();
    }

    pub(super) fn evict(&mut self) {
        self.replace_source();
        self.painted.set(false);
    }

    pub(super) fn visibility_changed(&mut self) {
        self.fade.borrow_mut().hide();
        self.painted.set(false);
    }

    pub(super) fn height(&self) -> f32 {
        self.measured_height
            .get()
            .max(Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT)
    }
}

impl ThinkingBlock {
    pub(super) fn prepare_body(&mut self, cx: &mut Context<Self>) {
        if self.body.revision == Some(self.source_revision) {
            return;
        }
        let source = self.visible_text().to_owned();
        let prefix = self.visible_window().prefix();
        let source_start = self.visible_window().start;
        let append = self.body.source_start == source_start
            && self.body.prefix == prefix
            && source.starts_with(&self.body.source);
        if append {
            self.body.stream.append(&source[self.body.source.len()..]);
        } else {
            self.body.stream = MarkdownStream::new();
            self.body.model = StreamModel::default();
            self.body.fade = Default::default();
            self.body.stream.append(&prefix);
            self.body.stream.append(&source);
        }
        self.body
            .model
            .sync(&self.body.stream.snapshot(), &StreamCounters::default());
        self.body.fade.borrow_mut().sync(&self.body.model);
        if self.progress_finished {
            self.body.fade.borrow_mut().finish();
        }
        if !self.expanded || !self.ancestor_visible {
            self.body.fade.borrow_mut().hide();
        }
        self.body.content = Content::from_model(&self.body.model, self.source_copy.id)
            .with_source(format!("{prefix}{source}"), prefix.len(), source_start)
            .with_source_domain(self.source_identity);
        self.body.source = source;
        self.body.source_start = source_start;
        self.body.prefix = prefix;
        self.body.revision = Some(self.source_revision);
        if self.expanded && self.ancestor_visible {
            cx.notify();
        }
    }

    pub(super) fn slide_body(&mut self) {
        self.source_copy.invalidate_selection();
        self.body.revision = None;
    }

    pub(crate) fn reset_body_visibility(&self) {
        self.body.fade.borrow_mut().hide();
        self.body.painted.set(false);
        if let Some(page) = &self.presentation {
            page.unmount();
        }
    }

    pub(super) fn render_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.prepare_body(cx);
        if self.progress_finished {
            self.body.fade.borrow_mut().finish();
        }
        let content = render_native_body(
            self.body.content.clone(),
            &self.source_copy,
            true,
            theme(cx).colors,
            Some(self.body.fade.clone()),
            window,
            cx,
        );
        let content = render_rows::selection_context_menu(
            content,
            &self.source_copy,
            true,
            self.ancestor_visible,
            None,
        );
        Presence {
            content,
            painted: self.body.painted.clone(),
            height: self.body.measured_height.clone(),
            card: cx.weak_entity(),
        }
        .into_any_element()
    }
}

struct PresenceLease(Rc<Cell<bool>>);

impl Drop for PresenceLease {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

struct Presence {
    content: AnyElement,
    painted: Rc<Cell<bool>>,
    height: Rc<Cell<f32>>,
    card: WeakEntity<ThinkingBlock>,
}

impl IntoElement for Presence {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Presence {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some("thinking-nsh-body-presence".into())
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
    ) -> (LayoutId, ()) {
        (self.content.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.painted
            .set(!bounds.intersect(&window.content_mask().bounds).is_empty());
        if let Some(id) = id {
            window.with_element_state(id, |lease: Option<PresenceLease>, _| {
                (
                    (),
                    lease.unwrap_or_else(|| PresenceLease(self.painted.clone())),
                )
            });
        }
        let height = f32::from(bounds.size.height);
        let previous_height = self.height.replace(height);
        if (previous_height - height).abs() > 0.5 {
            #[cfg(feature = "nsh-reading-integration")]
            eprintln!(
                "[nsh-height] card={:?} old={previous_height} new={height}",
                self.card.entity_id()
            );
            let card = self.card.clone();
            window.defer(cx, move |_, cx| {
                let _ = card.update(cx, |_, cx| {
                    cx.emit(ThinkingChanged {
                        header: false,
                        stage: false,
                        geometry: true,
                    });
                });
            });
        }
        self.content.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.paint(window, cx);
    }
}
