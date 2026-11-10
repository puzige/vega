use std::sync::Arc;

use gpui_kit::{prelude::*, *};
use vega_theme::{Layout, ThemeColors};

use super::ConversationStream;
#[cfg(test)]
use super::model::MessageCopy;
use crate::interaction::{Interaction, InteractionExt};

mod native;

#[derive(Clone)]
pub(super) enum CopySource {
    #[cfg(test)]
    Message(MessageCopy),
    Code(Arc<str>),
}

impl CopySource {
    fn generation(&self) -> u64 {
        match self {
            #[cfg(test)]
            Self::Message(copy) => copy.content_generation(),
            Self::Code(_) => 0,
        }
    }

    fn same_source(&self, other: &Self) -> bool {
        match (self, other) {
            #[cfg(test)]
            (Self::Message(left), Self::Message(right)) => left.id == right.id,
            (Self::Code(left), Self::Code(right)) => Arc::ptr_eq(left, right),
            #[cfg(test)]
            _ => false,
        }
    }

    fn text(&self) -> String {
        match self {
            #[cfg(test)]
            Self::Message(copy) => copy.display_text(),
            Self::Code(code) => code.to_string(),
        }
    }
}

type CopyOwner = (WeakEntity<ConversationStream>, u64, Option<(u64, usize)>);

pub(super) struct CopyReceiptButton {
    id: SharedString,
    source: CopySource,
    label: &'static str,
    colors: ThemeColors,
    style: Option<Stateful<Interaction>>,
    owner: Option<CopyOwner>,
    body: Option<AnyElement>,
    route: native::Route,
}

impl CopyReceiptButton {
    pub(super) fn new(
        id: SharedString,
        source: CopySource,
        label: &'static str,
        colors: ThemeColors,
        style: impl FnOnce(Interaction) -> Stateful<Interaction>,
    ) -> Self {
        let transparent = Rgba::from(transparent_black());
        let bridge = div()
            .id(id.clone())
            .size(px(Layout::COPY_ACTION_SIZE))
            .flex_shrink_0()
            .interaction_surface(transparent, transparent, transparent, true);
        Self {
            id,
            source,
            label,
            colors,
            style: Some(style(bridge)),
            owner: None,
            body: None,
            route: native::Route::default(),
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
        Some(self.id.clone().into())
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
        if self.body.is_none() {
            let bridge = self.style.take().map(|bridge| {
                div()
                    .opacity(0.)
                    .child(self.route.bridge(bridge))
                    .into_any_element()
            });
            self.body = Some(
                div()
                    .relative()
                    .flex_shrink_0()
                    .child(div().absolute().inset_0().child(native::Native::new(
                        self.id.clone(),
                        self.source.clone(),
                        self.label,
                        self.colors,
                        self.owner.clone(),
                        self.route.clone(),
                        cx,
                    )))
                    .children(bridge)
                    .into_any_element(),
            );
        }
        match &mut self.body {
            Some(body) => (body.request_layout(window, cx), ()),
            None => (window.request_layout(Style::default(), [], cx), ()),
        }
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(body) = &mut self.body {
            body.prepaint(window, cx);
        }
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
        if let Some(body) = &mut self.body {
            body.paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CopySource, MessageCopy};
    use std::sync::Arc;

    #[test]
    fn message_identity_survives_cloning_but_not_equal_text_replacement() {
        let source = CopySource::Message(MessageCopy::new("same text"));
        let retained = source.clone();
        let replacement = CopySource::Message(MessageCopy::new("same text"));
        let code = CopySource::Code(Arc::from("same text"));

        assert!(source.same_source(&retained));
        assert_eq!(source.text(), replacement.text());
        assert!(!source.same_source(&replacement));
        assert!(!replacement.same_source(&source));
        assert!(!source.same_source(&code));
        assert!(!code.same_source(&source));
    }

    #[test]
    fn code_identity_requires_the_original_allocation() {
        let original: Arc<str> = Arc::from("same code");
        let source = CopySource::Code(original.clone());
        let retained = CopySource::Code(original);
        let replacement = CopySource::Code(Arc::from("same code"));

        assert!(source.same_source(&retained));
        assert_eq!(source.text(), replacement.text());
        assert!(!source.same_source(&replacement));
        assert!(!replacement.same_source(&source));
    }

    #[test]
    fn restoring_message_text_does_not_restore_a_queued_generation() {
        let copy = MessageCopy::new("first");
        let source = CopySource::Message(copy.clone());
        let retained = source.clone();
        let queued_generation = source.generation();

        copy.replace_text("second");
        let changed_generation = source.generation();
        assert_ne!(changed_generation, queued_generation);
        assert_eq!(source.text(), "second");
        assert_eq!(retained.generation(), changed_generation);

        copy.replace_text("first");
        assert_eq!(source.text(), "first");
        assert!(source.same_source(&retained));
        assert_ne!(source.generation(), queued_generation);
        assert_ne!(source.generation(), changed_generation);
        assert_eq!(retained.generation(), source.generation());
        assert_eq!(retained.text(), "first");
    }
}
