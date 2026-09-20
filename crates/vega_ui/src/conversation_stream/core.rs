use super::*;

/// R59 §3 R1–R3: which level of the model picker is showing.
///
/// With tiers, the model button opens the slider and its title opens the list
/// (R59). Without tiers, the button opens the list directly (I68). The views
/// are **mutually exclusive by construction**. R57 mounted the
/// slider as the model menu's last child, which put both on screen at once
/// (R59 §1 D1/D2/D3); this single enum is what replaces that pair of
/// independent booleans.
///
/// One value means one mounted layer. `render_model_picker_layers` matches on
/// this enum and mounts exactly one floating layer, so no frame can ever hold
/// both (R59 A3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelPickerLevel {
    /// Nothing is mounted.
    #[default]
    Closed,
    /// The thinking-tier slider card for models with tiers (R59 R1).
    Slider,
    /// The model list, reached from the slider title or directly when tiers are absent.
    List,
}

impl ModelPickerLevel {
    /// Whether any level is mounted. The composer's trigger and its popover
    /// exclusivity both read this instead of the R57 boolean, and the
    /// application acceptance harness reads it through the public accessor.
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Closed)
    }
}

/// The opened-thread content view: thread header (title and trusted actions),
/// the virtualized message stream, and the fixed-bottom Composer. One entity
/// per open thread; rebuilt by the window root when another thread opens.
pub struct ConversationStream {
    pub(crate) context_control: context_control::ContextControl,
    pub(crate) attachments: Vec<(u64, attachments::ImagePreview)>,
    pub(crate) attachment_generation: u64,
    pub(crate) attachment_import_pending: bool,
    pub(crate) attachment_error: Option<&'static str>,
    pub(crate) submitted_attachments: Vec<(u64, attachments::ImagePreview)>,
    pub(crate) thread: Thread,
    /// 消息块列表（T18）：user 回显与 assistant 流交替，顺序即会话顺序。
    pub(crate) entries: Vec<StreamEntry>,
    pub(crate) counters: Arc<StreamCounters>,
    /// Variable-height list state (S8-T44/C4): one item per semantic entry,
    /// natural heights. The same state owns the scroll position and the P4
    /// tail-follow semantics (`FollowMode::Tail`).
    pub(crate) list: gpui_kit::ListState,
    /// Active demo injection (`None` = idle/finished).
    pub(crate) injecting: Option<InjectionState>,
    /// Composer 输入状态（独立 `TextInput` Entity，1–8 行自适应多行）。
    pub(crate) input: Entity<TextInput>,
    /// Synthetic block-id counter for user echo rows (diagnostics only).
    pub(crate) user_block_seq: u64,
    /// Opaque provider call ids are retained only as non-rendered map keys.
    pub(crate) tool_cards: HashMap<String, Entity<ToolCard>>,
    /// Exact call id to its sole inline artifact card.
    pub(crate) artifact_cards: HashMap<String, Entity<ArtifactCard>>,
    /// Route-owned safe branch selector; Git authority remains in the app controller.
    pub(crate) branch_selector: Entity<BranchSelector>,
    /// IO-free canonical commit panel; repository authority stays in app/headless.
    pub(crate) commit_panel: Entity<CommitPanel>,
    /// Concrete runtime permission hook shared by the owning conversation.
    pub(crate) permission_queue: PermissionQueue,
    /// Request retained after the queue listener wakes before its durable
    /// ToolCallProposed event reaches this stream.
    pub(crate) deferred_permission: Option<PendingPermission>,
    /// The sole visible prompt; the opaque call id is only a map association.
    pub(crate) active_permission: Option<Entity<PermissionCard>>,
    /// Opaque call id paired with `active_permission` for exact terminal
    /// cleanup. PermissionCard deliberately stores no provider call id.
    pub(crate) active_permission_call_id: Option<String>,
    /// Plan ids are opaque map keys; card content is a typed projection.
    pub(crate) plan_cards: HashMap<String, Entity<PlanCard>>,
    /// Sole applied per-task cost summary keyed by assistant message id
    /// (S7-T40); duplicate/later stale applications are ignored.
    pub(crate) summary_cards: HashMap<String, Entity<SummaryCard>>,
    /// Scroll-up hydration state (S8-T45/C7): keyset cursor of the oldest
    /// loaded page, in-flight flag, and the failure pause. The stream itself
    /// never queries SQLite — pages arrive as typed projections.
    pub(crate) hydration: HistoryHydration,
    /// Exact active durable assistant id and its stream-entry index.
    pub(crate) active_agent_message: Option<(String, usize)>,
    /// Whether the current Markdown segment contains text; a tool boundary
    /// may leave the active run without a segment until the next text delta.
    pub(crate) active_segment_has_text: bool,
    pub(crate) active_thinking: Option<Entity<ThinkingBlock>>,
    pub(crate) thinking_bytes: usize,
    pub(crate) thinking_blocks: usize,
    /// Most recently finished assistant entry, retained until the typed Plan
    /// projection can replace it in place.
    pub(crate) last_finished_agent_message: Option<(String, usize)>,
    /// Bounded token/cost meter projection (S7-T39/C3/C4). Pure shared
    /// `vega_conversation::types` state: no IO, no persistence; the Composer
    /// renders its snapshot and every update is checked arithmetic only.
    pub(crate) meter: ConversationMeter,
    /// Submitted drafts, scoped to this thread view.
    pub(crate) composer_history: Vec<String>,
    pub(crate) composer_submit_pending: bool,
    pub(crate) skill_projection: Option<SkillComposerProjection>,
    pub(crate) skill_projection_generation: u64,
    pub(crate) skill_projection_loading: bool,
    pub(crate) skill_picker_open: bool,
    pub(crate) skill_mutation_pending: bool,
    pub(crate) skill_intent: Option<SkillSelectionIntent>,
    pub(crate) active_skills: Vec<vega_conversation::types::ActiveSkillView>,
    pub(crate) actions: ComposerActions,
    pub(crate) action_focus: [FocusHandle; 2],
    pub(crate) history_cursor: Option<usize>,
    pub(crate) history_draft: Option<String>,
    pub(crate) approved_not_started: bool,
    pub(crate) trusted_action_busy: bool,
    pub(crate) controller_error: Option<String>,
    /// Nonfatal run-start MCP omission. Kept separate so a provider failure
    /// cannot overwrite it or be overwritten by it.
    pub(crate) mcp_warning: Option<String>,
    /// Bounded `@file` selector model (A2-12). Pure UI state over the typed
    /// [`FileIndexSnapshot`]; the app layer owns the filesystem walk.
    pub(crate) file_selector: FileSelectorModel,
    /// The latest bounded candidate projection for this thread's project.
    pub(crate) file_snapshot: FileIndexSnapshot,
    /// One bounded walk in flight / completed bookkeeping (one request per
    /// stream lifetime; later opens re-filter the projection locally).
    pub(crate) file_index_loading: bool,
    pub(crate) file_index_loaded: bool,
    /// Monotonic owner fence for index requests and their late results.
    pub(crate) file_index_generation: u64,
    /// Whether the current `@` token still wants a visible selector. This is
    /// separate from the input query so Esc/route close cannot be undone by a
    /// late worker result.
    pub(crate) file_selector_wanted: bool,
    /// A failed index stays visible as a retryable state until the user leaves
    /// the token or starts a new retry.
    pub(crate) file_index_failure: Option<FileIndexFailureCode>,
    /// Keyboard stop for the visible failed-index Retry action.
    pub(crate) file_retry_focus: FocusHandle,
    /// Provider/model/thinking composer defaults (A2-14). Display state for
    /// the selector; authority is the app-level config seam.
    pub(crate) composer_defaults: ComposerDefaults,
    /// R57 P3: the thinking-tier slider (P2a), shown as a picker layer when tiers exist.
    /// It reads the exact `(provider, model)` profile the stream already
    /// received through [`Self::apply_reasoning_profile`]; this entity owns no
    /// store handle and performs no IO.
    pub(crate) thinking_slider: Entity<ThinkingSlider>,
    /// Priced model options for the selector (from the T36 pricing catalog
    /// via the app layer's typed projection; zero file IO here).
    pub(crate) model_options: Vec<String>,
    /// R59 R1–R3: which model-picker level is mounted, if any.
    ///
    /// A single three-state value replaces R57's `model_selector_open`
    /// boolean, so the tier slider and the model list are mutually exclusive
    /// by construction: `render_model_picker_layers` matches on this and
    /// mounts exactly one floating layer per frame (R59 A3).
    pub(crate) model_picker_level: ModelPickerLevel,
    /// Scroll state for the model list. The list carries an
    /// explicit `max_h` (R59 R5), so a long catalog scrolls inside the layer
    /// and the highlighted row is kept in view during keyboard navigation.
    pub(crate) model_menu_scroll: ScrollHandle,
    pub(crate) compact_workspace: bool,
    pub(crate) project_label: String,
    /// Window-owned R69 draft route, never inferred from a durable `Thread`.
    pub(crate) draft_route: bool,
    /// R49 utility-bar project menu rows (`(id, name)`), loaded from the same
    /// store query the sidebar's `ProjectsBlock` renders and only while the
    /// folder chip's menu is being opened — never during a frame.
    pub(crate) utility_projects: Vec<(String, String)>,
    /// Whether the R49 folder chip's project menu is visible.
    pub(crate) utility_projects_open: bool,
    /// R62 R10: the folder chip's menu filter query. Display-only state for
    /// the visible rows — selecting a row still means "switch to it", filtered
    /// or not (R62 R11).
    pub(crate) utility_project_query: String,
    /// R62 R10: the folder chip's menu search field. An entity rather than a
    /// string because the field is a real editable input with IME support;
    /// `utility_project_query` mirrors its text for the row filter.
    pub(crate) utility_project_search: Entity<TextInput>,
    /// R62 R7: whether the bottom row's permission picker is mounted. The
    /// second entry (`+` menu) and this one share the same request path and
    /// the same `thread.permission_mode`, so the two can never disagree.
    pub(crate) permission_picker_open: bool,
    pub(crate) model_selector_highlight: usize,
    /// Keyboard focus stop for the model selector trigger (A2-14).
    pub(crate) model_focus: FocusHandle,
    /// In-flight in-session model selection (R1): `Some((request_id, model))`
    /// while persistence is in flight. Blocks submit/thinking until the
    /// exact-owner acknowledgement or failure lands.
    pub(crate) model_selection_pending: Option<(u64, String)>,
    /// Exact app-level owner for the in-flight model save. This is separate
    /// from `trusted_action_busy`, which also represents branch, commit, and
    /// artifact operations.
    pub(crate) model_selection_save_owner: Option<u64>,
    /// Monotonic request-id counter for model-selection intents (R1). The
    /// app echoes it back so late callbacks can never release another
    /// request's owner.
    pub(crate) next_model_request_id: u64,
    /// Keyboard-accessible return-to-tail action shown after the user scrolls
    /// away from the live bottom (P4).
    pub(crate) resume_tail_focus: FocusHandle,
    /// Cancels the watch listener and drops its fail-closed guard with the view.
    pub(crate) _permission_listener_task: gpui_kit::Task<()>,
}

