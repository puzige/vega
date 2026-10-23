use super::*;
use core::prelude::v1::test;
use gpui_kit::base::motion::MotionStatus;
use gpui_kit::test::{ElementSnapshot, TestWindowExt};
use std::{rc::Rc, time::Duration};
use vega_conversation::terminal::test_support::{SessionDriver, session};
use vega_conversation::terminal::test_support::{diff_projection, workspace_file_id};
use vega_conversation::types::{
    ConversationEvent, ConversationStopReason, DiffLanguage, TerminalStatus, WorkspaceChangeKind,
    WorkspaceFile, WorkspaceHead, WorkspaceLineCount, WorkspaceSnapshot, WorkspaceStats,
};
use vega_ui::conversation_stream::ConversationStream;
use vega_ui::diff_view::{DiffLayout, NextDiffHunk, ToggleDiffLayout};
use vega_ui::file_preview::FilePreview;
use vega_ui::terminal::TerminalView;
use vega_ui::test_support::{PaintLog, PaintedText};

struct ParentOpacity(f32);

impl Global for ParentOpacity {}

#[derive(Default)]
struct RenderCount(usize);

impl Global for RenderCount {}

#[derive(Default)]
struct SelectionSamples(std::collections::HashMap<(usize, TabKey), Option<MotionStatus>>);

impl Global for SelectionSamples {}

pub(super) fn observe_selection_sample(
    pane: usize,
    key: TabKey,
    status: MotionStatus,
    cx: &mut App,
) {
    if cx.has_global::<SelectionSamples>() {
        cx.global_mut::<SelectionSamples>()
            .0
            .insert((pane, key), Some(status));
    }
}

#[derive(Default)]
struct PaintedBodies(Vec<EntityId>);

impl Global for PaintedBodies {}

struct BodyObserver {
    inner: AnyElement,
    entity: EntityId,
}

impl IntoElement for BodyObserver {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BodyObserver {
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
        (self.inner.request_layout(window, cx), ())
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
        self.inner.prepaint(window, cx);
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
        if cx.has_global::<PaintedBodies>() {
            cx.global_mut::<PaintedBodies>().0.push(self.entity);
        }
        self.inner.paint(window, cx);
    }
}

pub(super) fn observe_body<V: Render + 'static>(view: Entity<V>) -> AnyElement {
    BodyObserver {
        entity: view.entity_id(),
        inner: view.into_any_element(),
    }
    .into_any_element()
}

struct FrameObserver {
    inner: AnyElement,
}

impl IntoElement for FrameObserver {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FrameObserver {
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
        (self.inner.request_layout(window, cx), ())
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
        self.inner.prepaint(window, cx);
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
        cx.global_mut::<PaintedBodies>().0.clear();
        cx.global_mut::<PaintLog>().0.clear();
        self.inner.paint(window, cx);
    }
}

struct WorkspaceRoot {
    owner: Entity<VegaWindow>,
}

impl Render for WorkspaceRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        cx.global_mut::<RenderCount>().0 += 1;
        cx.global_mut::<SelectionSamples>().0 = self
            .owner
            .read(cx)
            .workspace
            .tabs
            .iter()
            .map(|(key, bottom)| ((usize::from(*bottom), key.clone()), None))
            .collect();
        let opacity = cx.global::<ParentOpacity>().0;
        let workspace = self.owner.update(cx, |owner, cx| {
            let conversation = owner.stream_view.as_ref().map_or_else(
                || div().size_full().into_any_element(),
                |(_, stream)| stream.clone().into_any_element(),
            );
            owner.render_workspace(conversation, window, cx)
        });
        FrameObserver {
            inner: div()
                .size_full()
                .opacity(opacity)
                .child(workspace)
                .into_any_element(),
        }
    }
}

fn isolated_owner(cx: &mut Context<VegaWindow>) -> VegaWindow {
    VegaWindow {
        composer_prediction_config_generation: 0,
        composer_prediction: None,
        updater: crate::updater::Updater::default(),
        update_notice_dismissed: false,
        navigation: navigation::Navigation::new(cx, None),
        completion_attention: notifications::CompletionAttention::default(),
        palette: crate::app_palette::AppPalette::default(),
        sidebar: cx.new(Sidebar::new),
        workspace: Workspace::default(),
        subagent_panel: subagent_panel::SubagentPanel::new(cx),
        automation_editors: Default::default(),
        image_preview: image_preview::ImagePreviewController::new(cx),
        onboarding: cua_onboarding::OnboardingHost::default(),
        environment_collapsed: true,
        environment_overlay_open: false,
        sidebar_resize_dragging: false,
        appearance_subscription: None,
        settings_view: None,
        mcp_settings: None,
        pricing_controller: PricingController::new(None),
        stream_view: None,
        thread_scroll_states: scroll_state::ThreadScrollStateLru::default(),
        message_location_route_thread_id: None,
        message_location_route_generation: 0,
        message_location_request_generation: 0,
        deferred_message_location: None,
        draft: None,
        agent_controller: AppAgentController::default(),
        automation_controller: crate::automation_controller::AutomationController::default(),
        context_controller: context::ContextController::default(),
        file_index_controller: FileIndexController::default(),
        diff_controller: DiffController::default(),
        artifact_controller: ArtifactController::default(),
        branch_controller: BranchController::default(),
        commit_controller: CommitController::default(),
        trusted_actions: TrustedActionCoordinator::default(),
        window_alive: Arc::new(AtomicBool::new(true)),
        configured_models: None,
        model_capability_authority: None,
        model_capability_projection: ModelCapabilitySettingsProjection::Loading,
        model_capability_generation: 0,
        model_capability_pending: None,
        configured_reasoning: None,
        configured_reasoning_error: None,
        configured_reasoning_authority: None,
        loaded_replay_config: None,
        reasoning_save_pending: None,
        composer_reasoning_operation: 0,
        pending_composer_thinking: None,
        model_catalog_refresh_pending: false,
        reasoning_error_hold: None,
        model_catalog_loading: false,
        model_catalog_generation: 0,
        deferred_model_refresh: None,
    }
}

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "workspace-motion".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Workspace motion".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::ReadOnly,
        model: String::new(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    }
}

struct Harness {
    visual: VisualTestContext,
    owner: Entity<VegaWindow>,
    files: Vec<Entity<FilePreview>>,
}

impl Harness {
    fn new(cx: &mut TestAppContext, count: usize, dark: bool, reduce: bool) -> Self {
        cx.update(|cx| {
            gpui_kit::init(cx);
            vega_ui::init(cx);
            cx.set_global(if dark { Theme::dark() } else { Theme::light() });
            cx.set_global(VegaStore(Err("isolated workspace".into())));
            cx.set_global(SelectedProject(None));
            cx.set_global(OpenedThread(Some(thread())));
            cx.set_global(SettingsOpen(false));
            cx.set_global(SidebarCollapsed(true));
            cx.set_global(ProjectsCollapsed(true));
            cx.set_global(SessionsCollapsed(true));
            cx.set_global(PaintedBodies::default());
            cx.set_global(PaintLog::default());
            cx.set_global(ParentOpacity(1.));
            cx.set_global(RenderCount::default());
            cx.set_global(SelectionSamples::default());
            cx.set_reduce_motion(reduce);
        });
        let mut owner = None;
        let mut files = Vec::new();
        let handle = cx.open_window(size(px(1400.), px(900.)), |window, cx| {
            let entity = cx.new(isolated_owner);
            entity.update(cx, |owner, cx| {
                for index in 0..count {
                    let file = cx.new(|cx| {
                        FilePreview::new(
                            PaletteFilePreview {
                                relative_path: format!("file-{index}.txt"),
                                content: (0..120)
                                    .map(|row| format!("file {index}, line {row}\n"))
                                    .collect(),
                            },
                            cx,
                        )
                    });
                    owner.workspace_open_file(file.clone(), window, cx);
                    files.push(file);
                }
                owner.workspace.open(TabKey::File(1));
            });
            cx.observe(&entity, |_, _, cx| cx.notify()).detach();
            owner = Some(entity.clone());
            WorkspaceRoot { owner: entity }
        });
        let mut harness = Self {
            visual: VisualTestContext::from_window(handle.into(), cx),
            owner: owner.expect("workspace owner"),
            files,
        };
        harness.visual.update(|window, _| window.activate_window());
        harness.visual.run_until_parked();
        harness
            .visual
            .update(|window, _| assert!(window.is_window_active()));
        harness.frame();
        harness.update(|owner, window, cx| owner.workspace_focus(0, window, cx));
        harness
    }

