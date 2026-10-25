use super::*;
use crate::cua_onboarding::lifecycle::TaskContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelPickerLevel {
    /// Nothing is mounted.
    #[default]
    Closed,
    Slider,
    /// The model list, reached from the slider title or directly when tiers are absent.
    List,
}

impl ModelPickerLevel {
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Closed)
    }
}

/// The opened-thread content view: thread header (title and trusted actions),
/// the virtualized message stream, and the fixed-bottom Composer. One entity
/// per open thread; rebuilt by the window root when another thread opens.
pub struct ConversationStream {
    pub(crate) prediction_waiting_for_queue: bool,
    pub(crate) prediction_owner: Option<super::composer_predictions::ComposerPredictionOwner>,
    pub(crate) prediction_generation: u64,
    pub(crate) prediction_completed: Option<String>,
    pub(crate) reading_preview: bool,
    pub(crate) subagent_executions:
        HashMap<(String, String), vega_conversation::types::SubagentSnapshot>,
    pub(crate) subagent_groups: HashMap<String, Entity<super::subagents::SubagentGroup>>,
    pub(crate) subagent_parent_focus: FocusHandle,
    pub(crate) delegated_run_active: bool,
    pub(crate) automation: super::automation::AutomationUi,
    pub(crate) privacy: super::privacy::PrivacyPanel,
    pub(crate) computer_use_control: Option<vega_conversation::types::ComputerUseControl>,
    pub(crate) computer_use_task: TaskContext,
    pub(crate) context_control: context_control::ContextControl,
    pub(crate) attachments: Vec<(u64, attachments::ImagePreview)>,
    pub(crate) attachment_generation: u64,
    pub(crate) attachment_import_pending: bool,
    pub(crate) attachment_error: Option<&'static str>,
    pub(crate) submitted_attachments: Vec<(u64, attachments::ImagePreview)>,
    pub(crate) thread: Thread,
    pub(crate) entries: Vec<StreamEntry>,
    pub(crate) assistant_role_rows: Vec<bool>,
    pub(crate) counters: Arc<StreamCounters>,
    pub(crate) list: gpui_kit::ListState,
    pub(crate) foreground_height: Pixels,
    pub(crate) entry_identities: Vec<StreamEntryIdentity>,
    pub(crate) measured_entry_heights: HashMap<String, f32>,
    pub(super) text_cache: std::cell::RefCell<super::text_cache::TextCache>,
    pub(crate) message_anchor_layout: super::message_anchors::MessageAnchorLayout,
    pub(crate) message_anchor_hovered: Option<String>,
    pub(crate) message_anchor_rail_hovered: bool,
    pub(crate) message_anchor_preview_hovered: Option<String>,
    pub(crate) message_anchor_keyboard_id: Option<String>,
    pub(crate) message_anchor_focus: FocusHandle,
    pub(crate) message_anchor_rail_last_shown: bool,
    pub(crate) message_anchor_window_start: usize,
    pub(crate) next_local_entry_id: u64,
    /// Active demo injection (`None` = idle/finished).
    pub(crate) injecting: Option<InjectionState>,
    /// Composer 输入状态（独立 `TextInput` Entity，1–8 行自适应多行）。
    pub(crate) input: Entity<TextInput>,
    pub(crate) selection_focus: FocusHandle,
    pub(crate) selection_view_initialized: bool,
    /// Synthetic block-id counter for user echo rows (diagnostics only).
    pub(crate) user_block_seq: u64,
    /// Opaque provider call ids are retained only as non-rendered map keys.
    pub(crate) tool_cards: HashMap<String, Entity<ToolCard>>,
    pub(crate) run_responses: HashMap<String, phase::RunResponses>,
    pub(crate) run_activity_groups: HashMap<String, Entity<RunActivityGroup>>,
    pub(crate) run_continuations: HashMap<String, vega_conversation::types::RunContinuation>,
    pub(crate) artifact_cards: HashMap<String, Entity<ArtifactCard>>,
    pub(crate) artifact_call_owners: HashMap<String, Option<String>>,
    pub(crate) artifact_history_generation: u64,
    pub(crate) artifact_groups: HashMap<String, Entity<ArtifactGroup>>,
    pub(crate) finished_artifact_turns: HashSet<String>,
    /// Route-owned safe branch selector; Git authority remains in the app controller.
    pub(crate) branch_selector: Entity<BranchSelector>,
    /// IO-free canonical commit panel; repository authority stays in app/headless.
    pub(crate) commit_panel: Entity<CommitPanel>,
    /// Concrete runtime permission hook shared by the owning conversation.
    pub(crate) permission_queue: PermissionQueue,
    pub(crate) user_question_queue: vega_conversation::agent::UserQuestionQueue,
    pub(crate) deferred_user_question: Option<vega_conversation::agent::PendingUserQuestion>,
    pub(crate) active_user_question: Option<Entity<crate::user_question_panel::UserQuestionPanel>>,
    pub(crate) active_user_question_call_id: Option<String>,
    pub(crate) user_question_attention: Option<vega_conversation::types::UserQuestionId>,
    pub(crate) _user_question_listener_task: gpui_kit::Task<()>,
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
    pub(crate) summary_cards: HashMap<String, Entity<SummaryCard>>,
    pub(crate) hydration: HistoryHydration,
    pub(crate) message_location_status: Option<MessageLocationStatus>,
    /// Exact active durable assistant id and its stream-entry index.
    pub(crate) active_agent_message: Option<(String, usize)>,
    pub(crate) assistant_model_dirty: bool,
    pub(crate) active_run_activity: Option<(String, Entity<RunActivityGroup>)>,
    pub(crate) active_run_activity_segment: Option<usize>,
    pub(crate) active_agent_sequence: Option<i64>,
    pub(crate) active_segment_ordinal: usize,
    pub(crate) active_response_start: Option<usize>,
    pub(crate) active_response_id: Option<String>,
    pub(crate) response_reasoning_completed: bool,
    pub(crate) response_checkpoint: Option<network_retry::ResponseCheckpoint>,
    pub(crate) network_retry: Option<(String, u32, u32)>,
    /// Whether the current Markdown segment contains text; a tool boundary
    /// may leave the active run without a segment until the next text delta.
    pub(crate) active_segment_has_text: bool,
    pub(crate) active_segment_has_visible_text: bool,
    pub(crate) active_thinking: Option<Entity<ThinkingBlock>>,
    pub(crate) thinking_bytes: usize,
    pub(crate) thinking_blocks: usize,
    pub(crate) retained_thinking: std::collections::VecDeque<gpui_kit::WeakEntity<ThinkingBlock>>,
    /// Most recently finished assistant entry, retained until the typed Plan
    /// projection can replace it in place.
    pub(crate) last_finished_agent_message: Option<(String, usize)>,
    pub(crate) meter: ConversationMeter,
    /// Submitted drafts, scoped to this thread view.
    pub(crate) composer_history: Vec<String>,
    pub(crate) composer_submit_pending: bool,
    pub(crate) pending_queue: PendingQueueProjection,
    pub(crate) pending_behavior_override: Option<PendingMessageBehavior>,
    pub(crate) pending_submission_revision: Option<u64>,
    pub(crate) pending_ui: pending_messages::PendingQueueUi,
    pub(crate) pending_echoes: Vec<PendingEcho>,
    pub(crate) skill_projection: Option<SkillComposerProjection>,
    pub(crate) skill_projection_generation: u64,
    pub(crate) skill_mutation_pending: bool,
    pub(crate) skill_intent: Option<SkillSelectionIntent>,
    pub(crate) active_skills: Vec<vega_conversation::types::ActiveSkillView>,
    pub(crate) active_skill_focuses: Vec<[FocusHandle; 2]>,
    pub(crate) actions: ComposerActions,
    pub(crate) action_focus: [FocusHandle; 2],
    pub(crate) history_cursor: Option<usize>,
    pub(crate) history_draft: Option<String>,
    pub(crate) approved_not_started: bool,
    pub(crate) trusted_action_busy: bool,
    pub(crate) controller_error: Option<String>,
    pub(crate) controller_runtime_error: Option<(String, RunFailureKind)>,
    pub(crate) run_failures: HashMap<String, RunFailureKind>,
    pub(crate) run_notices: HashMap<usize, super::run_notice::RunNotice>,
    /// Nonfatal run-start MCP omission. Kept separate so a provider failure
    /// cannot overwrite it or be overwritten by it.
    pub(crate) mcp_warning: Option<String>,
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
    pub(crate) composer_defaults: ComposerDefaults,
    pub(crate) thinking_slider: Entity<ThinkingSlider>,
    pub(crate) model_options: Vec<String>,
    pub(crate) model_picker_level: ModelPickerLevel,
    pub(crate) model_menu_scroll: ScrollHandle,
    pub(crate) compact_workspace: bool,
    pub(crate) workspace_width: Option<f32>,
    pub(crate) project_label: String,
    pub(crate) draft_route: bool,
    pub(crate) utility_projects: Vec<(String, String)>,
    pub(crate) utility_projects_open: bool,
    pub(crate) utility_project_query: String,
    pub(crate) backend_selector_open: bool,
    pub(crate) codex_mode_confirmed: bool,
    pub(crate) utility_project_search: Entity<TextInput>,
    pub(crate) permission_picker_open: bool,
    pub(crate) model_selector_highlight: usize,
    pub(crate) context_usage_focus: FocusHandle,
    pub(crate) context_usage_focus_subscriptions:
        Option<(gpui_kit::Subscription, gpui_kit::Subscription)>,
    pub(crate) context_usage_trigger_hovered: bool,
    pub(crate) context_usage_tooltip_hovered: bool,
    pub(crate) context_usage_open: bool,
    pub(crate) context_usage_dismissed: bool,
    pub(crate) model_focus: FocusHandle,
    pub(crate) model_selection_pending: Option<(u64, String)>,
    /// Exact app-level owner for the in-flight model save. This is separate
    /// from `trusted_action_busy`, which also represents branch, commit, and
    /// artifact operations.
    pub(crate) model_selection_save_owner: Option<u64>,
    pub(crate) next_model_request_id: u64,
    /// Keyboard-accessible return-to-tail action shown after the user scrolls
    /// away from the live bottom (P4).
    pub(crate) resume_tail_focus: FocusHandle,
    /// Cancels the watch listener and drops its fail-closed guard with the view.
    pub(crate) _permission_listener_task: gpui_kit::Task<()>,
}