impl EventEmitter<PlanReviewRequested> for ConversationStream {}
impl EventEmitter<ThreadSettingsRequested> for ConversationStream {}
impl EventEmitter<ComposerSubmitted> for ConversationStream {}
impl EventEmitter<SkillComposerProjectionRequested> for ConversationStream {}
impl EventEmitter<SkillComposerMutationRequested> for ConversationStream {}
impl EventEmitter<ContextSettingsRequested> for ConversationStream {}
impl EventEmitter<ContextCompactionRequested> for ConversationStream {}
impl EventEmitter<ContextCompactionCancelRequested> for ConversationStream {}
impl EventEmitter<OpenWorkspaceDiffRequested> for ConversationStream {}
impl EventEmitter<OpenCommitPanelRequested> for ConversationStream {}
impl EventEmitter<WorkspaceToolTerminal> for ConversationStream {}
impl EventEmitter<HistoryPageRequested> for ConversationStream {}
impl EventEmitter<FileIndexRequested> for ConversationStream {}
impl EventEmitter<FileIndexCancelled> for ConversationStream {}
impl EventEmitter<ComposerStopRequested> for ConversationStream {}
impl EventEmitter<FileIndexRetryRequested> for ConversationStream {}
impl EventEmitter<ComposerDefaultsRequested> for ConversationStream {}
impl EventEmitter<ThreadModelSelectionRequested> for ConversationStream {}

pub(crate) struct InjectionState {
    /// Which assistant entry the replayer feeds.
    pub(crate) entry_index: usize,
    /// The public mock replayer (vega_markdown::replay，T18 公共化).
    pub(crate) replay: MockReplay,
}