    fn frame(&mut self) {
        self.visual.update(|window, cx| window.render_frame(cx));
    }

    fn update<R>(
        &mut self,
        update: impl FnOnce(&mut VegaWindow, &mut Window, &mut Context<VegaWindow>) -> R,
    ) -> R {
        self.visual
            .update(|window, cx| self.owner.update(cx, |owner, cx| update(owner, window, cx)))
    }

    fn tab(&mut self, key: &TabKey) -> ElementSnapshot {
        let selector = SharedString::from(format!("workspace-tab-{key:?}"));
        self.visual.update(|window, _| window.find(selector))
    }

    fn pointer_out(&mut self) {
        self.visual
            .simulate_mouse_move(point(px(8.), px(700.)), None, Modifiers::none());
        self.frame();
    }

    fn reveal_tab(&mut self, key: &TabKey) -> usize {
        let pane = self.update(|owner, _, cx| {
            let bottom = owner
                .workspace
                .tabs
                .iter()
                .find(|(tab, _)| tab == key)
                .expect("tab must exist before a pointer gesture")
                .1;
            let position = owner
                .workspace
                .tabs
                .iter()
                .filter(|(_, dock)| *dock == bottom)
                .position(|(tab, _)| tab == key)
                .expect("tab must belong to its pane");
            let pane = usize::from(bottom);
            owner.workspace.tab_scroll[pane].scroll_to_item(position);
            cx.notify();
            pane
        });
        self.frame();
        assert!(self.tab(key).visible(), "tab must be visible before input");
        pane
    }

    fn click(&mut self, key: &TabKey) {
        let pane = self.reveal_tab(key);
        let label = self.update(|owner, _, cx| owner.workspace_label(key, cx));
        let record = self.painted(self.owner.entity_id(), &label);
        let visible = record.bounds.intersect(&record.clip);
        assert!(visible.size.width > px(0.) && visible.size.height > px(0.));
        let position = visible.center();
        assert!(self.tab(key).bounds().contains(&position));
        self.visual
            .simulate_mouse_move(position, None, Modifiers::none());
        self.visual.simulate_click(position, Modifiers::none());
        self.pointer_out();
        self.update(|owner, window, cx| {
            assert_eq!(owner.workspace.selected[pane].as_ref(), Some(key));
            match key {
                TabKey::File(id) => assert!(
                    owner.workspace.files[id]
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                ),
                TabKey::Terminal(id) => assert!(
                    owner.workspace.terminals[id]
                        .view
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                ),
                TabKey::Diff => assert!(
                    owner
                        .diff_controller
                        .active
                        .as_ref()
                        .expect("selected Diff must have its real view")
                        .view
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                ),
                _ => {}
            }
        });
    }

    fn close(&mut self, key: &TabKey) {
        let pane = self.reveal_tab(key);
        let tab = self.tab(key).bounds();
        self.visual
            .simulate_mouse_move(tab.center(), None, Modifiers::none());
        self.frame();
        let record = self.painted(
            self.owner.entity_id(),
            &format!("close-workspace-tab-{key:?}"),
        );
        let visible = record.bounds.intersect(&record.clip);
        assert!(visible.size.width > px(0.) && visible.size.height > px(0.));
        let position = visible.center();
        assert!(self.tab(key).bounds().contains(&position));
        self.visual
            .simulate_mouse_move(position, None, Modifiers::none());
        self.visual.simulate_click(position, Modifiers::none());
        self.pointer_out();
        self.update(|owner, window, cx| {
            assert!(!owner.workspace.tabs.iter().any(|(tab, _)| tab == key));
            assert!(
                !owner
                    .workspace
                    .selected
                    .iter()
                    .any(|tab| tab.as_ref() == Some(key))
            );
            match key {
                TabKey::File(id) => assert!(!owner.workspace.files.contains_key(id)),
                TabKey::Terminal(id) => assert!(!owner.workspace.terminals.contains_key(id)),
                TabKey::Diff => assert!(owner.diff_controller.active.is_none()),
                _ => {}
            }
            match owner.workspace.selected[pane].as_ref() {
                Some(TabKey::File(id)) => assert!(
                    owner.workspace.files[id]
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                ),
                Some(TabKey::Terminal(id)) => assert!(
                    owner.workspace.terminals[id]
                        .view
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                ),
                _ => {}
            }
        });
    }

    fn open_file(&mut self, index: usize) {
        let file = self.files[index].clone();
        self.update(|owner, window, cx| owner.workspace_open_file(file, window, cx));
        self.frame();
    }

    fn advance(&mut self, millis: u64) {
        self.visual
            .executor()
            .advance_clock(Duration::from_millis(millis));
        self.visual.run_until_parked();
        self.frame();
    }

    fn background(&mut self, key: &TabKey) -> Option<Rgba> {
        let bounds = self.tab(key).bounds();
        self.visual.update(|window, _| {
            let scaled = bounds.scale(window.scale_factor());
            let colors: Vec<Rgba> = window
                .painted_quads()
                .iter()
                .filter(|quad| quad.bounds == scaled)
                .filter_map(|quad| quad.background.as_solid().map(Rgba::from))
                .filter(|color| color.a > 0.)
                .collect();
            assert!(colors.len() <= 1, "ambiguous tab background: {colors:?}");
            colors.first().copied()
        })
    }

    fn strength(&mut self, key: &TabKey) -> f32 {
        let Some(color) = self.background(key) else {
            return 0.;
        };
        let endpoint = self.visual.read(|cx| theme(cx).colors.bg_active_alpha);
        for (observed, expected) in [
            (color.r, endpoint.r),
            (color.g, endpoint.g),
            (color.b, endpoint.b),
        ] {
            assert!((observed - expected).abs() < 0.005, "wrong painted surface");
        }
        color.a / endpoint.a
    }

    fn assert_strength(&mut self, key: &TabKey, expected: f32) {
        let actual = self.strength(key);
        assert!(
            (actual - expected).abs() < 0.005,
            "{key:?} fill strength {actual}, expected {expected}"
        );
    }

    fn assert_selected_file(&mut self, index: usize, pane: usize) {
        let file = self.files[index].clone();
        self.update(|owner, window, cx| {
            assert_eq!(
                owner.workspace.selected[pane],
                Some(TabKey::File(index as u64 + 1))
            );
            assert!(file.read(cx).focus_handle(cx).contains_focused(window, cx));
            assert_eq!(cx.global::<PaintedBodies>().0, vec![file.entity_id()]);
            for other in owner
                .workspace
                .files
                .values()
                .filter(|other| *other != &file)
            {
                assert!(!other.read(cx).focus_handle(cx).contains_focused(window, cx));
            }
        });
        self.visible_content(file.entity_id());
    }

    fn next_frame_callbacks(&mut self) -> usize {
        self.visual
            .update(|window, cx| window.simulate_next_frame(cx))
    }

    fn add_diff(&mut self) -> Entity<DiffView> {
        let view = self.update(|owner, _, cx| {
            let view = cx.new(|cx| DiffView::new(thread().id, String::new(), cx));
            assert!(
                owner
                    .diff_controller
                    .begin(
                        thread().id,
                        String::new(),
                        view.clone(),
                        DiffFocusIntent::PreserveCurrent,
                    )
                    .is_some()
            );
            owner.workspace.tabs.push((TabKey::Diff, false));
            cx.notify();
            view
        });
        self.frame();
        view
    }
}

