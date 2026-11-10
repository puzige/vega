use super::*;
use crate::conversation_stream::{
    ConversationStream, StreamEntry, ThinkingBlock,
    scroll_composition_integration::{self, BodyKind, BodyMount, BodyObservation},
};
use nsh_platform_gpui::{AppView, reading::ReadingGeometry};
use vega_conversation::types::{
    ConversationEvent, PermissionMode, TaskBackend, Thread, ThreadMode, ThreadStatus,
};

pub(crate) struct NativeScrollRequest {
    pub surface: Entity<AppView>,
    pub area: u32,
    pub geometry: ReadingGeometry,
    pub kind: BodyKind,
    pub vertical_node: Option<u32>,
    pub viewport: Bounds<Pixels>,
}

pub(crate) struct NativeScrollObservation {
    pub node: Option<u32>,
    pub viewport: Bounds<Pixels>,
    pub offset: Point<Pixels>,
    pub maximum: Point<Pixels>,
}

#[derive(Clone, Copy)]
pub(crate) struct NativeScrollAccess {
    pub observe: fn(&NativeScrollRequest, &Window, &App) -> Option<NativeScrollObservation>,
    pub position: fn(
        &NativeScrollRequest,
        &NativeScrollObservation,
        Point<Pixels>,
        &mut Window,
        &mut App,
    ) -> bool,
}

impl Global for NativeScrollAccess {}

pub fn install(cx: &mut App) {
    cx.set_global(NativeScrollAccess {
        observe: |request, _, cx| {
            let observed = request.surface.read(cx).integration_reading_scroll(
                &request.geometry,
                request.area,
                request
                    .vertical_node
                    .filter(|_| !matches!(request.kind, BodyKind::Horizontal)),
                matches!(request.kind, BodyKind::Horizontal),
            )?;
            let viewport = observed.viewport.intersect(&request.viewport);
            (!viewport.is_empty()).then_some(NativeScrollObservation {
                node: observed.node,
                viewport,
                offset: observed.offset,
                maximum: observed.maximum,
            })
        },
        position: |request, observed, desired, _, cx| {
            request
                .surface
                .read(cx)
                .integration_position_reading_scroll(
                    &request.geometry,
                    request.area,
                    observed.node,
                    desired,
                )
        },
    });
}

struct Mount {
    stream: Entity<ConversationStream>,
    card: Entity<ThinkingBlock>,
    kind: BodyKind,
    access: NativeScrollAccess,
    body: RefCell<Option<Entity<BodyState>>>,
}

fn thread() -> Thread {
    Thread {
        id: "nested-reading-composition".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Reading".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::ReadOnly,
        permission_revision: 0,
        draft_authority_epoch: None,
        full_access_grant: None,
        default_permission: None,
        model: "fixture".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
        reasoning_preferences: None,
        last_response_failed: false,
    }
}

fn source(kind: BodyKind, slot: usize) -> String {
    match kind {
        BodyKind::NoTarget => format!("Reading body {slot}.\n\n"),
        BodyKind::Vertical => (0..80)
            .map(|row| format!("Body {slot} row {row}.\n\n"))
            .collect(),
        BodyKind::Horizontal => {
            format!("```text\n{}\n```\n\n", format!("body-{slot}-").repeat(160))
        }
    }
}

impl Mount {
    fn body(&self, cx: &App) -> Option<Entity<BodyState>> {
        if let Some(body) = self.body.borrow().as_ref() {
            return Some(body.clone());
        }
        self.card
            .read(cx)
            .integration_source_copy()
            .nsh_reading
            .borrow()
            .as_ref()?
            .state
            .upgrade()
    }

    fn request(&self, window: &Window, cx: &App) -> Option<NativeScrollRequest> {
        let body = self.body(cx)?;
        let state = body.read(cx);
        if !state.valid(window, cx) || !state.mounted_content_current(cx) {
            return None;
        }
        let host = state.lease.upgrade()?.geometry.get()?;
        let viewport = host.bounds.intersect(&host.clip).intersect(&host.list);
        if viewport.is_empty() {
            return None;
        }
        let surface = state.surface.as_ref()?.entity();
        let area = state.area?;
        let snapshot = surface.read(cx).reading_snapshot(area)?;
        let geometry = surface.read(cx).reading_geometry(&snapshot)?;
        Some(NativeScrollRequest {
            surface,
            area,
            geometry,
            kind: self.kind,
            vertical_node: state.scroll_node,
            viewport,
        })
    }

