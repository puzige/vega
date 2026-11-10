use super::*;
use std::{future::Future, pin::Pin, sync::Arc};

pub(crate) type PreviewResult = Result<LinkPreviewData, LinkPreviewFailure>;
pub(crate) type Request = Arc<
    dyn Fn(String, bool, CancellationToken) -> Pin<Box<dyn Future<Output = PreviewResult> + Send>>
        + Send
        + Sync,
>;

pub(crate) fn install_request(cx: &mut App, request: Request) {
    registry(cx).request = Some(request);
}

pub(crate) struct Observation {
    pub(crate) visible: Vec<ObservedLink>,
    pub(crate) active: Option<ObservedActive>,
}

pub(crate) struct ObservedLink {
    pub(crate) target: String,
    pub(crate) current: bool,
    pub(crate) text_bounds: Vec<Bounds<Pixels>>,
}

pub(crate) struct ObservedActive {
    pub(crate) target: String,
    pub(crate) current: bool,
    pub(crate) data: Option<PreviewResult>,
    pub(crate) open: FocusHandle,
    pub(crate) retry: FocusHandle,
    pub(crate) card_bounds: Option<Bounds<Pixels>>,
}

pub(crate) fn observe(window: &Window, cx: &App) -> Option<Observation> {
    let entity = coordinator(window, cx)?;
    let state = entity.read(cx);
    Some(Observation {
        visible: state
            .visible
            .iter()
            .map(|link| ObservedLink {
                target: link.target.clone(),
                current: link.current(window, cx),
                text_bounds: link.text_bounds.clone(),
            })
            .collect(),
        active: state.active.as_ref().map(|active| ObservedActive {
            target: active.target.clone(),
            current: state.active_current(window, cx),
            data: active.data.clone(),
            open: active.open.clone(),
            retry: active.retry.clone(),
            card_bounds: state.card_bounds,
        }),
    })
}