fn snapshot(files: usize) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        generation: 1,
        head: WorkspaceHead::Branch {
            label: "test-branch".into(),
        },
        files: (0..files)
            .map(|index| WorkspaceFile {
                id: workspace_file_id(index as u32),
                label: format!("diff-file-{index}.txt"),
                previous_label: None,
                staged: WorkspaceChangeKind::Unchanged,
                unstaged: WorkspaceChangeKind::Modified,
                additions: WorkspaceLineCount::Known(0),
                deletions: WorkspaceLineCount::Known(0),
                language: DiffLanguage::Plain,
            })
            .collect(),
        stats: WorkspaceStats {
            file_count: files as u32,
            additions: WorkspaceLineCount::Known(0),
            deletions: WorkspaceLineCount::Known(0),
        },
    }
}

impl Harness {
    fn painted(&mut self, entity: EntityId, text: &str) -> PaintedText {
        self.visual.read(|cx| {
            let found: Vec<_> = cx
                .global::<PaintLog>()
                .0
                .iter()
                .filter(|record| record.entity == entity && record.text.as_ref() == text)
                .cloned()
                .collect();
            assert_eq!(found.len(), 1, "expected one actual paint for {text:?}");
            found[0].clone()
        })
    }

    fn assert_text(&mut self, entity: EntityId, text: &str) -> PaintedText {
        let record = self.painted(entity, text);
        self.visual.update(|window, _| {
            assert!(record.bounds.intersects(&record.clip));
            let opacity = record
                .inherited_opacity(window)
                .expect("paint probe missing or ambiguous, not evidence of full opacity");
            assert!(
                (opacity - 1.).abs() < 0.001,
                "{text:?} inherited opacity {opacity}"
            );
            assert!(
                (record.color.a - 1.).abs() < 0.001,
                "{text:?} text style alpha"
            );
        });
        record
    }

    fn assert_no_text(&mut self, entity: EntityId, text: &str) {
        self.visual.read(|cx| {
            assert!(
                !cx.global::<PaintLog>()
                    .0
                    .iter()
                    .any(|record| record.entity == entity && record.text.as_ref() == text)
            )
        });
    }

    fn body_position(&mut self, entity: EntityId) -> Point<Pixels> {
        self.visual.read(|cx| {
            let record = cx
                .global::<PaintLog>()
                .0
                .iter()
                .find(|record| record.entity == entity && !record.text.trim().is_empty())
                .expect("visible content required before scrolling");
            record.bounds.intersect(&record.clip).center()
        })
    }

    fn scroll_at(&mut self, position: Point<Pixels>, delta: f32) {
        self.visual.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
        });
        self.frame();
    }

    fn add_terminal(&mut self) -> (TabKey, Entity<TerminalView>, SessionDriver) {
        let (session, mut driver) = session();
        driver.status(TerminalStatus::Running);
        let (key, view) = self.update(|owner, _, cx| {
            let view = cx.new(|cx| TerminalView::with_test_session(session, cx));
            owner.workspace.next_terminal += 1;
            let id = owner.workspace.next_terminal;
            let key = TabKey::Terminal(id);
            owner.workspace.terminals.insert(
                id,
                TerminalTab {
                    project_id: String::new(),
                    view: view.clone(),
                    bottom: false,
                },
            );
            owner.workspace.tabs.push((key.clone(), false));
            cx.observe(&view, |_, _, cx| cx.notify()).detach();
            cx.notify();
            (key, view)
        });
        self.frame();
        (key, view, driver)
    }

    fn populated_diff(&mut self) -> Entity<DiffView> {
        let diff = self.add_diff();
        self.visual.update(|_, cx| {
            diff.update(cx, |view, cx| {
                view.apply_snapshot(snapshot(1), cx);
            })
        });
        self.frame();
        self.click(&TabKey::Diff);
        let row = self.assert_text(diff.entity_id(), "diff-file-0.txt");
        self.visual
            .simulate_click(row.bounds.intersect(&row.clip).center(), Modifiers::none());
        self.frame();
        self.visual.update(|_, cx| {
            diff.update(cx, |view, cx| {
                assert_eq!(view.expanded_file(), Some(workspace_file_id(0)));
                assert!(view.apply_projection(diff_projection(workspace_file_id(0)), cx));
            })
        });
        self.frame();
        self.assert_text(diff.entity_id(), "diff hunk 0, row 0");
        self.open_file(0);
        diff
    }

    fn terminal_poll(&mut self, driver: &mut SessionDriver) {
        driver.drain();
        self.advance(30);
    }

    fn add_stream(&mut self) -> Entity<ConversationStream> {
        let stream = self.update(|owner, _, cx| {
            let stream = cx.new(|cx| ConversationStream::new(thread(), cx));
            cx.observe(&stream, |_, _, cx| cx.notify()).detach();
            owner.stream_view = Some((thread().id, stream.clone()));
            cx.notify();
            stream
        });
        self.frame();
        stream
    }

    fn status(&mut self, pane: usize, key: &TabKey) -> MotionStatus {
        self.visual.read(|cx| cx.global::<SelectionSamples>().0
            .get(&(pane, key.clone())).copied().flatten()
            .expect("missing status from the actual production fill sampler; observer must not synthesize it"))
    }

    fn visible_content(&mut self, entity: EntityId) -> Vec<(String, Bounds<Pixels>)> {
        self.visual.update(|window, cx| {
            let records: Vec<_> = cx
                .global::<PaintLog>()
                .0
                .iter()
                .filter(|record| record.entity == entity && !record.text.trim().is_empty())
                .cloned()
                .collect();
            assert!(!records.is_empty(), "actual visible content is required");
            records
                .into_iter()
                .map(|record| {
                    let opacity = record
                        .inherited_opacity(window)
                        .expect("actual content probe");
                    assert!((opacity - 1.).abs() < 0.001);
                    (
                        record.text.to_string(),
                        record.bounds.intersect(&record.clip),
                    )
                })
                .collect()
        })
    }

    fn focus_ring(
        &mut self,
        key: &TabKey,
    ) -> Vec<(Bounds<ScaledPixels>, Hsla, Edges<ScaledPixels>)> {
        let bounds = self.tab(key).bounds();
        self.visual.update(|window, cx| {
            let accent = Hsla::from(theme(cx).colors.accent);
            let scaled = bounds.scale(window.scale_factor());
            let quads: Vec<_> = window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.border_color == accent && quad.bounds.intersects(&scaled))
                .map(|quad| (quad.bounds, quad.border_color, quad.border_widths))
                .collect();
            assert!(
                !quads.is_empty(),
                "actual keyboard focus border must be painted immediately"
            );
            quads
        })
    }

    fn assert_stopped(&mut self, pane: usize, key: &TabKey) {
        assert!(matches!(
            self.status(pane, key),
            MotionStatus::Idle | MotionStatus::Finished
        ));
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let handle = self.visual.window_handle();
        if self.visual.windows().contains(&handle) {
            self.visual
                .update_window(handle, |_, window, _| window.remove_window())
                .expect("existing fixture window must be removable");
        }
    }
}

#[gpui_kit::test]
fn real_mouse_handler_body_focus_and_painted_background_are_observable(cx: &mut TestAppContext) {
    for dark in [false, true] {
        let mut h = Harness::new(cx, 3, dark, true);
        h.assert_strength(&TabKey::File(1), 1.);
        h.assert_strength(&TabKey::File(2), 0.);
        h.click(&TabKey::File(2));
        h.assert_selected_file(1, 0);
        h.assert_strength(&TabKey::File(1), 0.);
        h.assert_strength(&TabKey::File(2), 1.);
        assert_eq!(h.tab(&TabKey::File(2)).label(), Some("file-1.txt"));
        h.click(&TabKey::File(1));
        h.assert_selected_file(0, 0);
    }
}