    fn sample(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<(NativeScrollRequest, NativeScrollObservation)> {
        let request = self.request(window, cx)?;
        let observed = (self.access.observe)(&request, window, cx)?;
        let values = [
            observed.viewport.origin.x,
            observed.viewport.origin.y,
            observed.viewport.size.width,
            observed.viewport.size.height,
            observed.offset.x,
            observed.offset.y,
            observed.maximum.x,
            observed.maximum.y,
        ];
        if values.iter().any(|value| !f32::from(*value).is_finite())
            || observed.viewport.is_empty()
            || observed.viewport.intersect(&request.viewport) != observed.viewport
            || observed.offset.x < px(0.)
            || observed.offset.y < px(0.)
            || observed.offset.x > observed.maximum.x
            || observed.offset.y > observed.maximum.y
            || !request
                .surface
                .read(cx)
                .reading_geometry_is_current(&request.geometry)
        {
            return None;
        }
        match self.kind {
            BodyKind::NoTarget if observed.node.is_some() => return None,
            BodyKind::Vertical
                if observed.node != request.vertical_node || observed.node.is_none() =>
            {
                return None;
            }
            BodyKind::Horizontal if observed.node.is_none() => return None,
            _ => {}
        }
        Some((request, observed))
    }

    fn render(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let groups = self
            .stream
            .read(cx)
            .entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::RunActivity { group }
                | StreamEntry::RunActivitySegment { group, .. } => Some(group.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        self.stream.update(cx, |stream, cx| {
            for group in &groups {
                stream.rebind_thinking_presentation(group, cx);
            }
        });
        let child = ThinkingBlock::render_stage_source(self.card.clone(), window, cx);
        let body = self.body(cx).expect("production render creates BodyState");
        self.body.replace(Some(body.clone()));
        assert!(body.read(cx).surface.is_some(), "production body Surface");
        child
    }

    fn stop(&self, cx: &mut App) {
        if let Some(body) = self.body(cx) {
            body.update(cx, |state, cx| {
                state.clear_opacity(cx);
                state.interrupt_anchor();
                if let Some(lease) = state.lease.upgrade() {
                    lease.live.set(false);
                    lease.geometry.set(None);
                }
                if let Some(surface) = state.surface.take() {
                    surface.entity().read(cx).stop();
                }
                state.owner = None;
                state.copy.nsh_reading.borrow_mut().take();
            });
        }
        self.stream.update(cx, |stream, cx| {
            stream.suspend_reading_navigation(cx);
        });
    }
}

pub fn factory(
    kind: BodyKind,
    slot: usize,
    list: ListState,
    _: &mut Window,
    cx: &mut App,
) -> BodyMount {
    let access = *cx
        .try_global::<NativeScrollAccess>()
        .expect("install_scroll_body_access must register the real native backend before mounting");
    cx.set_global(crate::settings::SettingsOpen(false));
    cx.set_global(crate::sidebar::OpenedThread(Some(thread())));
    let stream = cx.new(|cx| ConversationStream::new(thread(), cx));
    let card = stream.update(cx, |stream, cx| {
        stream.set_reading_navigation_authority(scroll_composition_integration::owns_stream, cx);
        let message_id = format!("reading-body-{slot}");
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: message_id.clone(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id,
                delta: source(kind, slot),
            },
            cx,
        );
        let segments = stream
            .entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::RunActivitySegment { group, segment } => {
                    Some((group.clone(), *segment))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!segments.is_empty(), "production thinking segment");
        for (group, segment) in segments {
            assert!(group.update(cx, |group, cx| group.reveal_segment(segment, cx)));
            stream.rebind_thinking_presentation(&group, cx);
        }
        stream.list = list.clone();
        stream
            .active_thinking
            .clone()
            .expect("production ThinkingBlock")
    });
    let owner = stream.downgrade();
    let mount = Rc::new(RefCell::new(Some(Mount {
        stream,
        card,
        kind,
        access,
        body: RefCell::new(None),
    })));
    let rendering = mount.clone();
    let observing = mount.clone();
    let positioning = mount.clone();
    BodyMount {
        stream: owner,
        render: Rc::new(move |window, cx| {
            rendering
                .borrow()
                .as_ref()
                .map(|mount| mount.render(window, cx))
                .unwrap_or_else(|| div().into_any_element())
        }),
        observe: Rc::new(move |window, cx| {
            let mount = observing.borrow();
            let mount = mount.as_ref()?;
            let (request, native) = mount.sample(window, cx)?;
            let body = mount.body(cx)?;
            let error = body.read(cx).surface.as_ref()?.errors().borrow().clone();
            Some(BodyObservation {
                surface: request.surface,
                viewport: native.viewport,
                offset: native.offset,
                maximum: native.maximum,
                error,
            })
        }),
        scroll_to: Rc::new(move |desired, window, cx| {
            let mount = positioning.borrow();
            let mount = mount.as_ref().expect("live production body for setup");
            let (request, native) = mount
                .sample(window, cx)
                .expect("current native setup target");
            assert!(f32::from(desired.x).is_finite() && f32::from(desired.y).is_finite());
            assert!(desired.x >= px(0.) && desired.x <= native.maximum.x);
            assert!(desired.y >= px(0.) && desired.y <= native.maximum.y);
            assert!((mount.access.position)(
                &request, &native, desired, window, cx
            ));
            window.refresh();
        }),
        stop: Rc::new(move |window, cx| {
            if let Some(mount) = mount.borrow_mut().take() {
                mount.stop(cx);
                window.refresh();
            }
        }),
    }
}