impl ConversationStream {
    /// Projects the host column width without rebuilding composer state.
    pub fn set_workspace_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let compact = width < 500.;
        if self.compact_workspace != compact {
            self.compact_workspace = compact;
            cx.notify();
        }
    }

    /// Builds the view for `thread` with an empty in-memory stream (S3 无消息
    /// 持久化：会话内容由流式注入与 Composer 回显产生，不落库；重启后清空
    /// 是预期行为).
    pub fn new(thread: Thread, cx: &mut Context<Self>) -> Self {
        Self::new_with_permission_queue(thread, PermissionQueue::new(), cx)
    }

    /// Builds a stream with the exact permission queue passed to the runtime.
    pub fn new_with_permission_queue(
        thread: Thread,
        permission_queue: PermissionQueue,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new_multiline(cx, "描述任务，或用 @ 引用文件", COMPOSER_ROWS)
                .with_image_paste()
        });
        let branch_selector = cx.new(|cx| {
            let mut selector =
                BranchSelector::new(thread.id.clone(), thread.project_id.clone(), cx);
            // R49: the composer utility bar is this selector's only mount
            // point, so it always renders the borderless chip chrome.
            selector.set_chip_chrome(true);
            selector
        });
        // C2: the branch request is the production boundary for opening its
        // popup. Close sibling Composer surfaces here, but deliberately do
        // not call `close_composer_popovers` on the branch entity itself —
        // this subscription runs because that entity just opened and must not
        // re-close it or disturb its pending-operation ownership.
        cx.subscribe(&branch_selector, |this, _, _: &BranchListRequested, cx| {
            this.close_composer_popovers(cx);
            cx.notify();
        })
        .detach();
        let commit_panel =
            cx.new(|cx| CommitPanel::new(thread.id.clone(), thread.project_id.clone(), cx));
        // R62 R10: the folder chip's menu search field. Bare chrome, because
        // the menu row around it draws the surface; the query is mirrored into
        // `utility_project_query` and only filters the visible rows.
        let utility_project_search =
            cx.new(|cx| TextInput::new(cx, "搜索项目", false).with_bare_chrome());
        cx.observe(&utility_project_search, |this, input, cx| {
            this.sync_utility_project_query(&input, cx);
        })
        .detach();
        // 空输入禁用发送 + `@` 触发的文件选择跟随输入内容变化：输入内容
        // 变化即重渲染 Composer。
        cx.observe(&input, |this, input, cx| {
            this.sync_composer_actions(&input, cx);
            this.sync_at_query(&input, cx);
        })
        .detach();
        cx.subscribe(
            &input,
            |this, _, event: &crate::text_input::ImagePaste, cx| {
                this.paste_images(event.0.clone(), cx);
            },
        )
        .detach();
        let mut listener = permission_queue.subscribe();
        let permission_listener_task = cx.spawn(async move |this, cx| {
            while listener.changed().await {
                let alive = this
                    .update(cx, |this, cx| this.install_pending_permission(cx))
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });
        cx.observe_global::<SettingsOpen>(|this, cx| {
            if cx
                .try_global::<SettingsOpen>()
                .is_some_and(|settings| settings.0)
            {
                this.timeout_permission(cx);
            }
        })
        .detach();
        cx.observe_global::<crate::sidebar::OpenedThread>(|this, cx| {
            if cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| {
                    route
                        .0
                        .as_ref()
                        .is_none_or(|thread| thread.id != this.thread.id)
                })
            {
                this.cancel_image_import(cx);
            }
        })
        .detach();
        // One item per semantic entry at natural height; Tail follow is the
        // P4 anchor (贴底跟随 / 上翻 detach / 回底 resume)，由列表原生承担
        // （任何上滚事件 detach，回到距底 1px 内恢复——容差与旧锚定状态机
        // 一致）。600px overdraw 保证滚动方向切换时前后各一屏已测量。
        let list = gpui_kit::ListState::new(0, gpui_kit::ListAlignment::Top, px(600.0));
        list.set_follow_mode(gpui_kit::FollowMode::Tail);
        let initial_model = thread.model.clone();
        // R57 P3: the tier slider starts with the durable thread model and no
        // tiers. The exact profile arrives later through
        // `apply_reasoning_profile` (the same app worker that already feeds
        // the composer's reasoning state), so no capability is guessed here.
        // R58: no Off position either, for the same reason — R2 requires a
        // declared disabled capability before the position may appear.
        let thinking_slider =
            cx.new(|cx| ThinkingSlider::new(initial_model.clone(), Vec::new(), false, "", "", cx));
        // The slider owns its knob and emits the user's intent; the stream
        // routes that intent onto the existing `ComposerDefaultsRequested`
        // save path rather than persisting anything itself.
        cx.subscribe(
            &thinking_slider,
            |this, _, event: &ThinkingTierSelected, cx| {
                this.apply_thinking_tier_selected(event, cx);
            },
        )
        .detach();
        // R59 R2/R3: the slider's title row is the only entry to level two.
        cx.subscribe(
            &thinking_slider,
            |this, _, _: &ThinkingSliderTitleActivated, cx| {
                this.on_thinking_slider_title_activated(&ThinkingSliderTitleActivated, cx);
            },
        )
        .detach();
        Self {
            context_control: context_control::ContextControl::new(cx),
            attachments: Vec::new(),
            attachment_generation: 0,
            attachment_import_pending: false,
            attachment_error: None,
            submitted_attachments: Vec::new(),
            thread,
            entries: Vec::new(),
            counters: Arc::new(StreamCounters::default()),
            list,
            injecting: None,
            input,
            user_block_seq: USER_BLOCK_BASE,
            tool_cards: HashMap::new(),
            artifact_cards: HashMap::new(),
            branch_selector,
            commit_panel,
            permission_queue,
            deferred_permission: None,
            active_permission: None,
            active_permission_call_id: None,
            plan_cards: HashMap::new(),
            summary_cards: HashMap::new(),
            hydration: HistoryHydration::default(),
            active_agent_message: None,
            active_thinking: None,
            thinking_bytes: 0,
            thinking_blocks: 0,
            active_segment_has_text: false,
            last_finished_agent_message: None,
            meter: ConversationMeter::default(),
            composer_history: Vec::new(),
            composer_submit_pending: false,
            skill_projection: None,
            skill_projection_generation: 0,
            skill_projection_loading: false,
            skill_picker_open: false,
            skill_mutation_pending: false,
            skill_intent: None,
            active_skills: Vec::new(),
            actions: ComposerActions::default(),
            action_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            history_cursor: None,
            history_draft: None,
            approved_not_started: false,
            trusted_action_busy: false,
            controller_error: None,
            mcp_warning: None,
            file_selector: FileSelectorModel::default(),
            file_snapshot: FileIndexSnapshot::default(),
            file_index_loading: false,
            file_index_loaded: false,
            file_index_generation: 0,
            file_selector_wanted: false,
            file_index_failure: None,
            file_retry_focus: cx.focus_handle().tab_stop(true),
            // R1: a reopened stream starts from the durable thread model so
            // its first rendered chip cannot lag behind `threads.model`.
            composer_defaults: ComposerDefaults {
                model: initial_model,
                thinking: "provider_default".to_string(),
                reasoning: None,
                reasoning_unavailable: false,
            },
            thinking_slider,
            model_options: Vec::new(),
            model_picker_level: ModelPickerLevel::Closed,
            model_menu_scroll: ScrollHandle::new(),
            compact_workspace: false,
            project_label: String::new(),
            draft_route: false,
            utility_projects: Vec::new(),
            utility_projects_open: false,
            utility_project_query: String::new(),
            utility_project_search,
            permission_picker_open: false,
            model_selector_highlight: 0,
            model_focus: cx.focus_handle().tab_index(16).tab_stop(true),
            model_selection_pending: None,
            model_selection_save_owner: None,
            next_model_request_id: 0,
            resume_tail_focus: cx.focus_handle().tab_index(18).tab_stop(true),
            _permission_listener_task: permission_listener_task,
        }
    }

    // ── variable-height list maintenance (S8-T44/C4) ────────────────────────
    //
    // One list item per semantic entry. Structural entry mutations notify the
    // list through `splice`; content mutations through `remeasure_items`
    // (Absolute scroll anchor: the pixel offset into the scroll-top item is
    // preserved, keeping anchor drift <1px). Frozen items are never touched.

    /// Registers `entries.len() - previous_len` appended items with the list.
    pub(crate) fn list_append(&mut self, previous_len: usize) {
        let count = self.entries.len();
        debug_assert!(count >= previous_len);
        if count > previous_len {
            self.list
                .splice(previous_len..previous_len, count - previous_len);
        }
    }

    /// Registers one item inserted at `index` (e.g. an inline artifact after
    /// its exact tool).
    pub(crate) fn list_insert(&mut self, index: usize) {
        self.list.splice(index..index, 1);
    }

    /// Registers `count` items prepended above the loaded history
    /// (S8-T45/C7). `splice` shifts `logical_scroll_top` by the insert
    /// count while keeping the pixel offset into the scroll-top item, so
    /// the page-boundary anchor is exact (drift <1px by construction).
    pub(crate) fn list_prepend(&mut self, count: usize) {
        if count > 0 {
            self.list.splice(0..0, count);
        }
    }

    /// Registers the removal of the item at `index` (permission resolution).
    pub(crate) fn list_remove(&mut self, index: usize) {
        self.list.splice(index..index + 1, 0);
    }

    /// Marks the item at `index` for height re-measurement (mutable tail or
    /// explicitly invalidated item; the C4 rematerialize whitelist).
    pub(crate) fn invalidate_item(&mut self, index: Option<usize>) {
        if let Some(index) = index
            && index < self.list.item_count()
        {
            self.list.remeasure_items(index..index + 1);
        }
    }

    /// Entry index matching `predicate`, for in-place card invalidation.
    pub(crate) fn entry_index_where(
        &self,
        predicate: impl Fn(&StreamEntry) -> bool,
    ) -> Option<usize> {
        self.entries.iter().position(predicate)
    }

    /// Whether the list is currently following the tail (the P4 anchor
    /// state, read by the header indicator).
    pub(crate) fn following_tail(&self) -> bool {
        self.list.is_following_tail()
    }

    /// Re-engages the native list tail-follow mode after the user has scrolled
    /// away from the live bottom. The list owns the scroll anchor and keeps the
    /// next layout pinned to the newest entry.
    pub(crate) fn resume_tail(&mut self, cx: &mut Context<Self>) {
        self.list.set_follow_mode(gpui_kit::FollowMode::Tail);
        cx.notify();
    }

    /// Applies the authoritative persisted thread settings after a request.
    /// The composer's displayed model always mirrors the durable
    /// `threads.model` (R1): the selection chip is a projection of the
    /// authoritative thread, never an independent optimistic value.
    pub fn apply_thread(&mut self, thread: Thread, cx: &mut Context<Self>) {
        if thread.id == self.thread.id && thread.project_id == self.thread.project_id {
            if thread.model != self.thread.model {
                self.reset_context_control(cx);
            }
            if !thread.model.is_empty() {
                self.composer_defaults.model = thread.model.clone();
            }
            self.thread = thread;
            self.acknowledge_mode_action(cx);
            self.controller_error = None;
            cx.notify();
        }
    }

    /// Projects the window's draft status without changing a durable thread.
    /// The app root owns this bit because `Thread` mirrors the database row.
    pub fn set_draft_route(&mut self, draft: bool, cx: &mut Context<Self>) {
        if self.draft_route != draft {
            self.draft_route = draft;
            cx.notify();
        }
    }

    /// Project identity of the route currently projected by this cached view.
    pub fn route_project_id(&self) -> &str {
        &self.thread.project_id
    }

    /// Rebinds an unmaterialized draft in place. Its stable stream and input
    /// entities retain text, focus, model and settings; only project-scoped
    /// controllers and cached data are invalidated before the new route runs.
    pub fn rebind_draft_project(
        &mut self,
        thread: Thread,
        label: String,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.draft_route || thread.id != self.thread.id || !self.entries.is_empty() {
            return false;
        }
        if thread.project_id == self.thread.project_id {
            return true;
        }
        self.close_composer_popovers(cx);
        self.clear_skill_route_state();
        self.branch_selector.update(cx, |selector, cx| {
            selector.rebind_draft_project(&thread.project_id, cx);
        });
        self.commit_panel.update(cx, |panel, cx| {
            panel.rebind_draft_project(&thread.project_id, cx);
        });
        self.invalidate_file_index(cx);
        self.thread = thread;
        self.project_label = label;
        cx.notify();
        true
    }

    /// Projects only a renamed title onto the cached route entity. The window
    /// root uses this when Sidebar updates `OpenedThread`; keeping the update
    /// field-scoped preserves the R1 model authority and any pending owner.
    pub fn apply_thread_title(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        if thread.id != self.thread.id || thread.project_id != self.thread.project_id {
            return;
        }
        if self.thread.title != thread.title {
            self.thread.title.clone_from(&thread.title);
            cx.notify();
        }
    }

    /// Returns the model currently shown by the Composer selector. It is a
    /// projection of the durable thread model after an acknowledged update.
    pub fn displayed_model(&self) -> &str {
        &self.composer_defaults.model
    }

    /// Whether the exact pending selection owns this acknowledgement. The
    /// app handler checks this before publishing any global/thread projection;
    /// a late callback therefore cannot clear a newer request or overwrite a
    /// route with an unrelated authoritative row.
    pub fn model_selection_ack_is_valid(&self, request_id: u64, thread: &Thread) -> bool {
        self.model_selection_pending
            .as_ref()
            .is_some_and(|(owner_id, requested_model)| {
                *owner_id == request_id
                    && requested_model == &thread.model
                    && thread.id == self.thread.id
                    && thread.project_id == self.thread.project_id
            })
    }

    /// Clears the exact selection owner after a callback that cannot be
    /// applied to this stream (for example, after a route switch). It does
    /// not manufacture a failure for a hidden/stale entity.
    pub fn clear_model_selection_owner(&mut self, request_id: u64, cx: &mut Context<Self>) {
        let Some((owner_id, _)) = self.model_selection_pending.as_ref() else {
            return;
        };
        if *owner_id != request_id {
            return;
        }
        self.model_selection_pending = None;
        self.clear_model_selection_save_owner(request_id, cx);
        cx.notify();
    }

    /// Installs the exact app-level owner after the trusted-action lease has
    /// been acquired. A generic trusted-action busy flag never installs this
    /// owner, so a model event can distinguish a true duplicate from a
    /// request that must be rejected.
    pub fn install_model_selection_save_owner(
        &mut self,
        request_id: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.model_selection_save_owner.is_some()
            || !self.owns_model_selection_request(request_id)
        {
            return false;
        }
        self.model_selection_save_owner = Some(request_id);
        self.set_trusted_action_busy(true, cx);
        true
    }

    /// Releases only this request's model-save owner. If no exact owner is
    /// installed, the generic trusted-action busy state is left untouched.
    fn clear_model_selection_save_owner(
        &mut self,
        request_id: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.model_selection_save_owner != Some(request_id) {
            return false;
        }
        self.model_selection_save_owner = None;
        self.set_trusted_action_busy(false, cx);
        true
    }

    /// Projects a durable thread read onto a newly-created route entity. This
    /// path is deliberately separate from request acknowledgement: the new
    /// entity has no pending owner, but it still must converge to the worker's
    /// authoritative row after A→B→A.
    pub fn apply_authoritative_thread(&mut self, thread: Thread, cx: &mut Context<Self>) {
        if self.model_selection_pending.is_some() {
            return;
        }
        self.apply_thread(thread, cx);
    }

    /// Applies the model field to the newest route projection. The caller
    /// supplies that projection so a late model result cannot replace a title,
    /// pin, status, mode, or permission changed since the save began.
    pub fn apply_authoritative_model(
        &mut self,
        current_thread: Thread,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        if self.model_selection_pending.is_some()
            || current_thread.id != self.thread.id
            || current_thread.project_id != self.thread.project_id
        {
            return;
        }
        let mut projected = current_thread;
        projected.model = model.to_owned();
        if model != self.thread.model {
            self.reset_context_control(cx);
        }
        self.thread = projected;
        self.composer_defaults.model = model.to_owned();
        self.controller_error = None;
        cx.notify();
    }

    /// Seeds thread-scoped Composer history from the typed conversation
    /// projection. This is called once when a stream entity is constructed.
    pub fn apply_composer_history(
        &mut self,
        thread_id: &str,
        history: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if thread_id != self.thread.id || self.composer_submit_pending {
            return;
        }
        self.composer_history = history;
        self.history_cursor = None;
        self.history_draft = None;
        cx.notify();
    }

    /// Displays a bounded controller failure without changing authoritative
    /// selected state.
    pub fn apply_controller_error(&mut self, cx: &mut Context<Self>) {
        self.actions.pending_mode = None;
        self.controller_error = Some("操作未保存，请重试".into());
        cx.notify();
    }

    /// Rejects request preparation while preserving the draft for credential repair.
    pub fn apply_credential_error(&mut self, cx: &mut Context<Self>) {
        self.reject_composer_submission(cx);
        self.controller_error =
            Some("本地凭据缺失或无法读取，请在设置中重新填写 API Key 后重试".into());
        cx.notify();
    }

    /// Displays a typed provider-readiness rejection from the preflight
    /// worker. The draft/input remain untouched because this runs before a
    /// durable thread row or agent run exists.
    pub fn apply_provider_preflight_error(
        &mut self,
        failure: ProviderPreflightFailure,
        cx: &mut Context<Self>,
    ) {
        self.reject_composer_submission(cx);
        self.controller_error = Some(failure.message());
        cx.notify();
    }

    /// Applies an already-loaded project display label without filesystem IO.
    pub fn set_project_label(&mut self, label: String, cx: &mut Context<Self>) {
        self.project_label = label;
        cx.notify();
    }

    /// Projects the exact provider/model capability loaded by the app worker
    /// onto this route. A profile refresh also resets the displayed choice to
    /// its persisted preference, keeping model switches deterministic.
    ///
    /// R57 P3: the same projection feeds the tier slider, so the card and the
    /// composer always name one model and one tier list.
    pub fn apply_reasoning_profile(
        &mut self,
        profile: ReasoningProfileProjection,
        cx: &mut Context<Self>,
    ) {
        self.composer_defaults.reasoning = Some(profile.clone());
        self.composer_defaults.reasoning_unavailable = false;
        self.composer_defaults.thinking = reasoning_choice_name(&profile.preference);
        self.sync_thinking_slider(cx);
        cx.notify();
    }

    /// Clears a stale provider/model capability projection when the current
    /// model has no exact profile. A missing profile is provider-default with
    /// no controls; retaining the previous model's profile would send a
    /// capability declaration to the wrong model.
    pub fn clear_reasoning_profile(&mut self, cx: &mut Context<Self>) {
        self.composer_defaults.reasoning = None;
        self.composer_defaults.reasoning_unavailable = false;
        self.composer_defaults.thinking = "provider_default".to_string();
        self.sync_thinking_slider(cx);
        cx.notify();
    }

    /// Marks the independent reasoning authority unavailable. A later
    /// provider-default fallback is allowed only after an explicit reload
    /// establishes a valid authority or a valid missing profile.
    pub fn mark_reasoning_unavailable(&mut self, cx: &mut Context<Self>) {
        self.composer_defaults.reasoning = None;
        self.composer_defaults.reasoning_unavailable = true;
        self.composer_defaults.thinking = "provider_default".to_string();
        self.sync_thinking_slider(cx);
        cx.notify();
    }

    /// R57 P3: re-projects the model name, the model's declared tiers, and the
    /// current/default tier onto the slider.
    ///
    /// The tiers are exactly the profile's own `efforts` (R7) — a model that
    /// declares three tiers renders three dots, never the reference's fixed
    /// seven. A profile that declares no legal effort vocabulary
    /// (`Unsupported`/`Unknown`, or an empty list) renders nothing (R12).
    /// The name and the tier list come from the same projection, so the card
    /// cannot name one model while showing another model's tiers.
    ///
    /// R58: the Off position is a *view* flag, not a tier. `supports_off`
    /// carries the same `supports_disabled` + `disabled_wire` pairing the
    /// store and runtime validators enforce, so the slider can only offer Off
    /// where the wire can actually carry it (R2). `current` stays the persisted
    /// preference string, so `"disabled"` selects the Off position (R4).
    pub(crate) fn sync_thinking_slider(&mut self, cx: &mut Context<Self>) {
        let (model_name, tiers, supports_off, default_tier) =
            match self.composer_defaults.reasoning.as_ref() {
                Some(profile) => (
                    profile.model.clone(),
                    declared_tiers(profile.support, &profile.efforts),
                    supports_off_position(profile),
                    preferred_tier_name(&profile.preference)
                        .unwrap_or_default()
                        .to_string(),
                ),
                None => (
                    self.composer_defaults.model.clone(),
                    Vec::new(),
                    false,
                    String::new(),
                ),
            };
        let current = self.composer_defaults.thinking.clone();
        self.thinking_slider.update(cx, |slider, cx| {
            slider.set_model_and_tiers(model_name, tiers, supports_off, current, default_tier, cx);
        });
        if self.model_picker_level == ModelPickerLevel::Slider
            && !self.thinking_slider.read(cx).has_tiers()
        {
            self.model_picker_level = ModelPickerLevel::Closed;
        }
    }

    /// R57 P3: routes one slider selection onto the existing
    /// `ComposerDefaultsRequested` save path.
    ///
    /// The event is refused unless it names the profile the composer currently
    /// projects *and* a choice that profile actually permits, so an invalid
    /// preference can never reach the reasoning authority or a later run's
    /// frozen snapshot. Nothing is persisted here: the app handler owns the
    /// durable write and re-projects the authoritative profile back through
    /// [`Self::apply_reasoning_profile`].
    ///
    /// R58: the event carries a persisted *choice name*. [`OFF_CHOICE_NAME`]
    /// (`"disabled"`) is accepted only when the profile declares the disabled
    /// operation (R2) — the same pairing the store validator enforces — and is
    /// stored as the preference string `"disabled"`, never as an effort.
    fn apply_thinking_tier_selected(
        &mut self,
        event: &ThinkingTierSelected,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = self.composer_defaults.reasoning.as_ref() else {
            return;
        };
        if profile.model != event.model || self.composer_defaults.thinking == event.choice {
            return;
        }
        let accepted = if event.choice == OFF_CHOICE_NAME {
            supports_off_position(profile)
        } else {
            profile.efforts.iter().any(|effort| effort == &event.choice)
        };
        if !accepted {
            return;
        }
        self.composer_defaults.thinking = event.choice.clone();
        cx.emit(ComposerDefaultsRequested {
            thread_id: self.thread.id.clone(),
            defaults: self.composer_defaults.clone(),
        });
        cx.notify();
    }

    /// Captures the displayed provider/model selection for one run. The app
    /// receives only identifiers and capability metadata; reasoning text is
    /// created later by the runtime and never crosses this UI event.
    pub fn frozen_reasoning_for_submit(
        &self,
    ) -> Result<Option<FrozenReasoning>, ReasoningSubmitError> {
        if self.composer_defaults.reasoning_unavailable {
            return Err(ReasoningSubmitError::Unavailable);
        }
        let Some(profile) = self.composer_defaults.reasoning.as_ref() else {
            return Ok(None);
        };
        let mut frozen = profile
            .freeze()
            .map_err(|_| ReasoningSubmitError::Invalid)?;
        frozen.choice = reasoning_choice_for_name(&self.composer_defaults.thinking);
        frozen
            .validate()
            .map_err(|_| ReasoningSubmitError::Invalid)?;
        Ok(Some(frozen))
    }

    /// Reports an unresolved reasoning authority to the app run gate. A
    /// missing profile remains a valid provider-default state and returns
    /// `false` here.
    pub fn reasoning_unavailable(&self) -> bool {
        self.composer_defaults.reasoning_unavailable
    }

    /// Installs configured model options for the composer selector (#60 R1).
    /// The app supplies the catalog; the stream never reads config files.
    pub fn apply_model_options(&mut self, options: Vec<String>, cx: &mut Context<Self>) {
        self.model_options = options;
        self.model_selector_highlight = self
            .model_options
            .iter()
            .position(|model| *model == self.composer_defaults.model)
            .unwrap_or(0);
        cx.notify();
    }

    /// Emits the in-session model selection intent (R1): the displayed model
    /// stays authoritative until the app acknowledges the durable save.
    /// Single owner at a time; while a selection is pending, further
    /// selections are refused and the request id carries the exact owner
    /// identity through the acknowledgement.
    pub fn request_model_selection(&mut self, model: &str, cx: &mut Context<Self>) {
        // A branch/commit/artifact operation uses the same generic busy flag.
        // Refuse at the UI boundary so an invalid event cannot install a
        // model pending owner that the app handler would have to unwind.
        if self.model_selection_pending.is_some()
            || self.model_selection_save_busy()
            || self.trusted_action_busy
            || model.is_empty()
        {
            return;
        }
        let request_id = self.next_model_request_id;
        self.next_model_request_id += 1;
        self.model_selection_pending = Some((request_id, model.to_string()));
        cx.emit(ThreadModelSelectionRequested {
            thread_id: self.thread.id.clone(),
            model: model.to_string(),
            request_id,
        });
        cx.notify();
    }

    /// Applies the authoritative thread after the durable model save (R1):
    /// only the exact request owner and the owning thread accept it, so a
    /// late or stale callback can never write another thread's state or
    /// release a newer selection's owner.
    pub fn apply_thread_model_acknowledged(
        &mut self,
        thread_id: &str,
        request_id: u64,
        model: &str,
        current_thread: Thread,
        cx: &mut Context<Self>,
    ) {
        if thread_id != self.thread.id {
            return;
        }
        let Some((owner_id, _)) = self.model_selection_pending.as_ref() else {
            return;
        };
        if *owner_id != request_id {
            return;
        }
        if current_thread.id != self.thread.id
            || current_thread.project_id != self.thread.project_id
        {
            return;
        }
        if self
            .model_selection_pending
            .as_ref()
            .is_none_or(|(_, requested_model)| requested_model != model)
        {
            return;
        }
        let mut projected = current_thread;
        projected.model = model.to_owned();
        if model != self.thread.model {
            self.reset_context_control(cx);
        }
        self.composer_defaults.model = model.to_owned();
        self.thread = projected;
        self.model_selection_pending = None;
        self.clear_model_selection_save_owner(request_id, cx);
        // R59 R3: the durable acknowledgement closes the picker. A model change
        // is a completed round trip, so leaving a level mounted would strand
        // the user on a layer whose selection is already applied.
        self.model_picker_level = ModelPickerLevel::Closed;
        self.controller_error = None;
        cx.notify();
    }

    /// Applies a failed in-session model save (R1): the durable/displayed
    /// model is preserved exactly as it was (the write failed closed, so the
    /// previous authoritative value still stands), the failure is shown and
    /// the exact request owner is released. A late or stale callback for
    /// another request or another thread never lands.
    pub fn apply_thread_model_failed(
        &mut self,
        thread_id: &str,
        request_id: u64,
        cx: &mut Context<Self>,
    ) {
        if thread_id != self.thread.id {
            return;
        }
        let Some((owner_id, _)) = self.model_selection_pending.as_ref() else {
            return;
        };
        if *owner_id != request_id {
            return;
        }
        self.model_selection_pending = None;
        self.clear_model_selection_save_owner(request_id, cx);
        self.controller_error = Some("模型选择保存失败，请重试".into());
        cx.notify();
    }

    /// Enter/Space on the focused model selector trigger (A2-14).
    ///
    /// R59 R1 / I68: the trigger opens the slider when tiers exist, otherwise
    /// it opens the model list directly. Only the list keeps the pre-existing
    /// "Enter accepts the highlighted model" behaviour.
    pub(crate) fn on_activate_model(
        &mut self,
        _: &ActivateModel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_options.is_empty()
            || self.model_selection_pending.is_some()
            || self.model_selection_save_busy()
            || self.trusted_action_busy
        {
            return;
        }
        if !self.model_picker_level.is_open() {
            self.close_composer_popovers(cx);
            if self.thinking_slider.read(cx).has_tiers() {
                self.open_model_picker_slider(cx);
            } else {
                self.open_model_picker_list(cx);
            }
            return;
        }
        if self.model_picker_level == ModelPickerLevel::Slider {
            // Level one has no rows to accept; the trigger's only remaining
            // job is to close the picker again.
            self.model_picker_level = ModelPickerLevel::Closed;
            cx.notify();
            return;
        }
        if let Some(model) = self
            .model_options
            .get(self.model_selector_highlight)
            .cloned()
        {
            self.select_model_option(&model, cx);
        }
        cx.notify();
    }

    /// R59 R1: mounts the tier slider (level one) and focuses its scroll
    /// state on the current model, so keyboard navigation of the list level
    /// starts from the durable selection.
    pub(crate) fn open_model_picker_slider(&mut self, cx: &mut Context<Self>) {
        self.model_picker_level = ModelPickerLevel::Slider;
        self.model_selector_highlight = self
            .model_options
            .iter()
            .position(|model| *model == self.thread.model)
            .unwrap_or(0);
        self.model_menu_scroll
            .scroll_to_item(self.model_selector_highlight);
        cx.notify();
    }

    /// R59 R2/R3: the slider's title row was activated, so level one is
    /// replaced by level two. The slider unmounts in the same frame the list
    /// mounts, which is what makes the two levels mutually exclusive (A3).
    pub(crate) fn on_thinking_slider_title_activated(
        &mut self,
        _: &ThinkingSliderTitleActivated,
        cx: &mut Context<Self>,
    ) {
        if self.model_options.is_empty()
            || self.model_selection_pending.is_some()
            || self.model_selection_save_busy()
            || self.trusted_action_busy
        {
            return;
        }
        self.open_model_picker_list(cx);
    }

    fn open_model_picker_list(&mut self, cx: &mut Context<Self>) {
        self.model_picker_level = ModelPickerLevel::List;
        self.model_selector_highlight = self
            .model_options
            .iter()
            .position(|model| *model == self.thread.model)
            .unwrap_or(0);
        self.model_menu_scroll
            .scroll_to_item(self.model_selector_highlight);
        cx.notify();
    }

    pub(crate) fn on_model_previous(
        &mut self,
        _: &PreviousModel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_picker_level != ModelPickerLevel::List
            || self.model_options.is_empty()
            || self.trusted_action_busy
        {
            return;
        }
        self.model_selector_highlight = self.model_selector_highlight.saturating_sub(1);
        self.model_menu_scroll
            .scroll_to_item(self.model_selector_highlight);
        cx.notify();
    }

    pub(crate) fn on_model_next(&mut self, _: &NextModel, _: &mut Window, cx: &mut Context<Self>) {
        if self.model_picker_level != ModelPickerLevel::List || self.trusted_action_busy {
            return;
        }
        if self.model_selector_highlight + 1 < self.model_options.len() {
            self.model_selector_highlight += 1;
        }
        self.model_menu_scroll
            .scroll_to_item(self.model_selector_highlight);
        cx.notify();
    }

    /// Esc closes whichever level is mounted (R59 R3).
    pub(crate) fn on_model_close(
        &mut self,
        _: &CloseModel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_picker_level = ModelPickerLevel::Closed;
        cx.notify();
    }

    /// R59 R3/A4: picking a model **closes** the picker.
    ///
    /// The spec leaves this to the implementer ("回到滑块（或关闭）") and asks
    /// for the reason. Closing is the reading that cannot lie: the model change
    /// starts a durable save, and until `apply_reasoning_profile` re-projects
    /// the new model's capability the mounted slider would still be showing the
    /// **previous** model's tier ladder under the new model's name. The trigger
    /// already carries the honest feedback for that window (`保存中…`), so
    /// unmounting both levels is strictly more truthful than returning to a
    /// stale card. A **tier** selection is a different case and keeps the
    /// picker open (R58 R8): that write does not change which model the slider
    /// describes.
    ///
    /// This agrees with [`Self::apply_thread_model_acknowledged`], which also
    /// closes on the durable acknowledgement, so no path can leave the picker
    /// mounted on a selection that has already been applied.
    pub(crate) fn select_model_option(&mut self, model: &str, cx: &mut Context<Self>) {
        if self.model_selection_pending.is_some()
            || self.model_selection_save_busy()
            || self.trusted_action_busy
        {
            return;
        }
        self.model_picker_level = ModelPickerLevel::Closed;
        self.request_model_selection(model, cx);
    }

    /// Displays a bounded provider/runner failure after durable preparation.
    pub fn apply_agent_error(&mut self, cx: &mut Context<Self>) {
        self.controller_error = Some("执行未完成，可安全重试".into());
        // S7-T39: run-scoped estimate state never survives a failure path.
        self.meter.end_run();
        cx.notify();
    }

    /// An enabled-but-unavailable MCP server contributes no tools to this
    /// run. The count is enough to disclose the omission; the Settings page
    /// owns the per-server code and no remote error body enters Composer.
    pub fn apply_mcp_unavailable(
        &mut self,
        diagnostics: &[vega_conversation::types::McpServerDiagnostic],
        cx: &mut Context<Self>,
    ) {
        if diagnostics.is_empty() {
            return;
        }
        self.mcp_warning = Some(format!(
            "{} 个已启用的 MCP 服务器不可用，本轮不提供其工具；请在设置 → MCP 查看详情",
            diagnostics.len()
        ));
        cx.notify();
    }

    /// Projects a terminal runtime/provider failure after app refresh. Unlike
    /// a preparation rejection, this run already has durable message rows.
    pub fn apply_agent_runtime_error(&mut self, failure: RunFailureKind, cx: &mut Context<Self>) {
        self.controller_error = Some(failure.message());
        self.meter.end_run();
        cx.notify();
    }

    /// Displays a typed resolver rejection while keeping the original draft
    /// editable. The app clears the submit owner before calling this method;
    /// no user echo or durable row is created for the rejected attempt.
    pub fn apply_reference_error(
        &mut self,
        code: FileReferenceFailureCode,
        cx: &mut Context<Self>,
    ) {
        self.controller_error = Some(code.message().into());
        self.meter.end_run();
        cx.notify();
    }

    /// Installs the frozen per-run provisional estimator (S7-T39/C3 run
    /// ownership). Called by the app exactly once per agent run, immediately
    /// after `agent_controller.begin`.
    pub fn install_meter_estimator(
        &mut self,
        estimator: Option<RunUsageEstimator>,
        cx: &mut Context<Self>,
    ) {
        self.meter.install_run_estimator(estimator);
        cx.notify();
    }

    /// Restores the calibrated counter baseline from the durable checked
    /// aggregate (S7-T39/C4 restart recovery). Called once when a stream
    /// entity is constructed for a thread with priced usage history.
    pub fn restore_meter(&mut self, usage: RestoredUsage, cx: &mut Context<Self>) {
        self.meter.restore(usage);
        cx.notify();
    }

    /// Current counter projection (C4): the Composer renders exactly this.
    pub fn meter_snapshot(&self) -> MeterSnapshot {
        self.meter.snapshot()
    }

    /// Feeds one conversation event through the meter projection, fenced by
    /// the same acceptance rules the stream applies (S7-T39 thread/run fence:
    /// late or stale-message events never calibrate or estimate).
    pub(crate) fn feed_meter(&mut self, event: &ConversationEvent, cx: &mut Context<Self>) {
        let accepted = match event {
            ConversationEvent::MessageStarted { .. } => self.active_agent_message.is_none(),
            ConversationEvent::TextDelta { message_id, .. }
            | ConversationEvent::UsageUpdated { message_id, .. } => self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == message_id),
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id } => self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == message_id),
            ConversationEvent::Error { message_id, .. } => match message_id {
                Some(message_id) => self
                    .active_agent_message
                    .as_ref()
                    .is_some_and(|(active_id, _)| active_id == message_id),
                None => true,
            },
            // Tool proposals/results carry no assistant id; the meter gates
            // them on its own run state. Thinking is excluded from answer estimates.
            ConversationEvent::ToolCallProposed { .. }
            | ConversationEvent::ToolCallApproved { .. }
            | ConversationEvent::ToolCallOutput { .. }
            | ConversationEvent::ToolCallFinished { .. } => true,
            ConversationEvent::ThinkingDelta { .. }
            | ConversationEvent::SkillActivated { .. }
            | ConversationEvent::ContextCompactionStatus { .. }
            | ConversationEvent::ContextAccounting { .. } => false,
            // R7: app accepts these only after its thread/run ownership fence.
            ConversationEvent::ContextCompactionUsageUpdated { .. } => true,
        };
        if accepted && self.meter.apply(event) {
            cx.notify();
        }
    }
}