impl EventEmitter<crate::cua_onboarding::PreparationRequested> for ConversationStream {}
impl EventEmitter<ImagePreviewRequested> for ConversationStream {}
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
impl EventEmitter<NewerHistoryPageRequested> for ConversationStream {}
impl EventEmitter<MessageLocationRequested> for ConversationStream {}
impl EventEmitter<FileIndexRequested> for ConversationStream {}
impl EventEmitter<FileIndexCancelled> for ConversationStream {}
impl EventEmitter<ComposerStopRequested> for ConversationStream {}
impl EventEmitter<FileIndexRetryRequested> for ConversationStream {}
impl EventEmitter<ComposerDefaultsRequested> for ConversationStream {}
impl EventEmitter<ThreadModelSelectionRequested> for ConversationStream {}
impl EventEmitter<TaskBackendSelectionRequested> for ConversationStream {}

pub(crate) struct InjectionState {
    /// Which assistant entry the replayer feeds.
    pub(crate) entry_index: usize,
    pub(crate) replay: MockReplay,
}

pub(crate) struct PendingEcho {
    pub(crate) pending_id: String,
    pub(crate) indices: Vec<usize>,
}

impl ConversationStream {
    pub fn set_reading_preview(&mut self, reading: bool, cx: &mut Context<Self>) {
        if self.reading_preview == reading {
            return;
        }
        self.reading_preview = reading;
        for card in self.artifact_cards.values() {
            card.update(cx, |card, cx| card.set_reading_preview(reading, cx));
        }
        self.hydration.loading = false;
        self.hydration.paused = false;
        self.hydration.paused_direction = None;
        self.foreground_height = px(0.);
        self.measured_entry_heights.clear();
        self.list.remeasure();
        cx.notify();
    }

