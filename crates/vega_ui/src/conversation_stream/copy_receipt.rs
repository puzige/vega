use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
    time::{Duration, Instant},
};

use gpui_kit::{
    AnyElement, App, Bounds, Div, Element, ElementId, GlobalElementId, Hitbox, InspectorElementId,
    InteractiveElement, IntoElement, LayoutId, Pixels, SharedString, Task, Window, div, prelude::*,
    px,
};
use vega_theme::{Layout, Motion, ThemeColors};

use super::model::MessageCopy;
use crate::{icons::Icon, interaction::Interaction};

#[derive(Clone)]
pub(super) enum CopySource {
    Message(MessageCopy),
    Code(Arc<str>),
}

impl CopySource {
    fn generation(&self) -> u64 {
        match self {
            Self::Message(copy) => copy.content_generation(),
            Self::Code(_) => 0,
        }
    }

    fn same_source(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Message(left), Self::Message(right)) => left.id == right.id,
            (Self::Code(left), Self::Code(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }

    fn copy(&self, cx: &mut App) -> bool {
        match self {
            Self::Message(copy) => {
                if !copy.has_text() {
                    return false;
                }
                copy.copy(cx);
            }
            Self::Code(source) => {
                if source.is_empty() {
                    return false;
                }
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(source.to_string()));
            }
        }
        true
    }
}

struct Receipt {
    source: CopySource,
    generation: u64,
    deadline: Option<Instant>,
    timer: Option<Task<()>>,
    mounted: bool,
}

impl Receipt {
    fn clear(&mut self) {
        self.deadline = None;
        self.timer = None;
    }

    fn active(&self, now: Instant) -> bool {
        self.mounted
            && self.generation == self.source.generation()
            && self.deadline.is_some_and(|deadline| now < deadline)
    }
}

struct MountLease(Rc<RefCell<Receipt>>);

impl Drop for MountLease {
    fn drop(&mut self) {
        let mut receipt = self.0.borrow_mut();
        receipt.mounted = false;
        receipt.clear();
    }
}

fn activate(weak: &Weak<RefCell<Receipt>>, window: &mut Window, cx: &mut App) {
    let Some(owner) = weak.upgrade() else {
        return;
    };
    let mut receipt = owner.borrow_mut();
    if !receipt.mounted || !receipt.source.copy(cx) {
        receipt.clear();
        window.refresh();
        return;
    }
    receipt.generation = receipt.source.generation();
    receipt.deadline =
        Some(cx.background_executor().now() + Duration::from_millis(Motion::COPY_RECEIPT_MS));
    if receipt.timer.is_none() {
        let weak = weak.clone();
        receipt.timer = Some(window.spawn(cx, async move |cx| {
            loop {
                let remaining = cx.update(|window, cx| {
                    let owner = weak.upgrade()?;
                    let mut receipt = owner.borrow_mut();
                    if !receipt.active(cx.background_executor().now()) {
                        receipt.clear();
                        if receipt.mounted {
                            window.refresh();
                        }
                        return None;
                    }
                    receipt.deadline.map(|deadline| {
                        deadline.saturating_duration_since(cx.background_executor().now())
                    })
                });
                let Ok(Some(remaining)) = remaining else {
                    break;
                };
                cx.background_executor().timer(remaining).await;
                let expired = cx.update(|window, cx| {
                    let Some(owner) = weak.upgrade() else {
                        return true;
                    };
                    let mut receipt = owner.borrow_mut();
                    if receipt.active(cx.background_executor().now()) {
                        return false;
                    }
                    receipt.deadline = None;
                    receipt.timer = None;
                    if receipt.mounted {
                        window.refresh();
                    }
                    true
                });
                if expired.unwrap_or(true) {
                    break;
                }
            }
        }));
    }
    window.refresh();
}

pub(super) struct CopyReceiptButton {
    id: ElementId,
    source: CopySource,
    body: AnyElement,
    binding: Rc<RefCell<Weak<RefCell<Receipt>>>>,
    active: Rc<Cell<bool>>,
}