#[gpui_kit::test]
fn selection_commits_before_only_the_background_eases_for_one_hundred_twenty_millis(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    let old = h.tab(&TabKey::File(1)).bounds();
    let new = h.tab(&TabKey::File(2)).bounds();
    h.click(&TabKey::File(2));
    h.assert_selected_file(1, 0);
    h.assert_strength(&TabKey::File(1), 1.);
    h.assert_strength(&TabKey::File(2), 0.);
    assert_eq!(h.tab(&TabKey::File(1)).bounds(), old);
    assert_eq!(h.tab(&TabKey::File(2)).bounds(), new);
    h.advance(60);
    h.assert_selected_file(1, 0);
    h.assert_strength(&TabKey::File(1), 0.125);
    h.assert_strength(&TabKey::File(2), 0.875);
    h.advance(60);
    h.assert_strength(&TabKey::File(1), 0.);
    h.assert_strength(&TabKey::File(2), 1.);
    assert_eq!(h.tab(&TabKey::File(1)).bounds(), old);
    assert_eq!(h.tab(&TabKey::File(2)).bounds(), new);
}

#[gpui_kit::test]
fn keyboard_tab_shift_tab_enter_space_and_programmatic_open_keep_real_focus_intent(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, false, false);
    h.update(|owner, window, cx| {
        window.focus(&owner.workspace.tab_focuses[&TabKey::File(1)], cx);
    });
    h.visual.simulate_keystrokes("tab tab");
    h.update(|owner, window, cx| {
        assert!(owner.workspace.tab_focuses[&TabKey::File(2)].contains_focused(window, cx));
        assert_eq!(owner.workspace.selected[0], Some(TabKey::File(1)));
    });
    h.visual.simulate_keystrokes("shift-tab shift-tab");
    h.update(|owner, window, cx| {
        assert!(owner.workspace.tab_focuses[&TabKey::File(1)].contains_focused(window, cx));
    });
    h.visual.simulate_keystrokes("tab tab enter");
    h.pointer_out();
    h.assert_selected_file(1, 0);
    h.assert_strength(&TabKey::File(2), 0.);
    h.advance(120);
    h.update(|owner, window, cx| {
        window.focus(&owner.workspace.tab_focuses[&TabKey::File(3)], cx);
    });
    h.visual.simulate_keystrokes("space");
    h.pointer_out();
    h.assert_selected_file(2, 0);
    h.assert_strength(&TabKey::File(3), 0.);
    h.advance(120);
    h.open_file(0);
    h.assert_selected_file(0, 0);
    h.assert_strength(&TabKey::File(1), 0.);
    h.advance(120);
    h.assert_strength(&TabKey::File(1), 1.);
}

#[gpui_kit::test]
fn rapid_reversal_starts_at_the_current_painted_sample_and_never_replays_old_selection(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    h.click(&TabKey::File(2));
    h.advance(40);
    let a = h.strength(&TabKey::File(1));
    let b = h.strength(&TabKey::File(2));
    assert!(a > 0. && a < 1. && b > 0. && b < 1.);
    h.click(&TabKey::File(1));
    h.assert_selected_file(0, 0);
    h.assert_strength(&TabKey::File(1), a);
    h.assert_strength(&TabKey::File(2), b);
    h.advance(20);
    assert!(h.strength(&TabKey::File(1)) > a);
    assert!(h.strength(&TabKey::File(2)) < b);
    h.advance(240);
    h.assert_selected_file(0, 0);
    h.assert_strength(&TabKey::File(1), 1.);
    h.assert_strength(&TabKey::File(2), 0.);
}

#[gpui_kit::test]
fn rapid_third_target_cancels_the_old_target_without_queuing_navigation(cx: &mut TestAppContext) {
    let mut h = Harness::new(cx, 3, true, false);
    h.click(&TabKey::File(2));
    h.advance(30);
    let a = h.strength(&TabKey::File(1));
    let b = h.strength(&TabKey::File(2));
    assert!(a > 0. && a < 1. && b > 0. && b < 1.);
    h.click(&TabKey::File(3));
    h.assert_selected_file(2, 0);
    h.assert_strength(&TabKey::File(1), a);
    h.assert_strength(&TabKey::File(2), b);
    h.assert_strength(&TabKey::File(3), 0.);
    h.advance(120);
    h.assert_selected_file(2, 0);
    h.assert_strength(&TabKey::File(1), 0.);
    h.assert_strength(&TabKey::File(2), 0.);
    h.assert_strength(&TabKey::File(3), 1.);
}

#[gpui_kit::test]
fn initial_mount_and_repeated_activation_are_already_at_the_real_target(cx: &mut TestAppContext) {
    let mut h = Harness::new(cx, 3, true, false);
    h.update(|owner, window, cx| owner.workspace_focus(0, window, cx));
    h.assert_selected_file(0, 0);
    h.assert_strength(&TabKey::File(1), 1.);
    h.assert_strength(&TabKey::File(2), 0.);
    h.next_frame_callbacks();
    h.click(&TabKey::File(1));
    h.assert_selected_file(0, 0);
    h.assert_strength(&TabKey::File(1), 1.);
    h.next_frame_callbacks();
    h.frame();
    assert_eq!(h.next_frame_callbacks(), 0);
}

#[gpui_kit::test]
fn hiding_and_restoring_adopts_current_selection_without_replaying_the_old_transition(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    h.click(&TabKey::File(2));
    h.advance(30);
    assert!(h.strength(&TabKey::File(2)) > 0. && h.strength(&TabKey::File(2)) < 1.);
    h.update(|owner, window, cx| owner.workspace_hide(0, window, cx));
    h.frame();
    h.visual.update(|_, cx| {
        assert!(cx.global::<PaintedBodies>().0.is_empty());
    });
    h.next_frame_callbacks();
    h.frame();
    assert_eq!(h.next_frame_callbacks(), 0);
    h.open_file(2);
    h.assert_selected_file(2, 0);
    h.assert_strength(&TabKey::File(2), 0.);
    h.assert_strength(&TabKey::File(3), 1.);
    h.advance(240);
    h.assert_strength(&TabKey::File(3), 1.);
}

#[gpui_kit::test]
fn closing_selected_or_inactive_tabs_uses_real_fallback_and_cannot_resurrect_deleted_bodies(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 4, false, false);
    let removed = h.files[1].entity_id();
    h.click(&TabKey::File(2));
    h.advance(30);
    h.close(&TabKey::File(2));
    h.assert_selected_file(2, 0);
    h.update(|owner, _, cx| {
        assert!(!owner.workspace.files.contains_key(&2));
        assert!(!owner.workspace.tab_focuses.contains_key(&TabKey::File(2)));
        assert!(!cx.global::<PaintedBodies>().0.contains(&removed));
    });
    h.close(&TabKey::File(1));
    h.assert_selected_file(2, 0);
    h.advance(300);
    h.assert_selected_file(2, 0);
    h.assert_strength(&TabKey::File(3), 1.);
    h.visual.update(|window, _| {
        assert!(window.try_find("workspace-tab-File(2)").is_none());
    });
}

#[gpui_kit::test]
fn reordering_preserves_tab_key_samples_and_new_tabs_mount_at_their_current_target(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    h.click(&TabKey::File(2));
    h.advance(40);
    let a = h.strength(&TabKey::File(1));
    let b = h.strength(&TabKey::File(2));
    assert!(a > 0. && a < 1. && b > 0. && b < 1.);
    h.update(|owner, _, cx| {
        owner.workspace.tabs.swap(0, 2);
        cx.notify();
    });
    h.frame();
    h.assert_selected_file(1, 0);
    h.assert_strength(&TabKey::File(1), a);
    h.assert_strength(&TabKey::File(2), b);
    h.update(|owner, window, cx| {
        let view = cx.new(|cx| {
            FilePreview::new(
                PaletteFilePreview {
                    relative_path: "new.txt".into(),
                    content: "new body".into(),
                },
                cx,
            )
        });
        owner.workspace_open_file(view, window, cx);
    });
    h.frame();
    h.assert_strength(&TabKey::File(4), 1.);
    h.advance(240);
    h.assert_strength(&TabKey::File(1), 0.);
    h.assert_strength(&TabKey::File(2), 0.);
    h.assert_strength(&TabKey::File(4), 1.);
}

