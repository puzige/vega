use super::{ConversationStream, Lease};
use gpui_kit::*;
use std::rc::Rc;

type MountedListFrame = (
    WindowId,
    WeakEntity<ConversationStream>,
    Bounds<Pixels>,
    Rc<()>,
);

#[derive(Default)]
struct ListFrames(Vec<MountedListFrame>);
impl Global for ListFrames {}

pub(in crate::conversation_stream) fn list_bounds(
    stream: &WeakEntity<ConversationStream>,
    window: &Window,
    cx: &App,
) -> Option<Bounds<Pixels>> {
    list_frame(stream, window, cx).map(|(bounds, _)| bounds)
}

fn list_frame(
    stream: &WeakEntity<ConversationStream>,
    window: &Window,
    cx: &App,
) -> Option<(Bounds<Pixels>, Rc<()>)> {
    cx.try_global::<ListFrames>()?
        .0
        .iter()
        .rev()
        .find_map(|(id, owner, bounds, frame)| {
            (*id == window.window_handle().window_id() && owner == stream)
                .then(|| (*bounds, frame.clone()))
        })
}

pub(super) fn lease_current(
    lease: &Lease,
    stream: &WeakEntity<ConversationStream>,
    window: &Window,
    cx: &App,
) -> bool {
    list_frame(stream, window, cx).is_none_or(|(_, frame)| {
        lease
            .frame
            .borrow()
            .as_ref()
            .is_some_and(|mounted| Rc::ptr_eq(mounted, &frame))
    })
}

pub(crate) struct ReadingList {
    child: AnyElement,
    streams: Vec<WeakEntity<ConversationStream>>,
}

impl ReadingList {
    pub(crate) fn new(stream: WeakEntity<ConversationStream>, child: impl IntoElement) -> Self {
        Self {
            child: child.into_any_element(),
            streams: vec![stream],
        }
    }

    #[cfg(feature = "nsh-reading-integration")]
    pub(crate) fn with_owners(
        streams: Vec<WeakEntity<ConversationStream>>,
        child: impl IntoElement,
    ) -> Self {
        Self {
            child: child.into_any_element(),
            streams,
        }
    }
}

impl IntoElement for ReadingList {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ReadingList {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
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
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let frames = &mut cx.default_global::<ListFrames>().0;
        let previous = frames.len();
        let frame = Rc::new(());
        for stream in &self.streams {
            frames.push((
                window.window_handle().window_id(),
                stream.clone(),
                bounds,
                frame.clone(),
            ));
        }
        self.child.prepaint(window, cx);
        cx.global_mut::<ListFrames>().0.truncate(previous);
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
        self.child.paint(window, cx);
    }
}

pub(super) struct Mounted {
    pub(super) child: AnyElement,
    pub(super) lease: Rc<Lease>,
    pub(super) id: ElementId,
}

impl IntoElement for Mounted {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Mounted {
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
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
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
        self.lease.live.set(false);
        self.lease.geometry.set(None);
        *self.lease.frame.borrow_mut() = cx
            .try_global::<ListFrames>()
            .and_then(|frames| frames.0.last())
            .map(|(_, _, _, frame)| frame.clone());
        if let Some(id) = id {
            window.with_element_state::<Rc<Lease>, _>(id, |_, _| ((), self.lease.clone()));
            self.lease
                .live
                .set(!bounds.intersect(&window.content_mask().bounds).is_empty());
        }
        self.child.prepaint(window, cx);
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
        self.child.paint(window, cx);
    }
}