impl CopyReceiptButton {
    pub(super) fn new(
        id: SharedString,
        source: CopySource,
        label: &'static str,
        colors: ThemeColors,
        style: impl FnOnce(Interaction) -> Interaction,
    ) -> Self {
        let binding = Rc::new(RefCell::new(Weak::new()));
        let blend = Rc::new(Cell::new(0.));
        let active = Rc::new(Cell::new(false));
        let content = div()
            .size(px(Layout::ICON_SIZE))
            .flex_shrink_0()
            .relative()
            .child(GlyphLayer::new(Icon::Copy, colors, blend.clone(), false))
            .child(GlyphLayer::new(Icon::Check, colors, blend.clone(), true));
        let clicked = binding.clone();
        let button =
            crate::icons::icon_button_with_content(content, label, colors, move |_, window, cx| {
                activate(&clicked.borrow(), window, cx);
            });
        Self {
            id: id.into(),
            source,
            body: ReceiptSurface {
                button: style(button),
                binding: binding.clone(),
                blend,
                active: active.clone(),
            }
            .into_any_element(),
            binding,
            active,
        }
    }
}

impl IntoElement for CopyReceiptButton {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for CopyReceiptButton {
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
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.body.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if let Some(id) = id {
            let owner = window.with_element_state(id, |lease: Option<MountLease>, _| {
                let lease = lease.unwrap_or_else(|| {
                    MountLease(Rc::new(RefCell::new(Receipt {
                        source: self.source.clone(),
                        generation: self.source.generation(),
                        deadline: None,
                        timer: None,
                        mounted: true,
                    })))
                });
                (lease.0.clone(), lease)
            });
            let mut receipt = owner.borrow_mut();
            if !receipt.source.same_source(&self.source)
                || receipt.generation != self.source.generation()
            {
                receipt.clear();
            }
            receipt.source = self.source.clone();
            receipt.generation = self.source.generation();
            self.active
                .set(receipt.active(cx.background_executor().now()));
            *self.binding.borrow_mut() = Rc::downgrade(&owner);
        }
        self.body.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.body.paint(window, cx);
    }
}

struct ReceiptSurface {
    button: Interaction,
    binding: Rc<RefCell<Weak<RefCell<Receipt>>>>,
    blend: Rc<Cell<f32>>,
    active: Rc<Cell<bool>>,
}

impl IntoElement for ReceiptSurface {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ReceiptSurface {
    type RequestLayoutState = <Interaction as Element>::RequestLayoutState;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.button)
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn a11y_role(&self) -> Option<gpui_kit::Role> {
        self.button.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        self.button.write_a11y_info(node);
        if self.active.get() {
            node.set_description("已执行复制调用");
        }
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        self.button.a11y_synthetic_children(prepaint, builder);
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.button.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.button
            .prepaint(id, inspector, bounds, layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.active.set(
            self.binding
                .borrow()
                .upgrade()
                .is_some_and(|owner| owner.borrow().active(cx.background_executor().now())),
        );
        let style = self
            .button
            .interactivity()
            .compute_style(id, hitbox.as_ref(), window, cx);
        let visible = style.opacity.unwrap_or(1.) > 0.
            && style.visibility == gpui_kit::Visibility::Visible
            && style.display != gpui_kit::Display::None
            && !bounds.intersect(&window.content_mask().bounds).is_empty();
        let blend = crate::interaction::decorative_value(
            "copy-receipt-glyph".into(),
            if self.active.get() { 1. } else { 0. },
            if self.active.get() {
                Duration::from_millis(Motion::COPY_SWAP_MS)
            } else {
                Duration::ZERO
            },
            visible,
            window,
            cx,
        );
        self.blend.set(blend.value);
        self.button
            .paint(id, inspector, bounds, layout, hitbox, window, cx);
    }
}

struct GlyphLayer {
    body: Div,
    blend: Rc<Cell<f32>>,
    check: bool,
}

impl GlyphLayer {
    fn new(kind: Icon, colors: ThemeColors, blend: Rc<Cell<f32>>, check: bool) -> Self {
        Self {
            body: div()
                .absolute()
                .top_0()
                .left_0()
                .size(px(Layout::ICON_SIZE))
                .child(crate::icons::icon(kind, colors.text_secondary)),
            blend,
            check,
        }
    }
}

impl IntoElement for GlyphLayer {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for GlyphLayer {
    type RequestLayoutState = <Div as Element>::RequestLayoutState;
    type PrepaintState = <Div as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.body.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.body
            .prepaint(id, inspector, bounds, layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.body.style().opacity = Some(if self.check {
            self.blend.get()
        } else {
            1. - self.blend.get()
        });
        self.body
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }
}