#[gpui_kit::test]
fn docking_mounts_one_existing_body_in_the_new_pane_without_inheriting_another_panes_sample(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    let entity = h.files[1].entity_id();
    h.click(&TabKey::File(2));
    h.advance(30);
    h.update(|owner, window, cx| owner.workspace_move_selected(0, window, cx));
    h.frame();
    h.update(|owner, _, cx| {
        assert_eq!(owner.workspace.selected[0], Some(TabKey::File(3)));
        assert_eq!(owner.workspace.selected[1], Some(TabKey::File(2)));
        assert_eq!(owner.workspace.files[&2].entity_id(), entity);
        let painted = &cx.global::<PaintedBodies>().0;
        assert_eq!(painted.len(), 2);
        assert_eq!(painted.iter().filter(|id| **id == entity).count(), 1);
    });
    h.assert_strength(&TabKey::File(2), 1.);
    h.advance(240);
    h.assert_strength(&TabKey::File(2), 1.);
    h.update(|owner, window, cx| owner.workspace_move_selected(1, window, cx));
    h.frame();
    h.update(|owner, _, cx| {
        assert_eq!(owner.workspace.selected[0], Some(TabKey::File(2)));
        assert!(owner.workspace.selected[1].is_none());
        assert_eq!(cx.global::<PaintedBodies>().0, vec![entity]);
    });
    h.assert_strength(&TabKey::File(2), 1.);
}

#[gpui_kit::test]
fn diff_loading_failure_retry_and_missing_body_never_wait_for_the_decorative_clock(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    let diff = h.add_diff();
    let retries = Rc::new(std::cell::Cell::new(0));
    let observed = retries.clone();
    let subscription = h.visual.update(|_, cx| {
        cx.subscribe(&diff, move |_, _: &DiffRetryRequested, _| {
            observed.set(observed.get() + 1);
        })
    });
    h.visual.update(|_, cx| {
        diff.update(cx, |view, cx| {
            assert!(view.generation().is_none());
            view.begin_refresh(true, cx);
        });
    });
    h.click(&TabKey::Diff);
    h.assert_text(diff.entity_id(), "Loading workspace diff…");
    h.update(|owner, window, cx| {
        assert_eq!(owner.workspace.selected[0], Some(TabKey::Diff));
        assert!(diff.read(cx).focus_handle(cx).contains_focused(window, cx));
        assert_eq!(cx.global::<PaintedBodies>().0, vec![diff.entity_id()]);
    });
    h.visual.update(|_, cx| {
        diff.update(cx, |view, cx| {
            view.set_refreshing(false, cx);
            view.apply_refresh_error(GitWorkspaceErrorCode::TimedOut, cx);
        });
    });
    h.frame();
    h.assert_strength(&TabKey::Diff, 0.);
    h.assert_text(
        diff.entity_id(),
        "Git diff timed out. Retry when the repository is idle.",
    );
    h.assert_no_text(diff.entity_id(), "Loading workspace diff…");
    h.visual.simulate_keystrokes("tab enter");
    h.frame();
    assert_eq!(
        retries.get(),
        1,
        "real Diff retry must remain usable immediately"
    );
    let retry = h
        .visual
        .update(|window, _| window.find("workspace-diff-retry").bounds());
    h.visual.simulate_click(retry.center(), Modifiers::none());
    h.pointer_out();
    assert_eq!(
        retries.get(),
        2,
        "visible error recovery must also be clickable before motion completes"
    );
    h.visual.update(|_, cx| {
        diff.update(cx, |view, cx| {
            view.set_refreshing(false, cx);
            view.apply_snapshot(snapshot(0), cx);
        })
    });
    h.frame();
    h.assert_text(diff.entity_id(), "当前没有工作区变更");
    h.assert_no_text(
        diff.entity_id(),
        "Git diff timed out. Retry when the repository is idle.",
    );
    h.assert_strength(&TabKey::Diff, 0.);
    h.update(|owner, _, cx| {
        owner.diff_controller.close();
        cx.notify();
    });
    h.frame();
    h.visual.update(|_, cx| {
        assert!(cx.global::<PaintedBodies>().0.is_empty());
    });
    h.update(|owner, _, _| assert_eq!(owner.workspace.selected[0], Some(TabKey::Diff)));
    h.advance(240);
    h.update(|owner, _, _| assert_eq!(owner.workspace.selected[0], Some(TabKey::Diff)));
    drop(subscription);
}

#[gpui_kit::test]
fn offscreen_tab_reveal_is_consumed_on_the_first_render_not_animation_completion(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 16, false, false);
    h.open_file(15);
    h.assert_selected_file(15, 0);
    h.update(|owner, _, _| {
        let scroll = &owner.workspace.tab_scroll[0];
        assert!(!owner.workspace.reveal_tabs[0]);
        assert!(scroll.offset().x < px(0.));
        assert!(scroll.max_offset().x > px(0.));
    });
    let selected = h.tab(&TabKey::File(16)).bounds();
    h.update(|owner, _, _| {
        let viewport = owner.workspace.tab_scroll[0].bounds();
        assert!(selected.left() >= viewport.left() - px(1.));
        assert!(selected.right() <= viewport.right() + px(1.));
    });
    h.close(&TabKey::File(16));
    h.assert_selected_file(14, 0);
    h.update(|owner, _, _| assert!(!owner.workspace.reveal_tabs[0]));
}

#[gpui_kit::test]
fn hover_and_pressed_feedback_remain_immediate_and_do_not_change_selection_or_geometry(
    cx: &mut TestAppContext,
) {
    for dark in [false, true] {
        let mut h = Harness::new(cx, 3, dark, false);
        let key = TabKey::File(2);
        let bounds = h.tab(&key).bounds();
        let position = point(bounds.left() + px(18.), bounds.center().y);
        h.visual
            .simulate_mouse_move(position, None, Modifiers::none());
        h.frame();
        let hover = h.visual.read(|cx| theme(cx).colors.bg_hover);
        assert_eq!(h.background(&key), Some(hover));
        h.visual
            .simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
        h.frame();
        assert_eq!(h.background(&key), Some(hover));
        assert_eq!(h.tab(&key).bounds(), bounds);
        h.update(|owner, _, _| assert_eq!(owner.workspace.selected[0], Some(TabKey::File(1))));
        h.visual
            .simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
        h.pointer_out();
        h.assert_selected_file(1, 0);
        h.assert_strength(&key, 0.);
    }
}

#[gpui_kit::test]
fn reduced_motion_at_start_or_mid_transition_adopts_the_latest_target_and_stops_frames(
    cx: &mut TestAppContext,
) {
    for reduce_at_start in [false, true] {
        let mut h = Harness::new(cx, 3, true, reduce_at_start);
        h.click(&TabKey::File(2));
        if !reduce_at_start {
            h.advance(30);
            assert!(h.strength(&TabKey::File(2)) > 0. && h.strength(&TabKey::File(2)) < 1.);
            h.visual.update(|_, cx| cx.set_reduce_motion(true));
            h.frame();
        }
        h.assert_selected_file(1, 0);
        h.assert_strength(&TabKey::File(1), 0.);
        h.assert_strength(&TabKey::File(2), 1.);
        h.next_frame_callbacks();
        h.frame();
        assert_eq!(h.next_frame_callbacks(), 0);
    }
}

#[gpui_kit::test]
fn visible_motion_requests_frames_but_completion_and_window_inactivity_stop_them(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    h.next_frame_callbacks();
    h.click(&TabKey::File(2));
    assert!(
        h.next_frame_callbacks() > 0,
        "positive control for frame observation"
    );
    h.advance(120);
    h.assert_strength(&TabKey::File(2), 1.);
    h.next_frame_callbacks();
    h.frame();
    assert_eq!(h.next_frame_callbacks(), 0);
    h.click(&TabKey::File(3));
    h.advance(30);
    h.visual.deactivate_window();
    h.frame();
    h.assert_selected_file(2, 0);
    h.assert_strength(&TabKey::File(2), 0.);
    h.assert_strength(&TabKey::File(3), 1.);
    h.next_frame_callbacks();
    h.frame();
    assert_eq!(h.next_frame_callbacks(), 0);
    h.visual.update(|window, _| window.activate_window());
    h.visual.run_until_parked();
    h.frame();
    h.assert_strength(&TabKey::File(3), 1.);
    h.advance(240);
    h.assert_selected_file(2, 0);
    h.assert_strength(&TabKey::File(3), 1.);
}