    /// Projects the host column width without rebuilding composer state.
    pub fn set_workspace_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let automation_compact = self.automation_compact();
        let width_changed = self
            .workspace_width
            .is_some_and(|previous| (previous - width).abs() > 0.5);
        self.workspace_width = Some(width);
        if automation_compact != self.automation_compact()
            && !self.automation.projection.tasks.is_empty()
        {
            cx.notify();
        }
        if width_changed && !self.entries.is_empty() {
            self.measured_entry_heights.clear();
            self.ensure_entry_identities();
            let top = self.list.logical_scroll_top();
            let anchor = self.scroll_anchor_snapshot();
            self.list.remeasure();
            self.restore_scroll_anchor_at(&anchor, top.item_ix);
            cx.notify();
        }
        let compact = width < 500.;
        if self.compact_workspace != compact {
            self.compact_workspace = compact;
            cx.notify();
        }
    }

    pub fn new(thread: Thread, cx: &mut Context<Self>) -> Self {
        Self::new_with_permission_queue(thread, PermissionQueue::new(), cx)
    }

    /// Builds a stream with the exact permission queue passed to the runtime.
    pub fn new_with_permission_queue(
        thread: Thread,
        permission_queue: PermissionQueue,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_request_queues(
            thread,
            permission_queue,
            vega_conversation::agent::UserQuestionQueue::new(),
            cx,
        )
    }

    pub fn new_with_request_queues(
        thread: Thread,
        permission_queue: PermissionQueue,
        user_question_queue: vega_conversation::agent::UserQuestionQueue,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_global::<RunningMessageDefault>(|_, cx| cx.notify())
            .detach();
        let input = cx.new(|cx| {
            TextInput::new_multiline(cx, "描述任务，或用 @ 引用文件", COMPOSER_ROWS)
                .with_image_paste()
        });
        let branch_selector = cx.new(|cx| {
            let mut selector =
                BranchSelector::new(thread.id.clone(), thread.project_id.clone(), cx);

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

        let utility_project_search =
            cx.new(|cx| TextInput::new(cx, "搜索项目", false).with_bare_chrome());
        cx.observe(&utility_project_search, |this, input, cx| {
            this.sync_utility_project_query(&input, cx);
        })
        .detach();
        // 空输入禁用发送 + `@` 触发的文件选择跟随输入内容变化：输入内容
        // 变化即重渲染 Composer。
        cx.observe_global::<ComposerPredictionsEnabled>(|this, cx| {
            this.invalidate_composer_prediction(cx);
            cx.notify();
        })
        .detach();
        cx.observe_self(|this, cx| this.reconcile_composer_prediction(cx))
            .detach();
        cx.observe(&input, |this, input, cx| {
            this.reconcile_composer_prediction(cx);
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
        let mut question_listener = user_question_queue.subscribe();
        let user_question_listener_task = cx.spawn(async move |this, cx| {
            while question_listener.changed().await {
                if this
                    .update(cx, |this, cx| {
                        this.reconcile_composer_prediction(cx);
                        this.install_pending_user_question(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut listener = permission_queue.subscribe();
        let permission_listener_task = cx.spawn(async move |this, cx| {
            while listener.changed().await {
                let alive = this
                    .update(cx, |this, cx| {
                        this.reconcile_composer_prediction(cx);
                        this.install_pending_permission(cx);
                    })
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
                this.context_usage_trigger_hovered = false;
                this.context_usage_tooltip_hovered = false;
                this.context_usage_open = false;
                this.context_usage_dismissed = true;
                cx.notify();
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

        cx.subscribe(
            &thinking_slider,
            |this, _, _: &ThinkingSliderTitleActivated, cx| {
                this.on_thinking_slider_title_activated(&ThinkingSliderTitleActivated, cx);
            },
        )
        .detach();
        let computer_use_task = TaskContext::new(&thread.id);
        Self {
            prediction_waiting_for_queue: false,
            prediction_owner: None,
            prediction_generation: 0,
            prediction_completed: None,
            reading_preview: false,
            subagent_groups: HashMap::new(),
            subagent_executions: HashMap::new(),
            subagent_parent_focus: cx.focus_handle().tab_stop(true),
            delegated_run_active: false,
            context_control: context_control::ContextControl::new(cx),
            computer_use_control: None,
            computer_use_task,
            attachments: Vec::new(),
            attachment_generation: 0,
            attachment_import_pending: false,
            attachment_error: None,
            submitted_attachments: Vec::new(),
            thread,
            entries: Vec::new(),
            counters: Arc::new(StreamCounters::default()),
            list,
            foreground_height: px(Layout::READING_NAVIGATION_HEIGHT
                + Layout::COMPOSER_PADDING_TOP
                + Layout::COMPOSER_MIN_HEIGHT
                + Layout::COMPOSER_PADDING_BOTTOM),
            entry_identities: Vec::new(),
            measured_entry_heights: HashMap::new(),
            text_cache: Default::default(),
            assistant_role_rows: Vec::new(),
            message_anchor_layout: super::message_anchors::MessageAnchorLayout::default(),
            message_anchor_hovered: None,
            message_anchor_rail_hovered: false,
            message_anchor_preview_hovered: None,
            message_anchor_keyboard_id: None,
            message_anchor_focus: cx.focus_handle().tab_index(17).tab_stop(true),
            message_anchor_rail_last_shown: false,
            message_anchor_window_start: 0,
            next_local_entry_id: 0,
            injecting: None,
            input,
            selection_focus: cx.focus_handle(),
            selection_view_initialized: false,
            user_block_seq: USER_BLOCK_BASE,
            tool_cards: HashMap::new(),
            run_responses: HashMap::new(),
            run_activity_groups: HashMap::new(),
            run_continuations: HashMap::new(),
            artifact_cards: HashMap::new(),
            artifact_call_owners: HashMap::new(),
            artifact_history_generation: 0,
            artifact_groups: HashMap::new(),
            finished_artifact_turns: HashSet::new(),
            branch_selector,
            commit_panel,
            permission_queue,
            user_question_queue,
            deferred_user_question: None,
            active_user_question: None,
            active_user_question_call_id: None,
            user_question_attention: None,
            _user_question_listener_task: user_question_listener_task,
            deferred_permission: None,
            active_permission: None,
            active_permission_call_id: None,
            plan_cards: HashMap::new(),
            summary_cards: HashMap::new(),
            hydration: HistoryHydration::default(),
            message_location_status: None,
            active_agent_message: None,
            assistant_model_dirty: false,
            active_run_activity: None,
            active_run_activity_segment: None,
            active_agent_sequence: None,
            active_segment_ordinal: 0,
            active_response_start: None,
            active_response_id: None,
            response_reasoning_completed: false,
            response_checkpoint: None,
            network_retry: None,
            active_thinking: None,
            thinking_bytes: 0,
            thinking_blocks: 0,
            retained_thinking: Default::default(),
            active_segment_has_text: false,
            active_segment_has_visible_text: false,
            last_finished_agent_message: None,
            meter: ConversationMeter::default(),
            composer_history: Vec::new(),
            composer_submit_pending: false,
            automation: super::automation::AutomationUi::default(),
            pending_queue: PendingQueueProjection::default(),
            pending_behavior_override: None,
            pending_submission_revision: None,
            pending_ui: pending_messages::PendingQueueUi::new(cx),
            pending_echoes: Vec::new(),
            skill_projection: None,
            skill_projection_generation: 0,
            skill_mutation_pending: false,
            skill_intent: None,
            active_skills: Vec::new(),
            active_skill_focuses: Vec::new(),
            actions: ComposerActions::default(),
            action_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            history_cursor: None,
            history_draft: None,
            approved_not_started: false,
            trusted_action_busy: false,
            controller_error: None,
            privacy: Default::default(),
            controller_runtime_error: None,
            run_notices: HashMap::new(),
            run_failures: HashMap::new(),
            mcp_warning: None,
            file_selector: FileSelectorModel::default(),
            file_snapshot: FileIndexSnapshot::default(),
            file_index_loading: false,
            file_index_loaded: false,
            file_index_generation: 0,
            file_selector_wanted: false,
            file_index_failure: None,
            file_retry_focus: cx.focus_handle().tab_stop(true),

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
            workspace_width: None,
            project_label: String::new(),
            draft_route: false,
            utility_projects: Vec::new(),
            utility_projects_open: false,
            utility_project_query: String::new(),
            backend_selector_open: false,
            codex_mode_confirmed: false,
            utility_project_search,
            permission_picker_open: false,
            model_selector_highlight: 0,
            context_usage_focus: cx.focus_handle().tab_index(15).tab_stop(true),
            context_usage_focus_subscriptions: None,
            context_usage_trigger_hovered: false,
            context_usage_tooltip_hovered: false,
            context_usage_open: false,
            context_usage_dismissed: false,
            model_focus: cx.focus_handle().tab_index(16).tab_stop(true),
            model_selection_pending: None,
            model_selection_save_owner: None,
            next_model_request_id: 0,
            resume_tail_focus: cx.focus_handle().tab_index(18).tab_stop(true),
            _permission_listener_task: permission_listener_task,
        }
    }

    pub(crate) fn list_append(&mut self, previous_len: usize) {
        let count = self.entries.len();
        debug_assert!(count >= previous_len);
        self.ensure_entry_identities();
        if count > previous_len {
            self.refresh_assistant_roles();
            self.list
                .splice(previous_len..previous_len, count - previous_len);
        }
    }

    fn refresh_assistant_roles(&mut self) {
        let mut needs_role = true;
        self.assistant_role_rows = self
            .entries
            .iter()
            .map(|entry| match entry {
                StreamEntry::User { .. } | StreamEntry::UserImages { .. } => {
                    needs_role = true;
                    false
                }
                StreamEntry::Assistant { .. } => std::mem::replace(&mut needs_role, false),
                _ => false,
            })
            .collect();
    }

    fn shift_tracked_entry_indices(&mut self, adjust: impl Fn(&mut usize)) {
        if let Some((_, index)) = &mut self.active_agent_message {
            adjust(index);
        }
        if let Some((_, index)) = &mut self.last_finished_agent_message {
            adjust(index);
        }
        if let Some(injection) = &mut self.injecting {
            adjust(&mut injection.entry_index);
        }
        for echo in &mut self.pending_echoes {
            for index in &mut echo.indices {
                adjust(index);
            }
        }
    }

    pub(crate) fn shift_entry_indices(&mut self, removed: usize, count: usize) {
        self.shift_tracked_entry_indices(|index| {
            if *index != usize::MAX && *index > removed {
                *index = index.saturating_sub(count);
            }
        });
    }

    pub(crate) fn grow_entry_indices(&mut self, inserted: usize, count: usize) {
        self.shift_tracked_entry_indices(|index| {
            if *index != usize::MAX && *index >= inserted {
                *index = index.saturating_add(count);
            }
        });
    }

    pub(crate) fn ensure_entry_identities(&mut self) {
        while self.entry_identities.len() < self.entries.len() {
            let identity = self.new_local_identity();
            self.entry_identities.push(identity);
        }
        self.entry_identities.truncate(self.entries.len());
    }

    pub(crate) fn set_entry_identity(&mut self, index: usize, identity: StreamEntryIdentity) {
        self.ensure_entry_identities();
        if let Some(slot) = self.entry_identities.get_mut(index) {
            self.measured_entry_heights.remove(&slot.key);
            self.text_cache.get_mut().retire(&slot.key);
            *slot = identity;
        }
    }

    fn new_local_identity(&mut self) -> StreamEntryIdentity {
        let local_id = self.next_local_entry_id;
        self.next_local_entry_id = self.next_local_entry_id.saturating_add(1);
        let route_id = self.thread.id.as_str();
        StreamEntryIdentity {
            key: format!("route:{}:{}:local:{}", route_id.len(), route_id, local_id),
            message_id: None,
            pending_id: None,
            echo_kind: None,
            sequence: None,
        }
    }

    pub(crate) fn durable_entry_identity(
        &self,
        message_id: &str,
        kind: &str,
        sequence: Option<i64>,
    ) -> StreamEntryIdentity {
        let route_id = self.thread.id.as_str();
        StreamEntryIdentity {
            key: format!(
                "route:{}:{}:message:{}:{}:kind:{}:{}",
                route_id.len(),
                route_id,
                message_id.len(),
                message_id,
                kind.len(),
                kind
            ),
            message_id: Some(message_id.to_string()),
            pending_id: None,
            echo_kind: None,
            sequence,
        }
    }

    pub(crate) fn tool_call_identity(
        &self,
        call_id: &str,
        message_id: Option<&str>,
        sequence: Option<i64>,
    ) -> StreamEntryIdentity {
        let route_id = self.thread.id.as_str();
        StreamEntryIdentity {
            key: format!(
                "route:{}:{}:tool-call:{}:{}",
                route_id.len(),
                route_id,
                call_id.len(),
                call_id
            ),
            message_id: message_id.map(str::to_string),
            pending_id: None,
            echo_kind: None,
            sequence,
        }
    }

    pub(crate) fn entry_identity_at(&self, index: usize) -> Option<&str> {
        self.entry_identities
            .get(index)
            .map(|identity| identity.key.as_str())
    }

    pub fn scroll_anchor_snapshot(&self) -> ThreadScrollAnchor {
        let top = self.list.logical_scroll_top();
        let identity = self.entry_identities.get(top.item_ix);
        ThreadScrollAnchor {
            identity: identity.map(|identity| identity.key.clone()),
            message_id: identity.and_then(|identity| identity.message_id.clone()),
            offset_in_item_px: f32::from(top.offset_in_item),
            following_tail: self.list.is_following_tail(),
        }
    }

    pub fn restore_scroll_anchor(&mut self, anchor: &ThreadScrollAnchor) -> bool {
        if anchor.following_tail {
            self.list.set_follow_mode(gpui_kit::FollowMode::Tail);
            return true;
        }
        let Some(identity) = anchor.identity.as_deref() else {
            return false;
        };
        let Some(item_ix) = self
            .entry_identities
            .iter()
            .position(|candidate| candidate.key == identity)
        else {
            return false;
        };
        self.restore_scroll_anchor_at(anchor, item_ix)
    }

    fn restore_scroll_anchor_at(&mut self, anchor: &ThreadScrollAnchor, item_ix: usize) -> bool {
        if anchor.following_tail {
            self.list.set_follow_mode(gpui_kit::FollowMode::Tail);
            return true;
        }
        if anchor.identity.as_deref() != self.entry_identity_at(item_ix) {
            return false;
        }
        // `scroll_to` pauses an active Tail mode without dropping the mode.
        // Keep that paused state so scrolling back to the bottom can resume.
        self.list.scroll_to(gpui_kit::ListOffset {
            item_ix,
            offset_in_item: px(anchor.offset_in_item_px),
        });
        true
    }

    /// Registers one item inserted at `index` (e.g. an inline artifact after
    /// its exact tool).
    pub(crate) fn list_insert(&mut self, index: usize) {
        self.refresh_assistant_roles();
        let identity = self.new_local_identity();
        if index <= self.entry_identities.len() {
            self.entry_identities.insert(index, identity);
        }
        self.ensure_entry_identities();
        self.list.splice(index..index, 1);
    }

    pub(crate) fn list_prepend(&mut self, count: usize) {
        if count > 0 {
            self.refresh_assistant_roles();
            let old_count = self.entries.len().saturating_sub(count);
            if self.entry_identities.len() == old_count {
                let mut identities = (0..count)
                    .map(|_| self.new_local_identity())
                    .collect::<Vec<_>>();
                identities.append(&mut self.entry_identities);
                self.entry_identities = identities;
            }
            self.ensure_entry_identities();
            self.list.splice(0..0, count);
        }
    }

    /// Registers the removal of the item at `index` (permission resolution).
    pub(crate) fn list_remove(&mut self, index: usize) {
        self.refresh_assistant_roles();
        if index < self.entry_identities.len() {
            let identity = self.entry_identities.remove(index);
            self.measured_entry_heights.remove(&identity.key);
            self.text_cache.get_mut().retire(&identity.key);
        }
        self.list.splice(index..index + 1, 0);
    }

    /// Marks the item at `index` for height re-measurement (mutable tail or
    /// explicitly invalidated item; the C4 rematerialize whitelist).
    pub(crate) fn invalidate_item(&mut self, index: Option<usize>) {
        if let Some(index) = index
            && index < self.list.item_count()
        {
            if let Some(identity) = self.entry_identities.get(index) {
                self.measured_entry_heights.remove(&identity.key);
            }
            let top = self.list.logical_scroll_top();
            let anchor = self.scroll_anchor_snapshot();
            self.list.remeasure_items(index..index + 1);
            self.restore_scroll_anchor_at(&anchor, top.item_ix);
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
            self.controller_runtime_error = None;
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

    pub(crate) fn request_task_backend(&mut self, backend: TaskBackend, cx: &mut Context<Self>) {
        if !self.draft_route
            || !self.entries.is_empty()
            || self.thread.backend == backend
            || self.trusted_action_busy
            || self.composer_submit_pending
            || self.actions.running
            || self.model_selection_pending.is_some()
        {
            return;
        }
        self.backend_selector_open = false;
        cx.emit(TaskBackendSelectionRequested {
            thread_id: self.thread.id.clone(),
            backend,
        });
        cx.notify();
    }

    pub fn select_task_backend(
        &mut self,
        backend: TaskBackend,
        cx: &mut Context<Self>,
    ) -> Option<Thread> {
        if !self.draft_route
            || !self.entries.is_empty()
            || self.thread.backend == backend
            || self.trusted_action_busy
            || self.composer_submit_pending
            || self.actions.running
            || self.model_selection_pending.is_some()
        {
            return None;
        }
        self.thread.backend = backend;
        self.backend_selector_open = false;
        self.close_composer_popovers(cx);
        cx.notify();
        Some(self.thread.clone())
    }

    pub fn confirm_codex_mode(&mut self, cx: &mut Context<Self>) {
        if self.thread.backend == TaskBackend::Codex && !self.codex_mode_confirmed {
            self.codex_mode_confirmed = true;
            cx.notify();
        }
    }

    /// Project identity of the route currently projected by this cached view.
    pub fn route_project_id(&self) -> &str {
        &self.thread.project_id
    }

    pub fn task_backend(&self) -> TaskBackend {
        self.thread.backend
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
        self.begin_computer_use_task(cx);
        self.thread = thread;
        self.project_label = label;
        cx.notify();
        true
    }

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
        self.controller_runtime_error = None;
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
        if let Some(content) = history.last()
            && let Some(index) = self
                .entries
                .iter()
                .rposition(|entry| matches!(entry, StreamEntry::User { .. }))
            && matches!(&self.entries[index], StreamEntry::User { copy, .. } if copy.original_text() == "[凭据检查不可用]")
        {
            let block = self.user_block_seq;
            self.user_block_seq += 1;
            self.entries[index] = StreamEntry::User {
                copy: MessageCopy::new(content),
                lines: user_message_lines(block, content),
            };
            self.invalidate_item(Some(index));
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
        self.controller_runtime_error = None;
        self.controller_error = Some("操作未保存，请重试".into());
        cx.notify();
    }

    pub fn apply_codex_route_error(&mut self, message: &'static str, cx: &mut Context<Self>) {
        self.controller_runtime_error = None;
        self.controller_error = Some(message.into());
        cx.notify();
    }

    /// Keeps the exact in-flight submission intact when a duplicate send arrives.
    pub fn apply_agent_busy(&mut self, cx: &mut Context<Self>) {
        self.controller_runtime_error = None;
        self.controller_error = Some("此对话正在执行，请等待完成或停止当前任务".into());
        cx.notify();
    }

    /// Rejects request preparation while preserving the draft for credential repair.
    pub fn apply_credential_error(&mut self, cx: &mut Context<Self>) {
        self.reject_composer_submission(cx);
        self.controller_runtime_error = None;
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
        self.controller_runtime_error = None;
        self.controller_error = Some(failure.message());
        cx.notify();
    }

    /// Applies an already-loaded project display label without filesystem IO.
    pub fn set_project_label(&mut self, label: String, cx: &mut Context<Self>) {
        self.project_label = label;
        cx.notify();
    }

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

    pub(crate) fn sync_thinking_slider(&mut self, cx: &mut Context<Self>) {
        let (model_name, tiers, supports_off, default_tier) =
            match self.composer_defaults.reasoning.as_ref() {
                Some(profile) => (
                    profile.model.clone(),
                    if profile.supports_enabled {
                        vec!["enabled".into()]
                    } else {
                        declared_tiers(profile.support, &profile.efforts)
                    },
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
        let accepted = if event.choice == "provider_default" {
            true
        } else if event.choice == OFF_CHOICE_NAME {
            supports_off_position(profile)
        } else if event.choice == "enabled" {
            profile.supports_enabled
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
        let mut selected = profile.clone();
        selected.preference = reasoning_choice_for_name(&self.composer_defaults.thinking);
        let frozen = selected
            .freeze()
            .map_err(|_| ReasoningSubmitError::Invalid)?;
        frozen
            .validate()
            .map_err(|_| ReasoningSubmitError::Invalid)?;
        Ok(Some(frozen))
    }

    /// Reports an unresolved reasoning authority to the app run gate. A
    /// missing profile remains a valid provider-default state and returns
    /// `false` here.
    pub fn model_snapshot_for_submit(
        &self,
    ) -> Result<Option<Box<vega_conversation::types::ResolvedModel>>, ReasoningSubmitError> {
        self.frozen_reasoning_for_submit()?;
        Ok(self
            .composer_defaults
            .reasoning
            .as_ref()
            .and_then(|profile| profile.model_snapshot.clone()))
    }

    pub fn reasoning_unavailable(&self) -> bool {
        self.composer_defaults.reasoning_unavailable
    }

    pub fn apply_model_options(&mut self, options: Vec<String>, cx: &mut Context<Self>) {
        self.model_options = options;
        self.model_selector_highlight = self
            .model_options
            .iter()
            .position(|model| *model == self.composer_defaults.model)
            .unwrap_or(0);
        cx.notify();
    }

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

        self.model_picker_level = ModelPickerLevel::Closed;
        self.controller_runtime_error = None;
        self.controller_error = None;
        cx.notify();
    }

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
        self.controller_runtime_error = None;
        self.controller_error = Some("模型选择保存失败，请重试".into());
        cx.notify();
    }

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

    pub(crate) fn on_model_close(
        &mut self,
        _: &CloseModel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_picker_level = ModelPickerLevel::Closed;
        cx.notify();
    }

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
        self.controller_runtime_error = None;
        self.controller_error = Some("执行未完成，可安全重试".into());

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
        self.apply_agent_runtime_error_for(None, failure, cx);
    }

    pub fn apply_agent_runtime_error_for(
        &mut self,
        message_id: Option<&str>,
        failure: RunFailureKind,
        cx: &mut Context<Self>,
    ) {
        if message_id
            .and_then(|id| self.run_activity_groups.get(id))
            .is_some_and(|group| group.read(cx).status() == RunActivityStatus::Completed)
        {
            return;
        }
        let failure = if let Some(id) = message_id {
            let failure = self.remember_run_failure(id, failure);
            if let Some(group) = self.run_activity_groups.get(id).cloned() {
                group.update(cx, |group, cx| {
                    group.complete(RunActivityStatus::Failed, None, cx)
                });
            }
            failure
        } else {
            failure
        };
        if message_id.is_some_and(|id| {
            self.active_agent_message
                .as_ref()
                .is_some_and(|(active, _)| active != id)
        }) {
            cx.notify();
            return;
        }
        self.controller_runtime_error = message_id.map(|id| (id.to_owned(), failure));
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
        self.controller_runtime_error = None;
        self.controller_error = Some(code.message().into());
        self.meter.end_run();
        cx.notify();
    }

    pub fn install_meter_estimator(
        &mut self,
        estimator: Option<RunUsageEstimator>,
        cx: &mut Context<Self>,
    ) {
        self.meter.install_run_estimator(estimator);
        cx.notify();
    }

    pub fn restore_meter(&mut self, usage: RestoredUsage, cx: &mut Context<Self>) {
        self.meter.restore(usage);
        cx.notify();
    }

    pub(crate) fn feed_meter(&mut self, event: &ConversationEvent, cx: &mut Context<Self>) {
        let accepted = match event {
            ConversationEvent::MessageStarted { .. } => self.active_agent_message.is_none(),
            ConversationEvent::TextDelta { message_id, .. }
            | ConversationEvent::ResponseRetrying { message_id, .. }
            | ConversationEvent::UsageUpdated { message_id, .. } => self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == message_id),
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id, .. } => self
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
            | ConversationEvent::ToolCallRunning { .. }
            | ConversationEvent::ToolCallOutput { .. }
            | ConversationEvent::ToolCallFinished { .. } => true,
            ConversationEvent::PendingMessageDelivered { .. }
            | ConversationEvent::RunStarted { .. }
            | ConversationEvent::RunContinued { .. }
            | ConversationEvent::AssistantResponseStarted { .. }
            | ConversationEvent::AssistantResponseCompleted { .. }
            | ConversationEvent::ReasoningCompleted { .. }
            | ConversationEvent::ThinkingDelta { .. }
            | ConversationEvent::SummaryDelta { .. }
            | ConversationEvent::SkillActivated { .. }
            | ConversationEvent::ScreenshotDelivered { .. }
            | ConversationEvent::ContextCompactionStatus { .. }
            | ConversationEvent::ContextAccounting { .. } => false,

            ConversationEvent::ContextCompactionUsageUpdated { .. } => true,
        };
        if accepted && self.meter.apply(event) {
            cx.notify();
        }
    }
}

fn reasoning_choice_name(choice: &ReasoningChoice) -> String {
    match choice {
        ReasoningChoice::Enabled => "enabled".into(),
        ReasoningChoice::ProviderDefault => "provider_default".to_string(),
        ReasoningChoice::Disabled => "disabled".to_string(),
        ReasoningChoice::Effort(effort) => effort.clone(),
    }
}

pub(crate) fn supports_off_position(profile: &ReasoningProfileProjection) -> bool {
    profile.supports_disabled && profile.disabled_wire.is_some()
}

pub(crate) fn declared_tiers(support: ReasoningSupport, efforts: &[String]) -> Vec<String> {
    if matches!(
        support,
        ReasoningSupport::Unsupported | ReasoningSupport::Unknown
    ) {
        return Vec::new();
    }
    efforts.to_vec()
}

pub(crate) fn preferred_tier_name(preference: &ReasoningChoice) -> Option<&str> {
    match preference {
        ReasoningChoice::Enabled => Some("enabled"),
        ReasoningChoice::Effort(effort) => Some(effort.as_str()),
        ReasoningChoice::ProviderDefault | ReasoningChoice::Disabled => None,
    }
}

fn reasoning_choice_for_name(name: &str) -> ReasoningChoice {
    match name {
        "enabled" => ReasoningChoice::Enabled,
        "disabled" => ReasoningChoice::Disabled,
        "provider_default" | "" | "off" => ReasoningChoice::ProviderDefault,
        effort => ReasoningChoice::Effort(effort.to_string()),
    }
}

impl EventEmitter<CredentialContinuationRequested> for ConversationStream {}