fn reasoning_choice_name(choice: &ReasoningChoice) -> String {
    match choice {
        ReasoningChoice::ProviderDefault => "provider_default".to_string(),
        ReasoningChoice::Disabled => "disabled".to_string(),
        ReasoningChoice::Effort(effort) => effort.clone(),
    }
}

/// Whether the slider may offer the Off position for one profile (R58 R2).
///
/// Mirrors the pairing `vega_store::reasoning::ReasoningProfile::validate` and
/// `FrozenReasoning::validate` enforce: `supports_disabled` **and** a declared
/// `disabled_wire`. Offering Off on a profile that only sets one of the two
/// would let the UI emit a request the provider rejects, so the slider stays
/// conservative and hides Off entirely.
///
/// `ReasoningProfileProjection` is produced by `from_store`, which runs the
/// store validator first, so a projected profile normally satisfies this by
/// construction. The check is repeated here because the projection is a public
/// type that tests and other callers construct directly.
pub(crate) fn supports_off_position(profile: &ReasoningProfileProjection) -> bool {
    profile.supports_disabled && profile.disabled_wire.is_some()
}

/// The tiers the slider must render for one profile (R57 P3 / R7).
///
/// Only a profile whose capability is a real declaration contributes tiers:
/// `Unsupported`/`Unknown` carry no legal effort vocabulary, so they render no
/// slider (R12) rather than guessing one. The profile's own `efforts` order is
/// preserved, because that order is also the dot order.
pub(crate) fn declared_tiers(support: ReasoningSupport, efforts: &[String]) -> Vec<String> {
    if matches!(
        support,
        ReasoningSupport::Unsupported | ReasoningSupport::Unknown
    ) {
        return Vec::new();
    }
    efforts.to_vec()
}

/// The tier name a persisted preference names, when it names one at all.
///
/// `provider_default` and `disabled` are not tiers, so they carry no name and
/// the slider resolves through its own fallback rules instead — exactly like a
/// declared effort the model no longer supports.
///
/// R58: this feeds the slider's *fallback* tier, not its current selection.
/// The current selection travels as the raw preference string
/// (`composer_defaults.thinking`), so `"disabled"` still lands on the Off
/// position (R4) even though it has no tier name here.
pub(crate) fn preferred_tier_name(preference: &ReasoningChoice) -> Option<&str> {
    match preference {
        ReasoningChoice::Effort(effort) => Some(effort.as_str()),
        ReasoningChoice::ProviderDefault | ReasoningChoice::Disabled => None,
    }
}

fn reasoning_choice_for_name(name: &str) -> ReasoningChoice {
    match name {
        "disabled" => ReasoningChoice::Disabled,
        "provider_default" | "" | "off" => ReasoningChoice::ProviderDefault,
        effort => ReasoningChoice::Effort(effort.to_string()),
    }
}