#[gpui_kit::test]
fn native_tab_names_and_selected_semantics_follow_business_selection_not_painted_progress(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    h.click(&TabKey::File(2));
    h.assert_selected_file(1, 0);
    assert_eq!(h.tab(&TabKey::File(1)).label(), Some("file-0.txt"));
    assert_eq!(h.tab(&TabKey::File(2)).label(), Some("file-1.txt"));
    assert_eq!(h.tab(&TabKey::File(1)).selected(), Some(false));
    assert_eq!(h.tab(&TabKey::File(2)).selected(), Some(true));
    h.assert_strength(&TabKey::File(1), 1.);
    h.assert_strength(&TabKey::File(2), 0.);
    h.advance(120);
    assert_eq!(h.tab(&TabKey::File(2)).selected(), Some(true));
}

#[gpui_kit::test]
fn actual_text_paint_probe_detects_parent_opacity_and_hidden_content(cx: &mut TestAppContext) {
    let mut h = Harness::new(cx, 2, true, true);
    let entity = h.files[0].entity_id();
    let full = h.assert_text(entity, "file 0, line 0");
    h.visual
        .update(|_, cx| cx.global_mut::<ParentOpacity>().0 = 0.25);
    h.frame();
    let faded = h.painted(entity, "file 0, line 0");
    assert_eq!(full.bounds, faded.bounds);
    h.visual.update(|window, _| {
        let observed = faded.inherited_opacity(window).expect("real probe quad");
        assert!((observed - 0.25).abs() < 0.001);
        assert!(
            (faded.color.a - 1.).abs() < 0.001,
            "style alpha alone must not pass the opacity check"
        );
    });
    h.visual
        .update(|_, cx| cx.global_mut::<ParentOpacity>().0 = 1.);
    h.frame();
    h.assert_text(entity, "file 0, line 0");
    h.click(&TabKey::File(2));
    h.assert_no_text(entity, "file 0, line 0");
    let second = h.files[1].entity_id();
    h.assert_text(second, "file 1, line 0");
}

#[gpui_kit::test]
fn terminal_driver_uses_real_parser_poll_paste_input_scroll_and_close(cx: &mut TestAppContext) {
    let mut h = Harness::new(cx, 2, true, true);
    let (key, terminal, mut driver) = h.add_terminal();
    h.click(&key);
    h.terminal_poll(&mut driver);
    assert!(
        !driver.sizes().is_empty(),
        "real terminal canvas must enqueue resize"
    );
    driver.output(b"\x1b[2J\x1b[HTERMINAL_VISIBLE\x1b[?2004h");
    h.terminal_poll(&mut driver);
    let visible = h.visual.read(|cx| {
        cx.global::<PaintLog>()
            .0
            .iter()
            .find(|record| {
                record.entity == terminal.entity_id() && record.text.contains("TERMINAL_VISIBLE")
            })
            .expect("parsed output must reach real terminal line.paint")
            .text
            .to_string()
    });
    h.assert_text(terminal.entity_id(), &visible);
    h.visual.simulate_keystrokes("enter");
    h.visual
        .update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("paste-positive".into())));
    h.visual.dispatch_action(vega_ui::text_input::Paste);
    driver.drain();
    assert_eq!(
        driver.inputs(),
        &[b"\r".to_vec(), b"\x1b[200~paste-positive\x1b[201~".to_vec()]
    );
    driver.output(
        &(0..120)
            .map(|row| format!("terminal line {row}\r\n"))
            .collect::<String>()
            .into_bytes(),
    );
    h.terminal_poll(&mut driver);
    let position = h.body_position(terminal.entity_id());
    h.scroll_at(position, 180.);
    h.terminal_poll(&mut driver);
    assert!(driver.scrolls().iter().any(|rows| *rows > 0));
    h.visual.read(|cx| {
        assert!(
            terminal
                .read(cx)
                .observed_snapshot()
                .expect("polled snapshot")
                .scrollback
                > 0
        )
    });
    assert!(!driver.closed());
    let weak = terminal.downgrade();
    h.close(&key);
    h.visual.update(|_, _| drop(terminal));
    h.visual.run_until_parked();
    assert!(
        driver.closed(),
        "production close must release the real session"
    );
    assert!(
        weak.upgrade().is_none(),
        "removed body must not be retained by stale motion work"
    );
}

#[gpui_kit::test]
fn repeated_terminal_diff_file_switches_preserve_reading_entities_and_isolate_input(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new(cx, 3, true, false);
    let (terminal_key, terminal, mut driver) = h.add_terminal();
    let diff = h.populated_diff();
    let file = h.files[0].clone();
    let file_position = h.body_position(file.entity_id());
    h.scroll_at(file_position, -240.);
    let file_scroll = h.visual.read(|cx| file.read(cx).observed_scroll());
    assert!(file_scroll.y < px(0.), "file scroll positive control");
    let file_reading = h.visible_content(file.entity_id());
    h.click(&TabKey::Diff);
    h.visual.dispatch_action(NextDiffHunk);
    h.visual.dispatch_action(NextDiffHunk);
    h.visual.dispatch_action(NextDiffHunk);
    h.frame();
    let navigated = h.visual.read(|cx| diff.read(cx).observed_scroll());
    assert!(
        navigated.y < px(0.),
        "real Diff action must navigate content"
    );
    h.visual.dispatch_action(ToggleDiffLayout);
    h.frame();
    assert_eq!(
        h.visual.read(|cx| diff.read(cx).layout()),
        DiffLayout::SideBySide
    );
    h.visual.dispatch_action(ToggleDiffLayout);
    h.frame();
    let diff_scroll = h.visual.read(|cx| diff.read(cx).observed_scroll());
    assert!(diff_scroll.y < px(0.));
    let diff_position = h.body_position(diff.entity_id());
    h.scroll_at(diff_position, -96.);
    let diff_scroll = h.visual.read(|cx| diff.read(cx).observed_scroll());
    let diff_reading = h.visible_content(diff.entity_id());
    h.click(&terminal_key);
    h.terminal_poll(&mut driver);
    driver.output(
        &(0..140)
            .map(|row| format!("retained terminal {row}\r\n"))
            .collect::<String>()
            .into_bytes(),
    );
    h.terminal_poll(&mut driver);
    h.visual.simulate_keystrokes("enter");
    driver.drain();
    assert_eq!(
        driver.inputs(),
        &[b"\r".to_vec()],
        "active terminal input positive control"
    );
    h.visual.dispatch_action(ToggleDiffLayout);
    assert_eq!(
        h.visual.read(|cx| diff.read(cx).layout()),
        DiffLayout::Unified,
        "inactive Diff must not receive terminal actions"
    );
    let position = h.body_position(terminal.entity_id());
    h.scroll_at(position, 180.);
    h.terminal_poll(&mut driver);
    let terminal_scroll = h.visual.read(|cx| {
        terminal
            .read(cx)
            .observed_snapshot()
            .expect("terminal history")
            .scrollback
    });
    assert!(terminal_scroll > 0);
    let terminal_reading = h.visible_content(terminal.entity_id());
    h.visual.update(|_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("inactive-must-not-paste".into()))
    });
    let inputs = driver.inputs().to_vec();
    let sizes = driver.sizes().to_vec();
    for _ in 0..3 {
        h.click(&TabKey::File(1));
        h.assert_selected_file(0, 0);
        h.visual.simulate_keystrokes("enter");
        h.visual.dispatch_action(vega_ui::text_input::Paste);
        h.visual.dispatch_action(ToggleDiffLayout);
        driver.drain();
        assert_eq!(driver.inputs(), inputs);
        assert_eq!(
            h.visual.read(|cx| diff.read(cx).layout()),
            DiffLayout::Unified
        );
        assert_eq!(
            h.visual.read(|cx| file.read(cx).observed_scroll()),
            file_scroll
        );
        assert_eq!(h.visible_content(file.entity_id()), file_reading);
        h.click(&TabKey::Diff);
        h.visual.simulate_keystrokes("enter");
        h.visual.dispatch_action(vega_ui::text_input::Paste);
        driver.drain();
        assert_eq!(
            driver.inputs(),
            inputs,
            "inactive terminal must not receive Diff input"
        );
        assert_eq!(
            h.visual.read(|cx| diff.read(cx).observed_scroll()),
            diff_scroll
        );
        assert_eq!(h.visible_content(diff.entity_id()), diff_reading);
        h.visual.update(|window, cx| {
            assert!(diff.read(cx).focus_handle(cx).contains_focused(window, cx))
        });
        h.click(&terminal_key);
        h.frame();
        h.visual.update(|window, cx| {
            assert!(
                terminal
                    .read(cx)
                    .focus_handle(cx)
                    .contains_focused(window, cx)
            );
            assert_eq!(
                terminal
                    .read(cx)
                    .observed_snapshot()
                    .expect("same snapshot owner")
                    .scrollback,
                terminal_scroll
            );
        });
        assert_eq!(h.visible_content(terminal.entity_id()), terminal_reading);
        assert!(!driver.closed());
    }
    h.update(|owner, window, cx| owner.show_subagent_workspace(window, cx));
    h.frame();
    h.update(|owner, window, cx| {
        assert_eq!(owner.workspace.selected[0], Some(TabKey::Subagents));
        assert!(owner.subagent_panel.focus.contains_focused(window, cx));
        assert!(
            !terminal
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        );
    });
    h.visual.dispatch_action(ToggleDiffLayout);
    h.visual.dispatch_action(vega_ui::text_input::Paste);
    driver.drain();
    assert_eq!(driver.inputs(), inputs);
    assert_eq!(
        h.visual.read(|cx| diff.read(cx).layout()),
        DiffLayout::Unified
    );
    h.click(&terminal_key);
    assert_eq!(h.visible_content(terminal.entity_id()), terminal_reading);
    driver.drain();
    assert_eq!(
        driver.sizes(),
        sizes,
        "same-size selection must not recreate or resize sessions"
    );
    h.update(|owner, _, cx| {
        let TabKey::Terminal(id) = &terminal_key else {
            unreachable!()
        };
        assert_eq!(owner.workspace.terminals[id].view, terminal);
        assert_eq!(owner.workspace.files[&1], file);
        assert_eq!(
            owner
                .diff_controller
                .visible_view(&thread())
                .expect("retained controller"),
            diff
        );
        assert_eq!(cx.global::<PaintedBodies>().0, vec![terminal.entity_id()]);
    });
    h.click(&TabKey::File(1));
    let weak_diff = diff.downgrade();
    h.close(&TabKey::Diff);
    h.visual.update(|_, _| drop(diff));
    h.visual.run_until_parked();
    assert!(weak_diff.upgrade().is_none());
    let weak_file = file.downgrade();
    h.close(&TabKey::File(1));
    let fixture_file = h.files.remove(0);
    h.visual.update(|_, _| drop((file, fixture_file)));
    h.visual.run_until_parked();
    assert!(weak_file.upgrade().is_none());
    h.close(&terminal_key);
    h.visual.update(|_, _| drop(terminal));
    h.visual.run_until_parked();
    assert!(driver.closed());
}

#[gpui_kit::test]
fn text_close_and_focus_geometry_are_immediate_while_only_fill_interpolates(
    cx: &mut TestAppContext,
) {
    for dark in [false, true] {
        let mut h = Harness::new(cx, 3, dark, false);
        let owner = h.owner.entity_id();
        let old_text = h.assert_text(owner, "file-0.txt");
        let new_text = h.assert_text(owner, "file-1.txt");
        let close = h.painted(owner, "close-workspace-tab-File(2)");
        let bounds = h.tab(&TabKey::File(2)).bounds();
        h.visual.simulate_mouse_move(
            point(bounds.left() + px(18.), bounds.center().y),
            None,
            Modifiers::none(),
        );
        h.frame();
        h.assert_text(owner, "close-workspace-tab-File(2)");
        h.visual.simulate_click(
            point(bounds.left() + px(18.), bounds.center().y),
            Modifiers::none(),
        );
        h.pointer_out();
        h.update(|owner, window, cx| {
            window.focus(&owner.workspace.tab_focuses[&TabKey::File(3)], cx)
        });
        h.visual.simulate_keystrokes("shift-tab shift-tab");
        h.pointer_out();
        let ring = h.focus_ring(&TabKey::File(2));
        for millis in [0, 30, 30, 60] {
            if millis > 0 {
                h.advance(millis);
            }
            let old_now = h.assert_text(owner, "file-0.txt");
            let new_now = h.assert_text(owner, "file-1.txt");
            let close_now = h.assert_text(owner, "close-workspace-tab-File(2)");
            assert_eq!(old_now.bounds, old_text.bounds);
            assert_eq!(new_now.bounds, new_text.bounds);
            assert_eq!(close_now.bounds, close.bounds);
            h.visual.read(|cx| {
                assert_eq!(new_now.color, Hsla::from(theme(cx).colors.text_primary));
                assert_eq!(old_now.color, Hsla::from(theme(cx).colors.text_secondary));
            });
            assert_eq!(h.tab(&TabKey::File(2)).bounds(), bounds);
            assert_eq!(h.focus_ring(&TabKey::File(2)), ring);
        }
        h.update(|owner, window, cx| {
            window.focus(&owner.workspace.tab_focuses[&TabKey::File(3)], cx)
        });
        h.visual.simulate_keystrokes("shift-tab shift-tab");
        h.pointer_out();
        let focus_bounds = h.tab(&TabKey::File(2)).bounds();
        h.update(|owner, window, cx| {
            assert!(owner.workspace.tab_focuses[&TabKey::File(2)].contains_focused(window, cx));
            assert_eq!(owner.workspace.selected[0], Some(TabKey::File(2)));
        });
        h.assert_text(owner, "file-1.txt");
        h.assert_text(owner, "close-workspace-tab-File(2)");
        assert_eq!(h.focus_ring(&TabKey::File(2)), ring);
        assert_eq!(focus_bounds, bounds);
    }
}

#[gpui_kit::test]
fn inactivity_without_a_draw_then_quick_restore_adopts_latest_target(cx: &mut TestAppContext) {
    let mut h = Harness::new(cx, 3, true, false);
    h.click(&TabKey::File(2));
    h.advance(30);
    let running = h.strength(&TabKey::File(2));
    assert!(running > 0. && running < 1.);
    let before_inactive = h.visual.read(|cx| cx.global::<RenderCount>().0);
    h.visual.deactivate_window();
    h.visual.executor().advance_clock(Duration::from_millis(1));
    h.visual.update(|window, _| window.activate_window());
    h.visual.run_until_parked();
    h.visual.read(|cx| {
        assert_eq!(
            cx.global::<RenderCount>().0,
            before_inactive,
            "inactive/quick restore must have no intervening render"
        )
    });
    h.frame();
    h.assert_selected_file(1, 0);
    h.assert_strength(&TabKey::File(1), 0.);
    h.assert_strength(&TabKey::File(2), 1.);
    h.next_frame_callbacks();
    h.frame();
    assert_eq!(h.next_frame_callbacks(), 0);
    h.advance(240);
    h.assert_strength(&TabKey::File(2), 1.);
}

fn stream_start(
    stream: &Entity<ConversationStream>,
    id: &str,
    seq: u64,
    text: String,
    cx: &mut App,
) {
    stream.update(cx, |view, cx| {
        view.apply_event(
            ConversationEvent::MessageStarted {
                message_id: id.into(),
                seq,
            },
            cx,
        );
        view.apply_event(
            ConversationEvent::TextDelta {
                message_id: id.into(),
                delta: text,
            },
            cx,
        );
    });
}

fn stream_finish(stream: &Entity<ConversationStream>, id: &str, cx: &mut App) {
    stream.update(cx, |view, cx| {
        view.apply_event(
            ConversationEvent::MessageFinished {
                message_id: id.into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        )
    });
}

#[gpui_kit::test]
fn live_workspace_terminal_and_stream_keep_input_reading_and_business_updates_when_decoration_stops(
    cx: &mut TestAppContext,
) {
    for stop in ["complete", "reduce", "hide", "inactive"] {
        let mut h = Harness::new(cx, 3, true, false);
        let stream = h.add_stream();
        let retained_file = h.files[0].clone();
        h.visual.update(|_, cx| {
            for index in 0..12 {
                let id = format!("history-{index}");
                stream_start(
                    &stream,
                    &id,
                    index + 1,
                    format!("History {index}\n\n{}", "reading line\n\n".repeat(12)),
                    cx,
                );
                stream_finish(&stream, &id, cx);
            }
        });
        h.frame();
        let viewport = h.visual.read(|cx| stream.read(cx).observed_viewport());
        assert!(viewport.size.width > px(0.) && viewport.size.height > px(0.));
        h.visible_content(stream.entity_id());
        h.scroll_at(viewport.center(), 320.);
        let anchor = h.visual.read(|cx| stream.read(cx).scroll_anchor_snapshot());
        assert!(anchor.identity.is_some());
        assert!(
            !anchor.following_tail,
            "real wheel must suspend tail following"
        );
        let composer = h.visual.read(|cx| stream.read(cx).composer_input());
        h.visual
            .update(|window, cx| stream.update(cx, |view, cx| view.focus_composer(window, cx)));
        h.visual.simulate_input("draft preserved");
        let (terminal_key, terminal, mut driver) = h.add_terminal();
        h.click(&terminal_key);
        h.terminal_poll(&mut driver);
        driver.output(b"\x1b[2J\x1b[HLIVE_TERMINAL");
        h.terminal_poll(&mut driver);
        h.visual.simulate_keystrokes("enter");
        driver.drain();
        assert_eq!(driver.inputs(), &[b"\r".to_vec()]);
        h.visual.update(|_, cx| {
            stream_start(&stream, "live-message", 20, "live-stream-first".into(), cx)
        });
        h.click(&TabKey::File(1));
        h.advance(30);
        assert_eq!(
            h.status(0, &TabKey::File(1)),
            MotionStatus::Running,
            "sampler observation positive control"
        );
        assert!(h.strength(&TabKey::File(1)) > 0. && h.strength(&TabKey::File(1)) < 1.);
        driver.output(b"\r\nBUSINESS_WHILE_MOVING");
        h.visual.update(|_, cx| {
            stream.update(cx, |view, cx| {
                view.apply_event(
                    ConversationEvent::TextDelta {
                        message_id: "live-message".into(),
                        delta: "-during-motion".into(),
                    },
                    cx,
                )
            })
        });
        h.terminal_poll(&mut driver);
        match stop {
            "complete" => h.advance(120),
            "reduce" => {
                h.visual.update(|_, cx| cx.set_reduce_motion(true));
                h.frame();
            }
            "hide" => {
                h.update(|owner, window, cx| owner.workspace_hide(0, window, cx));
                h.frame();
            }
            "inactive" => {
                h.visual.deactivate_window();
                h.frame();
            }
            _ => unreachable!(),
        }
        if stop != "hide" {
            h.assert_stopped(0, &TabKey::File(1));
            h.assert_stopped(0, &terminal_key);
            h.assert_strength(&TabKey::File(1), 1.);
        } else {
            h.visual.read(|cx| {
                assert!(cx.global::<PaintedBodies>().0.is_empty());
                assert!(
                    cx.global::<SelectionSamples>()
                        .0
                        .values()
                        .all(Option::is_none),
                    "hidden tabs must not run a visual sampler"
                );
            });
        }
        driver.output(b"\r\nBUSINESS_AFTER_STOP");
        h.visual.update(|_, cx| {
            stream.update(cx, |view, cx| {
                view.apply_event(
                    ConversationEvent::TextDelta {
                        message_id: "live-message".into(),
                        delta: "-after-stop".into(),
                    },
                    cx,
                )
            })
        });
        h.terminal_poll(&mut driver);
        if stop != "hide" {
            h.assert_stopped(0, &TabKey::File(1));
            h.assert_stopped(0, &terminal_key);
        } else {
            h.visual.read(|cx| {
                assert!(
                    cx.global::<SelectionSamples>()
                        .0
                        .values()
                        .all(Option::is_none)
                )
            });
        }
        h.visible_content(stream.entity_id());
        h.visual.read(|cx| {
            let view = terminal.read(cx);
            let parsed: String = view
                .observed_snapshot()
                .expect("poll continues after decoration stops")
                .cells
                .iter()
                .flat_map(|row| row.iter().map(|cell| cell.text.as_str()))
                .collect();
            assert!(parsed.contains("BUSINESS_AFTER_STOP"));
            assert!(
                stream
                    .read(cx)
                    .observed_stream_text()
                    .contains("live-stream-first-during-motion-after-stop")
            );
            assert!(
                stream.read(cx).has_active_agent(),
                "business stream remains live"
            );
            let current = stream.read(cx).scroll_anchor_snapshot();
            assert_eq!(current.identity, anchor.identity);
            assert_eq!(current.message_id, anchor.message_id);
            assert!((current.offset_in_item_px - anchor.offset_in_item_px).abs() < 1.);
            assert!(!current.following_tail);
            assert_eq!(composer.read(cx).text(), "draft preserved");
        });
        if stop == "inactive" {
            h.visual.update(|window, _| window.activate_window());
            h.visual.run_until_parked();
        }
        if stop == "hide" {
            h.open_file(0);
        }
        h.frame();
        h.assert_selected_file(0, 0);
        h.assert_strength(&TabKey::File(1), 1.);
        h.visual.simulate_keystrokes("enter");
        h.visual.dispatch_action(vega_ui::text_input::Paste);
        driver.drain();
        assert_eq!(
            driver.inputs(),
            &[b"\r".to_vec()],
            "inactive terminal must not steal file input"
        );
        h.click(&terminal_key);
        h.terminal_poll(&mut driver);
        let output = h.visual.read(|cx| {
            cx.global::<PaintLog>()
                .0
                .iter()
                .find(|record| {
                    record.entity == terminal.entity_id()
                        && record.text.contains("BUSINESS_AFTER_STOP")
                })
                .expect("business output must actually paint on terminal restore")
                .text
                .to_string()
        });
        h.assert_text(terminal.entity_id(), &output);
        h.visual.update(|window, cx| {
            assert!(
                terminal
                    .read(cx)
                    .focus_handle(cx)
                    .contains_focused(window, cx)
            )
        });
        h.visual.simulate_keystrokes("enter");
        driver.drain();
        assert_eq!(driver.inputs(), &[b"\r".to_vec(), b"\r".to_vec()]);
        h.visual
            .update(|_, cx| stream_finish(&stream, "live-message", cx));
        h.visual
            .read(|cx| assert!(!stream.read(cx).has_active_agent()));
        h.visual
            .update(|window, cx| stream.update(cx, |view, cx| view.focus_composer(window, cx)));
        h.visual.simulate_input(" resumed");
        driver.drain();
        assert_eq!(
            driver.inputs(),
            &[b"\r".to_vec(), b"\r".to_vec()],
            "composer input must not leak to terminal"
        );
        h.visual.update(|window, cx| {
            assert!(
                composer
                    .read(cx)
                    .focus_handle(cx)
                    .contains_focused(window, cx)
            );
            assert_eq!(composer.read(cx).text(), "draft preserved resumed");
        });
        h.visual
            .dispatch_action(vega_ui::conversation_stream::ResumeTail);
        h.visual
            .update(|_, cx| stream.update(cx, |_, cx| cx.notify()));
        h.frame();
        h.assert_text(
            stream.entity_id(),
            "live-stream-first-during-motion-after-stop",
        );
        h.visual
            .read(|cx| assert!(stream.read(cx).scroll_anchor_snapshot().following_tail));
        h.update(|owner, _, _| {
            assert_eq!(owner.stream_view.as_ref().expect("same stream").1, stream);
            assert_eq!(owner.workspace.files[&1], retained_file);
        });
        assert!(!driver.closed());
    }
}
